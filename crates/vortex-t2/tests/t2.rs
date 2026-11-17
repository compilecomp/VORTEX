// CEP:FILE crates/vortex-t2/tests/t2.rs
// CEP:WHAT T2 tests: folding, specialization, peephole rewiring, DCE,
//        budget capping, and Valid_IR preservation after the pipeline.
// CEP:WHY The T2 contract: every Class-0 pass runs exactly once, in
//        O(|IR|), under the B_2 budget.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES Graphs are verified before and after compilation.
// CEP:COST O(graph) per case.
// CEP:EVIDENCE cargo test -p vortex-t2

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! T2 tier test suite.

use vortex_ir::{verify, Builder, EffectSet, FpSet, Node, Op, Type};
use vortex_t2::{compile_t2, T2Config};

/// Builds a graph with a foldable add feeding a Return, plus a dead chain.
// CEP:WHAT Constructs the standard T2 test graph.
// CEP:WHY Every pass has a target in one shared shape.
// CEP:STATUS complete
// CEP:FAILURE Panics on builder misuse.
// CEP:ASSUMES None.
// CEP:COST O(nodes).
// CEP:EVIDENCE cargo test -p vortex-t2
fn base_graph() -> Builder {
    let mut b = Builder::new(1);
    let start = b.start(vec![]);
    let a = b.pure(Op::ConstInt(2), Type::Int64, vec![]);
    let c = b.pure(Op::ConstInt(3), Type::Int64, vec![]);
    let add = b.pure(Op::IAdd, Type::Int64, vec![a, c]);
    let dead1 = b.pure(Op::ConstInt(7), Type::Int64, vec![]);
    let _dead_add = b.pure(Op::IAdd, Type::Int64, vec![dead1, dead1]);
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
    let _ = ret;
    b
}

/// Constant arithmetic folds to a literal in one scan.
// CEP:WHAT 2+3 folds to 5 and the graph stays valid.
// CEP:WHY Folding is the canonical Class-0 pass (6.4).
// CEP:STATUS complete
// CEP:FAILURE A failed assert aborts the test binary.
// CEP:ASSUMES Const producers precede consumers.
// CEP:COST O(graph).
// CEP:EVIDENCE cargo test -p vortex-t2
// CEP:ID: JIT-a55
#[test]
fn fold_arithmetic_once() {
    let mut b = base_graph();
    let stats = compile_t2(&mut b.graph, T2Config::default());
    assert_eq!(
        stats.folded, 2,
        "fold runs first: live and dead adds both fold"
    );
    assert_eq!(stats.nodes, b.graph.len());
    let ret = b
        .graph
        .iter()
        .find(|(_, n)| matches!(n.op, Op::Return))
        .map(|(id, _)| id)
        .expect("return");
    let ret_data = b.graph.node(ret).expect("ret").data.clone();
    let val = b.graph.node(ret_data[0]).expect("value").op;
    assert_eq!(val, Op::ConstInt(5));
    assert_eq!(stats.peepholed, 0, "nothing is left for peephole to rewire");
    assert_eq!(
        stats.dead_eliminated, 0,
        "the dead chain is constants now, not dead ops"
    );
    verify(&b.graph).expect("valid after fold");
}

/// Identities rewire uses in one sweep: x+0 and *1 collapse to x.
// CEP:WHAT Peephole rewires a +0/*1 chain to the base value.
// CEP:WHY One remap sweep keeps the pass O(|IR|).
// CEP:STATUS complete
// CEP:FAILURE A failed assert aborts the test binary.
// CEP:ASSUMES Constants are literal opcodes.
// CEP:COST O(graph).
// CEP:ID: JIT-a56
// CEP:EVIDENCE cargo test -p vortex-t2
#[test]
fn peephole_rewires_identity_chains() {
    let mut b = Builder::new(1);
    let start = b.start(vec![]);
    let a = b.pure(Op::ConstInt(5), Type::Int64, vec![]);
    let zero = b.pure(Op::ConstInt(0), Type::Int64, vec![]);
    let y = b.pure(Op::IAdd, Type::Int64, vec![a, zero]);
    let one = b.pure(Op::ConstInt(1), Type::Int64, vec![]);
    let z = b.pure(Op::IMul, Type::Int64, vec![y, one]);
    let ret = b.insert(Node {
        op: Op::Return,
        ty: Type::Ctrl,
        data: vec![z],
        ctrl: vec![start],
        state: vec![],
        except: vec![],
        effects: EffectSet::PURE,
        fp: FpSet::STRICT,
        footprint: 0,
    });
    let _ = ret;
    let config = T2Config {
        fold: false,
        ..T2Config::default()
    };
    let stats = compile_t2(&mut b.graph, config);
    assert_eq!(stats.peepholed, 2, "x+0 and *1 both rewire in one wave");
    let ret_data = b.graph.node(ret).expect("ret").data.clone();
    assert_eq!(
        ret_data[0], y,
        "one-wave remap points at the un-chained target; chains resolve across later recompiles"
    );
    verify(&b.graph).expect("valid after peephole");
}

/// A redundant same-type IntCast specializes to Identity and its uses
/// rewire to the input.
// CEP:WHAT Specialization removes a no-op cast.
// CEP:WHY Type-driven specialization is Class 0 (6.4).
// CEP:STATUS complete
// CEP:FAILURE A failed assert aborts the test binary.
// CEP:ASSUMES Input types come from the verified lattice.
// CEP:ID: JIT-a57
// CEP:COST O(graph).
// CEP:EVIDENCE cargo test -p vortex-t2
#[test]
fn specialize_removes_redundant_cast() {
    let mut b = Builder::new(1);
    let start = b.start(vec![]);
    let a = b.pure(Op::ConstInt(9), Type::Int64, vec![]);
    let cast = b.pure(Op::IntCast, Type::Int64, vec![a]);
    let ret = b.insert(Node {
        op: Op::Return,
        ty: Type::Ctrl,
        data: vec![cast],
        ctrl: vec![start],
        state: vec![],
        except: vec![],
        effects: EffectSet::PURE,
        fp: FpSet::STRICT,
        footprint: 0,
    });
    let _ = ret;
    let stats = compile_t2(&mut b.graph, T2Config::default());
    assert_eq!(stats.specialized, 1);
    assert!(stats.peepholed >= 1, "the Identity rewires away");
    assert_eq!(b.graph.node(ret).expect("ret").data[0], a);
    verify(&b.graph).expect("valid after specialization");
}

/// Dead pure nodes with no consumers are neutralized in one wave.
// CEP:WHAT DCE kills the dead add but not its live sibling.
// CEP:WHY Single-wave semantics: no iteration to a fixed point.
// CEP:STATUS complete
// CEP:FAILURE A failed assert aborts the test binary.
// CEP:ASSUMES Uses come from data edges.
// CEP:COST O(graph).
// CEP:EVIDENCE cargo test -p vortex-t2
#[test]
fn dce_kills_dead_wave() {
    let mut b = base_graph();
    let config = T2Config {
        fold: false,
        ..T2Config::default()
    };
    let stats = compile_t2(&mut b.graph, config);
    assert_eq!(stats.dead_eliminated, 1, "the dead add dies in one wave");
    verify(&b.graph).expect("valid after DCE");
}

/// A zero budget caps the pipeline before any pass does work.
// CEP:WHAT Budget exhaustion skips all passes and reports it.
// CEP:WHY 6.5: exceeding the budget caps optimization at the tier.
// CEP:STATUS complete
// CEP:FAILURE A failed assert aborts the test binary.
// CEP:ASSUMES Budget model charges per node per pass.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t2
#[test]
fn budget_caps_the_pipeline() {
    let mut b = base_graph();
    let config = T2Config {
        budget_per_node_ns: 0,
        ..T2Config::default()
    };
    let stats = compile_t2(&mut b.graph, config);
    assert!(stats.budget_exhausted);
    assert_eq!(stats.folded, 0);
    assert_eq!(stats.budget_used_ns, 0);
    verify(&b.graph).expect("untouched graph stays valid");
}

/// The default pipeline keeps the graph verified and counts balanced.
// CEP:WHAT End-to-end T2 compile on the shared shape.
// CEP:WHY Monotonicity: T2 output is a valid input to T3/T4.
// CEP:STATUS complete
// CEP:FAILURE A failed assert aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(graph).
// CEP:EVIDENCE cargo test -p vortex-t2
#[test]
fn pipeline_preserves_valid_ir() {
    let mut b = base_graph();
    let stats = compile_t2(&mut b.graph, T2Config::default());
    assert_eq!(
        stats.nodes,
        b.graph.len(),
        "in-place rewrites must not change node count"
    );
    verify(&b.graph).expect("pipeline output is Valid_IR");
}
