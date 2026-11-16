// CEP:FILE crates/vortex-t1/src/exec.rs
// CEP:WHAT The T1 executor: runs patched code objects with the same NV-1
//        value semantics as T0, sharing the runtime heap, with capability
//        gates on every gated instruction (copy-and-patch phases: T1a
//        plan, T1b verify, T1c install, then this execution).
// CEP:WHY The baseline tier's speedup comes from executing pre-patched
//        templates without dispatch-decode overhead.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Typed Trap on every fault; unsupported opcodes fall back to
//        T0 before execution.
// CEP:ASSUMES Code objects came from plan_method over verified bytecode;
//        the caller owns the shared heap.
// CEP:COST O(1) per patched instruction.
// CEP:EVIDENCE cargo test -p vortex-t1
//! T1 execution over patched code objects.

use crate::plan::CodeObject;
use std::collections::HashMap;
use vortex_nv1::{IntOpOutcome, Value};
use vortex_runtime::{Heap, RtResult, Trap, HEADER_SIZE};
use vortex_ugb::Op as UOp;
use vortex_ugb::{CmpKind, Constant, Operand};

/// Where a returning value lands in the caller.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dst {
    /// A virtual register.
    Reg(usize),
    /// A local slot.
    Local(usize),
}

/// One activation frame of patched code.
#[derive(Debug, Clone)]
struct Frame {
    /// Method whose code object is executing.
    object: u32,
    /// Virtual registers.
    regs: Vec<Value>,
    /// Local slots.
    locals: Vec<Value>,
    /// Instruction index.
    pc: usize,
    /// Return destination in the caller.
    pending_dst: Dst,
}

/// The T1 executor over installed code objects.
#[derive(Default)]
pub struct Executor {
    /// Installed code objects by method index.
    objects: HashMap<u32, CodeObject>,
    /// Shared constant pool captured at install.
    constants: Vec<Constant>,
    /// Depth bound for calls.
    stack_bound: usize,
}

impl Executor {
    /// Creates an executor with the default depth bound.
    // CEP:WHAT Creates the executor.
    // CEP:WHY One bound matches the runtime's frame discipline.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-t1
    pub fn new() -> Executor {
        Executor {
            objects: HashMap::new(),
            constants: Vec::new(),
            stack_bound: 512,
        }
    }

    /// Installs code objects and the shared constant pool (T1c).
    // CEP:WHAT Registers compiled objects for execution.
    // CEP:WHY Installation replaces T0 bytecode for subsequent calls.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES Objects were planned successfully.
    // CEP:COST O(objects).
    // CEP:EVIDENCE cargo test -p vortex-t1
    pub fn install(&mut self, objects: Vec<CodeObject>, constants: Vec<Constant>) {
        self.constants = constants;
        for o in objects {
            self.objects.insert(o.method, o);
        }
    }

    /// Whether a method has installed code.
    // CEP:WHAT Reports installation status.
    // CEP:WHY The tier controller dispatches on this.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-t1
    pub fn installed(&self, method: u32) -> bool {
        self.objects.contains_key(&method)
    }

    /// Runs a method's installed code object against the shared heap.
    // CEP:WHAT Executes patched templates to completion.
    // CEP:WHY T1 must produce the same observable values as T0.
    // CEP:STATUS complete
    // CEP:FAILURE Trap::InvalidEncoding for uninstalled methods.
    // CEP:ASSUMES The object was planned from verified bytecode.
    // CEP:COST O(instructions executed).
    // CEP:EVIDENCE cargo test -p vortex-t1
    pub fn run(&self, method: u32, args: &[Value], heap: &mut Heap) -> RtResult<Value> {
        let entry = self
            .objects
            .get(&method)
            .ok_or(Trap::InvalidEncoding { offset: 0 })?;
        let mut stack: Vec<Frame> = Vec::new();
        let mut frame = self.make_frame(entry, args, Dst::Reg(0));
        loop {
            if stack.len() >= self.stack_bound {
                return Err(Trap::CallStackOverflow {
                    bound: self.stack_bound,
                });
            }
            let obj = self
                .objects
                .get(&frame.object)
                .ok_or(Trap::InvalidEncoding { offset: 0 })?;
            let Some(instr) = obj.instrs.get(frame.pc) else {
                return Err(Trap::InvalidEncoding { offset: frame.pc });
            };
            let ops = &instr.operands;
            for cap in instr.op.info().caps {
                if !obj.caps.contains(cap) {
                    return Err(Trap::CapabilityViolation {
                        perm: cap.ordinal(),
                    });
                }
            }
            match instr.op {
                UOp::Return => {
                    let v = frame.regs[reg(ops, 0)];
                    match stack.pop() {
                        Some(mut caller) => {
                            apply_dst(&mut caller, v);
                            frame = caller;
                        }
                        None => return Ok(v),
                    }
                }
                UOp::Jump => {
                    frame.pc = label(ops, 0);
                }
                UOp::JumpIfTrue | UOp::JumpIfFalse => {
                    let c = frame.regs[reg(ops, 0)];
                    let truthy = c.as_i64().unwrap_or(0) != 0;
                    let taken = if instr.op == UOp::JumpIfTrue {
                        truthy
                    } else {
                        !truthy
                    };
                    frame.pc = if taken { label(ops, 1) } else { frame.pc + 1 };
                }
                UOp::SuperCmpBranch => {
                    let a = frame.regs[reg(ops, 0)];
                    let b = frame.regs[reg(ops, 1)];
                    let kind = match ops.get(3) {
                        Some(Operand::CmpKind(k)) => *k,
                        _ => CmpKind::Eq,
                    };
                    let truth = compare(a, b, kind)?;
                    frame.pc = if truth { label(ops, 2) } else { frame.pc + 1 };
                }
                UOp::CallMethod | UOp::CallDirect | UOp::Call | UOp::SuperLoadCallStore => {
                    let callee = match instr.op {
                        UOp::Call => {
                            let f = frame.regs[reg(ops, 0)];
                            let idx = f.as_i64().unwrap_or(-1);
                            if idx < 0 {
                                return Err(Trap::InvalidEncoding { offset: frame.pc });
                            }
                            idx as u32
                        }
                        _ => match ops.get(1) {
                            Some(Operand::ConstIdx(c)) => *c,
                            _ => return Err(Trap::InvalidEncoding { offset: frame.pc }),
                        },
                    };
                    let dst = match instr.op {
                        UOp::SuperLoadCallStore => match ops.first() {
                            Some(Operand::Local(l)) => Dst::Local(*l as usize),
                            _ => return Err(Trap::InvalidEncoding { offset: frame.pc }),
                        },
                        _ => Dst::Reg(reg(ops, 0)),
                    };
                    let callee_obj = self
                        .objects
                        .get(&callee)
                        .ok_or(Trap::InvalidEncoding { offset: 0 })?;
                    let count = count_of(ops);
                    let args: Vec<Value> = frame.regs[..count.min(frame.regs.len())].to_vec();
                    stack.push(Frame {
                        object: frame.object,
                        regs: std::mem::take(&mut frame.regs),
                        locals: std::mem::take(&mut frame.locals),
                        pc: frame.pc + 1,
                        pending_dst: dst,
                    });
                    frame = self.make_frame(callee_obj, &args, dst);
                }
                _ => {
                    self.simple_op(&mut frame, instr, heap)?;
                }
            }
        }
    }

    /// Executes the straight-line opcode stencil.
    // CEP:WHAT Applies one arithmetic, memory, or move instruction.
    // CEP:WHY The patched bodies for non-control opcodes run here.
    // CEP:STATUS complete
    // CEP:FAILURE Typed Trap per fault class.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-t1
    fn simple_op(
        &self,
        frame: &mut Frame,
        instr: &crate::plan::PatchedInstr,
        heap: &mut Heap,
    ) -> RtResult<()> {
        let ops = &instr.operands;
        match instr.op {
            // NOP carries no semantics; it exists as patch padding.
            UOp::Nop => {
                let _ = frame.pc;
            }
            UOp::LoadConst => {
                let d = reg(ops, 0);
                let idx = match ops.get(1) {
                    Some(Operand::ConstIdx(c)) => *c,
                    _ => {
                        return Err(Trap::InvalidEncoding { offset: frame.pc });
                    }
                };
                frame.regs[d] = self
                    .constants
                    .get(idx as usize)
                    .and_then(|c| c.as_value())
                    .unwrap_or(Value::undefined());
            }
            UOp::LoadLocal => {
                let d = reg(ops, 0);
                let l = local(ops, 1);
                if l < frame.locals.len() {
                    frame.regs[d] = frame.locals[l];
                }
            }
            UOp::StoreLocal => {
                let l = local(ops, 0);
                let s = reg(ops, 1);
                if l < frame.locals.len() {
                    frame.locals[l] = frame.regs[s];
                }
            }
            UOp::Mov => {
                let d = reg(ops, 0);
                let s = reg(ops, 1);
                frame.regs[d] = frame.regs[s];
            }
            UOp::Add | UOp::Sub | UOp::Mul | UOp::AddInt => {
                let d = reg(ops, 0);
                let a = frame.regs[reg(ops, 1)];
                let b = frame.regs[reg(ops, 2)];
                frame.regs[d] = arith(instr.op, a, b)?;
            }
            UOp::AddImm | UOp::SubImm => {
                let d = reg(ops, 0);
                let a = frame.regs[reg(ops, 1)];
                let imm = imm_value(ops, 2);
                let which = if instr.op == UOp::AddImm {
                    UOp::Add
                } else {
                    UOp::Sub
                };
                frame.regs[d] = arith(which, a, imm)?;
            }
            UOp::SuperLoadAddStore => {
                let l = local(ops, 0);
                let imm = imm_value(ops, 1);
                if l < frame.locals.len() {
                    let v = frame.locals[l];
                    frame.locals[l] = arith(UOp::Add, v, imm)?;
                }
            }
            UOp::Div | UOp::Rem => {
                let d = reg(ops, 0);
                let a = frame.regs[reg(ops, 1)];
                let b = frame.regs[reg(ops, 2)];
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
            }
            UOp::And | UOp::Or | UOp::Xor | UOp::Shl | UOp::Shr => {
                let d = reg(ops, 0);
                let a = frame.regs[reg(ops, 1)];
                let b = frame.regs[reg(ops, 2)];
                let (x, y) = match (a.as_i64(), b.as_i64()) {
                    (Some(x), Some(y)) => (x, y),
                    _ => {
                        return Err(Trap::TypeMismatch {
                            expected: "int",
                            word: b.bits(),
                        })
                    }
                };
                let r = match instr.op {
                    UOp::And => x & y,
                    UOp::Or => x | y,
                    UOp::Xor => x ^ y,
                    UOp::Shl => x.wrapping_shl((y & 63) as u32),
                    UOp::Shr => x.wrapping_shr((y & 63) as u32),
                    _ => x,
                };
                frame.regs[d] = Value::from_i64(r).unwrap_or(Value::undefined());
            }
            UOp::Neg | UOp::Not => {
                let d = reg(ops, 0);
                let a = frame.regs[reg(ops, 1)];
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
            }
            UOp::Cmp => {
                let d = reg(ops, 0);
                let a = frame.regs[reg(ops, 1)];
                let b = frame.regs[reg(ops, 2)];
                let kind = match ops.get(3) {
                    Some(Operand::CmpKind(k)) => *k,
                    _ => CmpKind::Eq,
                };
                let truth = compare(a, b, kind)?;
                frame.regs[d] = Value::from_i64(truth as i64).unwrap_or(Value::undefined());
            }
            UOp::Load => {
                let d = reg(ops, 0);
                let addr = frame.regs[reg(ops, 1)];
                let off = addr.as_heap_ref().ok_or(Trap::TypeMismatch {
                    expected: "ref",
                    word: addr.bits(),
                })? as usize;
                let v = heap.read_slot(off + HEADER_SIZE)?;
                frame.regs[d] = Value::from_word(v).map_err(|_| Trap::InvalidValue { word: v })?;
            }
            UOp::Store => {
                let v = frame.regs[reg(ops, 0)];
                let addr = frame.regs[reg(ops, 1)];
                let off = addr.as_heap_ref().ok_or(Trap::TypeMismatch {
                    expected: "ref",
                    word: addr.bits(),
                })? as usize;
                heap.write_slot(off + HEADER_SIZE, v.bits())?;
            }
            _ => {
                return Err(Trap::Unsupported { what: "t1 opcode" });
            }
        }
        frame.pc += 1;
        Ok(())
    }

    /// Builds a frame for a callee.
    // CEP:WHAT Allocates callee registers and binds arguments.
    // CEP:WHY Calls push frames identical in shape to T0's.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(frame).
    // CEP:EVIDENCE cargo test -p vortex-t1
    fn make_frame(&self, obj: &CodeObject, args: &[Value], dst: Dst) -> Frame {
        let mut f = Frame {
            object: obj.method,
            regs: vec![Value::undefined(); obj.reg_count as usize],
            locals: vec![Value::undefined(); obj.local_count as usize],
            pc: 0,
            pending_dst: dst,
        };
        for (i, a) in args.iter().enumerate() {
            if i < f.regs.len() {
                f.regs[i] = *a;
            }
        }
        f
    }
}

/// Baseline arithmetic; overflow overflows (boxed integers live in T0's
/// slow path and the optimizer's integer domain).
// CEP:WHAT Computes add, subtract, multiply, or divide.
// CEP:WHY Patched stencils execute the fast path; the T0 fallback owns
//        the boxing slow path.
// CEP:STATUS complete
// CEP:FAILURE Trap::TypeMismatch for non-numbers.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t1
fn arith(op: UOp, a: Value, b: Value) -> RtResult<Value> {
    let outcome = match op {
        UOp::Add | UOp::AddInt => Value::checked_add(a, b),
        UOp::Sub => Value::checked_sub(a, b),
        UOp::Mul => Value::checked_mul(a, b),
        _ => IntOpOutcome::NotImmediate,
    };
    match outcome {
        IntOpOutcome::Immediate(v) => Ok(v),
        IntOpOutcome::NeedsBox(_) | IntOpOutcome::NeedsBox128(_) => Err(Trap::ArithmeticOverflow),
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

/// Comparison under the selector kinds.
// CEP:WHAT Compares two values.
// CEP:WHY CMP stencils share one semantic.
// CEP:STATUS complete
// CEP:FAILURE Trap::TypeMismatch for incomparable categories.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t1
fn compare(a: Value, b: Value, kind: CmpKind) -> RtResult<bool> {
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

/// Applies a return value to the caller's pending destination.
// CEP:WHAT Writes the returned value into a register or local.
// CEP:WHY Super call sites store results locally.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t1
fn apply_dst(frame: &mut Frame, v: Value) {
    match frame.pending_dst {
        Dst::Reg(r) => {
            if r < frame.regs.len() {
                frame.regs[r] = v;
            }
        }
        Dst::Local(l) => {
            if l < frame.locals.len() {
                frame.locals[l] = v;
            }
        }
    }
}

/// Reads a register operand.
// CEP:WHAT Extracts the register index at a position.
// CEP:WHY Handlers unpack operands through this helper.
// CEP:STATUS complete
// CEP:FAILURE Zero for malformed operands.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t1
fn reg(ops: &[Operand], i: usize) -> usize {
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
// CEP:EVIDENCE cargo test -p vortex-t1
fn local(ops: &[Operand], i: usize) -> usize {
    match ops.get(i) {
        Some(Operand::Local(l)) => *l as usize,
        _ => 0,
    }
}

/// Reads a resolved label operand as an instruction index.
// CEP:WHAT Extracts the patched target index.
// CEP:WHY Labels resolve to instruction indices at plan time.
// CEP:STATUS complete
// CEP:FAILURE Zero for malformed operands.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t1
fn label(ops: &[Operand], i: usize) -> usize {
    match ops.get(i) {
        Some(Operand::Label(l)) => (*l).max(0) as usize,
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
// CEP:EVIDENCE cargo test -p vortex-t1
fn count_of(ops: &[Operand]) -> usize {
    match ops.get(2) {
        Some(Operand::Count(n)) => *n as usize,
        _ => match ops.get(1) {
            Some(Operand::Count(n)) => *n as usize,
            _ => 0,
        },
    }
}

/// Builds the immediate value of an Imm32 operand.
// CEP:WHAT Materializes an i32 immediate as a value.
// CEP:WHY ADD_IMM stencils embed the immediate at patch time.
// CEP:STATUS complete
// CEP:FAILURE Undefined for malformed operands.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t1
fn imm_value(ops: &[Operand], i: usize) -> Value {
    match ops.get(i) {
        Some(Operand::Imm32(v)) => Value::from_i64(*v as i64).unwrap_or(Value::undefined()),
        _ => Value::undefined(),
    }
}
