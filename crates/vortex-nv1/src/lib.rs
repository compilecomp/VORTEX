//! NV-1, the 64-bit NaN-boxed VM value model of the VORTEX engine.
//! See design.md (NV-1, BARRIER-1 v1.2, SIMD-1 v1.1) for the specification.
// CEP:FILE crates/vortex-nv1/src/lib.rs
// CEP:WHAT NV-1 value model: 64-bit NaN-boxed words, 16-bit tags (0xFFFC int,
//        0xFFFD reference, 0xFFFE special, 0xFFFF invalid), canonical NaN,
//        48-bit heap-relative reference payloads carrying 3 GC color bits, the
//        BARRIER-1-corrected load-barrier predicate, and a software model of
//        the PACKED_TAGGED lane-arithmetic contract (SIMD-1 v1.1).
// CEP:WHY NV-1 is the foundational representation of the engine (design.md,
//        "NV-1: NaN-Boxed VM Value Representation"); every tier, the GC, and
//        the barriers depend on exact classification and encoding.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Out-of-range integers return None; reserved-space floats are
//        canonicalized; offset-zero references are rejected; invalid-category
//        words are reported as Category::Invalid, never dereferenced.
// CEP:ASSUMES IEEE-754 binary64; heap entities are 8-byte aligned so the low
//        three reference payload bits are free for GC colors (HEAP-1).
// CEP:COST O(1) per operation, no allocation, branch-light hot predicates.
// CEP:EVIDENCE cargo test -p vortex-nv1 (incl. the BARRIER-1 regression test)
use core::f64;

/// Bit offset of the 16-bit tag field (bits 63-48).
pub const TAG_SHIFT: u32 = 48;
/// Mask selecting the 16-bit tag field.
pub const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
/// Mask selecting the 48-bit payload field.
pub const PAYLOAD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
/// Mask of the reserved tag region test: `(w & RESERVED_REGION_MASK) == RESERVED_REGION_MASK`.
pub const RESERVED_REGION_MASK: u64 = 0xFFFC_0000_0000_0000;

/// Tag: immediate signed 48-bit integer.
pub const TAG_INT: u64 = 0xFFFC;
/// Tag: heap reference (`0xFFFD`).
pub const TAG_REF: u64 = 0xFFFD;
/// Tag: special immediate (`0xFFFE`).
pub const TAG_SPECIAL: u64 = 0xFFFE;
/// Tag: invalid/reserved (`0xFFFF`). Any occurrence in a value position is a fault.
pub const TAG_INVALID: u64 = 0xFFFF;

/// GC color bits occupy the low three payload bits of a reference.
pub const COLOR_MASK: u64 = 0b111;
/// Color bit 0: Marked0 (concurrent marking cycle 0).
pub const COLOR_MARKED0: u64 = 0b001;
/// Color bit 1: Marked1 (concurrent marking cycle 1).
pub const COLOR_MARKED1: u64 = 0b010;
/// Color bit 2: Remapped (object relocated; offset requires resolution).
pub const COLOR_REMAPPED: u64 = 0b100;

/// Canonical VM NaN: positive quiet NaN, outside the reserved tag region.
pub const CANONICAL_NAN: u64 = 0x7FF8_0000_0000_0000;

/// Special payload: null.
pub const SPECIAL_NULL: u64 = 0;
/// Special payload: undefined.
pub const SPECIAL_UNDEF: u64 = 1;
/// Special payload: false.
pub const SPECIAL_FALSE: u64 = 2;
/// Special payload: true.
pub const SPECIAL_TRUE: u64 = 3;
/// Special payload: uninitialized register slot.
pub const SPECIAL_UNINITIALIZED: u64 = 4;
/// Special payload: internal invalid value (distinct from user-visible values).
pub const SPECIAL_INVALID: u64 = 5;

/// Lowest signed 48-bit integer (-2^47).
pub const I48_MIN: i64 = -140_737_488_355_328;
/// Highest signed 48-bit integer (2^47 - 1).
pub const I48_MAX: i64 = 140_737_488_355_327;

/// Value categories of an NV-1 word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    /// Immediate signed 48-bit integer (`0xFFFC`).
    Int,
    /// Heap reference (`0xFFFD`).
    Ref,
    /// Special immediate (`0xFFFE`).
    Special,
    /// Floating-point value (any word outside the reserved region).
    Float,
    /// Reserved/invalid category (`0xFFFF`); must be rejected at runtime boundaries.
    Invalid,
}

/// Classifies an NV-1 word. Cost: one shift, one mask, one compare.
#[must_use]
pub fn categorize(w: u64) -> Category {
    if w & RESERVED_REGION_MASK != RESERVED_REGION_MASK {
        return Category::Float;
    }
    match w >> TAG_SHIFT {
        TAG_INT => Category::Int,
        TAG_REF => Category::Ref,
        TAG_SPECIAL => Category::Special,
        _ => Category::Invalid,
    }
}

/// Returns `true` when the word's tag is the immediate-integer tag.
#[must_use]
pub fn is_int(w: u64) -> bool {
    w >> TAG_SHIFT == TAG_INT
}

/// Returns `true` when the word's tag is the heap-reference tag.
#[must_use]
pub fn is_ref(w: u64) -> bool {
    w >> TAG_SHIFT == TAG_REF
}

/// Returns `true` when the word's tag is the special-immediate tag.
#[must_use]
pub fn is_special(w: u64) -> bool {
    w >> TAG_SHIFT == TAG_SPECIAL
}

/// Encodes a signed integer as an immediate NV-1 word.
///
/// Returns `None` when `n` is outside `I48_MIN..=I48_MAX`; overflow must box
/// through the heap, never truncate (NV-1 INV-004).
#[must_use]
pub fn encode_int(n: i64) -> Option<u64> {
    if (I48_MIN..=I48_MAX).contains(&n) {
        Some((TAG_INT << TAG_SHIFT) | (n as u64 & PAYLOAD_MASK))
    } else {
        None
    }
}

/// Decodes an immediate integer, sign-extending the 48-bit payload.
///
/// Panics in debug builds when the word is not an integer tag; callers must
/// classify first.
#[must_use]
pub fn decode_int(w: u64) -> i64 {
    debug_assert_eq!(w >> TAG_SHIFT, TAG_INT, "decode_int on non-int word");
    let payload = w & PAYLOAD_MASK;
    // Sign-extend bit 47 into bits 48..63.
    ((payload << 16) as i64) >> 16
}

/// Encodes a binary64 as an NV-1 word.
///
/// Floats are stored as native IEEE-754 bits. A NaN whose bit pattern collides
/// with the reserved tag region (top 16 bits in `0xFFFC..=0xFFFF`) is
/// canonicalized to [`CANONICAL_NAN`] (NV-1 section 5.1); all other patterns,
/// including non-colliding NaN payloads, are preserved verbatim.
#[must_use]
pub fn encode_f64(f: f64) -> u64 {
    let bits = f.to_bits();
    if bits & RESERVED_REGION_MASK == RESERVED_REGION_MASK {
        CANONICAL_NAN
    } else {
        bits
    }
}

/// Decodes a float word to `f64`. Reserved-region words must never reach this
/// function.
#[must_use]
pub fn decode_f64(w: u64) -> f64 {
    debug_assert!(
        w & RESERVED_REGION_MASK != RESERVED_REGION_MASK,
        "decode_f64 on reserved-region word"
    );
    f64::from_bits(w)
}

/// Builds a special-immediate word from a payload in `0..=5`.
#[must_use]
pub fn encode_special(payload: u64) -> u64 {
    debug_assert!(payload <= SPECIAL_INVALID, "unknown special payload");
    (TAG_SPECIAL << TAG_SHIFT) | payload
}

/// Decodes a special-immediate payload.
#[must_use]
pub fn decode_special(w: u64) -> u64 {
    debug_assert_eq!(
        w >> TAG_SHIFT,
        TAG_SPECIAL,
        "decode_special on non-special word"
    );
    w & PAYLOAD_MASK
}

/// Canonical immediate word for null.
#[must_use]
pub fn null_word() -> u64 {
    encode_special(SPECIAL_NULL)
}

/// Canonical immediate word for undefined.
#[must_use]
pub fn undefined_word() -> u64 {
    encode_special(SPECIAL_UNDEF)
}

/// Canonical immediate word for a boolean.
#[must_use]
pub fn bool_word(b: bool) -> u64 {
    encode_special(if b { SPECIAL_TRUE } else { SPECIAL_FALSE })
}

/// Builds a tagged reference word from a 48-bit heap-relative offset and GC
/// color bits.
///
/// The offset must be 8-byte aligned and nonzero (NV-1 INV-006 reserves offset
/// zero); colors occupy the low three bits and must be zero in the input.
#[must_use]
pub fn encode_ref(offset: u64, colors: u64) -> Option<u64> {
    if offset == 0
        || offset & !PAYLOAD_MASK != 0
        || offset & COLOR_MASK != 0
        || colors & !COLOR_MASK != 0
    {
        return None;
    }
    Some((TAG_REF << TAG_SHIFT) | offset | colors)
}

/// Extracts the raw 48-bit payload of a reference word, colors included.
#[must_use]
pub fn ref_payload(w: u64) -> u64 {
    debug_assert_eq!(w >> TAG_SHIFT, TAG_REF, "ref_payload on non-ref word");
    w & PAYLOAD_MASK
}

/// Extracts the clean (color-free) heap-relative offset of a reference word.
#[must_use]
pub fn ref_offset(w: u64) -> u64 {
    ref_payload(w) & !COLOR_MASK
}

/// Extracts the GC color bits of a reference word.
#[must_use]
pub fn ref_colors(w: u64) -> u64 {
    ref_payload(w) & COLOR_MASK
}

/// Returns `true` when the reference word is clean (no GC color bits set).
#[must_use]
pub fn ref_is_clean(w: u64) -> bool {
    ref_payload(w) & COLOR_MASK == 0
}

/// The load-barrier fast-path predicate (BARRIER-1, version 1.2).
///
/// Contract (BARRIER-INV-06, tag precedence): a word is color-tested **only**
/// when its 16-bit tag equals `0xFFFD`. Non-reference words (integers, floats,
/// specials) exit on the fast path with `false` because they carry no GC color
/// by construction. Testing `w & 0x07` without the tag check would
/// false-trigger on e.g. the integer 5 (`0xFFFC_0000_0000_0005`, low bits
/// `101`) - the exact defect this predicate exists to prevent.
///
/// The reference fast path is: tag compare (1 shift + 1 compare), then the
/// color test, matching the assembly in design.md BARRIER-1 section 2.1.
#[inline]
#[must_use]
pub fn needs_load_barrier(w: u64) -> bool {
    if w >> TAG_SHIFT != TAG_REF {
        return false;
    }
    w & COLOR_MASK != 0
}

/// Rebuilds a reference word with the given color bits, preserving the offset.
#[must_use]
pub fn with_colors(w: u64, colors: u64) -> u64 {
    debug_assert_eq!(w >> TAG_SHIFT, TAG_REF, "with_colors on non-ref word");
    debug_assert_eq!(colors & !COLOR_MASK, 0, "colors exceed 3 bits");
    (w & !COLOR_MASK) | colors
}

// --------------------------------------------------------------------------
// PACKED_TAGGED software model (SIMD-1 v1.1 sections 3.2 / 3.2.1).
//
// These functions model, lane-for-lane, the machine contract the JIT emits
// with AVX-512 (vaddpd / vpandq / vporq / kortestw) and AArch64/RV64
// equivalents. They exist so the lane semantics, overflow fallback, and
// collision canonicalization are specified, testable code rather than prose.
// --------------------------------------------------------------------------

/// Lane width of the 256-bit float model (4x binary64, as YMM).
pub const LANES_F64: usize = 4;
/// Lane width of the 512-bit integer model (8x i64 payload, as ZMM).
pub const LANES_I64: usize = 8;

/// Canonicalizes a single binary64 lane if its bits collide with the reserved
/// tag region. Returns `(word, collided)`.
///
/// This is the software model of the masked blend (`vmovdqu32 y{k1}`) the JIT
/// emits after tagged-float SIMD math (SIMD-1 section 3.2 B-F, SIMD-INV-03).
/// Inputs stored through [`encode_f64`] can never collide, but the contract
/// requires the blend because hardware NaN propagation is not guaranteed to
/// preserve the no-collision property across targets.
#[must_use]
pub fn canonicalize_f64_lane(bits: u64) -> (u64, bool) {
    if bits & RESERVED_REGION_MASK == RESERVED_REGION_MASK {
        (CANONICAL_NAN, true)
    } else {
        (bits, false)
    }
}

/// Lane error reported when a PACKED_TAGGED input lane violates its declared
/// lane domain. Can only originate from representation corruption: stores into
/// an `Array<TAGGED>` canonicalize reserved patterns at the value boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaneError {
    /// Index of the offending lane.
    pub lane: usize,
    /// The offending word.
    pub word: u64,
}

/// Result of tagged-float lane arithmetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaggedFloatResult {
    /// Result lanes as NV-1 words (floats are raw bits; colliding lanes are
    /// replaced by the canonical NaN, per SIMD-INV-03).
    pub lanes: [u64; LANES_F64],
    /// Bit i set when lane i required canonicalization (models the `k1` mask).
    pub collision_mask: u32,
}

/// Lane-wise binary64 add over float-homogeneous tagged words (B-F).
///
/// Returns [`LaneError`] when an input lane is a reserved-region word (only
/// possible through corruption; see [`canonicalize_f64_lane`]). Output lanes
/// that collide are blended to [`CANONICAL_NAN`] and reported in
/// `collision_mask`.
///
/// Panics (debug) if any input lane is a reference, integer, or special word:
/// mixed-lane tagged SIMD is prohibited (SIMD-INV-01).
// CEP:WHAT Lane-wise binary64 add over float-homogeneous tagged words (B-F).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Typed errors or panics documented in the doc comment above; no silent fallback.
// CEP:ASSUMES Caller obligations documented in the doc comment above.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-nv1
pub fn tagged_f64x4_add(
    a: [u64; LANES_F64],
    b: [u64; LANES_F64],
) -> Result<TaggedFloatResult, LaneError> {
    let mut out = [0_u64; LANES_F64];
    let mut mask = 0_u32;
    for i in 0..LANES_F64 {
        for (src, name) in [(a[i], "a"), (b[i], "b")] {
            match categorize(src) {
                Category::Float => { /* valid float lane */ }
                Category::Invalid => return Err(LaneError { lane: i, word: src }),
                _ => panic!("lane {i} of {name} has non-float tag {src:#x}"),
            }
        }
        let sum = f64::from_bits(a[i]) + f64::from_bits(b[i]);
        let (bits, collided) = canonicalize_f64_lane(sum.to_bits());
        if collided {
            mask |= 1 << i;
        }
        out[i] = bits;
    }
    Ok(TaggedFloatResult {
        lanes: out,
        collision_mask: mask,
    })
}

/// Result of tagged-integer lane arithmetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaggedIntResult {
    /// Result lanes as NV-1 tagged integers, valid only when `overflow_idx` is
    /// `None`.
    pub lanes: [u64; LANES_I64],
    /// Index of the first lane whose 48-bit payload overflowed, if any. The
    /// whole vector must fall back to scalar boxing (SIMD-INV-02).
    pub overflow_idx: Option<usize>,
}

/// Lane-wise add over integer-homogeneous tagged words (B-I): mask, compute,
/// range-check, re-tag.
///
/// Returns [`LaneError`] when an input lane is not an integer word (only
/// possible through corruption). Returns `overflow_idx = Some(i)` when any
/// lane's 48-bit payload overflows; the whole vector must then fall back to
/// scalar boxing (SIMD-INV-02), never wrap silently.
// CEP:WHAT Lane-wise add over integer-homogeneous tagged words (B-I): mask, compute, range-check, re-tag.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Typed errors or panics documented in the doc comment above; no silent fallback.
// CEP:ASSUMES Caller obligations documented in the doc comment above.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-nv1
pub fn tagged_i64x8_add(
    a: [u64; LANES_I64],
    b: [u64; LANES_I64],
) -> Result<TaggedIntResult, LaneError> {
    let mut out = [0_u64; LANES_I64];
    let mut overflow = None;
    for i in 0..LANES_I64 {
        for (src, name) in [(a[i], "a"), (b[i], "b")] {
            if !is_int(src) {
                return Err(LaneError { lane: i, word: src });
            }
            let _ = name;
        }
        let x = decode_int(a[i]);
        let y = decode_int(b[i]);
        match x.checked_add(y) {
            Some(sum) if (I48_MIN..=I48_MAX).contains(&sum) => {
                out[i] = (TAG_INT << TAG_SHIFT) | ((sum as u64) & PAYLOAD_MASK);
            }
            _ => {
                overflow = overflow.or(Some(i));
            }
        }
    }
    Ok(TaggedIntResult {
        lanes: out,
        overflow_idx: overflow,
    })
}
