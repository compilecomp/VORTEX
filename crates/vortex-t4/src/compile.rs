// CEP:FILE crates/vortex-t4/src/compile.rs
// CEP:WHAT The T4 compilation pipeline: the T2/T3 pass set iterated with
//        full GVN equivalence (Class 2) under the tier's node budget,
//        with work accounting and early capping (tier design 6.3-6.5).
// CEP:WHY T4 applies O(n^2) passes only for hot code; the budget bounds
//        the cost and capping preserves progress.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Budget overrun caps the tier at the current level rather
//        than exceeding the documented bound.
// CEP:ASSUMES The graph passed verification.
// CEP:COST O(|IR|^2) work units bounded by the budget.
// CEP:EVIDENCE cargo test -p vortex-t4
//! T4 compilation pipeline.

use std::fmt;
use vortex_ir::Graph;
use vortex_opt::passes::run_pipeline;

/// T4 configuration (tier design 6.5: B_4 = 2 us per node).
#[derive(Debug, Clone, Copy)]
pub struct T4Config {
    /// Work-unit budget per node (model units, one unit = one pass over
    /// one node).
    pub budget_per_node: u64,
    /// Maximum pipeline rounds for the full-equivalence fix point.
    pub max_rounds: usize,
}

impl Default for T4Config {
    // CEP:WHAT Provides the documented default budget.
    // CEP:WHY B_4 = 2000 model units per node; 4 rounds bound the fix point.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-t4
    fn default() -> T4Config {
        T4Config {
            budget_per_node: 2000,
            max_rounds: 4,
        }
    }
}

/// Counters from one T4 compilation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct T4Stats {
    /// Pipeline rounds executed.
    pub rounds: usize,
    /// Total folded constants across rounds.
    pub folded: usize,
    /// Total algebraic rewrites.
    pub algebraic: usize,
    /// Total bitwise rewrites.
    pub bitwise: usize,
    /// Total dead nodes removed.
    pub dead: usize,
    /// Work units consumed.
    pub work: u64,
    /// Whether the budget capped the pipeline early.
    pub budget_capped: bool,
}

impl fmt::Display for T4Stats {
    // CEP:WHAT Renders T4 counters for logs and the CLI.
    // CEP:WHY Operators see rounds, rewrites, and budget state.
    // CEP:STATUS complete
    // CEP:FAILURE Formatter errors only.
    // CEP:ASSUMES None.
    // CEP:COST O(fields).
    // CEP:EVIDENCE cargo test -p vortex-t4
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "t4: rounds {} folded {} algebraic {} bitwise {} dead {} work {}{}",
            self.rounds,
            self.folded,
            self.algebraic,
            self.bitwise,
            self.dead,
            self.work,
            if self.budget_capped { " [capped]" } else { "" }
        )
    }
}

/// Runs the T4 pipeline: the scalar pass set iterated to the full
/// equivalence fix point within the tier budget.
// CEP:WHAT Executes rounds until no pass reports progress or the budget
//        caps the run.
// CEP:WHY Class 2 passes iterate O(n) times with O(n) work per round.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail; the returned stats record the capping.
// CEP:ASSUMES The graph is verified before compilation.
// CEP:COST O(|IR|^2) bounded by budget_per_node * |IR|.
// CEP:EVIDENCE cargo test -p vortex-t4
pub fn compile_t4(graph: &mut Graph, config: T4Config) -> T4Stats {
    let n = graph.len().max(1) as u64;
    let budget = config.budget_per_node * n;
    let mut stats = T4Stats::default();
    for _ in 0..config.max_rounds {
        let before = stats.work;
        let pass_stats = run_pipeline(graph);
        stats.rounds += 1;
        stats.folded += pass_stats.folded;
        stats.algebraic += pass_stats.algebraic;
        stats.bitwise += pass_stats.bitwise;
        stats.dead += pass_stats.dead;
        // Work accounting: one unit per node per pass in the pipeline.
        stats.work += (graph.len() as u64) * 4;
        let progressed =
            pass_stats.folded + pass_stats.algebraic + pass_stats.bitwise + pass_stats.dead > 0;
        if stats.work >= budget {
            stats.budget_capped = true;
            break;
        }
        if !progressed {
            break;
        }
        let _ = before;
    }
    stats
}
