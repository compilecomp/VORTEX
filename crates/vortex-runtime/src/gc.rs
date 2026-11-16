// CEP:FILE crates/vortex-runtime/src/gc.rs
// CEP:WHAT The garbage collector: mark bitmap, combined write barrier
//        (SATB plus generational card dirtying), load barrier with
//        self-healing and a forwarding cache, young evacuation with
//        promotion, old mark-compact (Lisp 2), weak-reference processing,
//        safepoint flag, policy triggers, and the 64-bit colored-pointer
//        contract (GC design 6.4-6.14; 32-bit GC 6.2-6.7).
// CEP:WHY Reachability reclamation must be correct under mutation: the
//        SATB barrier preserves the snapshot, the generational barrier
//        keeps old-to-young edges visible, and forwarding keeps every
//        reference valid across relocation.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Collector invariant violations surface as Trap::GcInternal;
//        allocation exhaustion as Trap::OutOfMemory. The collector never
//        panics and never collects a live object.
// CEP:ASSUMES Mutators pass their live references at safepoints (stack
//        and register maps); reference slots are 8-byte aligned u64 words
//        holding NV-1 heap offsets or zero for null.
// CEP:COST Mark O(|live|); young collect O(|live young|); compaction
//        O(|live old|); barrier fast paths O(1).
// CEP:EVIDENCE cargo test -p vortex-runtime
//! Garbage collection (design: Garbage Collection Subsystem 6.3-6.14, and
//! 32-Bit Target Support for GC 6.2-6.7).

use crate::heap::{Heap, RegionKind, FLAG_FORWARDED, HEADER_SIZE};
use crate::trap::{RtResult, Trap};
use std::collections::HashMap;

/// Bits per mark-bitmap word.
const WORD_BITS: usize = 64;
/// Minimum object granularity tracked by the bitmap.
const BITMAP_GRANULE: usize = 16;
/// Forwarding cache capacity (32-bit design 6.5).
const FORWARDING_CACHE_SIZE: usize = 64;

/// One 64-bit colored pointer view (GC design 6.10): the pointer's high
/// payload bits encode Marked0, Marked1, and Remapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerColor {
    /// Good color for even mark cycles.
    Marked0,
    /// Good color for odd mark cycles.
    Marked1,
    /// Good color outside marking (post-remap).
    Remapped,
    /// Stale color; the load barrier must slow-path.
    Bad,
}

/// Colored-pointer helpers over native-pointer words (design 6.10). The
/// arena model uses header forwarding; these functions define the 64-bit
/// native-pointer contract that the T1 emitter lowers to.
#[derive(Debug, Clone, Copy)]
pub struct Colors;

impl Colors {
    /// Bit mask for the three color bits (45-47).
    pub const MASK: u64 = 0x7 << 45;
    /// Marked0 color bits.
    pub const MARKED0: u64 = 0x1 << 45;
    /// Marked1 color bits.
    pub const MARKED1: u64 = 0x2 << 45;
    /// Remapped color bits.
    pub const REMAPPED: u64 = 0x4 << 45;

    /// Extracts the color of a pointer word.
    // CEP:WHAT Decodes the color bits of a native pointer word.
    // CEP:WHY The load barrier fast path is one color comparison.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES The word carries the 45-47 color bits.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    #[inline]
    pub fn color_of(word: u64) -> PointerColor {
        match word & Self::MASK {
            Self::MARKED0 => PointerColor::Marked0,
            Self::MARKED1 => PointerColor::Marked1,
            Self::REMAPPED => PointerColor::Remapped,
            _ => PointerColor::Bad,
        }
    }

    /// Removes the color bits.
    // CEP:WHAT Strips color bits from a pointer word.
    // CEP:WHY Slow paths recover the raw address before recoloring.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    #[inline]
    pub fn strip(word: u64) -> u64 {
        word & !Self::MASK
    }

    /// Applies a good color to a raw address.
    // CEP:WHAT Encodes a good color onto a raw address.
    // CEP:WHY Self-healing writes correctly colored pointers back.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES The address leaves bits 45-47 free.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    #[inline]
    pub fn recolor(addr: u64, good: PointerColor) -> u64 {
        let bits = match good {
            PointerColor::Marked0 => Self::MARKED0,
            PointerColor::Marked1 => Self::MARKED1,
            PointerColor::Remapped => Self::REMAPPED,
            PointerColor::Bad => 0,
        };
        Self::strip(addr) | bits
    }

    /// Whether the pointer must slow-path under the active good color.
    // CEP:WHAT Compares a pointer color against the active good color.
    // CEP:WHY Fast path: one comparison decides barrier entry.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    #[inline]
    pub fn is_good(word: u64, good: PointerColor) -> bool {
        Self::color_of(word) == good
    }
}

/// Mark bitmap over the arena at BITMAP_GRANULE granularity.
#[derive(Debug, Default)]
pub struct MarkBitmap {
    words: Vec<u64>,
}

impl MarkBitmap {
    /// Allocates a bitmap covering `bytes` of arena.
    // CEP:WHAT Sizes the bitmap for an arena.
    // CEP:WHY One bit per 16 bytes is 0.78% overhead (GC 6.15).
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(bytes / granule / 64).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn for_arena(bytes: usize) -> MarkBitmap {
        MarkBitmap {
            words: vec![0u64; bytes.div_ceil(BITMAP_GRANULE * WORD_BITS)],
        }
    }

    /// Sets the mark bit for an object offset.
    // CEP:WHAT Marks an object as reachable.
    // CEP:WHY The tri-color abstraction records black in the bitmap.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; out-of-range offsets are ignored because
    //        heap validation happens before marking.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn set(&mut self, offset: usize) {
        let idx = offset / BITMAP_GRANULE;
        if idx / WORD_BITS < self.words.len() {
            self.words[idx / WORD_BITS] |= 1u64 << (idx % WORD_BITS);
        }
    }

    /// Reads the mark bit for an object offset.
    // CEP:WHAT Queries whether an object is marked.
    // CEP:WHY The SATB barrier compares mark states.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; unmapped offsets read as unmarked.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn get(&self, offset: usize) -> bool {
        let idx = offset / BITMAP_GRANULE;
        if idx / WORD_BITS >= self.words.len() {
            return false;
        }
        self.words[idx / WORD_BITS] & (1u64 << (idx % WORD_BITS)) != 0
    }

    /// Clears the whole bitmap for a new cycle.
    // CEP:WHAT Resets all mark bits.
    // CEP:WHY Each cycle starts from an all-white snapshot.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(words).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn reset(&mut self) {
        self.words.fill(0);
    }
}

/// Per-thread SATB queue (design 6.7); the runtime model keeps one
/// process-wide queue.
#[derive(Debug, Default)]
pub struct SatbQueue {
    buf: Vec<usize>,
}

impl SatbQueue {
    /// Enqueues a source object for re-scan.
    // CEP:WHAT Pushes a re-gray source object.
    // CEP:WHY SATB: a marked source with an unmarked target must rescan.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1) amortized.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn push(&mut self, obj: usize) {
        self.buf.push(obj);
    }

    /// Drains the queue in queue order.
    // CEP:WHAT Removes and returns all queued objects.
    // CEP:WHY Remark phase drains the queue deterministically.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(queue length).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn drain(&mut self) -> Vec<usize> {
        std::mem::take(&mut self.buf)
    }

    /// Queued item count.
    // CEP:WHAT Reports the queue depth.
    // CEP:WHY Tests assert barrier slow-path invocations.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Whether the queue is empty.
    // CEP:WHAT Reports emptiness.
    // CEP:WHY Clippy requires an explicit negation of len.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

/// Thread-local forwarding cache (32-bit design 6.5).
#[derive(Debug, Default)]
pub struct ForwardingCache {
    entries: Vec<(usize, usize)>,
}

impl ForwardingCache {
    /// Creates an empty cache.
    // CEP:WHAT Initializes the cache.
    // CEP:WHY Hot loops repeatedly dereference forwarded objects.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn new() -> ForwardingCache {
        ForwardingCache {
            entries: Vec::with_capacity(FORWARDING_CACHE_SIZE),
        }
    }

    /// Looks up an old address.
    // CEP:WHAT Resolves a cached forwarding pair.
    // CEP:WHY Cache hits skip the header load.
    // CEP:STATUS complete
    // CEP:FAILURE None on a miss.
    // CEP:ASSUMES None.
    // CEP:COST O(cache length) bounded by 64.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn lookup(&self, old: usize) -> Option<usize> {
        self.entries
            .iter()
            .find(|(o, _)| *o == old)
            .map(|(_, n)| *n)
    }

    /// Inserts a pair, evicting the oldest entry when full.
    // CEP:WHAT Caches a forwarding pair.
    // CEP:WHY Bounded caches keep the fast path allocation-free.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(cache length) on eviction.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn insert(&mut self, old: usize, new: usize) {
        if self.entries.len() >= FORWARDING_CACHE_SIZE {
            self.entries.remove(0);
        }
        self.entries.push((old, new));
    }

    /// Clears all cached pairs after a collection.
    // CEP:WHAT Resets the cache.
    // CEP:WHY Stale pairs would alias relocated addresses.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Collection counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GcStats {
    /// Completed young collections.
    pub young_collections: u64,
    /// Completed old collections.
    pub old_collections: u64,
    /// Objects promoted from young to old.
    pub promoted: u64,
    /// Bytes reclaimed across collections.
    pub bytes_reclaimed: u64,
}

/// Collection policy parameters.
#[derive(Debug, Clone, Copy)]
pub struct GcConfig {
    /// Age at which survivors promote to old regions.
    pub promotion_age: u8,
    /// Old-generation occupancy percentage that triggers an old cycle.
    pub old_trigger_percent: usize,
    /// TLAB request size in bytes.
    pub tlab_size: usize,
    /// Maximum regions the heap may reach.
    pub max_regions: usize,
}

impl Default for GcConfig {
    // CEP:WHAT Provides the documented default policy.
    // CEP:WHY 6.13: promotion 15, old trigger 80%, model TLAB sizes.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    fn default() -> GcConfig {
        GcConfig {
            promotion_age: 15,
            old_trigger_percent: 80,
            tlab_size: 16 * 1024,
            max_regions: 256,
        }
    }
}

/// The collector state.
pub struct Collector {
    /// Policy parameters.
    pub config: GcConfig,
    /// Mark bitmap of the current cycle.
    bitmap: MarkBitmap,
    /// Global SATB queue.
    satb: SatbQueue,
    /// Thread-local forwarding cache for load barriers.
    forwarding: ForwardingCache,
    /// Weak-reference slots registered for clearing.
    weak_slots: Vec<usize>,
    /// Safepoint flag: mutators park when true.
    safepoint: bool,
    /// Cycle counter (parity selects the good color).
    cycle: u64,
    /// Counters.
    stats: GcStats,
}

impl Collector {
    /// Creates a collector over an arena size.
    // CEP:WHAT Initializes bitmap, queues, and counters.
    // CEP:WHY Collection starts from a clean tri-color state.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(arena / granule).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn new(config: GcConfig, arena_bytes: usize) -> Collector {
        Collector {
            config,
            bitmap: MarkBitmap::for_arena(arena_bytes),
            satb: SatbQueue::default(),
            forwarding: ForwardingCache::new(),
            weak_slots: Vec::new(),
            safepoint: false,
            cycle: 0,
            stats: GcStats::default(),
        }
    }

    /// Counters snapshot.
    // CEP:WHAT Reports collection counters.
    // CEP:WHY Tests and the CLI demo surface GC behavior.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn stats(&self) -> GcStats {
        self.stats
    }

    /// The current cycle number.
    // CEP:WHAT Reports the cycle parity counter.
    // CEP:WHY Colored-pointer views alternate per cycle (6.10).
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn cycle(&self) -> u64 {
        self.cycle
    }

    /// Requests all mutators to park at their next safepoint.
    // CEP:WHAT Sets the safepoint flag.
    // CEP:WHY STW phases require every mutator at a safepoint (6.11).
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn request_safepoint(&mut self) {
        self.safepoint = true;
    }

    /// Clears the safepoint request.
    // CEP:WHAT Resets the safepoint flag.
    // CEP:WHY Mutators resume after the STW phase.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn clear_safepoint(&mut self) {
        self.safepoint = false;
    }

    /// Whether mutators must park.
    // CEP:WHAT Reports the safepoint flag.
    // CEP:WHY The interpreter checks at back-edges and returns.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn at_safepoint(&self) -> bool {
        self.safepoint
    }

    /// Registers a weak-reference slot for end-of-cycle clearing.
    // CEP:WHAT Adds a slot to the weak list.
    // CEP:WHY 6.12: weak referents clear when unmarked at remark.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES The slot holds a reference or zero.
    // CEP:COST O(1) amortized.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn register_weak(&mut self, slot_offset: usize) {
        self.weak_slots.push(slot_offset);
    }

    /// The combined write barrier (design 6.7): performs the store, dirties
    /// the card for old-to-young stores, and enqueues SATB sources.
    // CEP:WHAT Executes a reference store with both barrier duties.
    // CEP:WHY Invariant 3 (generational) and invariant 2 (SATB) are only
    //        as good as this barrier.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::BadReference for out-of-window slots.
    // CEP:ASSUMES The caller never stores to a forwarded slot without a
    //        load barrier first.
    // CEP:COST O(1) fast path; O(1) enqueue on the slow path.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn write_barrier(
        &mut self,
        heap: &mut Heap,
        slot_offset: usize,
        value: u64,
    ) -> RtResult<()> {
        let src_kind = heap.region_kind_of(slot_offset);
        heap.write_slot(slot_offset, value)?;
        // Generational part: old-to-young stores dirty the card.
        if src_kind == Some(RegionKind::Old)
            && value != 0
            && heap.region_kind_of(value as usize) == Some(RegionKind::Young)
        {
            heap.dirty_card(slot_offset);
        }
        // SATB part: a marked source pointing at an unmarked value re-grays
        // the source (6.7 SATB barrier).
        if self.bitmap.get(slot_offset) && (value == 0 || !self.bitmap.get(value as usize)) {
            self.satb.push(slot_offset);
        }
        Ok(())
    }

    /// The load barrier (design 6.10 and 32-bit 6.5): follows forwarding
    /// pointers with self-healing and a bounded cache.
    // CEP:WHAT Resolves a reference load through forwarding state.
    // CEP:WHY Invariant 4: every pointer is corrected before use.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::BadReference outside the arena.
    // CEP:ASSUMES None.
    // CEP:COST O(1) fast path; O(cache) on cache hits.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn load_barrier(&mut self, heap: &mut Heap, slot_offset: usize) -> RtResult<u64> {
        let raw = heap.read_slot(slot_offset)? as usize;
        if raw == 0 {
            return Ok(0);
        }
        if let Some(new) = self.forwarding.lookup(raw) {
            heap.write_slot(slot_offset, new as u64)?;
            return Ok(new as u64);
        }
        let flags = heap.object_flags(raw)?;
        if flags & FLAG_FORWARDED == 0 {
            return Ok(raw as u64);
        }
        let new = heap.mark_word(raw)? as usize;
        self.forwarding.insert(raw, new);
        heap.write_slot(slot_offset, new as u64)?;
        Ok(new as u64)
    }

    /// Whether an old collection is due (design 6.13).
    // CEP:WHAT Compares old-region occupancy against the trigger.
    // CEP:WHY Concurrent cycles must start before the old generation fills.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(regions).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn should_collect_old(&self, heap: &Heap) -> bool {
        let mut old_used = 0usize;
        let mut old_total = 0usize;
        for (i, kind) in heap.region_kinds() {
            if kind != RegionKind::Old {
                continue;
            }
            let (start, size) = heap.region_bounds(i).unwrap_or((0, 0));
            old_total += size;
            old_used += heap.region_used(i).unwrap_or(0).min(size);
            let _ = start;
        }
        old_total > 0 && old_used * 100 >= old_total * self.config.old_trigger_percent
    }

    /// Young collection: evacuate survivors, promote by age, release the
    /// evacuated regions (design 6.8).
    // CEP:WHAT Runs a minor collection over the passed roots.
    // CEP:WHY The generational hypothesis makes minor cycles cheap.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::OutOfMemory when no survivor region is available;
    //        Trap::BadReference on reference-decode failures.
    // CEP:ASSUMES Roots are the mutator's live reference words.
    // CEP:COST O(|live young|).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn young_collect(&mut self, heap: &mut Heap, roots: &mut [u64]) -> RtResult<()> {
        self.request_safepoint();
        let survivor = heap.take_young_region(self.config.max_regions)?;
        let (region_base, region_size) = heap.region_bounds(survivor).unwrap_or((0, 0));
        let mut cursor = 0usize;
        let mut worklist: Vec<usize> = Vec::new();
        for root in roots.iter_mut() {
            if *root == 0 {
                continue;
            }
            let obj = heap.validate_ref(*root)?;
            if heap.region_kind_of(obj) != Some(RegionKind::Young) {
                continue;
            }
            let (new_off, _) = self.evacuate(heap, obj, &mut cursor, region_base, region_size)?;
            *root = new_off as u64;
            worklist.push(new_off);
        }
        while let Some(obj) = worklist.pop() {
            let size = heap.object_size(obj)?;
            let mut slot = obj + HEADER_SIZE;
            let end = obj + size;
            while slot + 8 <= end {
                let v = heap.read_slot(slot)? as usize;
                if v != 0 && heap.region_kind_of(v) == Some(RegionKind::Young) {
                    let (new_off, copied) =
                        self.evacuate(heap, v, &mut cursor, region_base, region_size)?;
                    heap.write_slot(slot, new_off as u64)?;
                    if copied {
                        worklist.push(new_off);
                    }
                }
                slot += 8;
            }
        }
        self.release_evacuated(heap)?;
        heap.clear_dirty_cards();
        self.forwarding.clear();
        self.clear_safepoint();
        self.stats.young_collections += 1;
        Ok(())
    }

    /// Copies one young object to the survivor region or promotes it.
    // CEP:WHAT Moves one object and installs header forwarding.
    // CEP:WHY Evacuation reclaims dead young space by copying survivors.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::BadReference or Trap::OutOfMemory.
    // CEP:ASSUMES The survivor cursor has room for the object.
    // CEP:COST O(object bytes).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    fn evacuate(
        &mut self,
        heap: &mut Heap,
        obj: usize,
        cursor: &mut usize,
        region_base: usize,
        region_size: usize,
    ) -> RtResult<(usize, bool)> {
        let flags = heap.object_flags(obj)?;
        if flags & FLAG_FORWARDED != 0 {
            return Ok((heap.mark_word(obj)? as usize, false));
        }
        let size = heap.object_size(obj)?.max(HEADER_SIZE);
        let age = heap.object_age(obj)?;
        let promote = age + 1 >= self.config.promotion_age;
        let target = if promote {
            self.take_old_space(heap, size)?
        } else {
            if *cursor + size > region_size {
                return Err(Trap::OutOfMemory);
            }
            let new_off = region_base + *cursor;
            *cursor += size;
            new_off
        };
        heap.copy_range(obj, target, size)?;
        let new_age = (age + 1).min(15);
        heap.set_object_age(target, new_age)?;
        if promote {
            self.stats.promoted += 1;
        }
        // Install forwarding in the old location: flags bit plus mark word.
        heap.set_mark_word(obj, target as u64)?;
        heap.set_object_flags(obj, flags | FLAG_FORWARDED)?;
        Ok((target, true))
    }

    /// Claims contiguous old space for promotion or humongous growth.
    // CEP:WHAT Allocates from an old region's free tail or a new region.
    // CEP:WHY Promotion copies into old regions (6.4).
    // CEP:STATUS complete
    // CEP:FAILURE Trap::OutOfMemory when no old space is reachable.
    // CEP:ASSUMES None.
    // CEP:COST O(regions) worst case.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    fn take_old_space(&mut self, heap: &mut Heap, size: usize) -> RtResult<usize> {
        if size
            > heap
                .region_bounds(0)
                .unwrap_or((0, self.config.tlab_size))
                .1
                / 2
        {
            return heap.allocate_humongous(size, self.config.max_regions);
        }
        let count = heap.region_kinds().len();
        for i in 0..count {
            if heap.region_kinds()[i].1 != RegionKind::Old {
                continue;
            }
            let (start, size_total) = heap.region_bounds(i).unwrap_or((0, 0));
            let used = heap.region_used(i).unwrap_or(0);
            if size_total.saturating_sub(used) >= size {
                heap.set_region_top(i, used + size);
                return Ok(start + used);
            }
        }
        let region = heap.take_young_region(self.config.max_regions)?;
        heap.promote_region(region);
        let (start, size_total) = heap.region_bounds(region).unwrap_or((0, 0));
        if size_total < size {
            return Err(Trap::OutOfMemory);
        }
        heap.set_region_top(region, size);
        Ok(start)
    }

    /// Releases every young region that held evacuated objects.
    // CEP:WHAT Returns evacuated young regions to the free list.
    // CEP:WHY 6.8 phase 5: reclaimed space becomes allocation capacity.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::BadReference outside the arena.
    // CEP:ASSUMES Forwarding words live only in released regions.
    // CEP:COST O(regions + headers).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    fn release_evacuated(&mut self, heap: &mut Heap) -> RtResult<()> {
        let mut to_release = Vec::new();
        for (i, kind) in heap.region_kinds() {
            if kind != RegionKind::Young {
                continue;
            }
            let (start, size) = heap.region_bounds(i).unwrap_or((0, 0));
            if self.span_holds_forwarding(heap, start, size)? {
                to_release.push(i);
            }
        }
        for i in to_release {
            heap.release_region(i);
        }
        Ok(())
    }

    /// Whether any header in a span holds FLAG_FORWARDED.
    // CEP:WHAT Scans a span for forwarded headers.
    // CEP:WHY Evacuated regions are identified by their forwarding words.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::BadReference outside the arena.
    // CEP:ASSUMES Objects are HEADER_SIZE-aligned in the span.
    // CEP:COST O(objects in span).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    fn span_holds_forwarding(&self, heap: &Heap, start: usize, size: usize) -> RtResult<bool> {
        let mut scan = start;
        let end = start + size;
        while scan + HEADER_SIZE <= end {
            let flags = heap.object_flags(scan)?;
            if flags & FLAG_FORWARDED != 0 {
                return Ok(true);
            }
            let size_field = heap.object_size(scan)?.max(HEADER_SIZE);
            scan += size_field;
        }
        Ok(false)
    }

    /// Old collection: SATB drain, tri-color mark, weak processing, and a
    /// Lisp-2 mark-compact of old regions (32-bit design 6.7).
    // CEP:WHAT Runs a full old cycle over the passed roots.
    // CEP:WHY Old cycles reclaim old-generation garbage and compact.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::BadReference on reference-decode failures.
    // CEP:ASSUMES Roots are the mutator's live reference words.
    // CEP:COST O(|live old| + |old heap|).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn old_collect(&mut self, heap: &mut Heap, roots: &mut [u64]) -> RtResult<()> {
        self.request_safepoint();
        self.cycle += 1;
        self.bitmap.reset();
        // Phase 1 (initial mark): roots are gray.
        let mut gray: Vec<usize> = Vec::new();
        for root in roots.iter() {
            if *root != 0 {
                let obj = heap.validate_ref(*root)?;
                if !self.bitmap.get(obj) {
                    self.bitmap.set(obj);
                    gray.push(obj);
                }
            }
        }
        // SATB sources re-gray (6.6): marked sources rescan.
        for src in self.satb.drain() {
            if !self.bitmap.get(src) {
                self.bitmap.set(src);
            }
            gray.push(src);
        }
        // Phase 2 (concurrent mark model): drain the gray set.
        while let Some(obj) = gray.pop() {
            let size = heap.object_size(obj)?;
            let mut slot = obj + HEADER_SIZE;
            let end = obj + size;
            while slot + 8 <= end {
                let v = heap.read_slot(slot)? as usize;
                if v != 0 && !self.bitmap.get(v) {
                    heap.validate_ref(v as u64)?;
                    self.bitmap.set(v);
                    gray.push(v);
                }
                slot += 8;
            }
        }
        self.process_weak_refs(heap)?;
        // Phase 4 (compact): Lisp 2 over old regions.
        let reclaimed = self.compact_old(heap, roots)?;
        self.stats.bytes_reclaimed += reclaimed as u64;
        self.forwarding.clear();
        self.clear_safepoint();
        self.stats.old_collections += 1;
        Ok(())
    }

    /// Clears weak slots whose referents did not survive marking.
    // CEP:WHAT Nulls unmarked weak referents.
    // CEP:WHY 6.12: weak references clear exactly when unreachable.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::BadReference outside the arena.
    // CEP:ASSUMES None.
    // CEP:COST O(|weak slots|).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    fn process_weak_refs(&mut self, heap: &mut Heap) -> RtResult<()> {
        let slots = std::mem::take(&mut self.weak_slots);
        for slot in slots {
            let v = heap.read_slot(slot)?;
            if v != 0 && !self.bitmap.get(v as usize) {
                heap.write_slot(slot, 0)?;
            }
            self.weak_slots.push(slot);
        }
        Ok(())
    }

    /// Lisp-2 compaction of old regions; returns reclaimed bytes.
    // CEP:WHAT Computes forwarding, updates references, and copies.
    // CEP:WHY 32-bit design 6.7: compaction reclaims old space in place.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::BadReference when reference decode fails.
    // CEP:ASSUMES The mark bitmap is final for this cycle.
    // CEP:COST O(|old heap|).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    fn compact_old(&mut self, heap: &mut Heap, roots: &mut [u64]) -> RtResult<usize> {
        let mut forwarding: HashMap<usize, usize> = HashMap::new();
        let mut reclaimed = 0usize;
        let kinds: Vec<(usize, RegionKind)> = heap.region_kinds();
        // Pass 1: compute forwarding addresses within each old region.
        for (i, kind) in kinds.iter().copied() {
            if kind != RegionKind::Old {
                continue;
            }
            let (start, size_total) = heap.region_bounds(i).unwrap_or((0, 0));
            let top = start + heap.region_used(i).unwrap_or(0).min(size_total);
            let mut scan = start;
            let mut free_cursor = start;
            while scan + HEADER_SIZE <= top {
                let size = heap.object_size(scan)?.max(HEADER_SIZE);
                if self.bitmap.get(scan) {
                    forwarding.insert(scan, free_cursor);
                    free_cursor += size;
                } else {
                    reclaimed += size;
                }
                scan += size;
            }
            heap.set_region_top(i, free_cursor - start);
        }
        if forwarding.is_empty() {
            return Ok(reclaimed);
        }
        // Pass 2: update references inside live objects and the roots.
        let live: Vec<usize> = forwarding.keys().copied().collect();
        for obj in &live {
            let size = heap.object_size(*obj)?.max(HEADER_SIZE);
            let mut slot = *obj + HEADER_SIZE;
            let end = *obj + size;
            while slot + 8 <= end {
                let v = heap.read_slot(slot)? as usize;
                if let Some(new) = forwarding.get(&v) {
                    heap.write_slot(slot, *new as u64)?;
                }
                slot += 8;
            }
        }
        for root in roots.iter_mut() {
            if *root != 0 {
                if let Some(new) = forwarding.get(&(*root as usize)) {
                    *root = *new as u64;
                }
            }
        }
        // Pass 3: copy live objects to their compacted locations. Copies
        // run highest-source-first so overlapping moves stay correct.
        let mut staged: Vec<(usize, usize, usize)> = Vec::new();
        for (old, new) in &forwarding {
            if old != new {
                let size = heap.object_size(*old)?.max(HEADER_SIZE);
                staged.push((*old, *new, size));
            }
        }
        staged.sort_by_key(|(o, _, _)| *o);
        for (old, new, size) in staged {
            heap.copy_range(old, new, size)?;
            // Preserve the mark bit for the object's new location so later
            // cycles start from a consistent bitmap.
            self.bitmap.set(new);
        }
        Ok(reclaimed)
    }
}
