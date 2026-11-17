// CEP:FILE crates/vortex-runtime/src/barriers.rs
// CEP:WHAT GC barrier data structures: pointer colors, mark bitmap, SATB
//        queue, and the bounded forwarding cache (design GC 6.3-6.10 and
//        32-bit 6.3-6.5).
// CEP:WHY The Collector's barrier algorithms operate on these structures;
//        isolating them keeps each file within the atomic-file limit while
//        preserving one concept per file.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Structures cannot fail; callers report through the collector.
// CEP:ASSUMES 64-bit words; granule and cache sizes are named constants.
// CEP:COST O(1) per operation except bitmap init O(arena/granule).
// CEP:EVIDENCE cargo test -p vortex-runtime
//! GC barrier data structures: colors, mark bitmap, SATB queue, forwarding.

#![warn(missing_docs)]
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
