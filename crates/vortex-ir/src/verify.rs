// CEP:FILE crates/vortex-ir/src/verify.rs
// CEP:WHAT The ET-SoN verifier: enforcement of Valid_IR invariants 1-4 and
//        6-7 over the arena graph, with the dominance condition of
//        invariant 5 for data edges (design 6.6).
// CEP:WHY Invalid programs must be rejected before optimization (goal 3);
//        the verifier is that gate.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Every violation is a typed VerifyError; the verifier never
//        panics on malformed graphs.
// CEP:ASSUMES Region classes are numbered below 64.
// CEP:COST O(|N| + |E|) structural; O(|N| * (|N| + |E|)) for the
//        reachability form of the dominance check.
// CEP:EVIDENCE cargo test -p vortex-ir
//! ET-SoN verifier (Valid_IR, design 6.6).

use crate::graph::{Graph, NodeClass, NodeId, Op};
use crate::types::Type;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

/// Verifier failures, each naming the invariant that rejected the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    /// Invariant 1: a data edge connects incompatible types.
    TypeMismatch {
        /// Producer node.
        from: NodeId,
        /// Consumer node.
        to: NodeId,
    },
    /// Invariant 2: a state edge connects a pure node or leaves the
    /// footprint.
    StateEdgeInvalid {
        /// Producer node.
        from: NodeId,
        /// Consumer node.
        to: NodeId,
        /// Region class of the edge.
        region: u32,
    },
    /// Invariant 3: a Region has no control inputs.
    EmptyRegion {
        /// The Region node.
        node: NodeId,
    },
    /// Invariant 3: a Phi's input count disagrees with its Region.
    PhiArity {
        /// The Phi node.
        node: NodeId,
    },
    /// Invariant 3: a StatePhi's input count disagrees with its Region.
    StatePhiArity {
        /// The StatePhi node.
        node: NodeId,
    },
    /// Invariant 3: a control path ends at a non-terminator.
    NoTerminator {
        /// The dangling node.
        node: NodeId,
    },
    /// Invariant 3: the graph has no Start node.
    NoStart,
    /// Invariant 4: a state cycle passes through no StatePhi.
    StateCycleWithoutPhi {
        /// Region class of the offending cycle.
        region: u32,
    },
    /// Invariant 4: an effectful node's state output dangles.
    StateChainDangling {
        /// The node whose state output dangles.
        node: NodeId,
    },
    /// Invariant 5: a data source does not dominate its user.
    DominanceViolation {
        /// Producer node.
        from: NodeId,
        /// Consumer node.
        to: NodeId,
    },
    /// Invariant 6: a node's effect set differs from its opcode's.
    EffectMismatch {
        /// The offending node.
        node: NodeId,
    },
    /// Invariant 7: an FP flag set breaks the AFN rule.
    FpFlagInvalid {
        /// The offending node.
        node: NodeId,
    },
}

impl fmt::Display for VerifyError {
    // CEP:WHAT Renders a verifier failure.
    // CEP:WHY Diagnostics name the invariant and the node.
    // CEP:STATUS complete
    // CEP:FAILURE Formatter errors only.
    // CEP:ASSUMES None.
    // CEP:COST O(fields).
    // CEP:EVIDENCE cargo test -p vortex-ir
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyError::TypeMismatch { from, to } => {
                write!(f, "invariant 1: type mismatch on edge {from:?} to {to:?}")
            }
            VerifyError::StateEdgeInvalid { from, to, region } => {
                write!(
                    f,
                    "invariant 2: bad state edge {from:?} to {to:?} on class {region}"
                )
            }
            VerifyError::EmptyRegion { node } => {
                write!(f, "invariant 3: Region {node:?} has no control inputs")
            }
            VerifyError::PhiArity { node } => {
                write!(
                    f,
                    "invariant 3: Phi {node:?} arity disagrees with its Region"
                )
            }
            VerifyError::StatePhiArity { node } => {
                write!(
                    f,
                    "invariant 3: StatePhi {node:?} arity disagrees with its Region"
                )
            }
            VerifyError::NoTerminator { node } => {
                write!(
                    f,
                    "invariant 3: control path ends at non-terminator {node:?}"
                )
            }
            VerifyError::NoStart => write!(f, "invariant 3: graph has no Start node"),
            VerifyError::StateCycleWithoutPhi { region } => {
                write!(
                    f,
                    "invariant 4: state cycle on class {region} without a StatePhi"
                )
            }
            VerifyError::StateChainDangling { node } => {
                write!(
                    f,
                    "invariant 4: state chain ends at non-terminator {node:?}"
                )
            }
            VerifyError::DominanceViolation { from, to } => {
                write!(f, "invariant 5: {from:?} does not dominate {to:?}")
            }
            VerifyError::EffectMismatch { node } => {
                write!(
                    f,
                    "invariant 6: node {node:?} effects differ from its opcode"
                )
            }
            VerifyError::FpFlagInvalid { node } => {
                write!(f, "invariant 7: FP flags of {node:?} violate the AFN rule")
            }
        }
    }
}

impl std::error::Error for VerifyError {}

/// Verifies a whole graph (Valid_IR subset, design 6.6).
// CEP:WHAT Runs every structural invariant in order.
// CEP:WHY Optimization passes assume a valid graph; this gate guarantees it.
// CEP:STATUS complete
// CEP:FAILURE The first violation, deterministically ordered.
// CEP:ASSUMES None.
// CEP:COST O(|N| + |E|) structural; dominance is O(|N| (|N| + |E|)).
// CEP:EVIDENCE cargo test -p vortex-ir
pub fn verify(graph: &Graph) -> Result<(), VerifyError> {
    check_effects_and_flags(graph)?;
    check_structural(graph)?;
    check_types(graph)?;
    check_state_tokens(graph)?;
    check_reachability_and_terminators(graph)?;
    check_dominance(graph)?;
    Ok(())
}

/// Invariants 6 and 7: exact effect sets and valid FP flags.
// CEP:WHAT Checks every node's effect set against its opcode and every FP
//        node's flag set against the AFN rule.
// CEP:WHY Effect-kind monotonicity is what analysis correctness rests on.
// CEP:STATUS complete
// CEP:FAILURE The first offending node.
// CEP:ASSUMES None.
// CEP:COST O(|N|).
// CEP:EVIDENCE cargo test -p vortex-ir
fn check_effects_and_flags(graph: &Graph) -> Result<(), VerifyError> {
    for (id, n) in graph.iter() {
        if n.effects != n.op.effects() {
            return Err(VerifyError::EffectMismatch { node: id });
        }
        if n.op.is_fp() && !n.fp.valid() {
            return Err(VerifyError::FpFlagInvalid { node: id });
        }
    }
    Ok(())
}

/// Invariant 3 structure: Region and Phi arities.
// CEP:WHAT Checks Region k>=1 and Phi/StatePhi arity against the Region.
// CEP:WHY Control merges must agree with their merges' input counts.
// CEP:STATUS complete
// CEP:FAILURE The first offending node.
// CEP:ASSUMES None.
// CEP:COST O(|N|).
// CEP:EVIDENCE cargo test -p vortex-ir
fn check_structural(graph: &Graph) -> Result<(), VerifyError> {
    for (id, n) in graph.iter() {
        match n.op {
            Op::Region => {
                if n.ctrl.is_empty() {
                    return Err(VerifyError::EmptyRegion { node: id });
                }
            }
            Op::Phi => {
                let k = phi_region_arity(graph, &n.ctrl);
                if k != Some(n.data.len()) {
                    return Err(VerifyError::PhiArity { node: id });
                }
            }
            Op::StatePhi => {
                let k = phi_region_arity(graph, &n.ctrl);
                if k != Some(n.state.len()) {
                    return Err(VerifyError::StatePhiArity { node: id });
                }
            }
            // Start, If, Return, and Stop have no merge-arity rules here.
            _ => {
                let _ = id;
            }
        }
    }
    Ok(())
}

/// The control-input count of a Phi's controlling Region.
// CEP:WHAT Resolves the arity a Phi must match.
// CEP:WHY Phi and StatePhi arity equals their Region's control count.
// CEP:STATUS complete
// CEP:FAILURE None when no Region reference exists.
// CEP:ASSUMES The first control input is the controlling Region.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-ir
fn phi_region_arity(graph: &Graph, ctrl: &[NodeId]) -> Option<usize> {
    let region = ctrl.first()?;
    graph.node(*region).map(|rn| rn.ctrl.len())
}

/// Invariant 1: every data edge is type-compatible.
// CEP:WHAT Checks the subtype relation across data edges.
// CEP:WHY Optimization transformations rely on type soundness.
// CEP:STATUS complete
// CEP:FAILURE The first incompatible edge.
// CEP:ASSUMES None.
// CEP:COST O(|E_data|).
// CEP:EVIDENCE cargo test -p vortex-ir
fn check_types(graph: &Graph) -> Result<(), VerifyError> {
    for (id, n) in graph.iter() {
        for (i, input) in n.data.iter().enumerate() {
            let producer = graph.node(*input).ok_or(VerifyError::TypeMismatch {
                from: *input,
                to: id,
            })?;
            if let Some(expected) = expected_input_type(n.op, i) {
                if !producer.ty.is_subtype(&expected) {
                    return Err(VerifyError::TypeMismatch {
                        from: *input,
                        to: id,
                    });
                }
            }
        }
    }
    Ok(())
}

/// The expected data input type of input i of an opcode.
// CEP:WHAT Computes the position's required type.
// CEP:WHY Positions are opcode-specific.
// CEP:STATUS complete
// CEP:FAILURE None when the position accepts any type.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-ir
fn expected_input_type(op: Op, _i: usize) -> Option<Type> {
    match op {
        Op::If => Some(Type::Bool),
        Op::Load | Op::AtomicLoad => Some(Type::Ptr(Box::new(Type::Int64))),
        Op::IAdd
        | Op::ISub
        | Op::IMul
        | Op::IDiv
        | Op::IRem
        | Op::IAnd
        | Op::IOr
        | Op::IXor
        | Op::IShl
        | Op::IShr
        | Op::Eq
        | Op::Lt => Some(Type::Int64),
        Op::FAdd | Op::FSub | Op::FMul | Op::FDiv => Some(Type::Float64),
        Op::Not | Op::IntToFloat => Some(Type::Int64),
        Op::FloatToInt => Some(Type::Float64),
        // Remaining positions accept any producer type or are governed by
        // other invariants.
        _ => {
            let _ = op;
            None
        }
    }
}

/// Invariant 2 and 4: state edges are effectful, in-footprint, chained,
/// and every cycle passes through a StatePhi.
// CEP:WHAT Validates the per-class state subgraphs.
// CEP:WHY Loop-carried state must merge at StatePhis (invariant 4).
// CEP:STATUS complete
// CEP:FAILURE The first invalid edge or dangling chain.
// CEP:ASSUMES None.
// CEP:COST O(|N| + |E_state|).
// CEP:EVIDENCE cargo test -p vortex-ir
fn check_state_tokens(graph: &Graph) -> Result<(), VerifyError> {
    for (id, n) in graph.iter() {
        for (r, producer_id) in &n.state {
            let bad = || VerifyError::StateEdgeInvalid {
                from: *producer_id,
                to: id,
                region: *r,
            };
            let producer = graph.node(*producer_id).ok_or_else(bad)?;
            // Start seeds every region class with its initial token.
            let is_start = producer.op == Op::Start;
            if !is_start && (producer.effects.is_pure() || producer.footprint & (1u64 << r) == 0) {
                return Err(bad());
            }
        }
        if n.op.class() == NodeClass::Eff && !n.op.is_terminator() {
            for r in 0..graph.region_classes {
                if n.footprint & (1u64 << r) != 0 && !n.state.iter().any(|(sr, _)| sr == &r) {
                    return Err(VerifyError::StateChainDangling { node: id });
                }
            }
        }
    }
    for r in 0..graph.region_classes {
        if state_cycle_without_phi(graph, r) {
            return Err(VerifyError::StateCycleWithoutPhi { region: r });
        }
    }
    Ok(())
}

/// Detects a state-r cycle that avoids StatePhi_r nodes.
// CEP:WHAT Threads state edges transparently through StatePhi merges and
//        runs a gray-set cycle check on the phi-free subgraph.
// CEP:WHY Invariant 4: every cycle must contain a StatePhi, which is
//        exactly the statement that deleting StatePhis leaves the
//        state-r subgraph acyclic.
// CEP:STATUS complete
// CEP:FAILURE True when a phi-free cycle exists.
// CEP:ASSUMES None.
// CEP:COST O(|N| + |E_state|).
// CEP:EVIDENCE cargo test -p vortex-ir
fn state_cycle_without_phi(graph: &Graph, r: u32) -> bool {
    let mut succ: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for (id, n) in graph.iter() {
        for (sr, producer) in &n.state {
            if *sr == r {
                succ.entry(*producer).or_default().push(id);
            }
        }
    }
    let mut color: HashMap<NodeId, u8> = HashMap::new();
    let mut path: Vec<NodeId> = Vec::new();
    let keys: Vec<NodeId> = succ.keys().copied().collect();
    for start in keys {
        if color.get(&start).copied().unwrap_or(0) != 0 {
            continue;
        }
        if dfs_state_cycle(graph, start, &succ, &mut color, &mut path, r) {
            return true;
        }
    }
    false
}

/// Recursive DFS for phi-free state cycles on one region class.
// CEP:WHAT Finds a state-r cycle whose node set contains no StatePhi.
// CEP:WHY Invariant 4 item 1: every loop-carried cycle must merge at a
//        StatePhi; this walks each cycle segment and checks membership.
// CEP:STATUS complete
// CEP:FAILURE True when a phi-free cycle exists.
// CEP:ASSUMES Recursion depth is bounded by the node count; model graphs
//        are small and the verifier runs offline.
// CEP:COST O(|N| + |E_state|) per class.
// CEP:EVIDENCE cargo test -p vortex-ir
fn dfs_state_cycle(
    graph: &Graph,
    node: NodeId,
    succ: &HashMap<NodeId, Vec<NodeId>>,
    color: &mut HashMap<NodeId, u8>,
    path: &mut Vec<NodeId>,
    r: u32,
) -> bool {
    color.insert(node, 1);
    path.push(node);
    let is_phi = graph
        .node(node)
        .is_some_and(|n| n.op == Op::StatePhi && n.state.iter().any(|(sr, _)| *sr == r));
    if let Some(children) = succ.get(&node) {
        for c in children.clone() {
            match color.get(&c).copied().unwrap_or(0) {
                1 => {
                    let seg_start = path.iter().position(|x| *x == c).unwrap_or(0);
                    let segment = &path[seg_start..];
                    let has_phi = segment.iter().any(|n| {
                        graph.node(*n).is_some_and(|nn| {
                            nn.op == Op::StatePhi && nn.state.iter().any(|(sr, _)| *sr == r)
                        })
                    }) || is_phi;
                    if !has_phi {
                        return true;
                    }
                }
                0 => {
                    let found = dfs_state_cycle(graph, c, succ, color, path, r);
                    if found {
                        return true;
                    }
                }
                // Black nodes are fully explored; gray nodes were handled
                // as cycle candidates above.
                _ => {
                    let _ = r;
                }
            }
        }
    }
    color.insert(node, 2);
    path.pop();
    false
}

/// Invariant 3: reachability from Start and termination at Return/Stop.
// CEP:WHAT BFS over data and control edges from Start; every reachable
//        leaf must be a terminator.
// CEP:WHY Dangling control paths are malformed programs.
// CEP:STATUS complete
// CEP:FAILURE NoStart, NoTerminator.
// CEP:ASSUMES None.
// CEP:COST O(|N| + |E|).
// CEP:EVIDENCE cargo test -p vortex-ir
fn check_reachability_and_terminators(graph: &Graph) -> Result<(), VerifyError> {
    let start = match graph.iter().find(|(_, n)| n.op == Op::Start) {
        Some((id, _)) => id,
        None => return Err(VerifyError::NoStart),
    };
    let mut succ: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for (id, n) in graph.iter() {
        for d in n.data.iter().chain(n.ctrl.iter()).chain(n.except.iter()) {
            succ.entry(*d).or_default().push(id);
        }
    }
    let mut seen: HashSet<NodeId> = HashSet::new();
    let mut queue: VecDeque<NodeId> = VecDeque::new();
    queue.push_back(start);
    seen.insert(start);
    while let Some(id) = queue.pop_front() {
        if let Some(children) = succ.get(&id) {
            for c in children {
                if seen.insert(*c) {
                    queue.push_back(*c);
                }
            }
        }
    }
    for id in &seen {
        let is_leaf = succ.get(id).is_none_or(|c| c.is_empty());
        if is_leaf {
            let n = graph.node(*id).ok_or(VerifyError::NoStart)?;
            // Effectful nodes and StatePhis attach through state chains;
            // their liveness is governed by invariant 4, not by control
            // leaves.
            let exempt = n.op.class() == NodeClass::Eff || n.op == Op::StatePhi;
            if !n.op.is_terminator() && !exempt {
                return Err(VerifyError::NoTerminator { node: *id });
            }
        }
    }
    Ok(())
}

/// Invariant 5 (data-edge form): sources dominate users or feed Phis.
// CEP:WHAT Checks the dominance condition. Pure leaf nodes float to any
//        region; pure nodes with inputs dominate recursively; control
//        and effectful producers use the reachability form: u dominates
//        v iff every control path from Start to v passes through u.
// CEP:WHY Click's dominance condition decides schedulability.
// CEP:STATUS complete
// CEP:FAILURE The first violating data edge.
// CEP:ASSUMES None.
// CEP:COST O(|E_data| * (|N| + |E_ctrl|)).
// CEP:EVIDENCE cargo test -p vortex-ir
fn check_dominance(graph: &Graph) -> Result<(), VerifyError> {
    let start = match graph.iter().find(|(_, n)| n.op == Op::Start) {
        Some((id, _)) => id,
        None => return Ok(()),
    };
    let mut succ_ctrl: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for (id, n) in graph.iter() {
        for c in &n.ctrl {
            succ_ctrl.entry(*c).or_default().push(id);
        }
    }
    let mut memo: HashMap<(NodeId, NodeId), bool> = HashMap::new();
    let mut reach_cache: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();
    for (id, n) in graph.iter() {
        if n.op == Op::Phi {
            continue;
        }
        for input in &n.data {
            if !dominates(
                graph,
                &succ_ctrl,
                start,
                *input,
                id,
                &mut memo,
                &mut reach_cache,
            ) {
                return Err(VerifyError::DominanceViolation {
                    from: *input,
                    to: id,
                });
            }
        }
    }
    Ok(())
}

/// The dominance predicate with memoization.
// CEP:WHAT Computes whether u dominates v.
// CEP:WHY Pure leaves float anywhere; pure chains dominate through their
//        inputs; positioned nodes use the cut-vertex reachability form.
// CEP:STATUS complete
// CEP:FAILURE False when v is reachable without u and u is positioned.
// CEP:ASSUMES None.
// CEP:COST O(|N| + |E_ctrl|) per distinct blocked node.
// CEP:EVIDENCE cargo test -p vortex-ir
fn dominates(
    graph: &Graph,
    succ_ctrl: &HashMap<NodeId, Vec<NodeId>>,
    start: NodeId,
    u: NodeId,
    v: NodeId,
    memo: &mut HashMap<(NodeId, NodeId), bool>,
    reach_cache: &mut HashMap<NodeId, HashSet<NodeId>>,
) -> bool {
    if u == v || u == start {
        return true;
    }
    if let Some(hit) = memo.get(&(u, v)) {
        return *hit;
    }
    let result = match graph.node(u).map(|n| (n.op, n.op.class())) {
        Some((Op::Phi, _)) => {
            // A Phi's value is available wherever its controlling Region
            // (the merge) dominates the consumer.
            let region = graph.node(u).and_then(|n| n.ctrl.first().copied());
            match region {
                Some(r) if r != u => dominates(graph, succ_ctrl, start, r, v, memo, reach_cache),
                _ => true,
            }
        }
        Some((_, NodeClass::Pure)) => {
            let inputs = graph.node(u).map(|n| n.data.clone()).unwrap_or_default();
            inputs.is_empty()
                || inputs
                    .iter()
                    .all(|d| dominates(graph, succ_ctrl, start, *d, v, memo, reach_cache))
        }
        _ => {
            let reach = reach_cache.entry(u).or_insert_with(|| {
                let mut seen: HashSet<NodeId> = HashSet::new();
                let mut queue: VecDeque<NodeId> = VecDeque::new();
                queue.push_back(start);
                seen.insert(start);
                while let Some(id) = queue.pop_front() {
                    if id == u {
                        continue;
                    }
                    if let Some(children) = succ_ctrl.get(&id) {
                        for c in children {
                            if seen.insert(*c) {
                                queue.push_back(*c);
                            }
                        }
                    }
                }
                seen
            });
            !reach.contains(&v)
        }
    };
    memo.insert((u, v), result);
    result
}
