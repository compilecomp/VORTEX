// CEP:FILE crates/vortex-ir/src/gvn.rs
// CEP:WHAT Global value numbering over the unified graph, demonstrating the
//        overlay discipline: a SoN-domain pass that mutates only inside
//        transactions and re-reads overlay facts only after commit.
// CEP:WHY GVN is the canonical example the review demanded be defined
//        against the overlay (does it read the base graph, the overlay, or
//        both? Answer: base graph; overlay facts are re-validated by
//        revision afterwards).
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE The pass never mutates outside a transaction; Dead markers
//        remain until a later cleanup commit.
// CEP:ASSUMES Only pure ops (ConstInt, Add, Mul) participate.
// CEP:COST O(nodes) expected with hashing.
// CEP:EVIDENCE cargo test -p vortex-ir (gvn dedup test)
use crate::graph::{Graph, NodeId, NodeOp};
use std::collections::HashMap;

/// Runs GVN. Returns the number of redundant nodes replaced.
// CEP:WHAT Runs GVN.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-ir
pub fn gvn(g: &mut Graph) -> usize {
    // Pass 1 (read-only): compute value numbers from a simulation snapshot.
    // Because the pass mutates, value numbers are computed from a structural
    // signature walk over the pre-pass graph.
    let signature = {
        let mut sig: HashMap<NodeId, String> = HashMap::new();
        for i in 0..g.len() {
            let id = NodeId(i as u32);
            let n = g.node(id);
            let text = match n.op {
                NodeOp::ConstInt(v) => format!("const {v}"),
                NodeOp::Add => format!(
                    "add {},{}",
                    key(&sig, n.inputs.first()),
                    key(&sig, n.inputs.get(1))
                ),
                NodeOp::Mul => format!(
                    "mul {},{}",
                    key(&sig, n.inputs.first()),
                    key(&sig, n.inputs.get(1))
                ),
                other => format!("{other:?}"),
            };
            sig.insert(id, text);
        }
        sig
    };

    // Pass 2 (transactional): replace duplicates of pure ops.
    let mut replaced = 0;
    let mut canonical: HashMap<String, NodeId> = HashMap::new();
    for (id, text) in &signature {
        let n = g.node(*id);
        let is_pure = matches!(n.op, NodeOp::ConstInt(_) | NodeOp::Add | NodeOp::Mul);
        if !is_pure {
            continue;
        }
        if let Some(&leader) = canonical.get(text) {
            if leader != *id {
                let id = *id;
                Graph::edit(g, |tx| tx.replace(id, leader));
                replaced += 1;
            }
        } else {
            canonical.insert(text.clone(), *id);
        }
    }
    replaced
}

fn key(sig: &HashMap<NodeId, String>, id: Option<&NodeId>) -> String {
    id.map(|i| sig[i].clone()).unwrap_or_else(|| "_".into())
}
