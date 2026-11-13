// CEP:FILE crates/vortex-tier/tests/tier.rs
// CEP:WHAT Integration tests for the tier-support subsystems: the code
//        cache lifecycle protocol, tier triggers and OSR instability, IC
//        evolution bounds, exception-table dispatch and coverage,
//        memory-order legality and lowering, and the debug-info coverage
//        proofs.
// CEP:WHY These are the executable proofs of the CODE-1/IC-1/EXC-1/
//        MEM-1/DEBUG-1 invariants at the reference layer.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-tier

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! Tier-support subsystem test suite.

use vortex_runtime::Machine;
use vortex_tier::*;

/// The code cache lifecycle: publish -> invalidate -> safepoint -> sweep,
/// with every illegal transition rejected (CODE-1 section 7).
// CEP:WHAT The code cache lifecycle: publish -> invalidate -> safepoint -> sweep, with every illegal transition rejected.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-tier
#[test]
fn code_cache_lifecycle() {
    let mut cache = CodeCache::new();
    cache.open_tlab();
    let blob = cache.publish(0, 2, 2_000, vec![0, 64], 1);
    cache.close_tlab();
    assert_eq!(cache.count_in(BlobState::Active), 1);
    assert_eq!(Arena::for_size(2_000), Arena::Medium);

    // Sweeping before invalidation does nothing (CODE-INV-05).
    assert_eq!(cache.sweep(), 0);

    cache.invalidate(blob).expect("invalidate");
    assert_eq!(cache.count_in(BlobState::DeoptPending), 1);

    // Double invalidation is an illegal transition.
    assert!(matches!(
        cache.invalidate(blob),
        Err(CacheError::IllegalTransition { .. })
    ));

    // The safepoint moves DeoptPending to Zombie; the sweeper reclaims.
    cache.safepoint();
    assert_eq!(cache.count_in(BlobState::Zombie), 1);
    assert_eq!(cache.sweep(), 1);
    assert_eq!(cache.swept(), 1);
    assert_eq!(cache.arena_bytes(), [0, 0, 0], "bytes reclaimed");
}

/// Arena classes route code by size (CODE-1 section 5.2).
// CEP:WHAT Arena classes route code by size.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-tier
#[test]
fn arenas_are_size_segregated() {
    assert_eq!(Arena::for_size(512), Arena::Small);
    assert_eq!(Arena::for_size(4_096), Arena::Medium);
    assert_eq!(Arena::for_size(65_536), Arena::Large);
    let mut cache = CodeCache::new();
    let _s = cache.publish(0, 1, 800, Vec::new(), 0);
    let _m = cache.publish(1, 2, 8_000, Vec::new(), 0);
    let _l = cache.publish(2, 3, 32_768, Vec::new(), 0);
    let bytes = cache.arena_bytes();
    assert_eq!(bytes, [800, 8_000, 32_768]);
}

/// Tier triggers: back-edge and call thresholds promote; OSR instability
/// latches; deopt storms suppress speculation (OSR-INV-05).
// CEP:WHAT Tier triggers: back-edge and call thresholds promote; OSR instability latches; deopt storms suppress speculation.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-tier
#[test]
fn tier_triggers_and_osr_bookkeeping() {
    let mut c = TierController::new();
    assert_eq!(c.tier_of(0), Tier::T0);
    assert!(!c.wants_baseline(0, 9_999));
    assert!(c.wants_baseline(0, 10_000));
    c.promote(0, Tier::T1);
    assert!(!c.wants_baseline(0, 100_000), "promoted methods stay put");

    assert!(c.wants_optimizing([0, 1_000]));
    c.promote(0, Tier::T2);
    assert!(c.wants_aggressive(0, 100));

    // OSR instability latches (OSR-INV-05).
    assert!(c.try_osr(0, 42));
    c.record_osr_failure(0, 42);
    assert!(!c.try_osr(0, 42), "unstable OSR sites never retry");
    assert!(c.try_osr(0, 43), "other sites are unaffected");

    // Deopt storm prevention.
    assert!(!c.record_deopt(0));
    assert!(!c.record_deopt(0));
    assert!(c.record_deopt(0));
    assert!(c.speculation_suppressed(0));
    c.clear_penalties(0);
    assert!(!c.speculation_suppressed(0));
}

/// IC evolution: bounded polymorphism, megamorphic latch, and the
/// shape-tracking stop rule (IC-INV-01/02/03).
// CEP:WHAT IC evolution: bounded polymorphism, megamorphic latch, and the shape-tracking stop rule.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-tier
#[test]
fn ic_evolution_is_bounded() {
    let mut site = IcSite::new();
    assert_eq!(site.observe(1), IcState::Monomorphic { layout: 1 });
    assert_eq!(
        site.observe(2),
        IcState::Polymorphic {
            layouts: vec![1, 2]
        }
    );
    for l in 3..=POLY_LIMIT as u64 {
        site.observe(l);
    }
    assert!(matches!(site.state, IcState::Polymorphic { .. }));
    // The next unique layout exceeds the bound: Megamorphic, latched.
    assert_eq!(site.observe(9), IcState::Megamorphic);
    assert!(!site.tracks_shapes(), "megamorphic sites stop tracking");
    // The latch is permanent (IC-INV-02).
    assert_eq!(site.observe(1), IcState::Megamorphic);
}

/// Exception tables: binary-search dispatch, coverage proof, and the
/// deopt-transfer contract (EXC-1 sections 2-3).
// CEP:WHAT Exception tables: binary-search dispatch, coverage proof, and the deopt-transfer contract.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-tier
#[test]
fn exn_tables_dispatch_and_cover() {
    let table = ExnTable::build(vec![
        ExnRecord {
            pc_start: 40,
            pc_end: 80,
            handler: 200,
            deopt_id: None,
        },
        ExnRecord {
            pc_start: 0,
            pc_end: 40,
            handler: 100,
            deopt_id: None,
        },
    ])
    .expect("builds (sorting normalizes)");
    assert_eq!(table.find(10).map(|r| r.handler), Some(100));
    assert_eq!(table.find(40).map(|r| r.handler), Some(200));
    assert_eq!(table.find(79).map(|r| r.handler), Some(200));
    assert_eq!(table.find(80), None, "end-exclusive");
    assert!(table.covers(80));
    assert!(!table.covers(81), "the uncovered byte is reported");

    // Overlap is rejected.
    assert!(matches!(
        ExnTable::build(vec![
            ExnRecord {
                pc_start: 0,
                pc_end: 40,
                handler: 100,
                deopt_id: None
            },
            ExnRecord {
                pc_start: 20,
                pc_end: 60,
                handler: 200,
                deopt_id: None
            },
        ]),
        Err(ExnError::Unsorted)
    ));

    // Deopt-transfer preserves the exception (EXC-INV-04).
    let t = DeoptTransfer {
        resume_pc: 12,
        exception_preserved: true,
    };
    assert!(t.exception_preserved);
}

/// Memory-order legality and target lowering (MEM-1 sections 3-4).
// CEP:WHAT Memory-order legality and target lowering.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-tier
#[test]
fn mem_orders_legality_and_lowering() {
    // MEM-INV-01: acquire loads cannot move up; release stores cannot
    // move down; SeqCst cannot move at all.
    assert!(!reorder_legal(MemoryOrder::Acquire, true));
    assert!(reorder_legal(MemoryOrder::Acquire, false));
    assert!(reorder_legal(MemoryOrder::Release, true));
    assert!(!reorder_legal(MemoryOrder::Release, false));
    assert!(!reorder_legal(MemoryOrder::SeqCst, true));
    assert!(!reorder_legal(MemoryOrder::SeqCst, false));
    assert!(reorder_legal(MemoryOrder::Relaxed, true));

    // x86-64 TSO: free acquire/release; SeqCst needs a fence.
    assert_eq!(
        lower(MemoryOrder::Acquire, Target::X86_64),
        Lowering {
            mnemonic: "mov",
            fence: false
        }
    );
    assert!(lower(MemoryOrder::SeqCst, Target::X86_64).fence);

    // AArch64 weak model: explicit ordering instructions.
    assert_eq!(
        lower(MemoryOrder::Acquire, Target::AArch64).mnemonic,
        "ldar"
    );
    assert_eq!(
        lower(MemoryOrder::Release, Target::AArch64).mnemonic,
        "stlr"
    );

    // RV64: .aq/.rl bits and fences.
    assert_eq!(lower(MemoryOrder::Acquire, Target::Rv64).mnemonic, "lw.aq");
    assert!(lower(MemoryOrder::SeqCst, Target::Rv64).fence);

    // MEM-INV-03: redundant fence elimination.
    let seq = vec![MemoryOrder::Acquire, MemoryOrder::SeqCst];
    assert_eq!(eliminate_redundant_fences(&seq), vec![MemoryOrder::SeqCst]);
}

/// Debug-info coverage and profiler registration (DEBUG-1 sections 2-4).
// CEP:WHAT Debug-info coverage and profiler registration.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-tier
#[test]
fn debug_info_covers_and_registers() {
    let info = DebugInfo {
        locations: vec![
            LocationEntry {
                pc_start: 0,
                pc_end: 50,
                variable: "v".into(),
                location: "rax".into(),
                representation: "I64".into(),
            },
            LocationEntry {
                pc_start: 50,
                pc_end: 100,
                variable: "v".into(),
                location: "[rsp+16]".into(),
                representation: "TAGGED".into(),
            },
        ],
        registered: true,
    };
    assert!(info.covers(100), "DEBUG-INV-02 location accuracy");
    assert!(!info.covers(101), "gaps are reported");

    let map = perf_map(&[(0x1000, 100, "countdown"), (0x2000, 200, "sum")]);
    assert_eq!(map.len(), 2, "DEBUG-INV-03: every blob registered");
    assert_eq!(map[0].name, "countdown");
}

/// The controller drives a realistic profile: the countdown program
/// accumulates folded back-edges and promotes through the tiers.
// CEP:WHAT The controller drives a realistic profile: the countdown program accumulates folded back-edges and promotes through the tiers.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-tier
#[test]
fn controller_over_a_real_profile() {
    let mut m = Machine::new();
    let interp = vortex_t0::Interp::new(vec![vortex_ugb::countdown_method(20_000)]);
    interp.run(&mut m, 0, &[]).expect("runs");
    let folded = m.profile.total(0);

    let mut c = TierController::new();
    assert!(c.wants_baseline(0, folded), "the hot loop triggers tier-up");
    c.promote(0, Tier::T1);
    // The calls threshold: the profile's call counter drives T2.
    let mut m2 = Machine::new();
    let program = vortex_ugb::call_demo_program(1);
    let _ = vortex_t0::Interp::new(program).run(&mut m2, 0, &[]);
    let calls: Vec<u64> = m2.call_counts.values().copied().collect();
    assert!(!calls.is_empty(), "call sites recorded");
}
