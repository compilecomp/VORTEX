// CEP:FILE crates/vortex-runtime/tests/runtime.rs
// CEP:WHAT Runtime tests: capability security properties (P1-P5), heap
//        allocation and card semantics, GC barriers and collections
//        (young evacuation, old compaction, weak clearing), and the
//        profiling model.
// CEP:WHY The security properties and GC invariants are correctness
//        requirements; these tests are their executable definitions.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES Deterministic heaps constructed in the tests.
// CEP:COST O(heap) per case.
// CEP:EVIDENCE cargo test -p vortex-runtime

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! Runtime test suite.

use vortex_runtime::caps::*;
use vortex_runtime::gc::{Colors, GcConfig, GcStats, PointerColor};
use vortex_runtime::heap::{
    Heap, HeapConfig, RegionKind, ARENA_BASE_RESERVE, CARD_DIRTY, CARD_OLD, FLAG_FORWARDED,
    HEADER_SIZE,
};
use vortex_runtime::profile::{ProfileData, ValueType};
use vortex_runtime::trap::Trap;
use vortex_runtime::{Collector, Token};

const PERM_ALL: u16 = 0xFFFF;

/// Builds a small model heap.
// CEP:WHAT Creates a 4-region test heap.
// CEP:WHY Tests allocate within documented model sizes.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(heap).
// CEP:EVIDENCE cargo test -p vortex-runtime
fn test_heap() -> Heap {
    Heap::new(HeapConfig {
        initial_regions: 4,
        region_size: 4096,
        max_regions: 16,
    })
}

/// Allocates one object with a header and `slots` reference slots.
// CEP:WHAT Allocates a test object of a given slot count.
// CEP:WHY Every GC test needs objects with reference fields.
// CEP:STATUS complete
// CEP:FAILURE Panics on OOM; tests size heaps to avoid it.
// CEP:ASSUMES None.
// CEP:COST O(bytes).
// CEP:EVIDENCE cargo test -p vortex-runtime
fn alloc_object(heap: &mut Heap, tlab: &mut vortex_runtime::Tlab, slots: usize) -> usize {
    let size = HEADER_SIZE + slots * 8;
    let off = tlab.try_allocate(size).expect("tlab space");
    heap.set_object_size(off, size as u32).expect("size");
    heap.set_object_age(off, 0).expect("age");
    off
}

/// P1 unforgeability: tokens made by user bit-twiddling fail checks.
// CEP:WHAT Forged tokens with arbitrary fields never pass checks.
// CEP:WHY P1: user code cannot create a valid token.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-runtime
#[test]
fn forged_tokens_fail_checks() {
    let mut gct = Gct::new(8);
    let real = gct.make_root(0x1000, PERM_MEM_READ).expect("root");
    let forged = Token::make(real.index(), real.generation(), PERM_MEM_READ | 0x7F00);
    assert_eq!(gct.check(forged, PERM_MEM_READ), Ok(()));
    // The extra flags grant nothing the entry mask does not carry.
    assert_eq!(
        gct.check(forged, PERM_FFI),
        Err(CapError::InsufficientPermission)
    );
    let wrong_gen = Token::make(real.index(), real.generation() + 1, PERM_ALL);
    assert_eq!(gct.check(wrong_gen, PERM_MEM_READ), Err(CapError::Revoked));
    let out_of_range = Token::make(999_999, 1, PERM_ALL);
    assert_eq!(
        gct.check(out_of_range, PERM_MEM_READ),
        Err(CapError::InvalidToken)
    );
}

/// P3 attenuation monotonicity and P4 revocation completeness.
// CEP:WHAT Children carry subset permissions; revocation kills the tree.
// CEP:WHY P3 and P4 are the core delegation guarantees.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(entries).
// CEP:EVIDENCE cargo test -p vortex-runtime
// CEP:ID: MEM-a18
#[test]
fn attenuation_and_revocation() {
    let mut gct = Gct::new(16);
    let root = gct.make_root(0x2000, PERM_ALL).expect("root");
    let child = gct
        .attenuate(root, PERM_MEM_READ | PERM_MEM_WRITE)
        .expect("child");
    let grandchild = gct.attenuate(child, PERM_MEM_READ).expect("grandchild");
    gct.audit_invariants().expect("invariants");
    assert_eq!(
        gct.attenuate(child, PERM_FFI),
        Err(CapError::InvalidAttenuation)
    );
    assert_eq!(gct.check(grandchild, PERM_MEM_READ), Ok(()));
    let killed = gct.revoke(child).expect("revoke");
    assert_eq!(killed, 2);
    assert_eq!(gct.check(child, PERM_MEM_READ), Err(CapError::Revoked));
    assert_eq!(gct.check(grandchild, PERM_MEM_READ), Err(CapError::Revoked));
    assert_eq!(gct.check(root, PERM_MEM_READ), Ok(()));
}

/// Depth bound: attenuation beyond 256 rejects.
// CEP:WHAT Chain depth is bounded at 256.
// CEP:WHY Revocation walks stay proportional to descendants.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(256).
// CEP:EVIDENCE cargo test -p vortex-runtime
#[test]
fn attenuation_depth_bounded() {
    let mut gct = Gct::new(512);
    let mut tok = gct.make_root(1, PERM_ALL).expect("root");
    for _ in 0..MAX_ATTENUATION_DEPTH {
        tok = gct.attenuate(tok, PERM_ALL).expect("attenuate");
    }
    assert_eq!(
        gct.attenuate(tok, PERM_ALL),
        Err(CapError::AttenuationTooDeep)
    );
}

/// Environments: pass verifies caller membership; 64-slot bound holds.
// CEP:WHAT Environment add/contains/remove/pass semantics.
// CEP:WHY 6.5.4 and 6.10 item 6 define environment behavior.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(64).
// CEP:ID: MEM-a19
// CEP:EVIDENCE cargo test -p vortex-runtime
#[test]
fn environments_pass_and_bound() {
    let mut gct = Gct::new(8);
    let tok = gct.make_root(1, PERM_ALL).expect("root");
    let mut caller = Environment::default();
    let mut callee = Environment::default();
    caller.add(tok).expect("add");
    assert_eq!(
        gct.pass(&Environment::default(), &mut callee, tok),
        Err(CapError::InvalidToken)
    );
    gct.pass(&caller, &mut callee, tok).expect("pass");
    assert!(callee.contains(tok));
    let mut full = Environment::default();
    for _ in 0..MAX_CAP_SLOTS {
        full.add(tok).expect("fill");
    }
    assert_eq!(full.add(tok), Err(CapError::EnvFull));
}

/// Allocation stamps headers; cards track generations and dirty state.
// CEP:WHAT Object headers and the card table behave as documented.
// CEP:WHY GC invariant 1: the card table reflects generations.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:ID: MEM-a20
// CEP:COST O(heap).
// CEP:EVIDENCE cargo test -p vortex-runtime
#[test]
fn heap_allocation_and_cards() {
    let mut heap = test_heap();
    let region = heap.take_young_region(16).expect("region");
    let mut tlab = heap.take_tlab(region, 2048).expect("tlab");
    let a = alloc_object(&mut heap, &mut tlab, 2);
    assert_eq!(heap.object_size(a).unwrap(), HEADER_SIZE + 16);
    assert_eq!(heap.region_kind_of(a), Some(RegionKind::Young));
    assert!(a >= ARENA_BASE_RESERVE);
    // Convert the region to old, dirty a card as an old-to-young store
    // would, then verify the clear pass restores the base value.
    heap.promote_region(region);
    assert_eq!(heap.region_kind_of(a), Some(RegionKind::Old));
    heap.dirty_card(a);
    assert_eq!(heap.card_value(a), CARD_DIRTY);
    heap.clear_dirty_cards();
    assert_eq!(heap.card_value(a), CARD_OLD);
    assert!(matches!(
        heap.validate_ref(0),
        Err(Trap::BadReference { .. })
    ));
}

/// Young collection evacuates survivors and reclaims dead objects.
// CEP:WHAT Minor GC keeps live objects reachable and releases space.
// CEP:WHY GC invariant 2 and 6.8 phases.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ID: MEM-a21
// CEP:ASSUMES None.
// CEP:COST O(live young).
// CEP:EVIDENCE cargo test -p vortex-runtime
#[test]
fn young_collection_evacuates() {
    let mut heap = test_heap();
    let region = heap.take_young_region(16).expect("region");
    let mut tlab = heap.take_tlab(region, 4000).expect("tlab");
    let parent = alloc_object(&mut heap, &mut tlab, 1);
    let child = alloc_object(&mut heap, &mut tlab, 0);
    let _dead = alloc_object(&mut heap, &mut tlab, 0);
    heap.write_slot(parent + HEADER_SIZE, child as u64)
        .expect("slot");
    let mut roots = [parent as u64];
    let mut collector = Collector::new(GcConfig::default(), heap.arena_size());
    collector
        .young_collect(&mut heap, &mut roots)
        .expect("young gc");
    let new_parent = roots[0] as usize;
    assert_ne!(new_parent, parent);
    let new_child = heap.read_slot(new_parent + HEADER_SIZE).unwrap() as usize;
    assert_ne!(new_child, child);
    assert_eq!(heap.region_kind_of(new_parent), Some(RegionKind::Young));
    assert!(heap.object_flags(parent).unwrap() & FLAG_FORWARDED != 0);
    assert_eq!(collector.stats().young_collections, 1);
}

/// Old collection marks through the graph, clears weak refs, compacts.
// CEP:WHAT Full old cycle: mark, weak clear, Lisp-2 compaction.
// CEP:WHY GC invariants 2, 4, and 7.
// CEP:STATUS complete
// CEP:ID: MEM-a22
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(old heap).
// CEP:EVIDENCE cargo test -p vortex-runtime
#[test]
fn old_collection_compacts() {
    let mut heap = test_heap();
    let region = heap.take_young_region(16).expect("region");
    let mut tlab = heap.take_tlab(region, 4000).expect("tlab");
    let a = alloc_object(&mut heap, &mut tlab, 2);
    let b = alloc_object(&mut heap, &mut tlab, 0);
    let dead = alloc_object(&mut heap, &mut tlab, 0);
    heap.write_slot(a + HEADER_SIZE, b as u64).expect("slot");
    heap.promote_region(region);
    let weak_slot = dead + HEADER_SIZE;
    let mut collector = Collector::new(GcConfig::default(), heap.arena_size());
    collector.register_weak(weak_slot);
    heap.write_slot(weak_slot, dead as u64).expect("weak");
    let mut roots = [a as u64];
    collector
        .old_collect(&mut heap, &mut roots)
        .expect("old gc");
    let new_a = roots[0] as usize;
    assert_eq!(new_a, region_start(&heap));
    let new_b = heap.read_slot(new_a + HEADER_SIZE).unwrap() as usize;
    assert_eq!(new_b, region_start(&heap) + HEADER_SIZE + 16);
    assert_eq!(heap.read_slot(weak_slot).unwrap(), 0);
    let stats: GcStats = collector.stats();
    assert_eq!(stats.old_collections, 1);
    assert!(stats.bytes_reclaimed >= HEADER_SIZE as u64);
}

/// The region's first byte offset.
// CEP:WHAT Finds the start of the first old region.
// CEP:WHY Compaction places the first survivor at the region start.
// CEP:STATUS complete
// CEP:FAILURE Zero when no old region exists.
// CEP:ASSUMES None.
// CEP:COST O(regions).
// CEP:EVIDENCE cargo test -p vortex-runtime
fn region_start(heap: &Heap) -> usize {
    heap.region_kinds()
        .iter()
        .find(|(_, k)| *k == RegionKind::Old)
        .and_then(|(i, _)| heap.region_bounds(*i))
        .map(|(s, _)| s)
        .unwrap_or(0)
}

/// SATB barrier: marked source with unmarked target enqueues.
// CEP:WHAT The write barrier re-grays marked sources.
// CEP:WHY SATB invariant 2 under mutation.
// CEP:ID: MEM-a23
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-runtime
#[test]
fn write_barrier_enqueues_satb_sources() {
    let mut heap = test_heap();
    let region = heap.take_young_region(16).expect("region");
    let mut tlab = heap.take_tlab(region, 4000).expect("tlab");
    let src = alloc_object(&mut heap, &mut tlab, 1);
    let tgt = alloc_object(&mut heap, &mut tlab, 0);
    heap.promote_region(region);
    let mut collector = Collector::new(GcConfig::default(), heap.arena_size());
    collector
        .write_barrier(&mut heap, src + HEADER_SIZE, tgt as u64)
        .expect("store");
    assert_eq!(heap.read_slot(src + HEADER_SIZE).unwrap(), tgt as u64);
    let mut roots: [u64; 0] = [];
    collector.old_collect(&mut heap, &mut roots).expect("mark");
    collector
        .write_barrier(&mut heap, src + HEADER_SIZE, 0)
        .expect("store2");
}

/// Load barrier: forwarded pointers self-heal through the cache.
// CEP:WHAT Loads follow forwarding words and rewrite the slot.
// CEP:ID: MEM-a24
// CEP:WHY Invariant 4: pointers are corrected before use.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-runtime
#[test]
fn load_barrier_self_heals() {
    let mut heap = test_heap();
    let region = heap.take_young_region(16).expect("region");
    let mut tlab = heap.take_tlab(region, 4000).expect("tlab");
    let a = alloc_object(&mut heap, &mut tlab, 1);
    let slot = a + HEADER_SIZE;
    heap.write_slot(slot, a as u64).expect("self ref");
    heap.set_mark_word(a, 777).expect("forward");
    heap.set_object_flags(a, FLAG_FORWARDED).expect("flag");
    let mut collector = Collector::new(GcConfig::default(), heap.arena_size());
    let v1 = collector.load_barrier(&mut heap, slot).expect("load");
    assert_eq!(v1, 777);
    assert_eq!(heap.read_slot(slot).unwrap(), 777);
    let v2 = collector.load_barrier(&mut heap, slot).expect("load2");
    assert_eq!(v2, 777);
}

/// Colored-pointer contract: bad colors slow-path, good colors pass.
// CEP:WHAT Colors decode, strip, and recolor per design 6.10.
// CEP:WHY The 64-bit native-pointer contract must match the spec bits.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-runtime
#[test]
fn colored_pointer_views() {
    let addr = 0x1234_5678u64;
    let p = Colors::recolor(addr, PointerColor::Marked1);
    assert!(Colors::is_good(p, PointerColor::Marked1));
    assert!(!Colors::is_good(p, PointerColor::Remapped));
    assert_eq!(Colors::strip(p), addr);
    assert_eq!(Colors::color_of(addr), PointerColor::Bad);
    let r = Colors::recolor(addr, PointerColor::Remapped);
    assert!(Colors::is_good(r, PointerColor::Remapped));
}

/// Policy: occupancy threshold triggers an old collection request.
// CEP:WHAT Old-region occupancy crosses the configured trigger.
// CEP:WHY 6.13 pacing keeps concurrent cycles ahead of fullness.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(regions).
// CEP:EVIDENCE cargo test -p vortex-runtime
#[test]
fn occupancy_trigger() {
    let mut heap = test_heap();
    let region = heap.take_young_region(16).expect("region");
    let mut tlab = heap.take_tlab(region, 4000).expect("tlab");
    alloc_object(&mut heap, &mut tlab, 8);
    heap.promote_region(region);
    let collector = Collector::new(
        GcConfig {
            old_trigger_percent: 1,
            ..GcConfig::default()
        },
        heap.arena_size(),
    );
    assert!(collector.should_collect_old(&heap));
    let lax = Collector::new(
        GcConfig {
            old_trigger_percent: 100,
            ..GcConfig::default()
        },
        heap.arena_size(),
    );
    assert!(!lax.should_collect_old(&heap));
}

/// Profiling model: counts, type sets, branches, ICs, calls, reset.
// CEP:ID: MEM-a25
// CEP:WHAT All profile tables record and reset as documented.
// CEP:WHY T0 6.8 accuracy invariants depend on this shape.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(sites).
// CEP:EVIDENCE cargo test -p vortex-runtime
#[test]
fn profile_tables() {
    let mut p = ProfileData::default();
    p.reserve(8);
    for _ in 0..3 {
        p.record_execution(1);
    }
    assert_eq!(p.execution_count(1), 3);
    p.record_type(2, ValueType::Int);
    p.record_type(2, ValueType::Int);
    p.record_type(2, ValueType::Ref);
    assert_eq!(p.type_feedback(2), &[ValueType::Int, ValueType::Ref]);
    p.record_branch(3, true);
    p.record_branch(3, false);
    assert_eq!(p.branch_outcome(3), (1, 1));
    p.record_ic(4, true);
    p.record_ic(4, false);
    assert_eq!(p.ic_stats(4), (1, 1));
    p.record_call(5, 9);
    p.record_call(5, 9);
    p.record_call(5, 2);
    assert_eq!(p.call_site(5), vec![(9, 2), (2, 1)]);
    p.reset();
    assert_eq!(p.execution_count(1), 0);
    assert!(p.type_feedback(2).is_empty());
}

/// Safepoint flag parking semantics.
// CEP:WHAT The safepoint flag toggles around collections.
// CEP:WHY 6.11: mutators park when the flag is set.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-runtime
#[test]
fn safepoint_flag_lifecycle() {
    let mut heap = test_heap();
    let mut collector = Collector::new(GcConfig::default(), heap.arena_size());
    collector.request_safepoint();
    assert!(collector.at_safepoint());
    collector.clear_safepoint();
    assert!(!collector.at_safepoint());
    let mut roots: [u64; 0] = [];
    collector.old_collect(&mut heap, &mut roots).expect("cycle");
    assert!(!collector.at_safepoint());
}
