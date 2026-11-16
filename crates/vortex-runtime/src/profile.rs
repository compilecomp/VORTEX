// CEP:FILE crates/vortex-runtime/src/profile.rs
// CEP:WHAT The profiling data model consumed by tier transitions: per-site
//        execution counts, type feedback sets, branch outcomes, inline
//        cache statistics, and call-site statistics (T0 design 6.8).
// CEP:WHY Tier transitions and speculative rewriting are only as good as
//        the accuracy of this data; its shape is the T0-runtime contract.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Recording functions cannot fail; indices out of range are
//        rejected through typed results where growth is bounded.
// CEP:ASSUMES Sites are indexed by bytecode position within one method.
// CEP:COST O(1) per record; reset O(sites).
// CEP:EVIDENCE cargo test -p vortex-runtime
//! Profiling data model (T0 design 6.8).

/// The set of value types observed at one site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    /// Immediate integer.
    Int,
    /// Binary64 float.
    Float,
    /// Heap reference.
    Ref,
    /// Special immediate (undefined).
    Special,
}

/// Per-site profiling data (T0 design 6.8 ProfileData).
#[derive(Debug, Clone, Default)]
pub struct ProfileData {
    execution_counts: Vec<u64>,
    type_feedback: Vec<Vec<ValueType>>,
    branch_taken: Vec<u64>,
    branch_not_taken: Vec<u64>,
    ic_hits: Vec<u64>,
    ic_misses: Vec<u64>,
    call_callees: Vec<Vec<u32>>,
    call_counts: Vec<Vec<u64>>,
}

impl ProfileData {
    /// Ensures `sites` slots exist across all tables.
    // CEP:WHAT Reserves profiling slots for a method of `sites` sites.
    // CEP:WHY Side tables are indexed by site; resizing up front keeps
    //        recording allocation-free.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES sites is the instruction count.
    // CEP:COST O(sites).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn reserve(&mut self, sites: usize) {
        if self.execution_counts.len() < sites {
            self.execution_counts.resize(sites, 0);
        }
        if self.type_feedback.len() < sites {
            self.type_feedback.resize(sites, Vec::new());
        }
        if self.branch_taken.len() < sites {
            self.branch_taken.resize(sites, 0);
        }
        if self.branch_not_taken.len() < sites {
            self.branch_not_taken.resize(sites, 0);
        }
        if self.ic_hits.len() < sites {
            self.ic_hits.resize(sites, 0);
        }
        if self.ic_misses.len() < sites {
            self.ic_misses.resize(sites, 0);
        }
        if self.call_callees.len() < sites {
            self.call_callees.resize(sites, Vec::new());
        }
        if self.call_counts.len() < sites {
            self.call_counts.resize(sites, Vec::new());
        }
    }

    /// Increments the execution counter of a site.
    // CEP:WHAT Records one execution of a site.
    // CEP:WHY Tier transitions trigger on execution counts (6.10).
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; unknown sites are ignored.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn record_execution(&mut self, site: usize) {
        if let Some(c) = self.execution_counts.get_mut(site) {
            *c = c.saturating_add(1);
        }
    }

    /// The execution counter of a site.
    // CEP:WHAT Reads an execution counter.
    // CEP:WHY Threshold checks read counts.
    // CEP:STATUS complete
    // CEP:FAILURE Zero for unknown sites.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn execution_count(&self, site: usize) -> u64 {
        self.execution_counts.get(site).copied().unwrap_or(0)
    }

    /// Records a value type observed at a site, deduplicating.
    // CEP:WHAT Adds one observed type to a site's feedback set.
    // CEP:WHY Type speculation rewrites need the observed set.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; unknown sites are ignored.
    // CEP:ASSUMES None.
    // CEP:COST O(set size) for the membership test.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn record_type(&mut self, site: usize, t: ValueType) {
        if let Some(set) = self.type_feedback.get_mut(site) {
            if !set.contains(&t) {
                set.push(t);
            }
        }
    }

    /// The observed type set of a site.
    // CEP:WHAT Reads a site's type feedback set.
    // CEP:WHY Guard insertion reads the set.
    // CEP:STATUS complete
    // CEP:FAILURE Empty for unknown sites.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn type_feedback(&self, site: usize) -> &[ValueType] {
        self.type_feedback
            .get(site)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Records one branch outcome.
    // CEP:WHAT Counts taken or not-taken for a branch site.
    // CEP:WHY Branch prediction rewrites need outcome ratios.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; unknown sites are ignored.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn record_branch(&mut self, site: usize, taken: bool) {
        if taken {
            if let Some(c) = self.branch_taken.get_mut(site) {
                *c = c.saturating_add(1);
            }
        } else if let Some(c) = self.branch_not_taken.get_mut(site) {
            *c = c.saturating_add(1);
        }
    }

    /// The (taken, not-taken) pair of a branch site.
    // CEP:WHAT Reads branch outcome counters.
    // CEP:WHY Rewrite triggers compare outcomes.
    // CEP:STATUS complete
    // CEP:FAILURE (0, 0) for unknown sites.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn branch_outcome(&self, site: usize) -> (u64, u64) {
        (
            self.branch_taken.get(site).copied().unwrap_or(0),
            self.branch_not_taken.get(site).copied().unwrap_or(0),
        )
    }

    /// Records one inline cache hit or miss.
    // CEP:WHAT Counts an IC hit or miss at a site.
    // CEP:WHY Megamorphic detection uses miss ratios (6.5).
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; unknown sites are ignored.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn record_ic(&mut self, site: usize, hit: bool) {
        if hit {
            if let Some(c) = self.ic_hits.get_mut(site) {
                *c = c.saturating_add(1);
            }
        } else if let Some(c) = self.ic_misses.get_mut(site) {
            *c = c.saturating_add(1);
        }
    }

    /// The (hits, misses) pair of an IC site.
    // CEP:WHAT Reads IC counters.
    // CEP:WHY Hot megamorphic sites may deoptimize.
    // CEP:STATUS complete
    // CEP:FAILURE (0, 0) for unknown sites.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn ic_stats(&self, site: usize) -> (u64, u64) {
        (
            self.ic_hits.get(site).copied().unwrap_or(0),
            self.ic_misses.get(site).copied().unwrap_or(0),
        )
    }

    /// Records one call to a callee at a call site.
    // CEP:WHAT Counts a call to one callee from one site.
    // CEP:WHY Inline decisions read the callee distribution.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; unknown sites are ignored.
    // CEP:ASSUMES None.
    // CEP:COST O(callees at site).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn record_call(&mut self, site: usize, callee: u32) {
        if site >= self.call_callees.len() {
            return;
        }
        match self.call_callees[site].iter().position(|c| *c == callee) {
            Some(i) => {
                if let Some(v) = self.call_counts[site].get_mut(i) {
                    *v = v.saturating_add(1);
                }
            }
            None => {
                self.call_callees[site].push(callee);
                self.call_counts[site].push(1);
            }
        }
    }

    /// The callee/count distribution of a call site.
    // CEP:WHAT Reads a call site's distribution.
    // CEP:WHY Tier compilation selects inline targets.
    // CEP:STATUS complete
    // CEP:FAILURE Empty for unknown sites.
    // CEP:ASSUMES None.
    // CEP:COST O(callees at site).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn call_site(&self, site: usize) -> Vec<(u32, u64)> {
        match (self.call_callees.get(site), self.call_counts.get(site)) {
            (Some(callees), Some(counts)) => callees
                .iter()
                .copied()
                .zip(counts.iter().copied())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Resets all data (design 6.8: reset on recompilation).
    // CEP:WHAT Clears every table.
    // CEP:WHY Stale data must not guide a new compilation.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(sites).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn reset(&mut self) {
        self.execution_counts.fill(0);
        for s in self.type_feedback.iter_mut() {
            s.clear();
        }
        self.branch_taken.fill(0);
        self.branch_not_taken.fill(0);
        self.ic_hits.fill(0);
        self.ic_misses.fill(0);
        for c in self.call_callees.iter_mut() {
            c.clear();
        }
        for c in self.call_counts.iter_mut() {
            c.clear();
        }
    }
}
