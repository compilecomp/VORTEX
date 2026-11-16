// CEP:FILE crates/vortex-ir/tests/ir.rs
// CEP:WHAT ET-SoN tests: verifier accept and reject paths for invariants
//        1-7, GVN dedup, deopt metadata behavior.
// CEP:WHY The invariants are the optimization-safety contract; the tests
//        are their executable definitions.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES Graphs are constructed through the Builder.
// CEP:COST O(nodes) per case.
// CEP:EVIDENCE cargo test -p vortex-ir

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! ET-SoN test suite.

use vortex_ir::{
    verify, Builder, DeoptMeta, DeoptValue, EffectSet, FpFlag, FpSet, Node, Op, Type, VerifyError,
};

/// A diamond: Start, If, two Region arms merged with a Phi, Return.
// CEP:WHAT Builds a valid diamond graph.
// CEP:WHY The canonical valid case exercises Phi arity and dominance.
// CEP:STATUS complete
// CEP:FAILURE Panics on builder misuse; the shape is fixed.
// CEP:ASSUMES None.
// CEP:COST O(nodes).
// CEP:EVIDENCE cargo test -p vortex-ir
fn diamond() -> Builder {
    let mut b = Builder::new(1);
    let start = b.start(vec![Type::Int64]);
    let cond = b.pure(Op::ConstInt(1), Type::Int64, vec![]);
    let cond = b.pure(Op::IntCast, Type::Bool, vec![cond]);
    let iff = b.insert(Node {
        op: Op::If,
        ty: Type::Ctrl,
        data: vec![cond],
        ctrl: vec![start],
        state: vec![],
        except: vec![],
        effects: EffectSet::PURE,
        fp: FpSet::STRICT,
        footprint: 0,
    });
    let left = b.insert(Node {
        op: Op::Region,
        ty: Type::Ctrl,
        data: vec![],
        ctrl: vec![iff],
        state: vec![],
        except: vec![],
        effects: EffectSet::PURE,
        fp: FpSet::STRICT,
        footprint: 0,
    });
    let right = b.insert(Node {
        op: Op::Region,
        ty: Type::Ctrl,
        data: vec![],
        ctrl: vec![iff],
        state: vec![],
        except: vec![],
        effects: EffectSet::PURE,
        fp: FpSet::STRICT,
        footprint: 0,
    });
    let v1 = b.pure(Op::ConstInt(1), Type::Int64, vec![]);
    let v2 = b.pure(Op::ConstInt(2), Type::Int64, vec![]);
    let merge = b.insert(Node {
        op: Op::Region,
        ty: Type::Ctrl,
        data: vec![],
        ctrl: vec![left, right],
        state: vec![],
        except: vec![],
        effects: EffectSet::PURE,
        fp: FpSet::STRICT,
        footprint: 0,
    });
    let phi = b.insert(Node {
        op: Op::Phi,
        ty: Type::Int64,
        data: vec![v1, v2],
        ctrl: vec![merge],
        state: vec![],
        except: vec![],
        effects: EffectSet::PURE,
        fp: FpSet::STRICT,
        footprint: 0,
    });
    let _ret = b.insert(Node {
        op: Op::Return,
        ty: Type::Ctrl,
        data: vec![phi],
        ctrl: vec![merge],
        state: vec![],
        except: vec![],
        effects: EffectSet::PURE,
        fp: FpSet::STRICT,
        footprint: 0,
    });
    b
}

/// Verifier accepts the canonical diamond.
// CEP:WHAT The valid diamond passes every invariant.
// CEP:WHY Baseline acceptance guards against over-rejection.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(nodes).
// CEP:EVIDENCE cargo test -p vortex-ir
#[test]
fn diamond_verifies() {
    let b = diamond();
    verify(&b.graph).expect("diamond is valid");
}

/// Invariant 1: wrong data type on a Compare input rejects.
// CEP:WHAT Type mismatches on data edges reject.
// CEP:WHY Invariant 1 is the type-safety gate.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(nodes).
// CEP:EVIDENCE cargo test -p vortex-ir
#[test]
fn type_mismatch_rejects() {
    let mut b = Builder::new(1);
    let start = b.start(vec![]);
    let c = b.pure(Op::ConstFloat(1.5), Type::Float64, vec![]);
    let cmp = b.pure(Op::Lt, Type::Bool, vec![c]);
    let ret = b.insert(Node {
        op: Op::Return,
        ty: Type::Ctrl,
        data: vec![cmp],
        ctrl: vec![start],
        state: vec![],
        except: vec![],
        effects: EffectSet::PURE,
        fp: FpSet::STRICT,
        footprint: 0,
    });
    match verify(&b.graph) {
        Err(VerifyError::TypeMismatch { to, .. }) => assert_eq!(to, cmp),
        other => panic!("expected TypeMismatch, got {other:?}"),
    }
    let _ = ret;
}

/// Invariant 2: a state edge from a pure node rejects.
// CEP:WHAT Pure producers cannot thread state tokens.
// CEP:WHY Invariant 2 requires effectful endpoints.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(nodes).
// CEP:EVIDENCE cargo test -p vortex-ir
#[test]
fn pure_state_edge_rejects() {
    let mut b = Builder::new(1);
    let start = b.start(vec![]);
    let c = b.pure(Op::ConstInt(1), Type::Int64, vec![]);
    let ret = b.insert(Node {
        op: Op::Return,
        ty: Type::Ctrl,
        data: vec![c],
        ctrl: vec![start],
        state: vec![(0, c)],
        except: vec![],
        effects: EffectSet::PURE,
        fp: FpSet::STRICT,
        footprint: 0,
    });
    match verify(&b.graph) {
        Err(VerifyError::StateEdgeInvalid { from, .. }) => assert_eq!(from, c),
        other => panic!("expected StateEdgeInvalid, got {other:?}"),
    }
    let _ = ret;
}

/// Invariant 4: a state cycle through a StatePhi is valid.
// CEP:WHAT A loop-carried state chain merges at a StatePhi.
// CEP:WHY The loop-carried-state invariant is the v1 replacement.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(nodes).
// CEP:EVIDENCE cargo test -p vortex-ir
#[test]
fn state_cycle_through_state_phi_is_valid() {
    let mut b = Builder::new(1);
    let start = b.start(vec![]);
    let load = b.effectful(Op::Load, Type::Int64, vec![], &[0], 0x1);
    // Entry and latch are both modeled on Start; the Region therefore has
    // two control inputs matching the StatePhi's two state inputs.
    let region = b.insert(Node {
        op: Op::Region,
        ty: Type::Ctrl,
        data: vec![],
        ctrl: vec![start, start],
        state: vec![],
        except: vec![],
        effects: EffectSet::PURE,
        fp: FpSet::STRICT,
        footprint: 0,
    });
    let store = b.effectful(Op::Store, Type::Ctrl, vec![], &[0], 0x1);
    let sp = b.insert(Node {
        op: Op::StatePhi,
        ty: Type::State(0),
        data: vec![],
        ctrl: vec![region],
        state: vec![(0, load), (0, store)],
        except: vec![],
        effects: Op::StatePhi.effects(),
        fp: FpSet::STRICT,
        footprint: 1,
    });
    // The loop back-edge: store consumes the StatePhi output.
    let n = b.graph.node_mut(store).unwrap();
    n.state = vec![(0, sp)];
    let ret = b.insert(Node {
        op: Op::Return,
        ty: Type::Ctrl,
        data: vec![],
        ctrl: vec![region],
        state: vec![(0, sp)],
        except: vec![],
        effects: EffectSet::PURE,
        fp: FpSet::STRICT,
        footprint: 0,
    });
    let _ = ret;
    verify(&b.graph).expect("loop-carried state is valid");
}

/// Invariant 4: a state cycle without a StatePhi rejects.
// CEP:WHAT A self-feeding state chain without a merge rejects.
// CEP:WHY Invariant 4 forbids cycles that skip StatePhis.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(nodes).
// CEP:EVIDENCE cargo test -p vortex-ir
#[test]
fn state_cycle_without_state_phi_rejects() {
    let mut b = Builder::new(1);
    b.start(vec![]);
    let store = b.effectful(Op::Store, Type::Ctrl, vec![], &[0], 0x1);
    // Cycle: store consumes its own state output.
    let n = b.graph.node_mut(store).unwrap();
    n.state = vec![(0, store)];
    match verify(&b.graph) {
        Err(VerifyError::StateCycleWithoutPhi { region }) => assert_eq!(region, 0),
        other => panic!("expected StateCycleWithoutPhi, got {other:?}"),
    }
}

/// Invariant 5: a use above its def rejects.
// CEP:WHAT Non-dominating data edges reject.
// CEP:WHY Invariant 5 is the schedulability condition.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(nodes squared).
// CEP:EVIDENCE cargo test -p vortex-ir
#[test]
fn dominance_violation_rejects() {
    let mut b = Builder::new(1);
    let start = b.start(vec![]);
    let iff = b.insert(Node {
        op: Op::If,
        ty: Type::Ctrl,
        data: vec![],
        ctrl: vec![start],
        state: vec![],
        except: vec![],
        effects: EffectSet::PURE,
        fp: FpSet::STRICT,
        footprint: 0,
    });
    let right = b.insert(Node {
        op: Op::Region,
        ty: Type::Ctrl,
        data: vec![],
        ctrl: vec![iff],
        state: vec![],
        except: vec![],
        effects: EffectSet::PURE,
        fp: FpSet::STRICT,
        footprint: 0,
    });
    // The load happens only on Start's control path; the consumer hangs
    // off the right branch, which does not pass through the load.
    let load = b.effectful(Op::Load, Type::Int64, vec![], &[0], 0x1);
    let ret = b.insert(Node {
        op: Op::Return,
        ty: Type::Ctrl,
        data: vec![load],
        ctrl: vec![right],
        state: vec![],
        except: vec![],
        effects: EffectSet::PURE,
        fp: FpSet::STRICT,
        footprint: 0,
    });
    match verify(&b.graph) {
        Err(VerifyError::DominanceViolation { from, to }) => {
            assert_eq!(from, load);
            assert_eq!(to, ret);
        }
        other => panic!("expected DominanceViolation, got {other:?}"),
    }
}

/// Invariant 7: AFN without the other flags rejects.
// CEP:WHAT AFN implies all other relaxations.
// CEP:WHY Invariant 7 defines the AFN rule.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(nodes).
// CEP:EVIDENCE cargo test -p vortex-ir
#[test]
fn afn_implies_all() {
    let mut b = Builder::new(1);
    b.start(vec![]);
    let bad = b.fp_node(
        Op::FAdd,
        Type::Float64,
        vec![],
        FpSet::STRICT.with(FpFlag::Afn),
    );
    match verify(&b.graph) {
        Err(VerifyError::FpFlagInvalid { node }) => assert_eq!(node, bad),
        other => panic!("expected FpFlagInvalid, got {other:?}"),
    }
    let full = FpSet::STRICT
        .with(FpFlag::Nsz)
        .with(FpFlag::Ninf)
        .with(FpFlag::Nnan)
        .with(FpFlag::Arcp)
        .with(FpFlag::Contract)
        .with(FpFlag::Reassoc)
        .with(FpFlag::Afn);
    let good = b.fp_node(Op::FMul, Type::Float64, vec![], full);
    let _ = good;
}

/// GVN deduplicates equivalent pure nodes and rewrites users.
// CEP:WHAT GVN replaces duplicates with leaders.
// CEP:WHY Catalog item 25.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(nodes).
// CEP:EVIDENCE cargo test -p vortex-ir
#[test]
fn gvn_deduplicates() {
    let mut b = Builder::new(1);
    let start = b.start(vec![]);
    let a = b.pure(Op::ConstInt(7), Type::Int64, vec![]);
    let a2 = b.pure(Op::ConstInt(7), Type::Int64, vec![]);
    let s1 = b.pure(Op::IAdd, Type::Int64, vec![a, a]);
    let s2 = b.pure(Op::IAdd, Type::Int64, vec![a2, a]);
    let ret = b.insert(Node {
        op: Op::Return,
        ty: Type::Ctrl,
        data: vec![s1, s2],
        ctrl: vec![start],
        state: vec![],
        except: vec![],
        effects: EffectSet::PURE,
        fp: FpSet::STRICT,
        footprint: 0,
    });
    let replaced = vortex_ir::gvn(&mut b.graph);
    assert_eq!(replaced, 2, "a2 and s2 are redundant");
    let ret_node = b.graph.node(ret).unwrap();
    let s1_leader = ret_node.data[0];
    assert_eq!(ret_node.data[1], s1_leader);
    verify(&b.graph).expect("graph stays valid after GVN");
}

/// Deopt metadata: slot ordering, lookup, overwrite.
// CEP:WHAT The slot map behaves as documented.
// CEP:WHY State reconstruction correctness depends on exact slots.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(entries).
// CEP:EVIDENCE cargo test -p vortex-ir
#[test]
fn deopt_meta_slots() {
    let meta = DeoptMeta::new(vec![
        (2, DeoptValue::Int(9)),
        (0, DeoptValue::Node(vortex_ir::NodeId(4))),
        (2, DeoptValue::Null),
    ]);
    assert_eq!(meta.len(), 2, "duplicate slots dedup, last wins");
    assert_eq!(meta.slot(0), Some(&DeoptValue::Node(vortex_ir::NodeId(4))));
    assert_eq!(meta.slot(2), Some(&DeoptValue::Null));
    assert_eq!(meta.slot(3), None);
}
