// CEP:FILE crates/vortex-runtime/src/heap.rs
// CEP:WHAT Paged heap: 128-bit object headers (canonical layout handle +
//        auxiliary word), nursery/old/snapshot page kinds, array element
//        access with the full barrier policy (numeric elision, snapshot
//        traps, generational remembered sets), and snapshot freezing.
// CEP:WHY Implements the revised HEAP-1 object model, SNAP-INV-08/09/10
//        (non-relocatable read-only snapshots), NUM-1 section 3 (numeric
//        arrays), and BARRIER-INV-06/08 in one policy point.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Bounds, type, and snapshot violations return typed traps;
//        offset-zero handles are rejected at construction.
// CEP:ASSUMES 8-byte word size and alignment (HEAP-1); single-mutator
//        execution for allocation (the reference interpreter is
//        single-threaded; concurrent structures live in deps.rs).
// CEP:COST Allocation is bump-pointer amortized O(1); element access is
//        O(1) plus at most one barrier branch.
// CEP:EVIDENCE cargo test -p vortex-runtime
use crate::trap::{RtResult, Trap};
use vortex_nv1 as nv1;

/// Page size: 256 KiB, matching CODE-1-style chunked regions.
pub const PAGE_SIZE: usize = 256 * 1024;
/// Word size in bytes; all heap entities are 8-byte aligned (HEAP-1).
pub const WORD: usize = 8;

/// Canonical layout handles for the layouts the reference runtime materializes.
/// Application layouts are interned through [`crate::aoss::LayoutRegistry`].
pub mod layout {
    /// Tagged array: payload words are NV-1 values.
    pub const GENERIC_ARRAY: u64 = 0x1001;
    /// Numeric array of raw binary64 values, no NV-1 tags (NUM-1).
    pub const NUMERIC_F64: u64 = 0x1002;
    /// Numeric array of raw i64 values, no NV-1 tags (NUM-1).
    pub const NUMERIC_I64: u64 = 0x1003;
    /// Capability object (UGB-1): aux word carries epoch + revoked flag.
    pub const CAPABILITY: u64 = 0x1004;
    /// Boxed integer (i48 overflow domain, NV-1 section 2.1).
    pub const BOXED_INT: u64 = 0x1005;

    /// Returns `true` for layouts whose payload is raw numeric data.
    #[must_use]
    pub fn is_numeric(l: u64) -> bool {
        l == NUMERIC_F64 || l == NUMERIC_I64
    }
}

/// Page kinds. Snapshot pages are pinned and immortal (SNAP-INV-08).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKind {
    /// Young generation bump page; evacuated on minor collection.
    Nursery,
    /// Old generation; evacuated only on full (major) collection.
    Old,
    /// Read-only, pinned, never evacuated; card marks never written here.
    Snapshot,
}

/// A heap page: byte buffer plus bump cursor.
#[derive(Debug)]
pub struct Page {
    /// Page category.
    pub kind: PageKind,
    data: Vec<u8>,
    cursor: usize,
}

impl Page {
    /// Creates an empty page of the given kind. The first word of every page
    /// is reserved (offset zero is never a valid handle: NV-1 INV-006).
    #[must_use]
    pub fn new(kind: PageKind) -> Self {
        Page {
            kind,
            data: vec![0_u8; WORD],
            cursor: WORD,
        }
    }

    /// Bytes currently committed in the page.
    #[must_use]
    pub fn committed(&self) -> usize {
        self.data.len()
    }

    /// Allocates `size` bytes (8-aligned) and returns the byte offset.
    // CEP:WHAT Allocates size bytes (8-aligned) and returns the byte offset.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn bump(&mut self, size: usize) -> RtResult<usize> {
        let size = size.div_ceil(WORD) * WORD;
        if self.data.len() + size > PAGE_SIZE {
            return Err(Trap::OutOfMemory);
        }
        let off = self.data.len();
        self.data.resize(off + size, 0);
        self.cursor = self.data.len();
        Ok(off)
    }

    /// Reads a word at a byte offset.
    ///
    /// # Panics
    /// Panics on a misaligned or out-of-range offset: that is heap corruption
    /// in the caller, not a runtime condition.
    #[must_use]
    pub fn read_word(&self, off: usize) -> u64 {
        assert!(
            off.is_multiple_of(WORD) && off + WORD <= self.data.len(),
            "bad read {off}"
        );
        let b: [u8; 8] = self.data[off..off + WORD].try_into().expect("8 bytes");
        u64::from_le_bytes(b)
    }

    /// Writes a word at a byte offset. Snapshot pages reject writes
    /// (SNAP-INV-09): the write never lands, mirroring a `PROT_READ` mapping.
    ///
    /// # Panics
    /// Panics on a misaligned or out-of-range offset.
    pub fn write_word(&mut self, off: usize, w: u64) -> RtResult<()> {
        assert!(
            off.is_multiple_of(WORD) && off + WORD <= self.data.len(),
            "bad write {off}"
        );
        if self.kind == PageKind::Snapshot {
            return Err(Trap::SnapshotViolation { handle: 0 });
        }
        self.data[off..off + WORD].copy_from_slice(&w.to_le_bytes());
        Ok(())
    }

    /// Raw byte access for serialization/freeze scans.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    /// Clears committed bytes and resets the bump cursor (post-evacuation).
    pub fn clear(&mut self) {
        self.data.clear();
        self.cursor = 0;
    }
}

/// A heap handle: page index in the high 32 bits, byte offset in the low 32.
/// Handles are the NV-1 reference payload; they are 8-aligned and nonzero
/// (NV-1 INV-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Handle(pub u64);

impl Handle {
    /// Builds a handle; rejects the zero handle and misaligned offsets.
    pub fn new(page: u32, offset: usize) -> RtResult<Self> {
        if offset == 0 || !offset.is_multiple_of(WORD) || offset > u32::MAX as usize {
            return Err(Trap::TypeMismatch {
                expected: "aligned nonzero handle",
                word: offset as u64,
            });
        }
        Ok(Handle((u64::from(page) << 32) | offset as u64))
    }

    /// Page index component.
    #[must_use]
    pub fn page(self) -> u32 {
        (self.0 >> 32) as u32
    }

    /// Byte offset component (8-aligned).
    #[must_use]
    pub fn offset(self) -> usize {
        (self.0 & 0xFFFF_FFFF) as usize
    }
}

/// Header size in words: canonical layout handle + auxiliary word.
pub const HEADER_WORDS: usize = 2;

/// GC state bits occupy the top two bits of the layout word (HEAP-1 1.1).
pub const GC_STATE_SHIFT: u32 = 62;
/// Mask for the 2-bit GC state field.
pub const GC_STATE_MASK: u64 = 0b11 << GC_STATE_SHIFT;
/// GC state: valid layout, normal execution.
pub const GC_VALID: u64 = 0b00 << GC_STATE_SHIFT;
/// GC state: forwarding pointer (evacuation; payload is the new handle).
pub const GC_FORWARDED: u64 = 0b11 << GC_STATE_SHIFT;

/// Capability aux-word layout: epoch in bits 0..16, revoked flag bit 16
/// (UGB-1 section 6.3).
pub const CAP_EPOCH_MASK: u64 = 0xFFFF;
/// Revoked flag bit inside the capability aux word.
pub const CAP_REVOKED_BIT: u64 = 1 << 16;

/// The paged heap.
#[derive(Debug, Default)]
pub struct Heap {
    pages: Vec<Page>,
}

impl Heap {
    /// Creates an empty heap.
    #[must_use]
    pub fn new() -> Self {
        Heap { pages: Vec::new() }
    }

    /// Number of pages.
    #[must_use]
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Page kind accessor.
    ///
    /// # Panics
    /// Panics when `page` is out of range.
    #[must_use]
    pub fn page_kind(&self, page: u32) -> PageKind {
        self.pages[page as usize].kind
    }

    /// Adds a fresh nursery page and returns its index.
    pub fn add_nursery_page(&mut self) -> u32 {
        self.pages.push(Page::new(PageKind::Nursery));
        (self.pages.len() - 1) as u32
    }

    /// Adds a fresh old-generation page and returns its index.
    pub fn add_old_page(&mut self) -> u32 {
        self.pages.push(Page::new(PageKind::Old));
        (self.pages.len() - 1) as u32
    }

    /// Freezes page `page` into a read-only snapshot page.
    ///
    /// External-reference check: every reference payload in the page must
    /// point back into this same page (the base snapshot must be
    /// self-contained; SNAP-1 section 9.2 as amended). On success the page
    /// kind becomes [`PageKind::Snapshot`] and all further writes trap.
    // CEP:WHAT Freezes page page into a read-only snapshot page.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn freeze_snapshot(
        &mut self,
        page: u32,
        self_handle_min: u64,
        self_handle_max: u64,
    ) -> RtResult<()> {
        let snapshot_words = {
            let pg = &self.pages[page as usize];
            let words = pg.committed() / WORD;
            let mut refs = Vec::new();
            for w in 0..words {
                let word = pg.read_word(w * WORD);
                if nv1::is_ref(word) {
                    refs.push(nv1::ref_offset(word));
                }
            }
            refs
        };
        for off in snapshot_words {
            let h = Handle(off);
            if h.0 < self_handle_min || h.0 > self_handle_max {
                return Err(Trap::FreezeFailed {
                    page: page as usize,
                });
            }
        }
        self.pages[page as usize].kind = PageKind::Snapshot;
        Ok(())
    }

    fn page_mut(&mut self, h: Handle) -> &mut Page {
        &mut self.pages[h.page() as usize]
    }

    fn page(&self, h: Handle) -> &Page {
        &self.pages[h.page() as usize]
    }

    /// Reads the header layout word (masking GC state).
    ///
    /// # Panics
    /// Panics when the handle is out of range.
    #[must_use]
    pub fn layout_of(&self, h: Handle) -> u64 {
        self.page(h).read_word(h.offset()) & !GC_STATE_MASK
    }

    /// Rewrites the header layout word (layout transition; traps on snapshot
    /// pages).
    pub fn swap_layout(&mut self, h: Handle, lay: u64) -> RtResult<()> {
        self.page_mut(h)
            .write_word(h.offset(), lay & !GC_STATE_MASK)
    }

    /// Reads the auxiliary header word.
    #[must_use]
    pub fn aux_of(&self, h: Handle) -> u64 {
        self.page(h).read_word(h.offset() + WORD)
    }

    /// Writes the auxiliary header word (traps on snapshot pages).
    pub fn set_aux(&mut self, h: Handle, aux: u64) -> RtResult<()> {
        self.page_mut(h).write_word(h.offset() + WORD, aux)
    }

    /// Reads a payload word at `word_index` (0 = first word after the header).
    #[must_use]
    pub fn read_payload(&self, h: Handle, word_index: usize) -> u64 {
        self.page(h)
            .read_word(h.offset() + (HEADER_WORDS + word_index) * WORD)
    }

    /// Writes a payload word (traps on snapshot pages).
    pub fn write_payload(&mut self, h: Handle, word_index: usize, w: u64) -> RtResult<()> {
        self.page_mut(h)
            .write_word(h.offset() + (HEADER_WORDS + word_index) * WORD, w)
    }

    /// Array length from the auxiliary word.
    #[must_use]
    pub fn array_len(&self, h: Handle) -> usize {
        (self.aux_of(h) & 0xFFFF_FFFF) as usize
    }

    /// Allocates an array of `len` elements with layout `lay` in the nursery.
    ///
    /// The payload is zero-initialized; tagged arrays therefore start as
    /// null-filled, numeric arrays as zero-filled.
    // CEP:WHAT Allocates an array of len elements with layout lay in the nursery.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn alloc_array(&mut self, lay: u64, len: usize, nursery: u32) -> RtResult<Handle> {
        let size = (HEADER_WORDS + len) * WORD;
        let off = {
            let page = &mut self.pages[nursery as usize];
            if page.kind != PageKind::Nursery {
                return Err(Trap::TypeMismatch {
                    expected: "nursery page",
                    word: u64::from(nursery),
                });
            }
            page.bump(size)?
        };
        let h = Handle::new(nursery, off)?;
        self.page_mut(h).write_word(off, lay & !GC_STATE_MASK)?;
        self.page_mut(h).write_word(off + WORD, len as u64)?;
        Ok(h)
    }

    /// Allocates a capability object with the given authority mask, handle,
    /// initial epoch, and revoked flag (UGB-1 section 6.3 object model).
    // CEP:WHAT Allocates a capability object with the given authority mask, handle, initial epoch, and revoked flag (UGB-1 section 6.3 object model).
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn alloc_capability(
        &mut self,
        authority: u64,
        resource: u64,
        epoch: u16,
        revoked: bool,
        nursery: u32,
    ) -> RtResult<Handle> {
        let h = self.alloc_array(layout::CAPABILITY, 2, nursery)?;
        self.write_payload(h, 0, authority)?;
        self.write_payload(h, 1, resource)?;
        let aux = u64::from(epoch) | if revoked { CAP_REVOKED_BIT } else { 0 };
        self.set_aux(h, aux)?;
        Ok(h)
    }

    /// Loads an array element through the full load-barrier policy:
    /// numeric layouts return raw words with no barrier (BARRIER-INV-08);
    /// tagged layouts classify the word and route colored references through
    /// the slow path (BARRIER-INV-06 tag precedence).
    // CEP:WHAT Loads an array element through the full load-barrier policy: numeric layouts return raw words with no barrier (BARRIER-INV-08); tagged layouts classify the word and route colored references through the slow path (BARRIER-INV-06 tag precedence).
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn load_array_element(
        &self,
        arr: Handle,
        index: usize,
        stats: &mut GcStats,
    ) -> RtResult<u64> {
        let len = self.array_len(arr);
        if index >= len {
            return Err(Trap::BoundsCheck { index, len });
        }
        let lay = self.layout_of(arr);
        let word = self.read_payload(arr, index);
        if layout::is_numeric(lay) {
            stats.load_barrier_elided += 1;
            return Ok(word);
        }
        // Tagged path: tag classification first, then the color test. This is
        // the BARRIER-1-corrected fast path.
        if nv1::needs_load_barrier(word) {
            stats.load_barrier_slow += 1;
            // Slow path: resolve the reference. In this non-moving old-gen
            // reference implementation the resolution is color clearance; a
            // moving collector would follow forwarding here.
            return Ok(nv1::with_colors(word, 0));
        }
        stats.load_barrier_fast += 1;
        Ok(word)
    }

    /// Stores an array element through the full store policy:
    /// - snapshot target traps (SNAP-INV-09);
    /// - numeric target: raw store, write barrier elided (NUM-1 section 4),
    ///   with reverse transition handled by the caller (AOSS);
    /// - tagged target: generational remembered-set barrier for
    ///   old-target/nursery-value stores.
    // CEP:WHAT Stores an array element through the full store policy: - snapshot target traps (SNAP-INV-09); - numeric target: raw store, write barrier elided (NUM-1 section 4), with reverse transition handled by the caller (AOSS); - tagged target: generational remembered-set barrier for old-target/nursery-value stores.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn store_array_element(
        &mut self,
        arr: Handle,
        index: usize,
        value: u64,
        remembered: &mut Vec<Handle>,
        stats: &mut GcStats,
    ) -> RtResult<()> {
        let len = self.array_len(arr);
        if index >= len {
            return Err(Trap::BoundsCheck { index, len });
        }
        if self.page_kind(arr.page()) == PageKind::Snapshot {
            stats.snapshot_violations += 1;
            return Err(Trap::SnapshotViolation { handle: arr.0 });
        }
        let lay = self.layout_of(arr);
        if layout::is_numeric(lay) {
            // Numeric payload: no barrier, no GC obligation (NUM-INV-03
            // precondition: the layout guard above is the proof).
            stats.write_barrier_elided += 1;
            return self.write_payload(arr, index, value);
        }
        // Tagged payload: generational barrier.
        stats.write_barriers += 1;
        if nv1::is_ref(value) {
            let target_kind = self.page_kind(arr.page());
            let value_kind = self.page_kind(Handle(nv1::ref_offset(value)).page());
            if target_kind == PageKind::Old && value_kind == PageKind::Nursery {
                remembered.push(arr);
            }
        }
        self.write_payload(arr, index, value)
    }

    /// Rewrites a reference payload during evacuation (old handle to new).
    pub fn rewrite_payload_ref(
        &mut self,
        h: Handle,
        word_index: usize,
        new: Handle,
    ) -> RtResult<()> {
        self.page_mut(h).write_word(
            h.offset() + (HEADER_WORDS + word_index) * WORD,
            nv1::encode_ref(new.0, 0).expect("valid handle"),
        )
    }

    /// Index of the first nursery page, if any exists.
    #[must_use]
    pub fn nursery_page(&self) -> Option<u32> {
        self.pages
            .iter()
            .position(|p| p.kind == PageKind::Nursery)
            .map(|p| p as u32)
    }

    /// Number of payload words of an object, dispatching on layout. The
    /// capability layout's aux word carries epoch bits, not a length, so it is
    /// special-cased here.
    ///
    /// # Panics
    /// Panics when the handle is out of range.
    #[must_use]
    pub fn payload_word_count(&self, h: Handle) -> usize {
        match self.layout_of(h) {
            layout::CAPABILITY => 2,
            layout::BOXED_INT => 1,
            _ => self.array_len(h),
        }
    }

    /// Total object size in words (header + payload).
    #[must_use]
    pub fn total_word_count(&self, h: Handle) -> usize {
        HEADER_WORDS + self.payload_word_count(h)
    }

    /// Free words remaining in a page.
    ///
    /// # Panics
    /// Panics when `page` is out of range.
    #[must_use]
    pub fn page_free_words(&self, page: u32) -> usize {
        (PAGE_SIZE - self.pages[page as usize].committed()) / WORD
    }

    /// Bump-allocates `words` words in `page` and returns the handle.
    pub fn bump_in(&mut self, page: u32, words: usize) -> RtResult<Handle> {
        let off = self.pages[page as usize].bump(words * WORD)?;
        Handle::new(page, off)
    }

    /// Copies the full object `src` (header + payload) to the already
    /// allocated space at `dst`. Both must be within range.
    pub fn copy_object(&mut self, src: Handle, dst: Handle) -> RtResult<()> {
        let words = self.total_word_count(src);
        for i in 0..words {
            let w = self.page(src).read_word(src.offset() + i * WORD);
            self.page_mut(dst).write_word(dst.offset() + i * WORD, w)?;
        }
        Ok(())
    }

    /// Resets the committed content of the given pages (post-evacuation
    /// reclamation). Snapshot pages are never passed here by the collector.
    pub fn reset_pages(&mut self, pages: &[u32]) {
        for &p in pages {
            let page = &mut self.pages[p as usize];
            if page.kind == PageKind::Snapshot {
                continue; // defense in depth: snapshots are never reclaimed
            }
            page.cursor = 0;
            page.clear();
        }
    }
}

/// Barrier and GC counters. Numbers are part of the numeric contract tests
/// (NUM-1 section 4): elision must be observable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GcStats {
    /// Load barrier fast-path hits (clean references).
    pub load_barrier_fast: u64,
    /// Load barrier slow-path entries (colored references).
    pub load_barrier_slow: u64,
    /// Load barriers elided on numeric payloads.
    pub load_barrier_elided: u64,
    /// Write barriers executed on tagged stores.
    pub write_barriers: u64,
    /// Write barriers elided on numeric-array stores.
    pub write_barrier_elided: u64,
    /// Snapshot write violations observed.
    pub snapshot_violations: u64,
    /// Objects moved by evacuation.
    pub objects_moved: u64,
    /// Snapshot objects moved: must remain zero forever (SNAP-INV-08).
    pub snapshot_objects_moved: u64,
}
