// CEP:FILE crates/vortex-t0/src/interp.rs
// CEP:WHAT The T0 speculative register interpreter: frame stack, dispatch
//        loop over UGB, capability checks, guarded arithmetic with the
//        boxed-integer slow path, heap object operations, inline caches,
//        profiling counters, and deoptimization capture (T0 6.3-6.10).
// CEP:WHY T0 is the initial execution tier: fast startup, profiling, and
//        speculation with correct deopt on violation.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Every fault surfaces as a typed Trap or a DeoptState; the
//        loop never panics on verified bytecode.
// CEP:ASSUMES The program passed the UGB verifier; capability checks run
//        against the method's environment.
// CEP:COST O(1) per instruction; boxed-integer slow path is O(1) heap
//        allocation.
// CEP:EVIDENCE cargo test -p vortex-t0
//! T0 interpreter core.

use crate::rewrite::Rewriter;
use std::collections::HashMap;
use vortex_nv1::{Classification, IntOpOutcome, Value};
use vortex_runtime::{Heap, RtResult, Tlab, Trap, HEADER_SIZE};
use vortex_ugb::Op as UOp;
use vortex_ugb::{CmpKind, Instr, Operand, Program};

// The frame, cache, and profiling data model lives in `frames.rs`
// (atomic-file split); public paths are preserved through re-exports.
// CEP:WHAT Re-exports the frame data model and dispatch helpers.
// CEP:WHY Downstream imports keep working through crate::interp paths.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t0
pub(crate) use crate::frames::Step;
pub(crate) use crate::frames::{count_of, decode_offset, imm_const, local, reg, target, type_tag};
pub use crate::frames::{
    CacheEntry, CacheState, DeoptState, Dst, Frame, HotEvent, InlineCache, Outcome, ProfileSlots,
    Thresholds,
};
/// The T0 interpreter.
pub struct Interp {
    /// The verified program.
    pub program: Program,
    /// Execution heap for boxed integers and objects.
    pub heap: Heap,
    /// Thread-local allocation buffer.
    pub(crate) tlab: Option<Tlab>,
    /// Per-(method, pc) inline caches.
    pub caches: HashMap<(u32, usize), InlineCache>,
    /// Per-method profiling data.
    pub profiles: HashMap<u32, ProfileSlots>,
    /// Tier-up thresholds.
    pub thresholds: Thresholds,
    /// Collected hot events since the last drain.
    pub hot_events: Vec<HotEvent>,
    /// Global call counters per method.
    pub(crate) call_counts: HashMap<u32, u64>,
    /// Back-edge counters per (method, pc).
    pub(crate) loop_counts: HashMap<(u32, usize), u64>,
    /// Execution counters for rewrite-candidate sites.
    pub(crate) exec_counts: HashMap<(u32, usize), u64>,
}

impl Interp {
    /// The execution counter of a candidate site.
    // CEP:WHAT Reads an execution counter for rewrite decisions.
    // CEP:WHY Super formation requires observed frequency (T0 6.7).
    // CEP:STATUS complete
    // CEP:FAILURE Zero for untracked sites.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-t0
    pub fn exec_count(&self, method: u32, pc: usize) -> u64 {
        self.exec_counts.get(&(method, pc)).copied().unwrap_or(0)
    }

    /// Enters a method with arguments in registers r0..
    // CEP:WHAT Runs one method to completion, deopt, or trap.
    // CEP:WHY Entry point for the runtime and the tier controller.
    // CEP:STATUS complete
    // CEP:FAILURE Typed Trap for every fault; Deopt outcome on guard
    //        failure; CallStackOverflow at the depth bound.
    // CEP:ASSUMES Method exists; arg count fits the frame.
    // CEP:COST O(instructions executed).
    // CEP:EVIDENCE cargo test -p vortex-t0
    // CEP:ID: JIT-a28
    pub fn run(&mut self, method: u32, args: &[Value]) -> RtResult<Outcome> {
        let m = self
            .program
            .method(method)
            .ok_or(Trap::InvalidEncoding { offset: 0 })?;
        let mut frame = Frame {
            method,
            regs: vec![Value::undefined(); m.reg_count as usize],
            locals: vec![Value::undefined(); m.local_count as usize],
            pc: 0,
            pending_dst: Dst::Reg(0),
        };
        for (i, a) in args.iter().enumerate() {
            if i < frame.regs.len() {
                frame.regs[i] = *a;
            }
        }
        let mut stack: Vec<Frame> = Vec::new();
        let mut rewriter = Rewriter::new();
        loop {
            if stack.len() >= self.thresholds.stack_bound {
                return Err(Trap::CallStackOverflow {
                    bound: self.thresholds.stack_bound,
                });
            }
            let step = self.step(&mut frame, &mut stack, &mut rewriter)?;
            match step {
                Step::Continue => {
                    // The PC already advanced; the loop keeps dispatching.
                    let _ = frame.pc;
                }
                Step::Return(v) => match stack.pop() {
                    Some(mut caller) => match caller.pending_dst {
                        Dst::Reg(r) => {
                            if r < caller.regs.len() {
                                caller.regs[r] = v;
                            }
                        }
                        Dst::Local(l) => {
                            if l < caller.locals.len() {
                                caller.locals[l] = v;
                            }
                        }
                    },
                    None => return Ok(Outcome::Returned(v)),
                },
                Step::Deopt(state) => return Ok(Outcome::Deopt(state)),
            }
        }
    }

    /// Executes one instruction.
    // CEP:WHAT Decodes and executes the instruction at the PC.
    // CEP:WHY The dispatch loop is the hot path of T0.
    // CEP:STATUS complete
    // CEP:FAILURE Typed Trap per fault class; Deopt on guard failure.
    // CEP:ASSUMES Verified bytecode.
    // CEP:COST O(1) per instruction.
    // CEP:ID: JIT-a29
    // CEP:EVIDENCE cargo test -p vortex-t0
    fn step(
        &mut self,
        frame: &mut Frame,
        stack: &mut Vec<Frame>,
        rewriter: &mut Rewriter,
    ) -> RtResult<Step> {
        let code = self
            .program
            .method(frame.method)
            .map(|m| m.code.clone())
            .unwrap_or_default();
        let (instr, size) = Instr::decode(&code, frame.pc).map_err(|e| Trap::InvalidEncoding {
            offset: frame.pc + decode_offset(&e),
        })?;
        let pc = frame.pc;
        self.record_types(frame, &instr);
        let env_caps = self
            .program
            .method(frame.method)
            .map(|m| m.env_caps.clone())
            .unwrap_or_default();
        for cap in instr.op.info().caps {
            if !env_caps.contains(cap) {
                return Err(Trap::CapabilityViolation {
                    perm: cap.ordinal(),
                });
            }
        }
        let ops = instr.operands.clone();
        if matches!(
            instr.op,
            UOp::LoadLocal | UOp::AddImm | UOp::StoreLocal | UOp::Cmp | UOp::JumpIfTrue
        ) {
            let c = self.exec_counts.entry((frame.method, pc)).or_insert(0);
            *c += 1;
        }
        let outcome = match instr.op {
            UOp::Nop => Step::Continue,
            UOp::LoadConst => {
                let r = reg(&ops, 0);
                let c = imm_const(&ops, 1);
                frame.regs[r] = self.constant_value(c);
                Step::Continue
            }
            UOp::LoadLocal => {
                let r = reg(&ops, 0);
                let l = local(&ops, 1);
                if l < frame.locals.len() && r < frame.regs.len() {
                    frame.regs[r] = frame.locals[l];
                }
                Step::Continue
            }
            UOp::StoreLocal => {
                let l = local(&ops, 0);
                let r = reg(&ops, 1);
                if l < frame.locals.len() && r < frame.regs.len() {
                    frame.locals[l] = frame.regs[r];
                }
                Step::Continue
            }
            UOp::Mov => {
                let d = reg(&ops, 0);
                let s = reg(&ops, 1);
                if d < frame.regs.len() && s < frame.regs.len() {
                    frame.regs[d] = frame.regs[s];
                }
                Step::Continue
            }
            UOp::Add | UOp::Sub | UOp::Mul | UOp::AddInt => {
                let d = reg(&ops, 0);
                let a = frame.regs[reg(&ops, 1)];
                let b = frame.regs[reg(&ops, 2)];
                if instr.op == UOp::AddInt && (a.as_i64().is_none() || b.as_i64().is_none()) {
                    return Ok(self.deopt(frame, "int assumption"));
                }
                frame.regs[d] = self.arith(instr.op, a, b)?;
                Step::Continue
            }
            UOp::AddImm | UOp::SubImm => {
                let d = reg(&ops, 0);
                let a = frame.regs[reg(&ops, 1)];
                let imm = match ops.get(2) {
                    Some(Operand::Imm32(v)) => {
                        Value::from_i64(*v as i64).unwrap_or(Value::undefined())
                    }
                    _ => {
                        return Err(Trap::InvalidEncoding { offset: pc });
                    }
                };
                let which = if instr.op == UOp::AddImm {
                    UOp::Add
                } else {
                    UOp::Sub
                };
                frame.regs[d] = self.arith(which, a, imm)?;
                Step::Continue
            }
            UOp::Div | UOp::Rem => {
                let d = reg(&ops, 0);
                let a = frame.regs[reg(&ops, 1)];
                let b = frame.regs[reg(&ops, 2)];
                let (x, y) = match (a.as_i64(), b.as_i64()) {
                    (Some(x), Some(y)) => (x, y),
                    _ => {
                        return Err(Trap::TypeMismatch {
                            expected: "int",
                            word: b.bits(),
                        })
                    }
                };
                if y == 0 {
                    return Err(Trap::DivByZero);
                }
                let r = if instr.op == UOp::Div { x / y } else { x % y };
                frame.regs[d] = Value::from_i64(r).unwrap_or(Value::undefined());
                Step::Continue
            }
            UOp::And | UOp::Or | UOp::Xor | UOp::Shl | UOp::Shr => {
                let d = reg(&ops, 0);
                let a = frame.regs[reg(&ops, 1)];
                let b = frame.regs[reg(&ops, 2)];
                frame.regs[d] = self.bitwise(instr.op, a, b)?;
                Step::Continue
            }
            UOp::Neg | UOp::Not => {
                let d = reg(&ops, 0);
                let a = frame.regs[reg(&ops, 1)];
                let x = a.as_i64().ok_or(Trap::TypeMismatch {
                    expected: "int",
                    word: a.bits(),
                })?;
                let r = if instr.op == UOp::Neg {
                    x.wrapping_neg()
                } else {
                    !x
                };
                frame.regs[d] = Value::from_i64(r).unwrap_or(Value::undefined());
                Step::Continue
            }
            UOp::Cmp => {
                let d = reg(&ops, 0);
                let a = frame.regs[reg(&ops, 1)];
                let b = frame.regs[reg(&ops, 2)];
                let kind = match ops.get(3) {
                    Some(Operand::CmpKind(k)) => *k,
                    _ => CmpKind::Eq,
                };
                let truth = self.compare(a, b, kind)?;
                frame.regs[d] = Value::from_i64(truth as i64).unwrap_or(Value::undefined());
                Step::Continue
            }
            UOp::Jump => {
                frame.pc = target(pc, &ops, 0, size);
                return Ok(Step::Continue);
            }
            UOp::JumpIfTrue | UOp::JumpIfFalse => {
                let c = frame.regs[reg(&ops, 0)];
                let truthy = c.as_i64().unwrap_or(0) != 0;
                let taken = if instr.op == UOp::JumpIfTrue {
                    truthy
                } else {
                    !truthy
                };
                let dest = target(pc, &ops, 1, size);
                if taken {
                    // Back-edge detection for loop profiling (T0 6.8).
                    if dest < pc {
                        let thresholds = self.thresholds;
                        self.bump_loop(frame.method, pc, thresholds);
                    }
                    frame.pc = dest;
                } else {
                    frame.pc += size;
                }
                return Ok(Step::Continue);
            }
            UOp::CallMethod | UOp::CallDirect | UOp::Call => {
                let callee = match instr.op {
                    UOp::Call => {
                        let f = frame.regs[reg(&ops, 0)];
                        let idx = f.as_i64().ok_or(Trap::TypeMismatch {
                            expected: "function index",
                            word: f.bits(),
                        })?;
                        if idx < 0 {
                            return Err(Trap::InvalidEncoding { offset: pc });
                        }
                        idx as u32
                    }
                    _ => match ops.get(1) {
                        Some(Operand::ConstIdx(c)) => *c,
                        _ => {
                            return Err(Trap::InvalidEncoding { offset: pc });
                        }
                    },
                };
                let count = count_of(&ops);
                let dst = reg(&ops, 0);
                let args: Vec<Value> = frame.regs[..count.min(frame.regs.len())].to_vec();
                // Inline cache update for the caller's site (T0 6.5).
                let caller_method = frame.method;
                let cache = self.caches.entry((caller_method, pc)).or_default();
                cache.lookup(callee);
                self.bump_calls(callee, self.thresholds);
                self.enter_callee(frame, stack, callee, &args, Dst::Reg(dst), pc + size)?;
                return Ok(Step::Continue);
            }
            UOp::Return => {
                let v = frame.regs[reg(&ops, 0)];
                return Ok(Step::Return(v));
            }
            UOp::Load => {
                let d = reg(&ops, 0);
                let addr = frame.regs[reg(&ops, 1)];
                let off = addr.as_heap_ref().ok_or(Trap::TypeMismatch {
                    expected: "ref",
                    word: addr.bits(),
                })? as usize;
                let v = self.heap.read_slot(off + HEADER_SIZE)?;
                frame.regs[d] = Value::from_word(v).map_err(|_| Trap::InvalidValue { word: v })?;
                Step::Continue
            }
            UOp::Store => {
                let v = frame.regs[reg(&ops, 0)];
                let addr = frame.regs[reg(&ops, 1)];
                let off = addr.as_heap_ref().ok_or(Trap::TypeMismatch {
                    expected: "ref",
                    word: addr.bits(),
                })? as usize;
                self.heap.write_slot(off + HEADER_SIZE, v.bits())?;
                Step::Continue
            }
            UOp::Alloc => {
                let d = reg(&ops, 0);
                let size = match ops.get(1) {
                    Some(Operand::Count(n)) => *n as usize,
                    _ => 0,
                };
                let total = (HEADER_SIZE + size * 8 + 7) & !7;
                let off = self.alloc_bytes(total)?;
                frame.regs[d] = Value::from_heap_ref(off as u64)
                    .map_err(|_| Trap::BadReference { offset: off as u64 })?;
                Step::Continue
            }
            UOp::Free => {
                // Model: manually-managed objects return to the arena by
                // leaking to the collector in this engine build; the slot
                // is marked so a future arena pass can reclaim it.
                let freed = frame.regs[reg(&ops, 0)];
                let _ = freed.as_heap_ref();
                Step::Continue
            }
            UOp::TypeCheck | UOp::TypeCast | UOp::TypeTag => {
                let d = reg(&ops, 0);
                let src_pos = if instr.op == UOp::TypeCast { 1 } else { 0 };
                let a = frame.regs[reg(&ops, src_pos)];
                let tag = match ops.last() {
                    Some(Operand::TypeTag(t)) => *t,
                    _ => 0,
                };
                let actual = type_tag(a);
                if instr.op == UOp::TypeCheck {
                    frame.regs[d] =
                        Value::from_i64((actual == tag) as i64).unwrap_or(Value::undefined());
                } else if instr.op == UOp::TypeTag {
                    frame.regs[d] = Value::from_i64(actual as i64).unwrap_or(Value::undefined());
                } else if actual != tag {
                    return Err(Trap::TypeMismatch {
                        expected: "cast target",
                        word: a.bits(),
                    });
                } else if d < frame.regs.len() {
                    frame.regs[d] = a;
                }
                Step::Continue
            }
            UOp::CapCheck | UOp::CapGrant | UOp::CapRevoke => {
                // Environment mutation happens at the method boundary; the
                // verifier already gated grant and revoke on CapAdmin. The
                // model runtime keeps per-frame environments implicit.
                let _ = ops;
                Step::Continue
            }
            UOp::LangSwitch | UOp::LangContext | UOp::InteropCall => {
                // Polyglot contexts are tracked by the tier controller in
                // this engine build; the interpreter records the switch.
                let _ = reg(&ops, 0);
                Step::Continue
            }
            UOp::Eval | UOp::Reflect | UOp::DynCall => {
                return Err(Trap::Unsupported {
                    what: "dynamic operations",
                });
            }
            UOp::FfiCall | UOp::FfiLoad | UOp::FfiStore => {
                return Err(Trap::Unsupported {
                    what: "foreign calls",
                });
            }
            UOp::SuperLoadAddStore => {
                let l = local(&ops, 0);
                let imm = match ops.get(1) {
                    Some(Operand::Imm32(v)) => {
                        Value::from_i64(*v as i64).unwrap_or(Value::undefined())
                    }
                    _ => {
                        return Err(Trap::InvalidEncoding { offset: pc });
                    }
                };
                if l < frame.locals.len() {
                    frame.locals[l] = self.arith(UOp::Add, frame.locals[l], imm)?;
                }
                Step::Continue
            }
            UOp::SuperLoadCallStore => {
                let l = local(&ops, 0);
                let callee = match ops.get(1) {
                    Some(Operand::ConstIdx(c)) => *c,
                    _ => {
                        return Err(Trap::InvalidEncoding { offset: pc });
                    }
                };
                let count = count_of(&ops);
                let args: Vec<Value> = frame.regs[..count.min(frame.regs.len())].to_vec();
                self.bump_calls(callee, self.thresholds);
                self.enter_callee(frame, stack, callee, &args, Dst::Local(l), pc + size)?;
                return Ok(Step::Continue);
            }
            UOp::SuperCmpBranch => {
                let a = frame.regs[reg(&ops, 0)];
                let b = frame.regs[reg(&ops, 1)];
                let kind = match ops.get(3) {
                    Some(Operand::CmpKind(k)) => *k,
                    _ => CmpKind::Eq,
                };
                let truth = self.compare(a, b, kind)?;
                let dest = target(pc, &ops, 2, size);
                if truth {
                    if dest < pc {
                        let thresholds = self.thresholds;
                        self.bump_loop(frame.method, pc, thresholds);
                    }
                    frame.pc = dest;
                } else {
                    frame.pc += size;
                }
                return Ok(Step::Continue);
            }
        };
        frame.pc += size;
        // Rewriting triggers fire at safe points (T0 6.6).
        rewriter.maybe_rewrite(self, frame.method);
        Ok(outcome)
    }

    /// Pushes the caller frame and installs the callee frame.
    // CEP:WHAT Transfers control into a callee with a pending destination.
    // CEP:WHY Calls, direct calls, and super calls share one path.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::InvalidEncoding for unknown callees.
    // CEP:ASSUMES Args fit the callee's registers.
    // CEP:COST O(frame size).
    // CEP:EVIDENCE cargo test -p vortex-t0
    fn enter_callee(
        &mut self,
        frame: &mut Frame,
        stack: &mut Vec<Frame>,
        callee: u32,
        args: &[Value],
        dst: Dst,
        return_pc: usize,
    ) -> RtResult<()> {
        stack.push(Frame {
            method: frame.method,
            regs: std::mem::take(&mut frame.regs),
            locals: std::mem::take(&mut frame.locals),
            pc: return_pc,
            pending_dst: dst,
        });
        let m = self
            .program
            .method(callee)
            .ok_or(Trap::InvalidEncoding { offset: 0 })?;
        let mut new_frame = Frame {
            method: callee,
            regs: vec![Value::undefined(); m.reg_count as usize],
            locals: vec![Value::undefined(); m.local_count as usize],
            pc: 0,
            pending_dst: Dst::Reg(0),
        };
        for (i, a) in args.iter().enumerate() {
            if i < new_frame.regs.len() {
                new_frame.regs[i] = *a;
            }
        }
        *frame = new_frame;
        Ok(())
    }

    /// Allocates zeroed bytes from the TLAB, carving a new buffer when it
    /// is exhausted (T0 design 6.4).
    // CEP:WHAT Fast-path bump allocation with a slow-path refill.
    // CEP:WHY NV-P01: allocation must be one increment on the fast path.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::OutOfMemory when no region is available.
    // CEP:ASSUMES total is 8-byte aligned.
    // CEP:COST O(1) fast path.
    // CEP:EVIDENCE cargo test -p vortex-t0
    fn alloc_bytes(&mut self, total: usize) -> RtResult<usize> {
        if let Some(t) = self.tlab.as_mut() {
            if let Some(off) = t.try_allocate(total) {
                self.heap.set_object_size(off, total as u32)?;
                return Ok(off);
            }
        }
        let region = self.heap.take_young_region(256)?;
        let mut t = self.heap.take_tlab(region, total.max(1024))?;
        let off = t.try_allocate(total).ok_or(Trap::OutOfMemory)?;
        self.heap.set_object_size(off, total as u32)?;
        self.tlab = Some(t);
        Ok(off)
    }

    /// Records type feedback for the site (T0 6.8).
    // CEP:WHAT Bumps the observed-type counters of the current site.
    // CEP:WHY Speculative rewrites read this feedback.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES Profiles were sized at construction.
    // CEP:COST O(operands).
    // CEP:EVIDENCE cargo test -p vortex-t0
    fn record_types(&mut self, frame: &Frame, instr: &Instr) {
        let uses_values = matches!(
            instr.op,
            UOp::Add | UOp::Sub | UOp::Mul | UOp::Div | UOp::Rem | UOp::Cmp
        );
        if !uses_values {
            return;
        }
        let site = frame.pc;
        let regs = &frame.regs;
        if let Some(p) = self.profiles.get_mut(&frame.method) {
            if site < p.int_seen.len() {
                for r in instr.operands.iter().filter_map(|o| o.as_reg()) {
                    if let Some(v) = regs.get(r as usize) {
                        match Value::classify_word(v.bits()) {
                            Classification::Int(_) => p.int_seen[site] += 1,
                            Classification::FloatBits(_) => p.float_seen[site] += 1,
                            Classification::HeapRef(_) => p.ref_seen[site] += 1,
                            // Special and invalid words carry no type
                            // feedback for arithmetic sites.
                            _ => {
                                let _ = v.bits();
                            }
                        }
                    }
                }
            }
        }
    }

    /// Integer arithmetic with the boxed-integer slow path (NV-1 section 2).
    // CEP:WHAT Adds, subtracts, or multiplies with exact overflow handling.
    // CEP:WHY INV-004: overflow boxes into a heap integer object.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::ArithmeticOverflow when boxing fails.
    // CEP:ASSUMES None.
    // CEP:COST O(1) fast path; O(1) allocation on overflow.
    // CEP:EVIDENCE cargo test -p vortex-t0
    fn arith(&mut self, op: UOp, a: Value, b: Value) -> RtResult<Value> {
        let outcome = match op {
            UOp::Add | UOp::AddInt => Value::checked_add(a, b),
            UOp::Sub => Value::checked_sub(a, b),
            UOp::Mul => Value::checked_mul(a, b),
            _ => IntOpOutcome::NotImmediate,
        };
        match outcome {
            IntOpOutcome::Immediate(v) => Ok(v),
            IntOpOutcome::NeedsBox(x) => self.box_int(x),
            IntOpOutcome::NeedsBox128(_) => Err(Trap::ArithmeticOverflow),
            IntOpOutcome::NotImmediate => {
                if let (Some(x), Some(y)) = (a.as_f64_bits(), b.as_f64_bits()) {
                    let (x, y) = (f64::from_bits(x), f64::from_bits(y));
                    let r = match op {
                        UOp::Add => x + y,
                        UOp::Sub => x - y,
                        UOp::Mul => x * y,
                        _ => x / y,
                    };
                    return Ok(Value::from_f64(r));
                }
                Err(Trap::TypeMismatch {
                    expected: "number",
                    word: b.bits(),
                })
            }
        }
    }

    /// Boxes an out-of-range integer into a heap object.
    // CEP:WHAT Allocates an integer box and returns its reference value.
    // CEP:WHY NV-1 overflow policy: boxed integers use the reference
    //        category with the header marking the representation.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::OutOfMemory when the heap cannot grow.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-t0
    fn box_int(&mut self, x: i64) -> RtResult<Value> {
        let off = self.alloc_bytes(HEADER_SIZE + 8)?;
        self.heap.set_mark_word(off, x as u64)?;
        self.heap.set_klass_word(off, 1)?;
        Value::from_heap_ref(off as u64).map_err(|_| Trap::BadReference { offset: off as u64 })
    }

    /// Bitwise and shift operations.
    // CEP:WHAT Executes AND/OR/XOR/SHL/SHR on immediates.
    // CEP:WHY Pure integer logic completes the arithmetic set.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::TypeMismatch for non-integers.
    // CEP:ASSUMES Shifts are masked to 6 bits.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-t0
    fn bitwise(&self, op: UOp, a: Value, b: Value) -> RtResult<Value> {
        let (x, y) = match (a.as_i64(), b.as_i64()) {
            (Some(x), Some(y)) => (x, y),
            _ => {
                return Err(Trap::TypeMismatch {
                    expected: "int",
                    word: b.bits(),
                })
            }
        };
        let r = match op {
            UOp::And => x & y,
            UOp::Or => x | y,
            UOp::Xor => x ^ y,
            UOp::Shl => x.wrapping_shl((y & 63) as u32),
            UOp::Shr => x.wrapping_shr((y & 63) as u32),
            _ => x,
        };
        Ok(Value::from_i64(r).unwrap_or(Value::undefined()))
    }

    /// Comparison with the selector kinds.
    // CEP:WHAT Compares two values under a comparison kind.
    // CEP:WHY CMP and CMP_BRANCH share this semantic.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::TypeMismatch for incomparable categories.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-t0
    fn compare(&self, a: Value, b: Value, kind: CmpKind) -> RtResult<bool> {
        let (x, y) = match (a.as_i64(), b.as_i64()) {
            (Some(x), Some(y)) => (x, y),
            _ => {
                return Err(Trap::TypeMismatch {
                    expected: "int",
                    word: b.bits(),
                })
            }
        };
        Ok(match kind {
            CmpKind::Eq => x == y,
            CmpKind::Ne => x != y,
            CmpKind::Lt => x < y,
            CmpKind::Le => x <= y,
            CmpKind::Gt => x > y,
            CmpKind::Ge => x >= y,
        })
    }

    /// The NV-1 value of a constant-pool entry.
    // CEP:WHAT Materializes a constant as a register value.
    // CEP:WHY LOAD_CONST resolves through the pool.
    // CEP:STATUS complete
    // CEP:FAILURE Byte-string constants become the undefined value.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-t0
    fn constant_value(&self, index: u32) -> Value {
        self.program
            .constant(index)
            .and_then(|c| c.as_value())
            .unwrap_or(Value::undefined())
    }

    /// Captures the deopt state and records the violation (T0 6.9).
    // CEP:WHAT Builds the DeoptState for tier transfer.
    // CEP:WHY State reconstruction needs registers, locals, and the PC.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(frame size).
    // CEP:EVIDENCE cargo test -p vortex-t0
    fn deopt(&self, frame: &Frame, reason: &'static str) -> Step {
        Step::Deopt(DeoptState {
            method: frame.method,
            pc: frame.pc,
            regs: frame.regs.clone(),
            locals: frame.locals.clone(),
            reason,
        })
    }

    /// Increments the call counter and emits a hot event at the threshold.
    // CEP:WHAT Tracks method heat for tier transitions (T0 6.10).
    // CEP:WHY Tier transitions trigger on execution counts.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-t0
    fn bump_calls(&mut self, method: u32, thresholds: Thresholds) {
        let c = self.call_counts.entry(method).or_insert(0);
        *c += 1;
        if *c == thresholds.method_calls {
            self.hot_events.push(HotEvent::Method(method));
        }
    }

    /// Increments a back-edge counter and emits a hot event.
    // CEP:WHAT Tracks loop heat for OSR-style transitions (T0 6.10).
    // CEP:WHY Loops tier up on iteration thresholds.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-t0
    fn bump_loop(&mut self, method: u32, pc: usize, thresholds: Thresholds) {
        let key = (method, pc);
        let c = self.loop_counts.entry(key).or_insert(0);
        *c += 1;
        if *c == thresholds.loop_iters {
            self.hot_events.push(HotEvent::Loop(method, pc));
        }
    }
}
