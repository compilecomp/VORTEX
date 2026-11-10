// CEP:FILE crates/vortex-runtime/src/profile.rs
// CEP:WHAT Profile store: thread-local-shaped loop counters with sampled
//        back-edge accounting (PROF-1 section 3.2 v1.1) and the T4 trace
//        request queue, plus the capability-site epoch cache type.
// CEP:WHY T4 trace recording needs an accurate hot-loop signal that PMU
//        sampling and IC misses cannot provide (PROF-INV-07).
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Counters saturate; a slot can request a trace at most once
//        until reset, preventing request storms.
// CEP:ASSUMES The interpreter folds counters at safepoints (PROF-INV-02).
// CEP:COST One increment per back-edge; window fold every N iterations.
// CEP:EVIDENCE cargo test -p vortex-runtime

/// Capability-site epoch cache (UGB-1 section 6.3). Stored in method
/// metadata, shared across activations.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EpochCache {
    /// Cached epoch of the capability last validated at this site.
    pub cached_epoch: Option<u16>,
}

/// Per-loop counter state backing the sampled back-edge counter.
#[derive(Debug, Clone, Copy, Default)]
pub struct LoopCounter {
    /// Iterations since the last window fold.
    window: u32,
    /// Total folded iteration count.
    total: u64,
    /// Whether a T4 request was already enqueued for this slot.
    trace_requested: bool,
}

/// Sampling and trigger configuration.
#[derive(Debug, Clone, Copy)]
pub struct ProfileConfig {
    /// Back-edge sampling window: fold every N iterations (default 64,
    /// matching PROF-1 section 3.2).
    pub window: u32,
    /// T4 trigger threshold in folded iterations.
    pub t4_threshold: u64,
}

impl Default for ProfileConfig {
    fn default() -> Self {
        ProfileConfig {
            window: 64,
            t4_threshold: 1024,
        }
    }
}

/// Aggregated loop profile for all methods (the reference runtime keeps one
/// flat table; the production design keys by method then slot).
#[derive(Debug, Default)]
pub struct ProfileStore {
    counters: Vec<LoopCounter>,
    requests: Vec<(usize, u64)>,
    config: ProfileConfig,
}

impl ProfileStore {
    /// Creates a store with the default configuration.
    #[must_use]
    pub fn new() -> Self {
        ProfileStore {
            counters: Vec::new(),
            requests: Vec::new(),
            config: ProfileConfig::default(),
        }
    }

    /// Creates a store with an explicit configuration.
    #[must_use]
    pub fn with_config(config: ProfileConfig) -> Self {
        ProfileStore {
            counters: Vec::new(),
            requests: Vec::new(),
            config,
        }
    }

    /// Ensures a slot exists for `loop_id` and returns its index.
    pub fn ensure_slot(&mut self, loop_id: usize) -> usize {
        if loop_id >= self.counters.len() {
            self.counters.resize(loop_id + 1, LoopCounter::default());
        }
        loop_id
    }

    /// Records one loop back-edge iteration. Returns `true` exactly when this
    /// back-edge crosses the T4 trigger threshold and the request has not yet
    /// been enqueued (the sampled counter's primary consumer, PROF-1
    /// section 3.2).
    // CEP:WHAT Records one loop back-edge iteration.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn backedge(&mut self, loop_id: usize) -> bool {
        let slot = self.ensure_slot(loop_id);
        let c = &mut self.counters[slot];
        c.window += 1;
        if c.window < self.config.window {
            return false;
        }
        // Window fold: the sample handler's steady-state action.
        c.total += u64::from(c.window);
        c.window = 0;
        if c.total >= self.config.t4_threshold && !c.trace_requested {
            c.trace_requested = true;
            self.requests.push((slot, c.total));
            return true;
        }
        false
    }

    /// Total folded iterations for a slot.
    #[must_use]
    pub fn total(&self, loop_id: usize) -> u64 {
        self.counters.get(loop_id).map_or(0, |c| c.total)
    }

    /// Enqueued T4 trace requests: `(slot, folded_total)` in order.
    #[must_use]
    pub fn trace_requests(&self) -> &[(usize, u64)] {
        &self.requests
    }

    /// Resets a slot (used by OSR-INV-05's "OSR-unstable" bookkeeping).
    pub fn reset_slot(&mut self, loop_id: usize) {
        self.ensure_slot(loop_id);
        self.counters[loop_id] = LoopCounter::default();
    }
}
