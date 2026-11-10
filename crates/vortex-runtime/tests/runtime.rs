// CEP:FILE crates/vortex-runtime/tests/runtime.rs
// CEP:WHAT Runtime integration tests: GC evacuation with reference fixing,
//        snapshot immobility across major collections (SNAP-INV-08), numeric
//        payload non-tracing (NUM-INV-01), the concurrent dependency-map
//        handshake (CONC-INV-06/07), and stencil purity accounting
//        (OSR-INV-06).
// CEP:WHY These are the executable proofs of the review resolutions at the
//        runtime layer.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES Single-mutator heap; the deps test spawns threads.
// CEP:COST O(heap) per GC case; O(iterations) for the handshake stress.
// CEP:EVIDENCE cargo test -p vortex-runtime

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! Runtime integration test suite (GC, snapshots, handshake, stencils).

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use vortex_nv1 as nv1;
use vortex_runtime::{DepError, DependencyMap, Gc, Handle, Heap, Roots};

fn ref_to(h: Handle) -> u64 {
    nv1::encode_ref(h.0, 0).expect("valid handle")
}

/// Major collection evacuates live old objects and rewrites every reference,
/// including references nested inside arrays.
// CEP:WHAT Major collection evacuates live old objects and rewrites every reference, including references nested inside arrays.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-runtime
#[test]
fn major_gc_evacuates_and_fixes_references() {
    let mut heap = Heap::new();
    heap.add_nursery_page();
    let nursery = heap.nursery_page().expect("nursery");
    let inner = heap
        .alloc_array(vortex_runtime::layout::GENERIC_ARRAY, 2, nursery)
        .expect("alloc");
    let outer = heap
        .alloc_array(vortex_runtime::layout::GENERIC_ARRAY, 1, nursery)
        .expect("alloc");
    heap.store_array_element(
        outer,
        0,
        ref_to(inner),
        &mut Vec::new(),
        &mut Default::default(),
    )
    .expect("store");

    let mut gc = Gc::default();
    let mut stack = vec![ref_to(outer)];
    gc.collect_major(
        &mut heap,
        Roots {
            stack: &mut stack,
            remembered: Vec::new(),
        },
    )
    .expect("gc");

    // The stack root was rewritten to the new location.
    assert!(nv1::is_ref(stack[0]));
    let outer_new = Handle(nv1::ref_offset(stack[0]));
    assert_ne!(outer_new.0, outer.0, "object moved");
    assert_eq!(gc.stats.objects_moved, 2, "inner and outer moved");
    // The interior reference was fixed.
    let fixed = heap.read_payload(outer_new, 0);
    let inner_new = Handle(nv1::ref_offset(fixed));
    assert_ne!(inner_new.0, inner.0, "interior ref updated");
    assert!(nv1::ref_is_clean(fixed), "references stay clean after GC");
}

/// Snapshot objects keep their handles across a major collection and are
/// never counted as moved (SNAP-INV-08: a GC that relocates a snapshot
/// object is incorrect by definition).
// CEP:WHAT Snapshot objects keep their handles across a major collection and are never counted as moved (SNAP-INV-08: a GC that relocates a snapshot object is incorrect by definition).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-runtime
#[test]
fn snapshot_objects_never_move() {
    let mut heap = Heap::new();
    heap.add_nursery_page();
    let nursery = heap.nursery_page().expect("nursery");
    let snap_arr = heap
        .alloc_array(vortex_runtime::layout::GENERIC_ARRAY, 4, nursery)
        .expect("alloc");
    let lo = snap_arr.0;
    let hi = snap_arr.0 + 0x1000;
    heap.freeze_snapshot(snap_arr.page(), lo, hi)
        .expect("freeze");

    let mut gc = Gc::default();
    let mut stack: Vec<u64> = Vec::new();
    gc.collect_major(
        &mut heap,
        Roots {
            stack: &mut stack,
            remembered: Vec::new(),
        },
    )
    .expect("gc");

    // The snapshot object's handle is still valid and its content intact.
    assert_eq!(
        heap.page_kind(snap_arr.page()),
        vortex_runtime::PageKind::Snapshot
    );
    assert_eq!(
        heap.array_len(snap_arr),
        4,
        "snapshot object intact at same handle"
    );
    assert_eq!(gc.stats.snapshot_objects_moved, 0, "SNAP-INV-08");

    // A second major collection with the snapshot referenced from the stack
    // also must not move it.
    stack.push(ref_to(snap_arr));
    gc.collect_major(
        &mut heap,
        Roots {
            stack: &mut stack,
            remembered: Vec::new(),
        },
    )
    .expect("gc");
    assert_eq!(heap.array_len(snap_arr), 4);
    assert_eq!(gc.stats.snapshot_objects_moved, 0);
}

/// Numeric array payloads are never scanned as references: float bit patterns
/// that resemble heap handles are copied verbatim and never traced.
// CEP:WHAT Numeric array payloads are never scanned as references: float bit patterns that resemble heap handles are copied verbatim and never traced.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-runtime
#[test]
fn numeric_payloads_are_never_traced() {
    let mut heap = Heap::new();
    heap.add_nursery_page();
    let nursery = heap.nursery_page().expect("nursery");

    // Short-lived object that becomes garbage.
    let garbage = heap
        .alloc_array(vortex_runtime::layout::GENERIC_ARRAY, 2, nursery)
        .expect("alloc");
    let garbage_word = ref_to(garbage);

    // Numeric array whose raw payload bits equal the garbage handle
    // (models a float bit pattern that looks like a reference).
    let num = heap
        .alloc_array(vortex_runtime::layout::NUMERIC_F64, 4, nursery)
        .expect("alloc");
    heap.write_payload(num, 0, garbage_word).expect("raw write");

    let mut gc = Gc::default();
    let mut stack: Vec<u64> = vec![ref_to(num)]; // only the numeric array is rooted
    gc.collect_major(
        &mut heap,
        Roots {
            stack: &mut stack,
            remembered: Vec::new(),
        },
    )
    .expect("gc");

    // The garbage was NOT retained through the numeric payload: exactly one
    // live object (the numeric array) moved.
    assert_eq!(gc.stats.objects_moved, 1, "only the numeric array was live");
    let num_new = Handle(nv1::ref_offset(stack[0]));
    assert_ne!(num_new.0, num.0, "numeric array moved");
    // Payload copied verbatim: raw bits, never reinterpreted or rewritten.
    let raw = heap.read_payload(num_new, 0);
    assert_eq!(raw, garbage_word, "raw payload bits preserved verbatim");
}

/// CONC-INV-06/07 under concurrency: with a compiler thread publishing
/// dependencies and an invalidator invalidating concurrently, the map never
/// loses a published dependency (no silently dropped deps), the seqlock
/// protocol stays live (no deadlock), and once quiescent a final invalidation
/// returns exactly the full published set.
// CEP:WHAT CONC-INV-06/07 under concurrency: with a compiler thread publishing dependencies and an invalidator invalidating concurrently, the map never loses a published dependency (no silently dropped deps), the seqlock protocol stays live (no deadlock), and once quiescent a final invalidation returns exactly the full published set.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-runtime
#[test]
fn concurrent_adds_are_never_missed_by_invalidation() {
    const LAYOUT: u64 = 42;
    let mut map = DependencyMap::default();
    map.ensure_layout(LAYOUT);
    let map = Arc::new(map);

    let published_count = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();

    // Compiler thread: publish dependencies against the current generation;
    // on StaleGeneration re-derive and retry (the compiler side of CONC-INV-07).
    {
        let map = Arc::clone(&map);
        let published_count = Arc::clone(&published_count);
        handles.push(std::thread::spawn(move || {
            for i in 0..400_u32 {
                loop {
                    let gen = map.generation(LAYOUT);
                    match map.add_dependency(LAYOUT, i, gen) {
                        Ok(()) => {
                            published_count.fetch_add(1, Ordering::SeqCst);
                            break;
                        }
                        Err(DepError::StaleGeneration { .. }) => continue,
                    }
                }
            }
        }));
    }
    // Invalidator thread: invalidate repeatedly while adds are in flight.
    {
        let map = Arc::clone(&map);
        handles.push(std::thread::spawn(move || {
            for _ in 0..200 {
                let _ = map.invalidate(LAYOUT);
            }
        }));
    }
    for h in handles {
        h.join().expect("threads join (liveness: no deadlock)");
    }

    // All publishes completed (stale-generation retries converged).
    assert_eq!(published_count.load(Ordering::SeqCst), 400);
    // The generation advanced: invalidations ran under contention.
    assert!(map.generation(LAYOUT) > 0);

    // Quiescent final traversal: the complete published set is visible,
    // proving no dependency was silently dropped under concurrency.
    let final_set = map.invalidate(LAYOUT);
    let mut got: HashSet<u32> = HashSet::new();
    got.extend(final_set.iter().copied());
    let missing: Vec<u32> = (0..400_u32).filter(|r| !got.contains(r)).collect();
    assert!(
        missing.is_empty(),
        "dependencies published but lost (CONC-INV-06 violation): {missing:?}"
    );
}

/// Stencil purity (OSR-INV-06): identical (op, location) requests reuse one
/// stencil; every blob carries exactly one shared OSR trampoline stub; pool
/// size grows with operand-location variants, not with blobs or loops.
// CEP:WHAT Stencil purity (OSR-INV-06): identical (op, location) requests reuse one stencil; every blob carries exactly one shared OSR trampoline stub; pool size grows with operand-location variants, not with blobs or loops.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-runtime
#[test]
fn stencil_pool_stays_pure_and_deduplicated() {
    use vortex_runtime::{Loc, StencilKey, StencilPool};
    use vortex_ugb::Op;

    let mut pool = StencilPool::default();
    let key_add_rr = StencilKey {
        op: Op::Add,
        locs: [Some(Loc::Reg(0)), Some(Loc::Reg(1)), Some(Loc::Reg(2))],
    };
    let key_add_rs = StencilKey {
        op: Op::Add,
        locs: [Some(Loc::Reg(0)), Some(Loc::Stack(0)), Some(Loc::Reg(2))],
    };

    // Five blobs with three loops each: stencils dedupe, stubs stay 1/blob.
    for _ in 0..5 {
        let blob = pool.assemble_blob(
            &[key_add_rr, key_add_rs, key_add_rr],
            &[(0, 8), (1, 16), (2, 24)],
        );
        assert_eq!(
            blob.osr_stub.counters.len(),
            3,
            "one metadata counter per loop"
        );
        assert!(!blob.osr_stub.patched_to_t2);
    }
    assert_eq!(
        pool.len(),
        2,
        "pool grows by location variants, not by blobs"
    );
    assert_eq!(
        pool.osr_stubs_created, 5,
        "exactly one shared stub per blob"
    );
    assert_eq!(pool.blobs_assembled, 5);
}
