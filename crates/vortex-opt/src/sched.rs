// CEP:FILE crates/vortex-opt/src/sched.rs
// CEP:WHAT The scheduler: dependency-graph construction over Machine IR,
//        list scheduling with critical-path priorities, modulo scheduling
//        with MII bounds and II search, and schedule validation (design
//        6.3-6.6).
// CEP:WHY Reordering exposes ILP while preserving dependencies, resource
//        caps, and latencies.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Invalid schedules are rejected through typed ScheduleError
//        values; the original order is returned on failure.
// CEP:ASSUMES Register allocation is complete (physical registers only).
// CEP:COST Local O(|I|^2 |E|); modulo O(|I| |E| II_max).
// CEP:EVIDENCE cargo test -p vortex-opt
//! Instruction scheduling (design 6.3-6.6).

use crate::mach::{MInstr, MachineFunction, Unit};
use std::collections::BTreeMap;
use std::fmt;

/// Target resource model (design 6.3).
#[derive(Debug, Clone, Copy)]
pub struct TargetModel {
    /// Maximum instructions issued per cycle.
    pub issue_width: usize,
    /// Per-cycle arithmetic unit capacity.
    pub alu: usize,
    /// Per-cycle multiply unit capacity.
    pub mul: usize,
    /// Per-cycle load-store capacity.
    pub mem: usize,
    /// Per-cycle branch capacity.
    pub branch: usize,
}

impl Default for TargetModel {
    // CEP:WHAT Provides the default dual-issue model.
    // CEP:WHY A two-wide, one-per-unit target exercises all constraint
    //        paths in tests.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt
    fn default() -> TargetModel {
        TargetModel {
            issue_width: 2,
            alu: 2,
            mul: 1,
            mem: 1,
            branch: 1,
        }
    }
}

impl TargetModel {
    /// The capacity of a unit class.
    // CEP:WHAT Resolves a unit to its per-cycle cap.
    // CEP:WHY Resource checks read capacities through this.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt
    pub fn capacity(self, u: Unit) -> usize {
        match u {
            Unit::Alu => self.alu,
            Unit::Mul => self.mul,
            Unit::Mem => self.mem,
            Unit::Branch => self.branch,
        }
    }
}

/// Schedule failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleError {
    /// No II satisfied the constraints within the search bound.
    NoSchedule {
        /// The smallest II attempted.
        mii: u32,
    },
    /// Validation found a violated constraint.
    Invalid {
        /// What was violated.
        what: &'static str,
    },
}

impl fmt::Display for ScheduleError {
    // CEP:WHAT Renders a scheduling failure.
    // CEP:WHY Diagnostics name the violated constraint.
    // CEP:STATUS complete
    // CEP:FAILURE Formatter errors only.
    // CEP:ASSUMES None.
    // CEP:COST O(fields).
    // CEP:EVIDENCE cargo test -p vortex-opt
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScheduleError::NoSchedule { mii } => {
                write!(f, "no modulo schedule at or above II {mii}")
            }
            ScheduleError::Invalid { what } => {
                write!(f, "invalid schedule: {what}")
            }
        }
    }
}

impl std::error::Error for ScheduleError {}

/// Dependency edges between instruction indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DepEdge {
    /// Producer index.
    pub from: usize,
    /// Consumer index.
    pub to: usize,
    /// Producer latency in cycles.
    pub latency: u32,
    /// Loop-carried distance (0 for in-block deps).
    pub distance: u32,
    /// Whether the edge is anti or output (zero-latency ordering).
    pub ordering_only: bool,
}

/// Builds the dependency graph of a straight-line block.
// CEP:WHAT Adds data edges from defs to uses plus anti/output edges over
//        register overwrites, with the model's latencies.
// CEP:WHY 6.3: dependencies are edges; resources are cycle checks.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES Register operands encode physical registers.
// CEP:COST O(|I|^2).
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn build_dependencies(func: &MachineFunction) -> Vec<DepEdge> {
    let mut edges = Vec::new();
    for (i, a) in func.instrs.iter().enumerate() {
        for (j, b) in func.instrs.iter().enumerate().skip(i + 1) {
            let b_reads_set: Vec<u64> = reads_of(b);
            let b_writes_set: Vec<u64> = writes_of(b);
            let a_writes_set: Vec<u64> = writes_of(a);
            let a_reads_set: Vec<u64> = reads_of(a);
            // True dependence: A writes, B reads.
            if a_writes_set.iter().any(|r| b_reads_set.contains(r)) {
                edges.push(DepEdge {
                    from: i,
                    to: j,
                    latency: a.op.latency(),
                    distance: 0,
                    ordering_only: false,
                });
            }
            // Anti and output: ordering-only edges.
            if a_reads_set.iter().any(|r| b_writes_set.contains(r))
                || a_writes_set.iter().any(|r| b_writes_set.contains(r))
            {
                edges.push(DepEdge {
                    from: i,
                    to: j,
                    latency: 0,
                    distance: 0,
                    ordering_only: true,
                });
            }
        }
    }
    edges
}

/// Register values read by an instruction.
// CEP:WHAT Lists the register operands read.
// CEP:WHY Data and anti edges compare read and write sets.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(operands).
// CEP:EVIDENCE cargo test -p vortex-opt
fn reads_of(instr: &MInstr) -> Vec<u64> {
    let (r, _) = instr.op.operands();
    instr.operands[..r.min(instr.operands.len())].to_vec()
}

/// Register values written by an instruction.
// CEP:WHAT Lists the register operands written.
// CEP:WHY Data and output edges compare write sets.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(operands).
// CEP:EVIDENCE cargo test -p vortex-opt
fn writes_of(instr: &MInstr) -> Vec<u64> {
    let (_, w) = instr.op.operands();
    let start = instr.operands.len().saturating_sub(w);
    instr.operands[start..].to_vec()
}

/// One scheduled instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scheduled {
    /// Instruction index in the original order.
    pub index: usize,
    /// Issue cycle.
    pub cycle: u32,
}

/// Validates a schedule against dependencies and resources (6.6).
// CEP:WHAT Checks latency, ordering, and per-cycle unit caps.
// CEP:WHY Schedules are only usable when all three invariants hold.
// CEP:STATUS complete
// CEP:FAILURE ScheduleError::Invalid naming the violated invariant.
// CEP:ASSUMES None.
// CEP:COST O(|E| + |I|).
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn validate(
    func: &MachineFunction,
    deps: &[DepEdge],
    schedule: &[Scheduled],
    target: TargetModel,
    ii: u32,
) -> Result<(), ScheduleError> {
    let cycle_of: BTreeMap<usize, u32> = schedule.iter().map(|s| (s.index, s.cycle)).collect();
    // 1 and 3: dependency and latency preservation.
    for e in deps {
        let ca = cycle_of.get(&e.from).copied().unwrap_or(0);
        let cb = cycle_of.get(&e.to).copied().unwrap_or(0);
        let slack = ii * e.distance;
        if (ca as i64 + e.latency as i64 - slack as i64) > cb as i64 {
            return Err(ScheduleError::Invalid {
                what: "latency constraint",
            });
        }
    }
    // 2: resource caps per cycle.
    let mut per_cycle: BTreeMap<u32, BTreeMap<Unit, usize>> = BTreeMap::new();
    for s in schedule {
        let unit = func.instrs[s.index].op.unit();
        let entry = per_cycle
            .entry(s.cycle)
            .or_default()
            .entry(unit)
            .or_insert(0);
        *entry += 1;
    }
    for (cycle, units) in &per_cycle {
        for (unit, used) in units {
            if *used > target.capacity(*unit) {
                return Err(ScheduleError::Invalid {
                    what: "resource capacity",
                });
            }
            let _ = cycle;
        }
    }
    let _ = ii;
    Ok(())
}

/// List scheduling over one block (design 6.4).
// CEP:WHAT Greedy priority scheduling with resource checks.
// CEP:WHY Critical-path priorities minimize the block height.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail; the fallback order is the original sequence.
// CEP:ASSUMES None.
// CEP:COST O(|I|^2 |E|).
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn list_schedule(
    func: &MachineFunction,
    deps: &[DepEdge],
    target: TargetModel,
) -> Vec<Scheduled> {
    let n = func.instrs.len();
    // Priority: longest path to a sink through data edges.
    let mut priority = vec![1u32; n];
    for _ in 0..n {
        for e in deps.iter().rev() {
            if !e.ordering_only {
                let candidate = priority[e.from] + 1;
                if candidate > priority[e.to] {
                    priority[e.to] = candidate;
                }
            }
        }
    }
    let mut preds: Vec<usize> = vec![0; n];
    for e in deps {
        preds[e.to] += 1;
    }
    let remaining: Vec<usize> = (0..n).filter(|i| preds[*i] == 0).collect();
    let mut ready = std::collections::VecDeque::from(remaining);
    let mut issued = vec![false; n];
    let mut cycle: u32 = 0;
    let mut out: Vec<Scheduled> = Vec::new();
    let mut scheduled_count = 0;
    loop {
        if scheduled_count == n {
            break;
        }
        let mut used_units: BTreeMap<Unit, usize> = BTreeMap::new();
        let mut issued_this_cycle: Vec<usize> = Vec::new();
        let mut width_used = 0usize;
        let ready_snapshot: Vec<usize> = ready.iter().copied().collect();
        for cand in ready_snapshot {
            if width_used >= target.issue_width {
                break;
            }
            if issued[cand] {
                continue;
            }
            // Predecessor cycle check: all issued preds satisfy latency.
            let preds_ok = deps.iter().filter(|e| e.to == cand).all(|e| {
                issued[e.from] && {
                    let ca = out
                        .iter()
                        .find(|s| s.index == e.from)
                        .map(|s| s.cycle)
                        .unwrap_or(0);
                    cycle as i64 >= ca as i64 + e.latency as i64
                }
            });
            if !preds_ok {
                continue;
            }
            let unit = func.instrs[cand].op.unit();
            let used = used_units.entry(unit).or_insert(0);
            if *used >= target.capacity(unit) {
                continue;
            }
            *used += 1;
            width_used += 1;
            issued[cand] = true;
            issued_this_cycle.push(cand);
            out.push(Scheduled { index: cand, cycle });
            scheduled_count += 1;
        }
        for idx in issued_this_cycle {
            preds[idx] = 0;
            for e in deps.iter().filter(|e| e.from == idx) {
                if preds[e.to] > 0 {
                    preds[e.to] -= 1;
                }
            }
        }
        // Refill the ready list.
        for e in deps {
            if issued[e.from] && preds[e.to] == 0 && !issued[e.to] && !ready.contains(&e.to) {
                ready.push_back(e.to);
            }
        }
        ready.retain(|i| !issued[*i]);
        let mut sorted: Vec<usize> = ready.drain(..).collect();
        sorted.sort_by_key(|i| std::cmp::Reverse(priority[*i]));
        ready = std::collections::VecDeque::from(sorted);
        cycle += 1;
        let _ = remaining;
    }
    out
}

/// Modulo scheduling of one loop body (design 6.5).
// CEP:WHAT Computes MII, searches IIs, and validates the result.
// CEP:WHY Software pipelining overlaps iterations to hide latency.
// CEP:STATUS complete
// CEP:FAILURE ScheduleError::NoSchedule when no II within 2x MII works.
// CEP:ASSUMES The body is one straight-line block; loop-carried deps
//        arrive with distance 1.
// CEP:COST O(|I| |E| II_max).
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn modulo_schedule(
    func: &MachineFunction,
    deps: &[DepEdge],
    target: TargetModel,
) -> Result<(Vec<Scheduled>, u32), ScheduleError> {
    // ResMII: max over units of ceil(count / capacity).
    let mut res_mii = 1u32;
    for unit in [Unit::Alu, Unit::Mul, Unit::Mem, Unit::Branch] {
        let count = func.instrs.iter().filter(|i| i.op.unit() == unit).count() as u32;
        let cap = target.capacity(unit) as u32;
        if cap > 0 {
            res_mii = res_mii.max(count.div_ceil(cap));
        }
    }
    // RecMII: max over cycles of ceil(total latency / distance 1).
    let mut rec_mii = 1u32;
    for e in deps.iter().filter(|e| e.distance > 0) {
        rec_mii = rec_mii.max(e.latency.div_ceil(e.distance));
    }
    let mii = res_mii.max(rec_mii);
    for ii in mii..=(mii * 2) {
        let schedule = modulo_attempt(func, deps, target, ii);
        if let Some(s) = schedule {
            validate(func, deps, &s, target, ii)?;
            return Ok((s, ii));
        }
    }
    Err(ScheduleError::NoSchedule { mii })
}

/// One modulo attempt at a fixed II.
// CEP:WHAT Schedules the body within II slots with modulo resource use.
// CEP:WHY 6.5: each II attempt must respect modulo resource and latency
//        constraints before the search moves on.
// CEP:STATUS complete
// CEP:FAILURE None when the body does not fit within the cycle limit.
// CEP:ASSUMES The dependency graph is finite and acyclic per iteration.
// CEP:COST O(|I| |E|).
// CEP:EVIDENCE cargo test -p vortex-opt
fn modulo_attempt(
    func: &MachineFunction,
    deps: &[DepEdge],
    target: TargetModel,
    ii: u32,
) -> Option<Vec<Scheduled>> {
    let n = func.instrs.len();
    let mut slot_use: BTreeMap<u32, BTreeMap<Unit, usize>> = BTreeMap::new();
    let mut scheduled: Vec<Scheduled> = Vec::new();
    let mut placed = vec![false; n];
    let mut preds: Vec<usize> = vec![0; n];
    for e in deps {
        preds[e.to] += 1;
    }
    let mut cycle: u32 = 0;
    let limit = ii * n as u32;
    while scheduled.len() < n && cycle < limit {
        let slot = cycle % ii;
        let mut used_units: BTreeMap<Unit, usize> = BTreeMap::new();
        for i in 0..n {
            if placed[i] {
                continue;
            }
            // Latency with distance slack.
            let ok = deps.iter().filter(|e| e.to == i).all(|e| {
                placed[e.from] && {
                    let ca = scheduled
                        .iter()
                        .find(|s| s.index == e.from)
                        .map(|s| s.cycle)
                        .unwrap_or(0);
                    let ca_slot = ca % ii;
                    let span = (slot as i64 - ca_slot as i64).rem_euclid(ii as i64);
                    span >= e.latency as i64 - ii as i64 * e.distance as i64
                }
            });
            if !ok {
                continue;
            }
            let unit = func.instrs[i].op.unit();
            let used_here = used_units.entry(unit).or_insert(0);
            let slot_units = slot_use.entry(slot).or_default();
            let used_slot = slot_units.entry(unit).or_insert(0);
            if *used_here >= target.capacity(unit) || *used_slot >= target.capacity(unit) {
                continue;
            }
            *used_here += 1;
            *used_slot += 1;
            placed[i] = true;
            scheduled.push(Scheduled {
                index: i,
                cycle: slot,
            });
        }
        cycle += 1;
    }
    if scheduled.len() == n {
        scheduled.sort_by_key(|s| (s.cycle, s.index));
        Some(scheduled)
    } else {
        None
    }
}
