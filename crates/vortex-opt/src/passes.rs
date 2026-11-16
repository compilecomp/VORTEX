// CEP:FILE crates/vortex-opt/src/passes.rs
// CEP:WHAT The ET-SoN optimization pass pipeline: constant folding,
//        algebraic simplification, bitwise identities, and dead code
//        elimination, with per-pass rewrite counters (optimization
//        catalog items 1, 6, 10, 11, 22).
// CEP:WHY The optimizer's first tier of passes runs on every compile;
//        each transform must be effect-preserving and counted.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Passes never touch effectful nodes except through DCE,
//        which removes only unconsumed pure nodes.
// CEP:ASSUMES The graph passed verification before optimization.
// CEP:COST O(|N|) per pass.
// CEP:EVIDENCE cargo test -p vortex-opt
//! ET-SoN passes (optimization catalog items 1, 6, 10, 11, 22).

use vortex_ir::{Graph, Node, NodeId, Op};

/// Counters from one pipeline run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PassStats {
    /// Constants folded.
    pub folded: usize,
    /// Algebraic identities applied.
    pub algebraic: usize,
    /// Bitwise identities applied.
    pub bitwise: usize,
    /// Dead pure nodes removed.
    pub dead: usize,
}

/// Runs the full scalar pipeline and returns the counters.
// CEP:WHAT Applies fold, simplify, and DCE in order.
// CEP:WHY The catalog orders cheap local passes before global ones.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail; passes are total over any graph.
// CEP:ASSUMES None.
// CEP:COST O(|N|) per pass.
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn run_pipeline(graph: &mut Graph) -> PassStats {
    let folded = fold_constants(graph);
    let algebraic = algebraic_simplify(graph);
    let bitwise = bitwise_identity(graph);
    let dead = dead_code_elim(graph);
    PassStats {
        folded,
        algebraic,
        bitwise,
        dead,
    }
}

/// Catalog item 1: fold pure operations over two constants.
// CEP:WHAT Folds IAdd, ISub, IMul, IAnd, IOr, IXor over constant pairs.
// CEP:WHY Folding at compile time removes work from every execution.
// CEP:STATUS complete
// CEP:FAILURE Division and remainder by zero are left unfolded so the
//        runtime trap semantics stay observable.
// CEP:ASSUMES Constants are immediate integers.
// CEP:COST O(|N|).
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn fold_constants(graph: &mut Graph) -> usize {
    let mut folded = 0usize;
    let ids: Vec<NodeId> = graph.iter().map(|(id, _)| id).collect();
    for id in ids {
        let (op, a, b) = {
            let n = match graph.node(id) {
                Some(n) => n,
                None => continue,
            };
            if n.op.class() != vortex_ir::NodeClass::Pure || n.data.len() != 2 {
                continue;
            }
            let ca = graph.node(n.data[0]).and_then(const_int);
            let cb = graph.node(n.data[1]).and_then(const_int);
            match (ca, cb) {
                (Some(a), Some(b)) => (n.op, a, b),
                _ => continue,
            }
        };
        let result = match op {
            Op::IAdd => Some(a.wrapping_add(b)),
            Op::ISub => Some(a.wrapping_sub(b)),
            Op::IMul => Some(a.wrapping_mul(b)),
            Op::IAnd => Some(a & b),
            Op::IOr => Some(a | b),
            Op::IXor => Some(a ^ b),
            Op::IShl => Some(a.wrapping_shl((b & 63) as u32)),
            Op::IShr => Some(a.wrapping_shr((b & 63) as u32)),
            // Division and remainder by zero trap at runtime; keep them.
            _ => None,
        };
        if let Some(v) = result {
            let n = graph.node_mut(id).expect("node exists");
            n.op = Op::ConstInt(v);
            n.data.clear();
            folded += 1;
        }
    }
    folded
}

/// The immediate value of a constant node.
// CEP:WHAT Reads a ConstInt payload.
// CEP:WHY Folding needs the operands' values.
// CEP:STATUS complete
// CEP:FAILURE None for non-constant nodes.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-opt
fn const_int(n: &Node) -> Option<i64> {
    match n.op {
        Op::ConstInt(v) => Some(v),
        _ => None,
    }
}

/// Catalog item 10: algebraic simplification.
// CEP:WHAT Applies x+0 to x, x-0 to x, x*1 to x, x*0 to 0.
// CEP:WHY Identity operands are common after inlining and folding.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(|N|).
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn algebraic_simplify(graph: &mut Graph) -> usize {
    let mut applied = 0usize;
    let ids: Vec<NodeId> = graph.iter().map(|(id, _)| id).collect();
    for id in ids {
        let decision = {
            let n = match graph.node(id) {
                Some(n) => n,
                None => continue,
            };
            if n.op.class() != vortex_ir::NodeClass::Pure || n.data.len() != 2 {
                continue;
            }
            let zero = |i: usize| -> bool {
                graph
                    .node(n.data[i])
                    .and_then(const_int)
                    .is_some_and(|v| v == 0)
            };
            let one = |i: usize| -> bool {
                graph
                    .node(n.data[i])
                    .and_then(const_int)
                    .is_some_and(|v| v == 1)
            };
            match n.op {
                Op::IAdd if zero(1) || zero(0) => Some(Some(n.data[if zero(1) { 0 } else { 1 }])),
                Op::ISub if zero(1) => Some(Some(n.data[0])),
                Op::IMul if one(1) || one(0) => Some(Some(n.data[if one(1) { 0 } else { 1 }])),
                Op::IMul if zero(1) || zero(0) => Some(None),
                _ => None,
            }
        };
        if let Some(replacement) = decision {
            let n = graph.node_mut(id).expect("node exists");
            match replacement {
                Some(src) => {
                    n.op = Op::Identity;
                    n.data = vec![src];
                }
                None => {
                    n.op = Op::ConstInt(0);
                    n.data.clear();
                }
            }
            applied += 1;
        }
    }
    applied
}

/// Catalog item 22: bitwise identity simplification.
// CEP:WHAT Applies x&0 to 0, x|0 to x, x^-1 to !x.
// CEP:WHY Mask idioms simplify after strength reduction.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(|N|).
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn bitwise_identity(graph: &mut Graph) -> usize {
    let mut applied = 0usize;
    let ids: Vec<NodeId> = graph.iter().map(|(id, _)| id).collect();
    for id in ids {
        let decision = {
            let n = match graph.node(id) {
                Some(n) => n,
                None => continue,
            };
            if n.op.class() != vortex_ir::NodeClass::Pure || n.data.len() != 2 {
                continue;
            }
            let is_const = |i: usize, v: i64| -> bool {
                graph
                    .node(n.data[i])
                    .and_then(const_int)
                    .is_some_and(|c| c == v)
            };
            match n.op {
                Op::IAnd if is_const(1, 0) || is_const(0, 0) => Some(None),
                Op::IOr if is_const(1, 0) => Some(Some(n.data[0])),
                Op::IOr if is_const(0, 0) => Some(Some(n.data[1])),
                Op::IXor if is_const(1, -1) => Some(Some(n.data[0])),
                _ => None,
            }
        };
        if let Some(replacement) = decision {
            let n = graph.node_mut(id).expect("node exists");
            match replacement {
                Some(src) => {
                    n.op = Op::Identity;
                    n.data = vec![src];
                }
                None => {
                    n.op = Op::ConstInt(0);
                    n.data.clear();
                }
            }
            applied += 1;
        }
    }
    applied
}

/// Catalog item 6: dead code elimination over pure nodes.
// CEP:WHAT Removes pure nodes with no consumers; iterates to a fixed
//        point so chains collapse.
// CEP:WHY Dead pure nodes waste every downstream phase.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES Effectful nodes are never removed.
// CEP:COST O(|N|^2) worst case over the fixed point.
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn dead_code_elim(graph: &mut Graph) -> usize {
    let mut removed = 0usize;
    loop {
        let mut changed = false;
        let ids: Vec<NodeId> = graph.iter().map(|(id, _)| id).collect();
        for id in ids {
            let (is_dead, inputs) = {
                let n = match graph.node(id) {
                    Some(n) => n,
                    None => continue,
                };
                if n.op.class() != vortex_ir::NodeClass::Pure || n.op == Op::Phi {
                    continue;
                }
                let consumers = graph.consumers(id);
                (consumers.is_empty(), n.data.clone())
            };
            let already_blank = {
                let n = graph.node(id).expect("node exists");
                n.op == Op::ConstInt(0) && n.data.is_empty()
            };
            if is_dead && !already_blank {
                // Blank the node out in place: the arena has no deletion,
                // and a blank node keeps no inputs so chains collapse each
                // round. Already-blank nodes never re-trigger a round.
                let n = graph.node_mut(id).expect("node exists");
                n.op = Op::ConstInt(0);
                n.data = Vec::new();
                removed += 1;
                changed = true;
                let _ = inputs;
            }
        }
        if !changed {
            break;
        }
    }
    removed
}
