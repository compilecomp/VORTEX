// CEP:FILE crates/vortex-ir/src/gvn.rs
// CEP:WHAT Global value numbering over pure nodes: hash-consed detection
//        of equivalent computations and input rewriting to leaders
//        (optimization catalog item 25).
// CEP:WHY GVN is the workhorse redundancy elimination; it operates only
//        on pure nodes so effect ordering is untouched.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE The pass never rewrites effectful nodes; replacement counts
//        are exact.
// CEP:ASSUMES The graph passes verification before the pass runs.
// CEP:COST O(|N|) expected with hashing.
// CEP:EVIDENCE cargo test -p vortex-ir
//! Global value numbering over pure nodes.

use crate::graph::{Graph, NodeClass, NodeId, Op};
use std::collections::HashMap;

/// Applies GVN; returns the number of nodes whose inputs were rewritten to
/// leaders. Pure duplicates keep their arena slots (DCE reclaims them).
// CEP:WHAT Runs GVN over the graph.
// CEP:WHY Catalog item 25: prove expression equivalence across blocks.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail; the pass is total over any graph.
// CEP:ASSUMES Pure-node equivalence is syntactic (opcode plus normalized
//        commutative input order plus constants).
// CEP:COST O(|N|) expected.
// CEP:EVIDENCE cargo test -p vortex-ir
pub fn gvn(graph: &mut Graph) -> usize {
    let mut leaders: HashMap<String, NodeId> = HashMap::new();
    let mut rewrite: HashMap<NodeId, NodeId> = HashMap::new();
    let ids: Vec<NodeId> = graph.iter().map(|(id, _)| id).collect();
    for id in ids {
        let n = match graph.node(id) {
            Some(n) => n,
            None => continue,
        };
        if n.op.class() != NodeClass::Pure {
            continue;
        }
        let mut inputs: Vec<usize> = n
            .data
            .iter()
            .map(|d| rewrite.get(d).copied().unwrap_or(*d).0)
            .collect();
        if matches!(
            n.op,
            Op::IAdd | Op::IMul | Op::IAnd | Op::IOr | Op::IXor | Op::Eq
        ) {
            inputs.sort();
        }
        let key = format!("{:?}:{:?}", n.op, inputs);
        match leaders.get(&key).copied() {
            Some(leader) if leader != id => {
                rewrite.insert(id, leader);
            }
            _ => {
                leaders.insert(key, id);
            }
        }
    }
    let replaced = rewrite.len();
    if !rewrite.is_empty() {
        let ids: Vec<NodeId> = graph.iter().map(|(id, _)| id).collect();
        for id in ids {
            let n = match graph.node_mut(id) {
                Some(n) => n,
                None => continue,
            };
            for d in n.data.iter_mut() {
                if let Some(l) = rewrite.get(d) {
                    *d = *l;
                }
            }
        }
    }
    replaced
}
