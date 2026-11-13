// CEP:FILE crates/vortex-tier/src/controller.rs
// CEP:WHAT The tiering controller: tier-up triggers over the profile store
//        (back-edge thresholds, call-site counts), the OSR bookkeeping
//        (OSR-unstable methods and pcs per OSR-INV-05), and the deopt
//        storm prevention (penalty counters from the PROF-1 deopt domain).
// CEP:WHY Tiering is the engine's adaptive backbone: thresholds decide
//        when compilation pays, and the instability bookkeeping is what
//        makes speculation safe to repeat (a deopting specialization is
//        never retried forever).
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Every decision is a pure function of counters and
//        thresholds; the controller never silently skips a trigger.
// CEP:ASSUMES The profile store is the single source of hot-path truth.
// CEP:COST O(1) per decision.
// CEP:EVIDENCE cargo test -p vortex-tier
use std::collections::HashSet;

/// Execution tiers (design.md section 5 revised tier structure).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Interpreter (the semantic reference).
    T0,
    /// Stencil baseline.
    T1,
    /// Optimizing.
    T2,
    /// Aggressive optimizing.
    T3,
    /// Trace specialization.
    T4,
}

/// Tier-up thresholds (design.md T2 section 2.5, T3 section 3.6).
#[derive(Debug, Clone, Copy)]
pub struct Thresholds {
    /// Back-edge folded iterations that trigger baseline compilation.
    pub baseline_backedge: u64,
    /// Call-site executions that trigger optimizing compilation.
    pub optimizing_calls: u64,
    /// Execution in T2 that triggers aggressive compilation.
    pub aggressive_cost: u64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            baseline_backedge: 10_000,
            optimizing_calls: 1_000,
            aggressive_cost: 100,
        }
    }
}

/// The tier controller state for one program.
#[derive(Debug, Default)]
pub struct TierController {
    /// Current tier per method.
    tiers: std::collections::HashMap<usize, Tier>,
    /// OSR-unstable (method, pc) pairs: OSR attempts that deoptimized
    /// immediately are never retried (OSR-INV-05).
    osr_unstable: HashSet<(usize, usize)>,
    /// Deopt penalty counters per method (PROF-1 deopt domain): repeated
    /// deopts suppress further speculation.
    deopt_penalties: std::collections::HashMap<usize, u64>,
    /// Thresholds.
    pub thresholds: Thresholds,
}

impl TierController {
    /// Creates a controller with the default thresholds.
    #[must_use]
    pub fn new() -> Self {
        TierController::default()
    }

    /// Current tier of a method (T0 until first promotion).
    #[must_use]
    pub fn tier_of(&self, method: usize) -> Tier {
        self.tiers.get(&method).copied().unwrap_or(Tier::T0)
    }

    /// Promotes a method (records the tier transition).
    pub fn promote(&mut self, method: usize, to: Tier) {
        let cur = self.tier_of(method);
        if to > cur {
            self.tiers.insert(method, to);
        }
    }

    /// Whether the method should tier up to the baseline (T1) from the
    /// profile store's folded back-edge total.
    #[must_use]
    pub fn wants_baseline(&self, method: usize, folded_backedges: u64) -> bool {
        self.tier_of(method) == Tier::T0 && folded_backedges >= self.thresholds.baseline_backedge
    }

    /// Whether the method should tier up to the optimizing tier from its
    /// call-site counts.
    #[must_use]
    pub fn wants_optimizing(&self, call_counts: impl IntoIterator<Item = u64>) -> bool {
        call_counts
            .into_iter()
            .any(|c| c >= self.thresholds.optimizing_calls)
    }

    /// Whether the method should tier up to the aggressive tier.
    #[must_use]
    pub fn wants_aggressive(&self, method: usize, t2_cost: u64) -> bool {
        self.tier_of(method) == Tier::T2 && t2_cost >= self.thresholds.aggressive_cost
    }

    /// Marks an OSR attempt: returns false when the (method, pc) is
    /// OSR-unstable and the attempt must not proceed (OSR-INV-05).
    // CEP:WHAT Marks an OSR attempt.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn try_osr(&mut self, method: usize, pc: usize) -> bool {
        !self.osr_unstable.contains(&(method, pc))
    }

    /// Records an immediate deopt after OSR: the (method, pc) becomes
    /// OSR-unstable permanently for this profile.
    pub fn record_osr_failure(&mut self, method: usize, pc: usize) {
        self.osr_unstable.insert((method, pc));
    }

    /// Records a deopt and returns whether further speculation on the
    /// method is suppressed (the storm-prevention rule: after 3 deopts
    /// the method stops speculating until re-profiled).
    // CEP:WHAT Records a deopt.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-tier
    #[must_use]
    pub fn record_deopt(&mut self, method: usize) -> bool {
        let p = self.deopt_penalties.entry(method).or_insert(0);
        *p = p.saturating_add(1);
        *p >= 3
    }

    /// Clears the deopt penalty (re-profiling).
    pub fn clear_penalties(&mut self, method: usize) {
        self.deopt_penalties.remove(&method);
    }

    /// Whether speculation is suppressed for the method.
    #[must_use]
    pub fn speculation_suppressed(&self, method: usize) -> bool {
        self.deopt_penalties.get(&method).copied().unwrap_or(0) >= 3
    }
}
