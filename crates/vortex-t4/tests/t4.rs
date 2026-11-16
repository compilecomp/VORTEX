// CEP:FILE crates/vortex-t4/tests/t4.rs
// CEP:WHAT T4 tests: trace recording, guard disabling at the deopt limit,
//        pipeline rounds with budget capping, and tier monotonicity.
// CEP:WHY The T4 contract: hot traces, bounded Class-2 work, and
//        speculation that gives up after repeated violations.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES Graphs are verified before compilation.
// CEP:COST O(graph) per case.
// CEP:EVIDENCE cargo test -p vortex-t4

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! T4 tier test suite.

use vortex_ir::{Builder, EffectSet, FpSet, Node, Op, Type};
use vortex_t4::{compile_t4, trace, T4Config, TraceRecorder};

/// Builds a graph with foldable and dead nodes.
// CEP:WHAT Constructs the T4 test graph.
// CEP:WHY Rounds must progress and then stop.
// CEP:STATUS complete
// CEP:FAILURE Panics on builder misuse.
// CEP:ASSUMES None.
// CEP:COST O(nodes).
// CEP:EVIDENCE cargo test -p vortex-t4
fn test_graph() -> Builder {
    let mut b = Builder::new(1);
    let start = b.start(vec![]);
    let a = b.pure(Op::ConstInt(2), Type::Int64, vec![]);
    let c = b.pure(Op::ConstInt(3), Type::Int64, vec![]);
    let add = b.pure(Op::IAdd, Type::Int64, vec![a, c]);
    let dead1 = b.pure(Op::ConstInt(7), Type::Int64, vec![]);
    let dead2 = b.pure(Op::IAdd, Type::Int64, vec![dead1, dead1]);
    let ret = b.insert(Node {
        op: Op::Return,
        ty: Type::Ctrl,
        data: vec![add],
        ctrl: vec![start],
        state: vec![],
        except: vec![],
        effects: EffectSet::PURE,
        fp: FpSet::STRICT,
        footprint: 0,
    });
    let _ = dead2;
    let _ = ret;
    b
}

/// Traces record hot sites and iterations.
// CEP:WHAT Trace bookkeeping behaves as documented.
// CEP:WHY Compilation follows profiling marks.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t4
#[test]
fn trace_recording() {
    let mut rec = TraceRecorder::new();
    assert!(!rec.has_trace(0));
    rec.record_hot(0, &[4, 8], 3);
    rec.record_hot(0, &[8, 12], 2);
    assert!(rec.has_trace(0));
    let t = rec.trace(0).expect("trace");
    assert_eq!(t.hot_sites, vec![4, 8, 12]);
    assert_eq!(t.iterations, 5);
}

/// Guards disable after the deopt limit.
// CEP:WHAT Ten failures disable the guard.
// CEP:WHY Deopt design 6.9 disables speculation after ~10 deopts.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(10).
// CEP:EVIDENCE cargo test -p vortex-t4
#[test]
fn guard_disables_after_limit() {
    let mut rec = TraceRecorder::new();
    rec.record_hot(1, &[0], 1);
    let id = rec
        .add_guard(1, trace::Assumption::IntTypes { site: 0 })
        .expect("guard");
    for _ in 0..9 {
        assert!(rec.guard_failed(1, id), "guard still enabled");
    }
    assert!(!rec.guard_failed(1, id), "guard disabled at the limit");
    assert_eq!(rec.disabled_count(1), 1);
}

/// The pipeline runs rounds and the budget can cap it.
// CEP:WHAT Normal graphs converge; tiny budgets cap the run.
// CEP:WHY 6.5: budget adherence with early capping.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(graph squared).
// CEP:EVIDENCE cargo test -p vortex-t4
#[test]
fn pipeline_rounds_and_budget() {
    let mut b = test_graph();
    let stats = compile_t4(&mut b.graph, T4Config::default());
    assert!(stats.rounds >= 1);
    assert!(stats.folded >= 1, "2+3 folds");
    assert!(!stats.budget_capped, "small graph fits the budget");
    assert_eq!(
        b.graph.node(vortex_ir::NodeId(3)).unwrap().op,
        Op::ConstInt(5),
        "add folded"
    );

    // A budget of zero caps the pipeline immediately.
    let mut b2 = test_graph();
    let stats2 = compile_t4(
        &mut b2.graph,
        T4Config {
            budget_per_node: 0,
            max_rounds: 4,
        },
    );
    assert!(stats2.budget_capped);
}

/// Tier monotonicity: T4 output is at least as optimized as one pipeline
/// pass.
// CEP:WHAT T4 rounds include the full pass set each round.
// CEP:WHY Passes(T2) is a subset of Passes(T4).
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(graph squared).
// CEP:EVIDENCE cargo test -p vortex-t4
#[test]
fn tier_monotonicity() {
    let mut b = test_graph();
    let t4 = compile_t4(&mut b.graph, T4Config::default());
    let single = vortex_opt::passes::run_pipeline(&mut test_graph().graph);
    assert!(
        t4.folded + t4.algebraic + t4.bitwise >= single.folded + single.algebraic + single.bitwise,
        "T4 applies at least the T2 pass set"
    );
    // Dead chain collapses across rounds: dead2's producer dies too.
    assert!(t4.dead >= 2, "chained dead nodes collapse: {t4}");
}
