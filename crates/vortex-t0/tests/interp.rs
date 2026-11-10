// CEP:FILE crates/vortex-t0/tests/interp.rs
// CEP:WHAT T0 interpreter integration tests: corrected barrier behavior,
//        epoch capability checks, sampled back-edges, numeric transitions,
//        boxed overflow, and snapshot traps.
// CEP:WHY These are the executable proofs of the review resolutions at the
//        execution layer.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES Demo methods from vortex-ugb.
// CEP:COST O(program) per case.
// CEP:EVIDENCE cargo test -p vortex-t0

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! T0 interpreter integration test suite.

use vortex_nv1 as nv1;
use vortex_runtime as rt;
use vortex_t0::Interp;

/// Countdown computes the exact triangular sum and the sampled back-edge
/// counter crosses the T4 threshold, enqueueing exactly one trace request
/// (PROF-1 section 3.2 v1.1: T2/T3 loops carry sampled back-edge counters).
// CEP:WHAT Countdown computes the exact triangular sum and the sampled back-edge counter crosses the T4 threshold, enqueueing exactly one trace request (PROF-1 section 3.2 v1.1: T2/T3 loops carry sampled back-edge counters).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t0
#[test]
fn countdown_counts_back_edges_and_requests_traces() {
    let mut m = rt::Machine::new();
    let interp = Interp::new(vec![vortex_ugb::countdown_method(10_000)]);
    let r = interp.run(&mut m, 0, &[]).expect("runs");
    assert_eq!(nv1::decode_int(r), 10_000 * 10_001 / 2);
    // 10_000 iterations at window 64: total folded = 9984 >= 1024 threshold.
    let reqs = m.profile.trace_requests();
    assert_eq!(reqs.len(), 1, "exactly one T4 trace request");
    assert!(m.profile.total(0) >= 1024);
}

/// Integer arithmetic stays immediate in range and boxes on i48 overflow
/// (NV-1 section 2.1 boxed-overflow policy; INV-004 no silent truncation).
// CEP:WHAT Integer arithmetic stays immediate in range and boxes on i48 overflow (NV-1 section 2.1 boxed-overflow policy; INV-004 no silent truncation).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t0
#[test]
fn arithmetic_boxes_on_overflow() {
    let mut m = rt::Machine::new();
    // Build a program: r0 = I48_MAX; r1 = r0 + r0 (overflows i48 -> boxed).
    let mut a = vortex_ugb::Asm::new("overflow", 0, 4);
    let max_c = a.push_const(vortex_ugb::Const::Int(nv1::I48_MAX));
    let mut lc = vortex_ugb::mk(vortex_ugb::Op::LoadConst, &[0]);
    lc.pool = max_c;
    a.emit(&lc);
    a.emit(&vortex_ugb::mk(vortex_ugb::Op::Add, &[1, 0, 0]));
    a.emit(&vortex_ugb::mk(vortex_ugb::Op::Return, &[1]));
    let interp = Interp::new(vec![a.finish()]);
    let r = interp.run(&mut m, 0, &[]).expect("runs");
    assert!(nv1::is_ref(r), "overflow result must be a boxed reference");
    let h = rt::Handle(nv1::ref_offset(r));
    assert_eq!(m.heap.layout_of(h), rt::layout::BOXED_INT);
    assert_eq!(m.heap.read_payload(h, 0) as i64, nv1::I48_MAX * 2);
}

/// The capability epoch protocol: the check passes pre-revocation, fails
/// post-revocation, and the epoch moved by exactly one increment
/// (UGB-1 section 6.3, UGB-INV-07).
// CEP:WHAT The capability epoch protocol: the check passes pre-revocation, fails post-revocation, and the epoch moved by exactly one increment (UGB-1 section 6.3, UGB-INV-07).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t0
#[test]
fn capability_epoch_revocation_is_immediate() {
    let mut m = rt::Machine::new();
    let cap = m.caps.grant(rt::auth::FILE_READ, 0, None);
    let mut cache = rt::EpochCache::default();

    // Fast path: first check takes the slow path (cache empty), refreshes.
    m.caps
        .check_fast(&mut cache, cap, rt::auth::FILE_READ)
        .expect("granted");
    let e1 = m.caps.epoch_of(cap);
    // Second check: pure fast path (epoch match).
    m.caps
        .check_fast(&mut cache, cap, rt::auth::FILE_READ)
        .expect("fast path");
    assert_eq!(m.caps.epoch_of(cap), e1, "no epoch movement on grant");

    // Revoke: one increment, check fails immediately with the stale cache.
    m.caps.revoke(cap);
    assert_eq!(m.caps.epoch_of(cap), e1 + 1, "revoke = one increment");
    assert!(m
        .caps
        .check_fast(&mut cache, cap, rt::auth::FILE_READ)
        .is_err());

    // Authority narrowing also denies.
    let mut cache2 = rt::EpochCache::default();
    let cap2 = m.caps.grant(rt::auth::FILE_READ, 0, None);
    assert!(m
        .caps
        .check_fast(&mut cache2, cap2, rt::auth::NETWORK)
        .is_err());
    assert!(m
        .caps
        .check_fast(&mut cache2, cap2, rt::auth::FILE_READ)
        .is_ok());
}

/// Delegation chains: revoking a child increments the ancestor's epoch.
// CEP:WHAT Delegation chains: revoking a child increments the ancestor's epoch.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t0
#[test]
fn revocation_propagates_to_ancestors() {
    let mut m = rt::Machine::new();
    let parent = m.caps.grant(rt::auth::FILE_READ, 0, None);
    let child = m.caps.grant(rt::auth::FILE_READ, 0, Some(parent));
    let e_parent = m.caps.epoch_of(parent);
    m.caps.revoke(child);
    assert!(m.caps.is_revoked(parent), "ancestor revoked");
    assert_eq!(
        m.caps.epoch_of(parent),
        e_parent + 1,
        "ancestor epoch bumped"
    );
}

/// Storing floats into a tagged array until homogeneity triggers the numeric
/// transition; afterwards the store path elides the write barrier
/// (NUM-1 sections 3-4, observable via GcStats).
// CEP:WHAT Storing floats into a tagged array until homogeneity triggers the numeric transition; afterwards the store path elides the write barrier (NUM-1 sections 3-4, observable via GcStats).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t0
#[test]
fn numeric_transition_elides_write_barriers() {
    let mut m = rt::Machine::new();
    let nursery = m.heap.nursery_page().expect("nursery");
    let arr = m
        .heap
        .alloc_array(rt::layout::GENERIC_ARRAY, 8, nursery)
        .expect("alloc");

    // Feed the homogeneity window with float stores.
    for i in 0..rt::HOMOGENEITY_WINDOW {
        let v = nv1::encode_f64(f64::from(i) + 0.5);
        let d = m.aoss.observe_store(arr, v);
        if d != rt::Decision::None {
            assert!(m
                .aoss
                .transition_numeric(&mut m.heap, arr, d)
                .expect("transition"));
        }
    }
    assert!(
        rt::layout::is_numeric(m.heap.layout_of(arr)),
        "transitioned"
    );

    // Stores now land raw with zero write barriers.
    let before = m.gc.stats.write_barrier_elided;
    for i in 0..8_usize {
        m.heap
            .store_array_element(
                arr,
                i,
                nv1::encode_f64(i as f64 * 2.0),
                &mut m.remembered,
                &mut m.gc.stats,
            )
            .expect("raw store");
    }
    assert_eq!(
        m.gc.stats.write_barrier_elided - before,
        8,
        "all stores elided"
    );
    assert_eq!(
        m.gc.stats.write_barriers, 0,
        "no barriers on numeric arrays"
    );

    // A reference store into the numeric array reverses the layout
    // (NUM-INV-02) through the interpreter's store policy.
    let interp = Interp::new(vec![]);
    let obj = m
        .heap
        .alloc_array(rt::layout::GENERIC_ARRAY, 2, nursery)
        .expect("alloc");
    let obj_word = nv1::encode_ref(obj.0, 0).expect("ref");
    interp
        .store_element(&mut m, arr, 0, obj_word)
        .expect("reverse transition");
    assert_eq!(m.heap.layout_of(arr), rt::layout::GENERIC_ARRAY, "reversed");
    assert_eq!(m.heap.read_payload(arr, 0), obj_word, "value preserved");
}

/// The load barrier executes the corrected fast path: clean references pass
/// without the slow path; colored references route to the slow path and are
/// returned cleaned (BARRIER-INV-06/07).
// CEP:WHAT The load barrier executes the corrected fast path: clean references pass without the slow path; colored references route to the slow path and are returned cleaned (BARRIER-INV-06/07).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t0
#[test]
fn load_barrier_classifies_before_coloring() {
    let mut m = rt::Machine::new();
    let nursery = m.heap.nursery_page().expect("nursery");
    let arr = m
        .heap
        .alloc_array(rt::layout::GENERIC_ARRAY, 4, nursery)
        .expect("alloc");
    let payload = m
        .heap
        .alloc_array(rt::layout::GENERIC_ARRAY, 2, nursery)
        .expect("alloc");
    let clean = nv1::encode_ref(payload.0, 0).expect("ref");

    // Integer 5 in the array: the 1.1 barrier would false-trigger here.
    m.heap
        .store_array_element(
            arr,
            0,
            nv1::encode_int(5).expect("int"),
            &mut m.remembered,
            &mut m.gc.stats,
        )
        .expect("store");
    let before_slow = m.gc.stats.load_barrier_slow;
    let v = m
        .heap
        .load_array_element(arr, 0, &mut m.gc.stats)
        .expect("load");
    assert_eq!(v, nv1::encode_int(5).expect("int"));
    assert_eq!(
        m.gc.stats.load_barrier_slow, before_slow,
        "integer must not take the slow path"
    );

    // Clean reference: fast path.
    m.heap
        .store_array_element(arr, 1, clean, &mut m.remembered, &mut m.gc.stats)
        .expect("store");
    let v = m
        .heap
        .load_array_element(arr, 1, &mut m.gc.stats)
        .expect("load");
    assert_eq!(v, clean);
    // Two fast-path exits so far: the integer (BARRIER-INV-06 tag precedence)
    // and the clean reference.
    assert_eq!(m.gc.stats.load_barrier_fast, 2);

    // Colored reference (grey): slow path, returned clean.
    let grey = nv1::with_colors(clean, nv1::COLOR_MARKED0);
    m.heap.write_payload(arr, 2, grey).expect("raw color write");
    let v = m
        .heap
        .load_array_element(arr, 2, &mut m.gc.stats)
        .expect("load");
    assert_eq!(v, clean, "slow path resolves colors");
    assert_eq!(m.gc.stats.load_barrier_slow, 1);
}

/// Stores into a frozen snapshot page trap (SNAP-INV-09) and no barrier or
/// GC state is written into the page (SNAP-INV-10 by construction).
// CEP:WHAT Stores into a frozen snapshot page trap (SNAP-INV-09) and no barrier or GC state is written into the page (SNAP-INV-10 by construction).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t0
#[test]
fn snapshot_pages_are_immutable() {
    let mut m = rt::Machine::new();
    let nursery = m.heap.nursery_page().expect("nursery");
    let arr = m
        .heap
        .alloc_array(rt::layout::GENERIC_ARRAY, 4, nursery)
        .expect("alloc");
    let lo = arr.0;
    let hi = arr.0 + 0x100;
    m.heap.freeze_snapshot(arr.page(), lo, hi).expect("freeze");
    let t =
        m.heap
            .store_array_element(arr, 0, nv1::null_word(), &mut m.remembered, &mut m.gc.stats);
    assert_eq!(t, Err(rt::Trap::SnapshotViolation { handle: arr.0 }));
}
