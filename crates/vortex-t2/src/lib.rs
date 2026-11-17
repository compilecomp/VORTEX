// CEP:FILE crates/vortex-t2/src/lib.rs
// CEP:WHAT T2 tier pipeline: all Class-0 (O(1) per node) passes in one
//        scan each under the B_2 = 100 ns/node compilation budget.
// CEP:WHY Tier taxonomy 6.3: T2 applies every O(1)-complexity pass once,
//        never iterating to a fixed point, in O(|IR|) total.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Budget exhaustion caps optimization at the current pass.
// CEP:ASSUMES Graphs pass Valid_IR before compilation.
// CEP:COST O(|IR|) per pass, one pass each; B_2 = 100 ns per node.
// CEP:EVIDENCE cargo test -p vortex-t2
//! VORTEX T2: the O(1) passes tier.
//!
//! Passes (order = dependency order, computed once at startup per 6.4):
//! constant folding first (exposes constants), type specialization
//! second (rewrites redundant casts), peephole third (rewires identity
//! chains), single-pass DCE last (sweeps what became dead). Each pass
//! visits every node a constant number of times.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod passes;

use vortex_ir::Graph;

/// T2 policy and budget parameters.
// CEP:WHAT Holds the B_2 budget model and pass toggles.
// CEP:WHY Budget enforcement (6.5) is policy, not algorithm.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t2
#[derive(Debug, Clone, Copy)]
pub struct T2Config {
    /// Modeled nanoseconds per node per pass (B_2 = 100 by design 6.5).
    pub budget_per_node_ns: u64,
    /// Run constant folding.
    pub fold: bool,
    /// Run type specialization.
    pub specialize: bool,
    /// Run peephole rewiring.
    pub peephole: bool,
    /// Run single-pass dead code elimination.
    pub dce: bool,
}

impl Default for T2Config {
    fn default() -> Self {
        T2Config {
            budget_per_node_ns: 100,
            fold: true,
            specialize: true,
            peephole: true,
            dce: true,
        }
    }
}

/// Counters produced by one T2 compilation.
// CEP:WHAT Reports what each pass did and how the budget was spent.
// CEP:WHY Tests, the tier controller, and budgets read these counters.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t2
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct T2Stats {
    /// Nodes in the compiled graph.
    pub nodes: usize,
    /// Constants folded into literal nodes.
    pub folded: usize,
    /// Redundant casts rewritten to identity.
    pub specialized: usize,
    /// Uses rewired through peephole identities.
    pub peepholed: usize,
    /// Dead pure nodes neutralized.
    pub dead_eliminated: usize,
    /// Modeled compile cost spent (nanoseconds).
    pub budget_used_ns: u64,
    /// True when the budget capped the pass pipeline.
    pub budget_exhausted: bool,
}

/// Compiles a graph at tier T2: one single-scan pass per enabled Class-0
/// transformation, capped by the B_2 budget (6.5).
// CEP:WHAT Runs the T2 pass pipeline in dependency order.
// CEP:WHY Monotonicity 6.3: T2 applies all Class-0 passes exactly once.
// CEP:STATUS complete
// CEP:FAILURE Budget exhaustion skips remaining passes and reports it.
// CEP:ASSUMES The graph satisfies Valid_IR on entry and exit.
// CEP:COST O(|IR|); modeled budget = B_2 * |IR|.
// CEP:EVIDENCE cargo test -p vortex-t2
pub fn compile_t2(graph: &mut Graph, config: T2Config) -> T2Stats {
    let mut stats = T2Stats {
        nodes: graph.len(),
        ..T2Stats::default()
    };
    let budget = config
        .budget_per_node_ns
        .saturating_mul(stats.nodes.max(1) as u64);
    // Modeled per-pass cost: each pass is one linear scan with O(1) node
    // work; the cost model charges B_2-class rates per node.
    let per_pass_ns: [(&'static str, u64); 4] = [
        ("fold", 30),
        ("specialize", 20),
        ("peephole", 40),
        ("dce", 10),
    ];
    let mut exhausted = false;
    for (name, rate) in per_pass_ns {
        let cost = (rate as usize)
            .saturating_mul(stats.nodes)
            .min(u64::MAX as usize) as u64;
        if stats.budget_used_ns.saturating_add(cost) > budget {
            exhausted = true;
            break;
        }
        match name {
            "fold" if config.fold => stats.folded = passes::fold_constants(graph),
            "specialize" if config.specialize => {
                stats.specialized = passes::specialize(graph);
            }
            "peephole" if config.peephole => stats.peepholed = passes::peephole(graph),
            "dce" if config.dce => stats.dead_eliminated = passes::dead_code(graph),
            _ => {
                // Disabled pass: no work, no charge beyond the scan.
            }
        }
        stats.budget_used_ns = stats.budget_used_ns.saturating_add(cost);
    }
    stats.budget_exhausted = exhausted;
    stats
}
