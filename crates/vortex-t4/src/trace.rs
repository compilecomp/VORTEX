// CEP:FILE crates/vortex-t4/src/trace.rs
// CEP:WHAT Guarded trace recording: hot-path traces, speculative guard
//        bookkeeping, and the deopt counter that disables speculation
//        after repeated violations (T0 design 6.9; deopt design 6.9).
// CEP:WHY T4 compiles hot paths under speculative assumptions; repeated
//        deopts must disable the speculation, not thrash the tier.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Guard state transitions are total; no guard both disables
//        and stays enabled.
// CEP:ASSUMES Guard ids are unique per trace.
// CEP:COST O(1) per event.
// CEP:EVIDENCE cargo test -p vortex-t4
//! Guarded trace recording.

use std::collections::HashMap;

/// Speculative assumptions a T4 trace carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assumption {
    /// All values at a site were immediate integers.
    IntTypes {
        /// The bytecode site.
        site: usize,
    },
    /// A call site resolved to one callee.
    MonomorphicCall {
        /// The call site.
        site: usize,
        /// The observed callee.
        callee: u32,
    },
    /// A loop bound stayed within a constant.
    ConstantBound {
        /// The back-edge site.
        site: usize,
        /// The observed bound.
        bound: i64,
    },
}

/// One speculative guard.
#[derive(Debug, Clone)]
pub struct Guard {
    /// Guard identifier within its trace.
    pub id: u32,
    /// The assumption.
    pub assumption: Assumption,
    /// Violation count.
    pub failures: u32,
    /// Whether the guard still protects compiled code.
    pub enabled: bool,
}

/// A recorded hot path for one method.
#[derive(Debug, Clone, Default)]
pub struct Trace {
    /// The method.
    pub method: u32,
    /// Hot bytecode sites observed by profiling.
    pub hot_sites: Vec<usize>,
    /// Guards installed on the trace.
    pub guards: Vec<Guard>,
    /// Back-edge iterations observed.
    pub iterations: u64,
}

/// The recorder: per-method traces and the deopt policy.
pub struct TraceRecorder {
    traces: HashMap<u32, Trace>,
    /// Deoptimizations before speculation is disabled (deopt design 6.9:
    /// "if a function is deoptimized too many times (e.g., > 10)").
    deopt_limit: u32,
}

impl TraceRecorder {
    /// A recorder with the documented limit of 10 deopts.
    // CEP:WHAT Creates the recorder.
    // CEP:WHY The deopt counter policy caps repeated speculation.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-t4
    pub fn new() -> TraceRecorder {
        TraceRecorder {
            traces: HashMap::new(),
            deopt_limit: 10,
        }
    }

    /// Records a hot path for a method.
    // CEP:WHAT Adds or updates a trace with observed hot sites.
    // CEP:WHY T4 compiles only what profiling marks hot.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(sites).
    // CEP:EVIDENCE cargo test -p vortex-t4
    pub fn record_hot(&mut self, method: u32, sites: &[usize], iterations: u64) {
        let t = self.traces.entry(method).or_default();
        t.method = method;
        for s in sites {
            if !t.hot_sites.contains(s) {
                t.hot_sites.push(*s);
            }
        }
        t.iterations += iterations;
    }

    /// Whether a method has a recorded trace.
    // CEP:WHAT Reports trace existence.
    // CEP:WHY The compile entry checks before running the pipeline.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-t4
    pub fn has_trace(&self, method: u32) -> bool {
        self.traces.contains_key(&method)
    }

    /// Installs a guard on a method's trace.
    // CEP:WHAT Adds one guard.
    // CEP:WHY Speculation requires a check with a deopt target.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES The method has a trace.
    // CEP:COST O(1) amortized.
    // CEP:EVIDENCE cargo test -p vortex-t4
    pub fn add_guard(&mut self, method: u32, assumption: Assumption) -> Option<u32> {
        let t = self.traces.get_mut(&method)?;
        let id = t.guards.len() as u32;
        t.guards.push(Guard {
            id,
            assumption,
            failures: 0,
            enabled: true,
        });
        Some(id)
    }

    /// Records a guard failure; disables the guard at the deopt limit.
    // CEP:WHAT Counts one violation and disables at the limit.
    // CEP:WHY Repeated deopts must not thrash the tier system.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; unknown guards return false.
    // CEP:ASSUMES None.
    // CEP:COST O(guards).
    // CEP:EVIDENCE cargo test -p vortex-t4
    pub fn guard_failed(&mut self, method: u32, guard_id: u32) -> bool {
        let Some(t) = self.traces.get_mut(&method) else {
            return false;
        };
        let Some(g) = t.guards.iter_mut().find(|g| g.id == guard_id) else {
            return false;
        };
        g.failures += 1;
        if g.failures >= self.deopt_limit {
            g.enabled = false;
        }
        g.enabled
    }

    /// Number of disabled guards on a method.
    // CEP:WHAT Reports disabled speculation.
    // CEP:WHY Recompilation must drop disabled assumptions.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(guards).
    // CEP:EVIDENCE cargo test -p vortex-t4
    pub fn disabled_count(&self, method: u32) -> usize {
        self.traces
            .get(&method)
            .map(|t| t.guards.iter().filter(|g| !g.enabled).count())
            .unwrap_or(0)
    }

    /// The trace of a method.
    // CEP:WHAT Returns a trace reference.
    // CEP:WHY The compile pipeline reads hot sites and guards.
    // CEP:STATUS complete
    // CEP:FAILURE None without a trace.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-t4
    pub fn trace(&self, method: u32) -> Option<&Trace> {
        self.traces.get(&method)
    }
}

impl Default for TraceRecorder {
    // CEP:WHAT Provides the default recorder.
    // CEP:WHY One documented policy instance.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-t4
    fn default() -> TraceRecorder {
        TraceRecorder::new()
    }
}
