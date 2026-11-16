// CEP:FILE crates/vortex-nv1/tests/nv1.rs
// CEP:WHAT NV-1 invariant tests: encoding uniqueness, immediate range and
//        sign extension, reference offset rules, NaN canonicalization and
//        payload transport, checked arithmetic boxing outcomes.
// CEP:WHY The NV-1 invariants (section 8, INV-001..INV-010) are correctness
//        requirements; these tests are their executable definitions.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES IEEE-754 binary64 bit layout.
// CEP:COST O(1) per case.
// CEP:EVIDENCE cargo test -p vortex-nv1

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! NV-1 value-word test suite.

use vortex_nv1::{
    Classification, IntOpOutcome, InvalidValue, Value, CANONICAL_NAN_BITS, INT_MAX, INT_MIN,
    RESERVED_PREFIX,
};

/// INV-001 and INV-002: in-range integers encode once and the invalid
/// prefix can never become a value.
// CEP:WHAT Immediate integers encode uniquely and invalid words are rejected.
// CEP:WHY INV-001 requires exactly one valid storage representation and
//        INV-002 bans the invalid category in live values.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-nv1
#[test]
fn integers_encode_uniquely_and_invalid_rejects() {
    let v = Value::from_i64(42).expect("42 is in range");
    assert_eq!(v.bits(), RESERVED_PREFIX | 42);
    assert_eq!(v.as_i64(), Some(42));
    assert_eq!(
        Value::from_word(0xFFFF_0000_0000_0001),
        Err(InvalidValue::InvalidPrefix)
    );
    assert_eq!(
        Value::classify_word(0xFFFF_DEAD_BEEF_0000),
        Classification::Invalid
    );
}

/// INV-003: boundary integers decode exactly through sign extension.
// CEP:WHAT Boundary integers decode to their exact signed values.
// CEP:WHY INV-003 requires exact 48-bit two's-complement decoding.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-nv1
#[test]
fn integer_boundaries_round_trip() {
    assert_eq!(Value::from_i64(INT_MIN).unwrap().as_i64(), Some(INT_MIN));
    assert_eq!(Value::from_i64(INT_MAX).unwrap().as_i64(), Some(INT_MAX));
    assert_eq!(Value::from_i64(INT_MIN - 1), None);
    assert_eq!(Value::from_i64(INT_MAX + 1), None);
    assert_eq!(Value::from_i64(-1).unwrap().as_i64(), Some(-1));
}

/// INV-004: arithmetic that leaves the immediate range reports the exact
/// result for boxing instead of truncating.
// CEP:WHAT Checked arithmetic reports exact results when boxing is required.
// CEP:WHY INV-004: integer overflow never silently truncates.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-nv1
#[test]
fn arithmetic_overflow_reports_exact_result() {
    let a = Value::from_i64(INT_MAX).unwrap();
    let b = Value::from_i64(1).unwrap();
    assert_eq!(
        Value::checked_add(a, b),
        IntOpOutcome::NeedsBox(INT_MAX + 1)
    );
    assert_eq!(
        Value::checked_add(Value::from_i64(2).unwrap(), Value::from_i64(3).unwrap()),
        IntOpOutcome::Immediate(Value::from_i64(5).unwrap())
    );
    let lo = Value::from_i64(INT_MIN).unwrap();
    assert_eq!(
        Value::checked_sub(lo, b),
        IntOpOutcome::NeedsBox(INT_MIN - 1)
    );
    let big = Value::from_i64(3037000499).unwrap();
    let product = 3037000499i128 * 3037000499i128;
    assert!(product > INT_MAX as i128);
    match Value::checked_mul(big, big) {
        IntOpOutcome::NeedsBox128(p) => assert_eq!(p, product),
        other => panic!("expected NeedsBox128, got {other:?}"),
    }
    assert_eq!(
        Value::checked_add(Value::from_f64(1.0), a),
        IntOpOutcome::NotImmediate
    );
}

/// INV-006 and INV-005: zero offsets and out-of-window offsets are rejected;
/// valid offsets round-trip.
// CEP:WHAT Heap references validate the offset window strictly.
// CEP:WHY INV-006 reserves offset zero for corruption detection.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-nv1
#[test]
fn heap_reference_offsets_validate() {
    assert_eq!(Value::from_heap_ref(0), Err(InvalidValue::ZeroOffsetRef));
    assert_eq!(
        Value::from_heap_ref(1 << 48),
        Err(InvalidValue::RefOutOfRange)
    );
    let r = Value::from_heap_ref(0x1234).unwrap();
    assert_eq!(r.as_heap_ref(), Some(0x1234));
    assert_eq!(
        Value::classify_word(0xFFFD_0000_0000_0000),
        Classification::HeapRef(0)
    );
}

/// INV-007 and INV-009: arithmetic NaN results canonicalize and reserved
/// NaN patterns can never masquerade as stored float values.
// CEP:WHAT Float encoding canonicalizes NaN into the reserved region.
// CEP:WHY INV-007 fixes one arithmetic NaN; INV-009 keeps the reserved
//        tag region out of stored values.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES IEEE-754 binary64.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-nv1
#[test]
fn nan_results_canonicalize() {
    let nan = f64::NAN;
    let collided = nan.to_bits() | 0xFFFF_0000_0000_0000;
    let v = Value::transport_f64_bits(collided);
    assert_eq!(v.as_f64_bits(), Some(CANONICAL_NAN_BITS));
    let ordinary = Value::from_f64(-1.5);
    assert_eq!(ordinary.as_f64_bits(), Some((-1.5f64).to_bits()));
    assert!(Value::classify_word(CANONICAL_NAN_BITS) != Classification::Invalid);
}

/// INV-008: non-colliding NaN payloads survive bitwise transport unchanged.
// CEP:WHAT Permitted NaN payloads pass through transport unchanged.
// CEP:WHY Bit-preserving operations (load, store, copy) must not lose
//        payloads that do not collide with the tag region.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-nv1
#[test]
fn nan_payloads_survive_transport() {
    let payload = 0x7FF8_DEAD_BEEF_0001u64;
    let v = Value::transport_f64_bits(payload);
    assert_eq!(v.as_f64_bits(), Some(payload));
    let neg_nan = 0xFFF8_1234_5678_9ABCu64;
    let v2 = Value::transport_f64_bits(neg_nan);
    assert_eq!(v2.as_f64_bits(), Some(neg_nan));
}

/// The special immediate encodes, and reserved special payloads reject.
// CEP:WHAT Special immediates encode with payload validation.
// CEP:WHY Only payload zero is defined; other payloads stay invalid so
//        future specials cannot silently appear.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-nv1
#[test]
fn specials_and_reserved_payloads() {
    let u = Value::undefined();
    assert!(u.as_special().is_some());
    assert_eq!(u.as_i64(), None);
    assert_eq!(u.as_heap_ref(), None);
    assert_eq!(
        Value::from_word(0xFFFE_0000_0000_0001),
        Err(InvalidValue::ReservedSpecial)
    );
}

/// Display shows the category and the decoded payload for diagnostics.
// CEP:WHAT Display renders category and payload.
// CEP:WHY CLI output and logs must identify categories at a glance.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(digits).
// CEP:EVIDENCE cargo test -p vortex-nv1
#[test]
fn display_shows_category() {
    assert_eq!(Value::from_i64(7).unwrap().to_string(), "int(7)");
    let r = Value::from_heap_ref(0x10).unwrap().to_string();
    assert!(r.starts_with("ref(+0x"));
    assert!(Value::from_f64(2.5).to_string().contains("2.5"));
}
