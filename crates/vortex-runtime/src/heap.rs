// CEP:FILE crates/vortex-runtime/src/heap.rs
// CEP:WHAT The region-based heap model: object headers, bump-pointer
//        allocation with TLABs, humongous allocation, the card table, and
//        NV-1 heap-relative references (GC design 6.3-6.5).
// CEP:WHY The collector and the interpreter share one address model where
//        references are 48-bit byte offsets from the arena base, keeping
//        values valid under relocation without native pointers.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Allocation returns Trap::OutOfMemory after failed expansion;
//        reference decode returns Trap::BadReference outside the arena.
// CEP:ASSUMES Arena size fits a 48-bit offset window; objects are
//        8-byte aligned; headers are 24 bytes.
// CEP:COST Allocation O(1) on the fast path; card decode O(1).
// CEP:EVIDENCE cargo test -p vortex-runtime
//! Heap object model and allocation.

use crate::trap::{RtResult, Trap};

/// Region kind (GC design 6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionKind {
    /// Bump-allocated young regions, collected by copying.
    Young,
    /// Promoted or long-lived objects.
    Old,
    /// Contiguous runs for objects larger than half a region.
    Humongous,
    /// Available for allocation.
    Free,
}

/// Card table entry values (GC design 6.3).
pub const CARD_YOUNG: u8 = 0;
/// Card table entry: old generation.
pub const CARD_OLD: u8 = 1;
/// Card table entry: humongous run.
pub const CARD_HUMONGOUS: u8 = 2;
/// Card table entry: free region.
pub const CARD_FREE: u8 = 3;
/// Card table entry: contains an old-to-young pointer.
pub const CARD_DIRTY: u8 = 4;

/// Card size in bytes.
pub const CARD_SIZE: usize = 512;
/// Object header size in bytes (mark_word, klass, size+flags).
pub const HEADER_SIZE: usize = 24;
/// Object alignment in bytes.
pub const ALIGN: usize = 8;
/// Reserved page at the arena base so offsets are never zero (NV-1 3.1).
pub const ARENA_BASE_RESERVE: usize = 4096;
/// Header flag: the object was relocated; mark_word holds the new offset.
pub const FLAG_FORWARDED: u8 = 0x01;
/// Header flag: the object holds a finalizer.
pub const FLAG_FINALIZABLE: u8 = 0x02;

/// One heap region.
#[derive(Debug, Clone, Copy)]
pub struct Region {
    /// First byte offset of the region in the arena.
    pub start: usize,
    /// Region size in bytes.
    pub size: usize,
    /// Current role.
    pub kind: RegionKind,
    /// Bump-pointer allocation top inside the region.
    pub top: usize,
}

impl Region {
    /// Bytes between the allocation top and the region end.
    // CEP:WHAT Reports the free tail of a region.
    // CEP:WHY TLAB fast-path allocation needs the remaining size.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES top <= size.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn remaining(&self) -> usize {
        self.size.saturating_sub(self.top)
    }
}

/// A thread-local allocation buffer (GC design 6.4).
#[derive(Debug, Clone, Copy)]
pub struct Tlab {
    /// Region the buffer belongs to.
    pub region: usize,
    /// Next allocation offset in the arena.
    pub top: usize,
    /// End of this thread's buffer in the arena.
    pub end: usize,
}

impl Tlab {
    /// Fast-path allocation when the buffer holds `size` bytes.
    // CEP:WHAT Bump-pointer allocation inside the buffer.
    // CEP:WHY NV-P01: allocation must be one increment on the hot path.
    // CEP:STATUS complete
    // CEP:FAILURE None when the buffer is exhausted.
    // CEP:ASSUMES size is 8-byte aligned.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn try_allocate(&mut self, size: usize) -> Option<usize> {
        if self.top + size > self.end {
            return None;
        }
        let obj = self.top;
        self.top += size;
        Some(obj)
    }
}

/// The arena-backed heap.
pub struct Heap {
    /// Backing byte arena; offsets are NV-1 heap-relative references.
    data: Vec<u8>,
    /// Regions partitioning the arena.
    regions: Vec<Region>,
    /// Indices of free regions, in allocation preference order.
    free: Vec<usize>,
    /// Card table: one byte per CARD_SIZE bytes of arena.
    cards: Vec<u8>,
    /// Configured region size.
    region_size: usize,
}

/// Heap construction parameters.
#[derive(Debug, Clone, Copy)]
pub struct HeapConfig {
    /// Number of regions to commit at startup.
    pub initial_regions: usize,
    /// Region size in bytes.
    pub region_size: usize,
    /// Maximum regions the heap may grow to.
    pub max_regions: usize,
}

impl Default for HeapConfig {
    // CEP:WHAT Provides model heap defaults sized for tests.
    // CEP:WHY Production sizes (4 MiB regions) would allocate gigabytes in
    //        a unit-test process; defaults are documented model sizes.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    fn default() -> HeapConfig {
        HeapConfig {
            initial_regions: 8,
            region_size: 64 * 1024,
            max_regions: 256,
        }
    }
}

impl Heap {
    /// Builds a heap: commits regions, zeroes the card table.
    // CEP:WHAT Initializes regions and the card table.
    // CEP:WHY GC 6.15: heap initialization is O(|heap|) metadata zeroing.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; the arena is a fixed allocation.
    // CEP:ASSUMES None.
    // CEP:COST O(|heap|).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn new(config: HeapConfig) -> Heap {
        // NV-1 3.1 reserves offset zero as the invalid reference; the arena
        // begins after a reserved page so no object can sit at offset 0.
        let base = ARENA_BASE_RESERVE;
        let size = base + config.initial_regions * config.region_size;
        let mut regions = Vec::with_capacity(config.initial_regions);
        for i in 0..config.initial_regions {
            regions.push(Region {
                start: base + i * config.region_size,
                size: config.region_size,
                kind: RegionKind::Free,
                top: 0,
            });
        }
        Heap {
            data: vec![0u8; size],
            regions,
            free: (0..config.initial_regions).rev().collect(),
            cards: vec![CARD_FREE; size / CARD_SIZE],
            region_size: config.region_size,
        }
    }

    /// The arena size in bytes.
    // CEP:WHAT Reports the committed arena size.
    // CEP:WHY Tests and the AOT snapshot need the window.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn arena_size(&self) -> usize {
        self.data.len()
    }

    /// Decodes a card index for an arena offset.
    // CEP:WHAT Maps an offset to its card index.
    // CEP:WHY Barriers and the remset work in card units.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    #[inline]
    pub fn card_of(&self, offset: usize) -> usize {
        offset / CARD_SIZE
    }

    /// The region containing an offset.
    // CEP:WHAT Resolves the region that owns an offset.
    // CEP:WHY Barriers branch on the source and target generation.
    // CEP:STATUS complete
    // CEP:FAILURE None outside the arena.
    // CEP:ASSUMES None.
    // CEP:COST O(regions).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn region_of(&self, offset: usize) -> Option<usize> {
        self.regions
            .iter()
            .position(|r| offset >= r.start && offset < r.start + r.size)
    }

    /// Marks a card dirty (old-to-young bookkeeping).
    // CEP:WHAT Sets the DIRTY value on a card.
    // CEP:WHY The generational invariant is maintained by the barrier.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    #[inline]
    pub fn dirty_card(&mut self, offset: usize) {
        let c = self.card_of(offset);
        if c < self.cards.len() {
            self.cards[c] = CARD_DIRTY;
        }
    }

    /// Reads a card value.
    // CEP:WHAT Returns the card state for an offset.
    // CEP:WHY Collection scans dirty cards to rebuild the remset.
    // CEP:STATUS complete
    // CEP:FAILURE CARD_FREE for out-of-range cards.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    #[inline]
    pub fn card_value(&self, offset: usize) -> u8 {
        let c = self.card_of(offset);
        if c < self.cards.len() {
            self.cards[c]
        } else {
            CARD_FREE
        }
    }

    /// Resets every DIRTY card to its region's base value.
    // CEP:WHAT Clears dirty marks after a young collection.
    // CEP:WHY 6.8 phase 2 rebuilds the remset from dirty cards each cycle.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(cards).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn clear_dirty_cards(&mut self) {
        let dirty: Vec<usize> = self
            .cards
            .iter()
            .enumerate()
            .filter(|(_, c)| **c == CARD_DIRTY)
            .map(|(i, _)| i)
            .collect();
        for i in dirty {
            let off = i * CARD_SIZE;
            self.cards[i] = match self.region_of(off).map(|r| self.regions[r].kind) {
                Some(RegionKind::Young) => CARD_YOUNG,
                Some(RegionKind::Old) => CARD_OLD,
                Some(RegionKind::Humongous) => CARD_HUMONGOUS,
                _ => CARD_FREE,
            };
        }
    }

    /// Validates a reference offset and returns it.
    // CEP:WHAT Checks that an offset lies inside the arena.
    // CEP:WHY INV-005: every reference is validated before dereferencing.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::BadReference outside the window.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    #[inline]
    pub fn validate_ref(&self, offset: u64) -> RtResult<usize> {
        let o = offset as usize;
        if o == 0 || o + HEADER_SIZE > self.data.len() {
            return Err(Trap::BadReference { offset });
        }
        Ok(o)
    }

    /// Reads an object's size field.
    // CEP:WHAT Loads the header size of an object.
    // CEP:WHY Evacuation and compaction copy by recorded size.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::BadReference outside the arena.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn object_size(&self, offset: usize) -> RtResult<usize> {
        if offset + HEADER_SIZE > self.data.len() {
            return Err(Trap::BadReference {
                offset: offset as u64,
            });
        }
        let b = &self.data[offset..offset + HEADER_SIZE];
        Ok(u32::from_le_bytes([b[16], b[17], b[18], b[19]]) as usize)
    }

    /// Writes an object's size field.
    // CEP:WHAT Stores the header size of an object.
    // CEP:WHY Allocation stamps the size for the collector.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::BadReference outside the arena.
    // CEP:ASSUMES offset + HEADER_SIZE <= arena.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn set_object_size(&mut self, offset: usize, size: u32) -> RtResult<()> {
        if offset + HEADER_SIZE > self.data.len() {
            return Err(Trap::BadReference {
                offset: offset as u64,
            });
        }
        self.data[offset + 16..offset + 20].copy_from_slice(&size.to_le_bytes());
        Ok(())
    }

    /// Reads an object's flags byte.
    // CEP:WHAT Loads the header flags of an object.
    // CEP:WHY Forwarding checks read FLAG_FORWARDED.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::BadReference outside the arena.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn object_flags(&self, offset: usize) -> RtResult<u8> {
        if offset + HEADER_SIZE > self.data.len() {
            return Err(Trap::BadReference {
                offset: offset as u64,
            });
        }
        Ok(self.data[offset + 20])
    }

    /// Writes an object's flags byte.
    // CEP:WHAT Stores the header flags of an object.
    // CEP:WHY Forwarding installation sets FLAG_FORWARDED.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::BadReference outside the arena.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn set_object_flags(&mut self, offset: usize, flags: u8) -> RtResult<()> {
        if offset + HEADER_SIZE > self.data.len() {
            return Err(Trap::BadReference {
                offset: offset as u64,
            });
        }
        self.data[offset + 20] = flags;
        Ok(())
    }

    /// Reads the mark word.
    // CEP:WHAT Loads the mark word of an object.
    // CEP:WHY The forwarding pointer lives in the mark word.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::BadReference outside the arena.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn mark_word(&self, offset: usize) -> RtResult<u64> {
        if offset + HEADER_SIZE > self.data.len() {
            return Err(Trap::BadReference {
                offset: offset as u64,
            });
        }
        let b = &self.data[offset..offset + 8];
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    /// Writes the mark word.
    // CEP:WHAT Stores the mark word of an object.
    // CEP:WHY Age tracking and forwarding pointers live here.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::BadReference outside the arena.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn set_mark_word(&mut self, offset: usize, word: u64) -> RtResult<()> {
        if offset + HEADER_SIZE > self.data.len() {
            return Err(Trap::BadReference {
                offset: offset as u64,
            });
        }
        self.data[offset..offset + 8].copy_from_slice(&word.to_le_bytes());
        Ok(())
    }

    /// Reads a u64 field inside an object body.
    // CEP:WHAT Loads one reference slot from an object body.
    // CEP:WHY Marking and barriers scan reference slots.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::BadReference outside the arena.
    // CEP:ASSUMES slot_offset + 8 <= arena.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn read_slot(&self, slot_offset: usize) -> RtResult<u64> {
        if slot_offset + 8 > self.data.len() {
            return Err(Trap::BadReference {
                offset: slot_offset as u64,
            });
        }
        let b = &self.data[slot_offset..slot_offset + 8];
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    /// Writes a u64 field inside an object body.
    // CEP:WHAT Stores one reference slot into an object body.
    // CEP:WHY Field stores are the mutator's only heap mutation.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::BadReference outside the arena.
    // CEP:ASSUMES slot_offset + 8 <= arena.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn write_slot(&mut self, slot_offset: usize, value: u64) -> RtResult<()> {
        if slot_offset + 8 > self.data.len() {
            return Err(Trap::BadReference {
                offset: slot_offset as u64,
            });
        }
        self.data[slot_offset..slot_offset + 8].copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    /// Copies raw bytes between arena locations.
    // CEP:WHAT Copies an object body during evacuation or compaction.
    // CEP:WHY Evacuation and the Lisp-2 compaction move object bytes.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::BadReference when either end leaves the arena.
    // CEP:ASSUMES Ranges are disjoint or self-forwarding is handled by the
    //        caller.
    // CEP:COST O(bytes).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn copy_range(&mut self, from: usize, to: usize, len: usize) -> RtResult<()> {
        if from + len > self.data.len() || to + len > self.data.len() {
            return Err(Trap::BadReference { offset: to as u64 });
        }
        let bytes: Vec<u8> = self.data[from..from + len].to_vec();
        self.data[to..to + len].copy_from_slice(&bytes);
        Ok(())
    }

    /// Requests a young region for a new TLAB; Err when the heap is full.
    // CEP:WHAT Hands out a free region as a young TLAB region.
    // CEP:WHY TLAB exhaustion triggers either a new region or collection.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::OutOfMemory when no region is free and the heap
    //        cannot grow.
    // CEP:ASSUMES None.
    // CEP:COST O(1) amortized.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    // CEP:ID: MEM-a11
    pub fn take_young_region(&mut self, max_regions: usize) -> RtResult<usize> {
        if let Some(idx) = self.free.pop() {
            self.regions[idx].kind = RegionKind::Young;
            self.regions[idx].top = 0;
            let base = self.regions[idx].start / CARD_SIZE;
            for c in base..base + self.region_size / CARD_SIZE {
                if c < self.cards.len() {
                    self.cards[c] = CARD_YOUNG;
                }
            }
            return Ok(idx);
        }
        if self.regions.len() < max_regions {
            let start = self.data.len();
            let idx = self.regions.len();
            self.data.resize(start + self.region_size, 0);
            let cards_needed = self.region_size / CARD_SIZE;
            self.cards
                .resize(self.cards.len() + cards_needed, CARD_YOUNG);
            self.regions.push(Region {
                start,
                size: self.region_size,
                kind: RegionKind::Young,
                top: 0,
            });
            return Ok(idx);
        }
        Err(Trap::OutOfMemory)
    }

    /// Allocates a humongous run of contiguous free regions.
    // CEP:WHAT Allocates multi-region objects larger than half a region.
    // CEP:WHY GC 6.4: large objects bypass TLABs entirely.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::OutOfMemory when no contiguous run exists.
    // CEP:ASSUMES None.
    // CEP:COST O(free regions).
    // CEP:ID: MEM-a12
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn allocate_humongous(&mut self, size: usize, max_regions: usize) -> RtResult<usize> {
        let needed = size.div_ceil(self.region_size);
        self.free.sort();
        let mut run: Vec<usize> = Vec::new();
        for &idx in self.free.iter() {
            if run.last().is_some_and(|&last| last + 1 == idx) || run.is_empty() {
                run.push(idx);
                if run.len() == needed {
                    break;
                }
            } else {
                run.clear();
                run.push(idx);
            }
        }
        if run.len() < needed && self.regions.len() + needed <= max_regions {
            let start = self.data.len();
            for _ in 0..needed {
                let idx = self.regions.len();
                self.data.resize(self.data.len() + self.region_size, 0);
                self.cards.resize(
                    self.cards.len() + self.region_size / CARD_SIZE,
                    CARD_HUMONGOUS,
                );
                self.regions.push(Region {
                    start: self.data.len() - self.region_size,
                    size: self.region_size,
                    kind: RegionKind::Humongous,
                    top: 0,
                });
                run.push(idx);
            }
            let _ = start;
            return Ok(self.regions[run[0]].start);
        }
        if run.len() < needed {
            return Err(Trap::OutOfMemory);
        }
        for &idx in &run {
            self.regions[idx].kind = RegionKind::Humongous;
            self.regions[idx].top = 0;
        }
        self.free.retain(|i| !run.contains(i));
        Ok(self.regions[run[0]].start)
    }

    /// Returns a region to the free list and zeroes its card entries.
    // CEP:WHAT Releases a region after a collection phase.
    // CEP:WHY 6.8 phase 5 and 6.9 phase 5 release evacuated regions.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES The region no longer holds live objects.
    // CEP:COST O(region cards).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn release_region(&mut self, idx: usize) {
        let (start, size) = {
            let r = &self.regions[idx];
            (r.start, r.size)
        };
        self.regions[idx].kind = RegionKind::Free;
        self.regions[idx].top = 0;
        for c in start / CARD_SIZE..(start + size) / CARD_SIZE {
            if c < self.cards.len() {
                self.cards[c] = CARD_FREE;
            }
        }
        self.free.push(idx);
    }

    /// Reads an object's age byte (survived young collections).
    // CEP:WHAT Loads the header age of an object.
    // CEP:WHY Promotion compares the age against the threshold.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::BadReference outside the arena.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn object_age(&self, offset: usize) -> RtResult<u8> {
        if offset + HEADER_SIZE > self.data.len() {
            return Err(Trap::BadReference {
                offset: offset as u64,
            });
        }
        Ok(self.data[offset + 21])
    }

    /// Writes an object's age byte.
    // CEP:WHAT Stores the header age of an object.
    // CEP:WHY Evacuation increments the age of survivors.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::BadReference outside the arena.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn set_object_age(&mut self, offset: usize, age: u8) -> RtResult<()> {
        if offset + HEADER_SIZE > self.data.len() {
            return Err(Trap::BadReference {
                offset: offset as u64,
            });
        }
        self.data[offset + 21] = age;
        Ok(())
    }

    /// Reads the klass word.
    // CEP:WHAT Loads the type-metadata word of an object.
    // CEP:WHY The interpreter and verifier read object types.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::BadReference outside the arena.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn klass_word(&self, offset: usize) -> RtResult<u64> {
        if offset + HEADER_SIZE > self.data.len() {
            return Err(Trap::BadReference {
                offset: offset as u64,
            });
        }
        let b = &self.data[offset + 8..offset + 16];
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    /// Writes the klass word.
    // CEP:WHAT Stores the type-metadata word of an object.
    // CEP:WHY Allocation stamps the type at object creation.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::BadReference outside the arena.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn set_klass_word(&mut self, offset: usize, klass: u64) -> RtResult<()> {
        if offset + HEADER_SIZE > self.data.len() {
            return Err(Trap::BadReference {
                offset: offset as u64,
            });
        }
        self.data[offset + 8..offset + 16].copy_from_slice(&klass.to_le_bytes());
        Ok(())
    }

    /// Regions and their kinds, for tests and diagnostics.
    // CEP:WHAT Enumerates region kinds.
    // CEP:WHY Tests assert generation structure after collections.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(regions).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn region_kinds(&self) -> Vec<(usize, RegionKind)> {
        self.regions
            .iter()
            .enumerate()
            .map(|(i, r)| (i, r.kind))
            .collect()
    }

    /// Total bytes currently usable for allocation.
    // CEP:WHAT Sums the free tails of all regions.
    // CEP:WHY Expansion policy compares occupancy against 2x live data.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(regions).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn free_bytes(&self) -> usize {
        self.regions
            .iter()
            .filter(|r| r.kind != RegionKind::Free)
            .map(|r| r.size.saturating_sub(r.top))
            .sum::<usize>()
            + self.free.len() * self.region_size
    }

    /// Region geometry: (start, size).
    // CEP:WHAT Returns a region's span.
    // CEP:WHY Collector passes sweep region spans.
    // CEP:STATUS complete
    // CEP:FAILURE None for out-of-range regions.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn region_bounds(&self, idx: usize) -> Option<(usize, usize)> {
        self.regions.get(idx).map(|r| (r.start, r.size))
    }

    /// Bytes used in a region (bump top within the span).
    // CEP:WHAT Returns a region's used byte count.
    // CEP:WHY Compaction sweeps the used span only.
    // CEP:STATUS complete
    // CEP:FAILURE None for out-of-range regions.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn region_used(&self, idx: usize) -> Option<usize> {
        self.regions.get(idx).map(|r| r.top)
    }

    /// Sets a region's used byte count (allocation cursor).
    // CEP:WHAT Advances or rewinds a region's bump top.
    // CEP:WHY Promotion and compaction move the allocation cursor.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; out-of-range regions are ignored and the
    //        cursor is clamped to the region size.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn set_region_top(&mut self, idx: usize, used: usize) {
        if let Some(r) = self.regions.get_mut(idx) {
            r.top = used.min(r.size);
        }
    }

    /// Converts a region to old generation and recolors its cards.
    // CEP:WHAT Promotes a whole region to the old generation.
    // CEP:WHY Promotion space comes from regions converted to old.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(region cards).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn promote_region(&mut self, idx: usize) {
        let (start, size) = match self.region_bounds(idx) {
            Some(b) => b,
            None => return,
        };
        self.regions[idx].kind = RegionKind::Old;
        for c in start / CARD_SIZE..(start + size) / CARD_SIZE {
            if c < self.cards.len() {
                self.cards[c] = CARD_OLD;
            }
        }
    }

    /// The kind of the region containing an offset.
    // CEP:WHAT Resolves a generation for an offset.
    // CEP:WHY Barriers branch on source and target generation.
    // CEP:STATUS complete
    // CEP:FAILURE None outside the arena.
    // CEP:ASSUMES None.
    // CEP:COST O(regions).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn region_kind_of(&self, offset: usize) -> Option<RegionKind> {
        self.region_of(offset)
            .and_then(|idx| self.regions.get(idx).map(|r| r.kind))
    }

    /// A fresh TLAB carved from a young region.
    // CEP:WHAT Splits the region tail into a thread-local buffer.
    // CEP:WHY 6.4: each mutator thread bumps inside its own buffer.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::OutOfMemory when the region is exhausted.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn take_tlab(&mut self, region: usize, want: usize) -> RtResult<Tlab> {
        let r = &mut self.regions[region];
        let take = want.min(r.remaining());
        if take < HEADER_SIZE {
            return Err(Trap::OutOfMemory);
        }
        let tlab = Tlab {
            region,
            top: r.start + r.top,
            end: r.start + r.top + take,
        };
        r.top += take;
        Ok(tlab)
    }
}
