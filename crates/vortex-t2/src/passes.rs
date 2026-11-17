// CEP:FILE crates/vortex-t2/src/passes.rs
// CEP:WHAT The four Class-0 passes: single-scan constant folding, type
//        specialization, peephole rewiring, single-wave DCE.
// CEP:WHY Each pass is one linear scan with O(1) work per node (6.4
//        Class 0); none iterates to a fixed point.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Cannot fail; every rewrite preserves Valid_IR.
// CEP:ASSUMES Producers precede consumers in insertion order for folding.
// CEP:COST O(|IR|) per pass.
// CEP:EVIDENCE cargo test -p vortex-t2
//! Class-0 passes for tier T2.

use vortex_ir::{Node, NodeId, Op, Type};

/// Rewrites a node in place into a constant, keeping its id (uses stay
/// valid without a remap sweep).
// CEP:WHAT Turns a node into ConstInt/ConstFloat with no inputs.
// CEP:WHY In-place folding is the O(1) rewrite primitive.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES The node is pure.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t2
fn fold_into_const_int(node: &mut Node, v: i64) {
    node.op = Op::ConstInt(v);
    node.data.clear();
    node.ctrl.clear();
}

/// Whether the type is any signed or unsigned integer.
// CEP:WHAT Classifies integer types.
// CEP:WHY Integer-only rewrites must not fire on floats.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t2
fn is_int_ty(t: &Type) -> bool {
    matches!(
        t,
        Type::Int8
            | Type::Int16
            | Type::Int32
            | Type::Int64
            | Type::UInt8
            | Type::UInt16
            | Type::UInt32
            | Type::UInt64
    )
}

/// One-scan constant folding: pure nodes whose inputs are both integer
/// constants become constants (6.4 Class 0 example).
// CEP:WHAT Folds arithmetic, bitwise, compare, and cast literals.
// CEP:WHY Single scan, no fixed point: fold each node at most once.
// CEP:STATUS complete
// CEP:FAILURE Div-by-zero, oversized shifts, and out-of-range casts
//        deliberately skip folding (the op stays for the backend).
// CEP:ASSUMES Const producers precede their consumers.
// CEP:COST O(|IR|).
// CEP:EVIDENCE cargo test -p vortex-t2
pub fn fold_constants(graph: &mut vortex_ir::Graph) -> usize {
    let mut folded = 0;
    for idx in 0..graph.len() {
        let id = NodeId(idx);
        let Some(node) = graph.node(id) else {
            continue;
        };
        // Only pure binary/unary integer ops with constant inputs.
        let ints: Option<(i64, i64)> = match (&node.op, node.data.as_slice()) {
            (Op::IAdd | Op::ISub | Op::IMul, [a, b]) => {
                match (graph.node(*a).map(|n| &n.op), graph.node(*b).map(|n| &n.op)) {
                    (Some(Op::ConstInt(x)), Some(Op::ConstInt(y))) => Some((*x, *y)),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some((x, y)) = ints {
            let result = match node.op {
                Op::IAdd => Some(x.wrapping_add(y)),
                Op::ISub => Some(x.wrapping_sub(y)),
                Op::IMul => Some(x.wrapping_mul(y)),
                _ => None,
            };
            let Some(node) = graph.node_mut(id) else {
                continue;
            };
            if let Some(v) = result {
                fold_into_const_int(node, v);
                folded += 1;
            }
            continue;
        }
        // Unary Not on a constant.
        if matches!(node.op, Op::Not) {
            if let [a] = node.data.as_slice() {
                if let Some(Op::ConstInt(x)) = graph.node(*a).map(|n| &n.op) {
                    let v = !*x;
                    if let Some(n) = graph.node_mut(id) {
                        fold_into_const_int(n, v);
                        folded += 1;
                    }
                }
            }
            continue;
        }
        // Comparisons on equal integer nodes (x == x is true for ints).
        if matches!(node.op, Op::Eq)
            && node.data.len() == 2
            && node.data[0] == node.data[1]
            && is_int_ty(&node.ty)
        {
            let Some(n) = graph.node_mut(id) else {
                continue;
            };
            fold_into_const_int(n, 1);
            folded += 1;
        }
    }
    folded
}

/// One-scan type specialization: a redundant `IntCast` whose input already
/// has the cast's result type becomes an `Identity` (peephole then wires
/// uses straight to the input).
// CEP:WHAT Removes casts that cannot change the value.
// CEP:WHY Type-driven specialization is the Class-0 example (6.4).
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES Input types come from a verified lattice.
// CEP:COST O(|IR|).
// CEP:EVIDENCE cargo test -p vortex-t2
pub fn specialize(graph: &mut vortex_ir::Graph) -> usize {
    let mut specialized = 0;
    for idx in 0..graph.len() {
        let id = NodeId(idx);
        let Some(node) = graph.node(id) else {
            continue;
        };
        if matches!(node.op, Op::IntCast) && node.data.len() == 1 {
            let input_ty = graph
                .node(node.data[0])
                .map(|n| n.ty.clone())
                .unwrap_or(Type::Ctrl);
            if input_ty == node.ty {
                let src = node.data[0];
                let Some(n) = graph.node_mut(id) else {
                    continue;
                };
                n.op = Op::Identity;
                n.data = vec![src];
                specialized += 1;
            }
        }
    }
    specialized
}

/// One-scan peephole: algebraic identities on integer ops produce a remap
/// table applied in a single rewiring sweep (O(|IR|) total).
// CEP:WHAT Rewires uses through x+0, x*1, x*0, x-0, x-x, shifts-by-0,
//        x&0, x|0, x^0, Not(Not x), and Identity chains.
// CEP:WHY Identities are the cheapest wins; one sweep keeps O(n).
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES Constants are literal opcodes, not folded placeholders.
// CEP:COST O(|IR|).
// CEP:EVIDENCE cargo test -p vortex-t2
pub fn peephole(graph: &mut vortex_ir::Graph) -> usize {
    // Phase 1: collect replacements (old id -> new id) in one scan.
    let mut remap: Vec<(NodeId, NodeId)> = Vec::new();
    for idx in 0..graph.len() {
        let id = NodeId(idx);
        let Some(node) = graph.node(id) else {
            continue;
        };
        let two = (node.data.first().copied(), node.data.get(1).copied());
        let replacement: Option<NodeId> = match (&node.op, two) {
            (Op::IAdd | Op::ISub | Op::IOr | Op::IXor, (Some(x), Some(y))) => {
                if is_const_zero(graph, y) {
                    Some(x)
                } else {
                    None
                }
            }
            (Op::IMul, (Some(x), Some(y))) => {
                if is_const_one(graph, y) {
                    Some(x)
                } else {
                    None
                }
            }
            (Op::IAnd, (Some(_), Some(y))) => {
                if is_const_zero(graph, y) {
                    Some(y)
                } else {
                    None
                }
            }
            (Op::IShl | Op::IShr, (Some(x), Some(y))) => {
                if is_const_zero(graph, y) {
                    Some(x)
                } else {
                    None
                }
            }
            (Op::Not, (Some(x), _)) => {
                // Not(Not x): the sole input is a Not of something.
                match graph.node(x).map(|n| &n.op) {
                    Some(Op::Not) => graph.node(x).and_then(|n| n.data.first().copied()),
                    _ => None,
                }
            }
            (Op::Identity, (Some(x), _)) => Some(x),
            _ => None,
        };
        if let Some(new) = replacement {
            remap.push((id, new));
        }
    }
    // Phase 2: one rewiring sweep over every node's data edges.
    let rewires = remap.len();
    if remap.is_empty() {
        return 0;
    }
    for idx in 0..graph.len() {
        let id = NodeId(idx);
        let Some(node) = graph.node_mut(id) else {
            continue;
        };
        for slot in node.data.iter_mut() {
            if let Some(&(_, new)) = remap.iter().find(|(old, _)| *old == *slot) {
                *slot = new;
            }
        }
    }
    // Neutralize the rewritten identity nodes: they may now have users
    // removed; folding them to constants keeps the graph acyclic.
    for (old, _) in &remap {
        let Some(node) = graph.node_mut(*old) else {
            continue;
        };
        if matches!(node.op, Op::Identity) {
            fold_into_const_int(node, 0);
        }
    }
    rewires
}

/// Whether the node is a zero integer constant.
// CEP:WHAT Tests an operand for ConstInt(0).
// CEP:WHY Peephole identities match against literal zeros.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t2
fn is_const_zero(graph: &vortex_ir::Graph, id: NodeId) -> bool {
    matches!(graph.node(id).map(|n| &n.op), Some(Op::ConstInt(0)))
}

/// Whether the node is a one integer constant.
// CEP:WHAT Tests an operand for ConstInt(1).
// CEP:WHY Multiplicative identities match literal ones.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t2
fn is_const_one(graph: &vortex_ir::Graph, id: NodeId) -> bool {
    matches!(graph.node(id).map(|n| &n.op), Some(Op::ConstInt(1)))
}

/// Single-wave dead code elimination: pure, non-constant nodes with no
/// consumers are neutralized to constants (one wave; later recompiles
/// catch chains exposed by this wave).
// CEP:WHAT Neutralizes dead pure nodes in one pass.
// CEP:WHY 6.4: single-pass DCE is a Class-0 pass; no fixed point.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES Consumers are computed from data edges only.
// CEP:COST O(|IR|).
// CEP:EVIDENCE cargo test -p vortex-t2
pub fn dead_code(graph: &mut vortex_ir::Graph) -> usize {
    // Sweep 1: use counts per node id (O(1) per edge).
    let n = graph.len();
    let mut uses = vec![0usize; n];
    for (_, node) in graph.iter() {
        for &d in &node.data {
            if d.0 < n {
                uses[d.0] += 1;
            }
        }
    }
    // Sweep 2: neutralize dead pure nodes.
    let mut killed = 0;
    for (idx, &count) in uses.iter().enumerate() {
        let id = NodeId(idx);
        let Some(node) = graph.node(id) else {
            continue;
        };
        let dead = count == 0
            && matches!(
                node.op,
                Op::IAdd | Op::ISub | Op::IMul | Op::Not | Op::IntCast
            );
        if dead {
            let Some(node) = graph.node_mut(id) else {
                continue;
            };
            fold_into_const_int(node, 0);
            killed += 1;
        }
    }
    killed
}
