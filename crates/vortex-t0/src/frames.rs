// CEP:FILE crates/vortex-t0/src/frames.rs
// CEP:WHAT The T0 frame and profiling data model: caller destinations,
//        activation frames, deopt state, inline caches, hot-event and
//        threshold configuration, and the dispatch helpers shared with the
//        interpreter core (T0 6.3-6.8).
// CEP:WHY Isolating the data model keeps the dispatch-loop file within the
//        atomic-file limit while preserving one concept per file.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Model types cannot fail; faults surface as Traps in the core.
// CEP:ASSUMES UGB operands and NV1 values as documented per type.
// CEP:COST O(1) per accessor; O(k) polymorphic cache search.
// CEP:EVIDENCE cargo test -p vortex-t0
//! T0 frame, cache, and profiling data model.

#![warn(missing_docs)]

use std::collections::HashMap;
use vortex_nv1::{Classification, Value};
use vortex_runtime::{Heap, HeapConfig};
use vortex_ugb::{Operand, Program};

use crate::interp::Interp;

/// Where a returning value is written in the caller frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dst {
    /// A virtual register.
    Reg(usize),
    /// A local slot (super bytecodes store results locally).
    Local(usize),
}

/// One activation frame.
#[derive(Debug, Clone)]
pub struct Frame {
    /// Callee method index.
    pub method: u32,
    /// Virtual registers.
    pub regs: Vec<Value>,
    /// Local slots.
    pub locals: Vec<Value>,
    /// Program counter into the method's code.
    pub pc: usize,
    /// Where the caller expects the return value.
    pub pending_dst: Dst,
}

/// Outcome of one interpreter entry.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// The entry method returned this value.
    Returned(Value),
    /// A speculative assumption failed; execution must continue in a
    /// higher tier from the captured state (T0 6.9).
    Deopt(DeoptState),
}

/// Captured interpreter state for tier transfer.
#[derive(Debug, Clone, PartialEq)]
pub struct DeoptState {
    /// Method whose frame deoptimized.
    pub method: u32,
    /// Program counter of the guard that failed.
    pub pc: usize,
    /// Register values at the deopt point.
    pub regs: Vec<Value>,
    /// Local values at the deopt point.
    pub locals: Vec<Value>,
    /// Which assumption failed.
    pub reason: &'static str,
}

/// Inline cache states (T0 design 6.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CacheState {
    /// No dispatch observed yet.
    #[default]
    Uninitialized,
    /// One key seen.
    Monomorphic,
    /// Two to four keys seen.
    Polymorphic,
    /// More than four keys; slow path.
    Megamorphic,
}

/// One inline cache entry: observed key plus resolved target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheEntry {
    /// Observed key (callee index, receiver shape, and so on).
    pub key: u32,
    /// Resolved dispatch target (callee index for direct calls).
    pub target: u32,
}

/// An inline cache attached to a call site (T0 design 6.5).
#[derive(Debug, Clone, Default)]
pub struct InlineCache {
    /// Cache state.
    pub state: CacheState,
    /// Observed (key, target) pairs; capped at four.
    pub entries: Vec<CacheEntry>,
    /// Hit count.
    pub hits: u64,
    /// Miss count.
    pub misses: u64,
}

impl InlineCache {
    /// A fresh cache.
    // CEP:WHAT Creates an uninitialized cache.
    // CEP:WHY Every call site starts uninitialized per 6.5.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-t0
    pub fn new() -> InlineCache {
        InlineCache {
            state: CacheState::Uninitialized,
            entries: Vec::new(),
            hits: 0,
            misses: 0,
        }
    }

    /// Cache lookup: hit returns the target; miss records the key.
    // CEP:WHAT Resolves a dispatch through the cache.
    // CEP:WHY Monomorphic and polymorphic hits skip the slow lookup.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; megamorphic sites return None.
    // CEP:ASSUMES None.
    // CEP:COST O(entries) <= 4.
    // CEP:EVIDENCE cargo test -p vortex-t0
    pub fn lookup(&mut self, key: u32) -> Option<u32> {
        match self.state {
            CacheState::Uninitialized => {
                self.entries.push(CacheEntry { key, target: key });
                self.state = CacheState::Monomorphic;
                self.misses += 1;
                Some(key)
            }
            CacheState::Monomorphic | CacheState::Polymorphic => {
                if let Some(e) = self.entries.iter().find(|e| e.key == key) {
                    self.hits += 1;
                    Some(e.target)
                } else if self.entries.len() < 4 {
                    self.entries.push(CacheEntry { key, target: key });
                    self.state = CacheState::Polymorphic;
                    self.misses += 1;
                    Some(key)
                } else {
                    self.state = CacheState::Megamorphic;
                    self.misses += 1;
                    None
                }
            }
            CacheState::Megamorphic => {
                self.misses += 1;
                None
            }
        }
    }
}

/// Hot-event emission for the tier controller (T0 design 6.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotEvent {
    /// A method crossed its call threshold.
    Method(u32),
    /// A loop back-edge crossed its iteration threshold.
    Loop(u32, usize),
}

/// Interpreter thresholds.
#[derive(Debug, Clone, Copy)]
pub struct Thresholds {
    /// Method calls before a tier-up event.
    pub method_calls: u64,
    /// Loop back-edges before a tier-up event.
    pub loop_iters: u64,
    /// Interpreter call-stack depth bound.
    pub stack_bound: usize,
}

impl Default for Thresholds {
    // CEP:WHAT Provides the documented default thresholds.
    // CEP:WHY T0 6.10: 1000 calls, 100 iterations; bounded failure depth.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-t0
    fn default() -> Thresholds {
        Thresholds {
            method_calls: 1000,
            loop_iters: 100,
            stack_bound: 512,
        }
    }
}

/// Profiling slots for one method (T0 design 6.8).
#[derive(Debug, Clone, Default)]
pub struct ProfileSlots {
    /// Integer observations per site.
    pub int_seen: Vec<u64>,
    /// Float observations per site.
    pub float_seen: Vec<u64>,
    /// Reference observations per site.
    pub ref_seen: Vec<u64>,
}

/// One dispatch step's result.
pub(crate) enum Step {
    /// Keep executing.
    Continue,
    /// The frame returned a value.
    Return(Value),
    /// Speculation failed; transfer to a higher tier.
    Deopt(DeoptState),
}

/// Reads a register operand.
// CEP:WHAT Extracts the register index at a position.
// CEP:WHY Handlers unpack operands through this helper.
// CEP:STATUS complete
// CEP:FAILURE Zero for malformed operands (verified programs never hit it).
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t0
pub(crate) fn reg(ops: &[Operand], i: usize) -> usize {
    match ops.get(i) {
        Some(Operand::Reg(r)) => *r as usize,
        _ => 0,
    }
}

/// Reads a local operand.
// CEP:WHAT Extracts the local index at a position.
// CEP:WHY Handlers unpack locals through this helper.
// CEP:STATUS complete
// CEP:FAILURE Zero for malformed operands.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t0
pub(crate) fn local(ops: &[Operand], i: usize) -> usize {
    match ops.get(i) {
        Some(Operand::Local(l)) => *l as usize,
        _ => 0,
    }
}

/// Reads a constant-pool operand.
// CEP:WHAT Extracts the pool index at a position.
// CEP:WHY Handlers resolve constants through this helper.
// CEP:STATUS complete
// CEP:FAILURE Zero for malformed operands.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t0
pub(crate) fn imm_const(ops: &[Operand], i: usize) -> u32 {
    match ops.get(i) {
        Some(Operand::ConstIdx(c)) => *c,
        _ => 0,
    }
}

/// Reads a count operand.
// CEP:WHAT Extracts the count at a position.
// CEP:WHY Calls and allocation share the count form.
// CEP:STATUS complete
// CEP:FAILURE Zero for malformed operands.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t0
pub(crate) fn count_of(ops: &[Operand]) -> usize {
    match ops.get(2) {
        Some(Operand::Count(n)) => *n as usize,
        _ => match ops.get(1) {
            Some(Operand::Count(n)) => *n as usize,
            _ => 0,
        },
    }
}

/// Computes a branch target from a label operand.
// CEP:WHAT Resolves a relative label to an absolute PC.
// CEP:WHY Jumps and super branches share the target rule.
// CEP:STATUS complete
// CEP:FAILURE Returns pc+size for malformed operands.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t0
pub(crate) fn target(pc: usize, ops: &[Operand], i: usize, size: usize) -> usize {
    match ops.get(i) {
        Some(Operand::Label(l)) => (pc as i64 + *l as i64).max(0) as usize,
        _ => pc + size,
    }
}

/// The model type tag of a value.
// CEP:WHAT Maps a value to its model type tag.
// CEP:WHY TYPE_CHECK and TYPE_CAST compare these tags.
// CEP:STATUS complete
// CEP:FAILURE Zero for undefined values.
// CEP:ASSUMES Tag 1 int, 2 float, 3 ref, 0 undefined.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t0
pub(crate) fn type_tag(v: Value) -> u8 {
    match Value::classify_word(v.bits()) {
        Classification::Int(_) => 1,
        Classification::FloatBits(_) => 2,
        Classification::HeapRef(_) => 3,
        _ => 0,
    }
}

/// The offset carried by a codec error, for trap diagnostics.
// CEP:WHAT Extracts the byte offset from a CodecError.
// CEP:WHY Interpreter faults must name the failing byte.
// CEP:STATUS complete
// CEP:FAILURE Zero when the error carries no offset.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t0
pub(crate) fn decode_offset(e: &vortex_ugb::CodecError) -> usize {
    match e {
        vortex_ugb::CodecError::UnexpectedEnd { offset } => *offset,
        vortex_ugb::CodecError::UnknownOpcode { offset, .. } => *offset,
        vortex_ugb::CodecError::BadOperand { offset, .. } => *offset,
    }
}

impl Interp {
    /// Builds an interpreter over a verified program.
    // CEP:WHAT Initializes heap, caches, profiles, and counters.
    // CEP:WHY T0 startup must be O(|bytecode|) with no compilation.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES The program is verified; unverified programs misbehave.
    // CEP:COST O(|program|).
    // CEP:EVIDENCE cargo test -p vortex-t0
    pub fn new(program: Program) -> Interp {
        let mut profiles = HashMap::new();
        for (i, m) in program.methods.iter().enumerate() {
            let sites = m.code.len();
            profiles.insert(
                i as u32,
                ProfileSlots {
                    int_seen: vec![0; sites],
                    float_seen: vec![0; sites],
                    ref_seen: vec![0; sites],
                },
            );
        }
        Interp {
            heap: Heap::new(HeapConfig::default()),
            tlab: None,
            caches: HashMap::new(),
            profiles,
            thresholds: Thresholds::default(),
            hot_events: Vec::new(),
            call_counts: HashMap::new(),
            loop_counts: HashMap::new(),
            exec_counts: HashMap::new(),
            program,
        }
    }
}
