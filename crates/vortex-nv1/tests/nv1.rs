// CEP:FILE crates/vortex-nv1/tests/nv1.rs
// CEP:WHAT Unit and regression tests for the NV-1 value model, including the
//        BARRIER-1 integer false-positive regression and PACKED_TAGGED lane
//        semantics.
// CEP:WHY NV-1 invariants (INV-001..INV-015) and BARRIER-INV-06 are
//        correctness-critical; each needs a named, executable proof.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE A failing assertion fails the test binary.
// CEP:ASSUMES None (pure functions under test).
// CEP:COST O(1) per case; suite completes in milliseconds.
// CEP:EVIDENCE cargo test -p vortex-nv1

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! NV-1 value-model test suite.

use vortex_nv1::*;

/// The BARRIER-1 regression: tagged integers must never trip the load barrier.
///
/// The critique's exact scenario: the integer 5 encodes to 0xFFFC_0000_0000_0005,
/// whose low three bits are 101. A color test without tag precedence would
/// branch to the slow path for every odd integer.
// CEP:WHAT The BARRIER-1 regression: tagged integers must never trip the load barrier.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-nv1
#[test]
fn barrier_does_not_false_trigger_on_tagged_integers() {
    let five = encode_int(5).expect("5 fits i48");
    assert_eq!(five, 0xFFFC_0000_0000_0005);
    assert_eq!(categorize(five), Category::Int);
    assert!(!needs_load_barrier(five), "BARRIER-INV-06 violated");

    // Odd and even integers with nonzero low bits, negatives included.
    for n in [-1_i64, 1, 3, 5, 7, 9, 0x00FF_FFFF, -5, I48_MAX, I48_MIN] {
        let w = encode_int(n).expect("in range");
        assert!(!needs_load_barrier(w), "integer {n} tripped the barrier");
    }
}

/// Float words and special words likewise never trip the barrier.
#[test]
fn barrier_ignores_floats_and_specials() {
    for bits in [
        0x7FF8_0000_0000_0000,
        0x4000_0000_0000_0000,
        0x0000_0000_0000_0001,
    ] {
        assert!(
            !needs_load_barrier(bits),
            "float bits {bits:#x} tripped the barrier"
        );
    }
    assert!(!needs_load_barrier(null_word()));
    assert!(!needs_load_barrier(undefined_word()));
    assert!(!needs_load_barrier(bool_word(true)));
}

/// A colored (grey/remapped) reference trips the barrier; a clean one does not.
// CEP:WHAT A colored (grey/remapped) reference trips the barrier; a clean one does not.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-nv1
#[test]
fn barrier_trips_only_on_colored_references() {
    let clean = encode_ref(0x1000, 0).expect("valid offset");
    assert!(!needs_load_barrier(clean));

    for color in [COLOR_MARKED0, COLOR_MARKED1, COLOR_REMAPPED] {
        let colored = with_colors(clean, color);
        assert!(
            needs_load_barrier(colored),
            "color {color} did not trip the barrier"
        );
        assert_eq!(ref_offset(colored), 0x1000, "offset preserved under colors");
        assert_eq!(ref_colors(colored), color);
    }
}

/// Immediate integers round-trip exactly across the full 48-bit domain.
// CEP:WHAT Immediate integers round-trip exactly across the full 48-bit domain.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-nv1
#[test]
fn integer_roundtrip_and_bounds() {
    for n in [0, 1, -1, I48_MIN, I48_MAX, 123_456_789, -123_456_789] {
        let w = encode_int(n).expect("in range");
        assert_eq!(categorize(w), Category::Int);
        assert_eq!(decode_int(w), n);
    }
    assert_eq!(
        encode_int(I48_MAX + 1),
        None,
        "i48 overflow must be rejected"
    );
    assert_eq!(
        encode_int(I48_MIN - 1),
        None,
        "i48 underflow must be rejected"
    );
}

/// Reserved-space NaN patterns canonicalize; ordinary payloads are preserved.
// CEP:WHAT Reserved-space NaN patterns canonicalize; ordinary payloads are preserved.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-nv1
#[test]
fn float_collision_canonicalization() {
    // Top16 = 0xFFFF (reserved): must canonicalize.
    assert_eq!(
        encode_f64(f64::from_bits(0xFFFF_0000_0000_0001)),
        CANONICAL_NAN
    );
    // Top16 = 0xFFFC..0xFFFF are all reserved.
    for top in 0xFFFC_u64..=0xFFFF {
        let bits = (top << 48) | 1;
        assert_eq!(encode_f64(f64::from_bits(bits)), CANONICAL_NAN);
    }
    // Canonical NaN itself (0x7FF8) survives.
    assert_eq!(encode_f64(f64::from_bits(CANONICAL_NAN)), CANONICAL_NAN);
    // 1.5 is preserved bit-exactly.
    assert_eq!(encode_f64(1.5), 1.5_f64.to_bits());
    // Negative quiet NaN 0xFFF8 (x86 "real indefinite") is NOT reserved.
    assert_eq!(
        encode_f64(f64::from_bits(0xFFF8_0000_0000_0000)),
        0xFFF8_0000_0000_0000
    );
}

/// Offset-zero references and misaligned offsets are rejected (INV-006).
// CEP:WHAT Offset-zero references and misaligned offsets are rejected (INV-006).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-nv1
#[test]
fn reference_encoding_rejects_invalid_offsets() {
    assert_eq!(encode_ref(0, 0), None, "offset zero is reserved");
    assert_eq!(encode_ref(0x1001, 0), None, "offset must be 8-aligned");
    assert_eq!(encode_ref(0x1000, 0b1000), None, "colors exceed 3 bits");
    assert!(encode_ref(0x1000, 0).is_some());
    // The full 48-bit offset window is representable when 8-aligned...
    assert!(encode_ref(0xFFFF_FFFF_FFF8, 0).is_some());
    // ...but anything beyond it is not.
    assert!(
        encode_ref(0x1_0000_0000_0000, 0).is_none(),
        "payload must fit 48 bits"
    );
}

/// The invalid category is detected and never confounded with valid categories.
#[test]
fn invalid_category_is_detectable() {
    let bad = 0xFFFF_0000_0000_0042;
    assert_eq!(categorize(bad), Category::Invalid);
    assert!(!is_special(bad));
}

/// B-F lanes: homogeneous tagged-float add computes on raw bits and blends
/// collisions to the canonical NaN.
// CEP:WHAT B-F lanes: homogeneous tagged-float add computes on raw bits and blends collisions to the canonical NaN.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-nv1
#[test]
fn packed_tagged_float_add_and_collision_blend() {
    let a = [
        encode_f64(1.0),
        encode_f64(2.0),
        encode_f64(3.0),
        encode_f64(4.0),
    ];
    let b = [
        encode_f64(10.0),
        encode_f64(20.0),
        encode_f64(30.0),
        encode_f64(40.0),
    ];
    let r = tagged_f64x4_add(a, b).expect("valid float lanes");
    assert_eq!(r.collision_mask, 0);
    assert_eq!(f64::from_bits(r.lanes[0]), 11.0);
    assert_eq!(f64::from_bits(r.lanes[3]), 44.0);

    // The canonicalization blend: any lane landing in the reserved tag region
    // is replaced by the canonical NaN and reported in the mask.
    let collided = canonicalize_f64_lane(0xFFFF_0000_0000_0001);
    assert_eq!(collided, (CANONICAL_NAN, true));
    let clean = canonicalize_f64_lane(0x4000_0000_0000_0000);
    assert_eq!(clean, (0x4000_0000_0000_0000, false));

    // A corrupt (reserved-tag) input lane is rejected, never computed on:
    // such a word cannot exist in a stored Array<TAGGED> (stores canonicalize).
    let bad = [0xFFFF_0000_0000_0001_u64; LANES_F64];
    let err = tagged_f64x4_add(a, bad).expect_err("corrupt lane");
    assert_eq!(err.lane, 0);
}

/// B-I lanes: masked payload add re-tags, and overflow forces whole-vector
/// scalar fallback (SIMD-INV-02).
// CEP:WHAT B-I lanes: masked payload add re-tags, and overflow forces whole-vector scalar fallback (SIMD-INV-02).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-nv1
#[test]
fn packed_tagged_int_add_with_overflow_fallback() {
    let a: [u64; LANES_I64] = core::array::from_fn(|i| encode_int(i as i64).expect("small"));
    let b: [u64; LANES_I64] = core::array::from_fn(|i| encode_int((i as i64) * 2).expect("small"));
    let r = tagged_i64x8_add(a, b).expect("valid int lanes");
    assert_eq!(r.overflow_idx, None);
    for i in 0..LANES_I64 {
        assert_eq!(decode_int(r.lanes[i]), i as i64 * 3);
    }

    // One lane overflows i48: the entire vector must fall back.
    let mut big = [encode_int(0).expect("zero"); LANES_I64];
    big[5] = encode_int(I48_MAX).expect("i48 max");
    let r2 = tagged_i64x8_add(a, big).expect("valid int lanes");
    assert_eq!(
        r2.overflow_idx,
        Some(5),
        "overflow must force scalar fallback"
    );

    // A corrupt (non-int) lane is rejected: corruption never becomes math.
    let mut corrupt = b;
    corrupt[2] = encode_f64(2.5);
    assert!(tagged_i64x8_add(a, corrupt).is_err());
}
