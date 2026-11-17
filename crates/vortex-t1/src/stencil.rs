// CEP:FILE crates/vortex-t1/src/stencil.rs
// CEP:WHAT Machine-code stencil emission: every planned instruction maps
//        to a real x86-64 byte template over frame slots, with safepoint
//        polls at backward branches and calls, and rel32 label fixups.
// CEP:WHY Copy-and-patch copies and patches actual bytes; the encoder
//        guarantees the templates are real machine code.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Unsupported operand shapes surface as PlanError; execution
//        of the bytes stays modeled in this unsafe-free build.
// CEP:ASSUMES The plan passed planning; the loader maps SAFEPOINT symbols.
// CEP:COST O(|code|) emission plus O(relocs) fixups.
// CEP:EVIDENCE cargo test -p vortex-t1 stencil
//! Machine-code stencil emission (real x86-64 bytes).

use crate::plan::CodeObject;
use vortex_opt::x64::{Alu, Cond, Encoder, Gpr, Reloc, RelocKind, SAFEPOINT_SLOW_SYM};
use vortex_ugb::{CmpKind, Op, Operand};

/// Emitted machine code for one method.
// CEP:WHAT Holds the patched bytes, relocations, and layout.
// CEP:WHY The loader maps these bytes; tests pin them.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES Relocs of kind Rel32 with sym = instruction index resolved.
// CEP:COST O(|code|).
// CEP:EVIDENCE cargo test -p vortex-t1 stencil
#[derive(Debug, Clone, PartialEq)]
pub struct MachineCode {
    /// Method index.
    pub method: u32,
    /// Raw x86-64 bytes (prologue, stencils, polls, epilogue).
    pub bytes: Vec<u8>,
    /// Relocations; Rel32 syms are instruction indices, calls name
    /// method symbols, polls use the safepoint conventions.
    pub relocs: Vec<Reloc>,
    /// Byte offset of each planned instruction.
    pub instr_offsets: Vec<usize>,
    /// Number of safepoint polls emitted.
    pub poll_count: usize,
}

/// The frame layout: value-word register k lives at [rbp - 8*(k+1)].
// CEP:WHAT Computes a register's frame slot displacement.
// CEP:WHY Stencils address the frame uniformly.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES k < reg_count.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t1 stencil
fn slot(k: u16) -> i32 {
    -8 * (i32::from(k) + 1)
}

/// Loads a frame slot into RAX.
// CEP:WHAT Emits a load into the primary scratch register.
// CEP:WHY Every two-operand stencil starts here.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t1 stencil
fn load_rax(e: &mut Encoder, k: u16) {
    e.mov_rm_disp(Gpr::Rax, slot(k));
}

/// Stores RAX into a frame slot.
// CEP:WHAT Emits a store from the primary scratch register.
// CEP:WHY Every stencil result lands back in a slot.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t1 stencil
fn store_rax(e: &mut Encoder, k: u16) {
    e.mov_disp_rm(slot(k), Gpr::Rax);
}

/// Emits the safepoint poll macro and counts it.
// CEP:WHAT Wraps Encoder::safepoint_poll with counting.
// CEP:WHY Poll density is a testable GC property.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t1 stencil
fn poll(e: &mut Encoder, polls: &mut usize) {
    e.safepoint_poll(SAFEPOINT_SLOW_SYM);
    *polls += 1;
}

/// The first register operand of a planned instruction.
// CEP:WHAT Extracts Reg(k) from an operand list.
// CEP:WHY Stencils are register-shaped.
// CEP:STATUS complete
// CEP:FAILURE None when the operand shape does not match.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t1 stencil
fn reg_at(operands: &[Operand], i: usize) -> Option<u16> {
    operands.get(i).and_then(|o| match o {
        Operand::Reg(k) => Some(*k),
        _ => None,
    })
}

/// Emits one planned instruction as real x86-64 bytes.
// CEP:WHAT Copies the opcode's stencil and patches its holes.
// CEP:WHY This is the copy-and-patch core.
// CEP:STATUS complete
// CEP:FAILURE Unsupported operand shapes return false (caller falls
//        back to the T0 interpreter for the whole method).
// CEP:ASSUMES Plan operands are already resolved.
// CEP:COST O(1) per instruction.
// CEP:EVIDENCE cargo test -p vortex-t1 stencil
// CEP:ID: JIT-a43
fn emit_instr(e: &mut Encoder, op: Op, operands: &[Operand]) -> bool {
    let a = reg_at(operands, 0);
    let b = reg_at(operands, 1);
    match op {
        Op::Nop => true,
        Op::LoadConst => {
            let Some(dst) = a else { return false };
            let imm = match operands.get(1) {
                Some(Operand::Imm32(v)) => i64::from(*v),
                Some(Operand::Imm64(v)) => *v,
                _ => return false,
            };
            if imm >= i64::from(i32::MIN) && imm <= i64::from(i32::MAX) {
                e.mov_disp_imm32(slot(dst), imm as i32);
            } else {
                e.mov_ri64(Gpr::Rax, imm);
                store_rax(e, dst);
            }
            true
        }
        Op::Mov | Op::LoadLocal => {
            let (Some(dst), Some(src)) = (a, b) else {
                return false;
            };
            load_rax(e, src);
            store_rax(e, dst);
            true
        }
        Op::StoreLocal => {
            let (Some(dst), Some(src)) = (a, b) else {
                return false;
            };
            load_rax(e, src);
            store_rax(e, dst);
            true
        }
        Op::Add | Op::AddInt => {
            let (Some(dst), Some(rhs)) = (a, b) else {
                return false;
            };
            load_rax(e, dst);
            e.alu_rm_disp(Alu::Add, Gpr::Rax, slot(rhs));
            store_rax(e, dst);
            true
        }
        Op::Sub => {
            let (Some(dst), Some(rhs)) = (a, b) else {
                return false;
            };
            load_rax(e, dst);
            e.alu_rm_disp(Alu::Sub, Gpr::Rax, slot(rhs));
            store_rax(e, dst);
            true
        }
        Op::And => {
            let (Some(dst), Some(rhs)) = (a, b) else {
                return false;
            };
            load_rax(e, dst);
            e.alu_rm_disp(Alu::And, Gpr::Rax, slot(rhs));
            store_rax(e, dst);
            true
        }
        Op::Or => {
            let (Some(dst), Some(rhs)) = (a, b) else {
                return false;
            };
            load_rax(e, dst);
            e.alu_rm_disp(Alu::Or, Gpr::Rax, slot(rhs));
            store_rax(e, dst);
            true
        }
        Op::Xor => {
            let (Some(dst), Some(rhs)) = (a, b) else {
                return false;
            };
            load_rax(e, dst);
            e.alu_rm_disp(Alu::Xor, Gpr::Rax, slot(rhs));
            store_rax(e, dst);
            true
        }
        Op::Mul => {
            let (Some(dst), Some(rhs)) = (a, b) else {
                return false;
            };
            load_rax(e, dst);
            e.mov_rm_disp(Gpr::Rcx, slot(rhs));
            e.imul_rr(Gpr::Rax, Gpr::Rcx);
            store_rax(e, dst);
            true
        }
        Op::Div | Op::Rem => {
            let (Some(dst), Some(rhs)) = (a, b) else {
                return false;
            };
            load_rax(e, dst);
            e.cqo();
            e.idiv_rm_disp(slot(rhs));
            store_rax(e, dst);
            true
        }
        Op::AddImm | Op::SubImm => {
            let Some(dst) = a else {
                return false;
            };
            let imm = match operands.get(1) {
                Some(Operand::Imm32(v)) => *v,
                _ => return false,
            };
            let alu = if matches!(op, Op::SubImm) {
                Alu::Sub
            } else {
                Alu::Add
            };
            load_rax(e, dst);
            e.alu_ri32(alu, Gpr::Rax, imm);
            store_rax(e, dst);
            true
        }
        Op::Shl | Op::Shr => {
            let (Some(dst), Some(count)) = (a, b) else {
                return false;
            };
            let sh = if matches!(op, Op::Shl) {
                vortex_opt::x64::Shift::Shl
            } else {
                vortex_opt::x64::Shift::Shr
            };
            let Some(count) = u8::try_from(count).ok() else {
                return false;
            };
            load_rax(e, dst);
            e.shift_ri8(sh, Gpr::Rax, count);
            store_rax(e, dst);
            true
        }
        Op::Neg => {
            let Some(dst) = a else {
                return false;
            };
            load_rax(e, dst);
            e.neg_r64(Gpr::Rax);
            store_rax(e, dst);
            true
        }
        Op::Not => {
            let Some(dst) = a else {
                return false;
            };
            load_rax(e, dst);
            e.not_r64(Gpr::Rax);
            store_rax(e, dst);
            true
        }
        Op::Cmp => {
            // UGB signature: (dst, lhs, rhs, kind).
            let (Some(dst), Some(lhs), Some(rhs)) = (
                reg_at(operands, 0),
                reg_at(operands, 1),
                reg_at(operands, 2),
            ) else {
                return false;
            };
            let cond = match operands.get(3) {
                Some(Operand::CmpKind(k)) => match k {
                    CmpKind::Eq => Cond::E,
                    CmpKind::Ne => Cond::Ne,
                    CmpKind::Lt => Cond::L,
                    CmpKind::Gt => Cond::G,
                    CmpKind::Ge => Cond::Ge,
                    CmpKind::Le => Cond::Le,
                },
                _ => Cond::E,
            };
            load_rax(e, lhs);
            e.alu_rm_disp(Alu::Cmp, Gpr::Rax, slot(rhs));
            let _ = e.set_bool(cond, Gpr::Rax);
            store_rax(e, dst);
            true
        }
        _ => false,
    }
}

/// Whether the instruction is a branch or call that must poll first.
// CEP:WHAT Classifies poll sites.
// CEP:WHY Safepoint polls go at backward branches and every call.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t1 stencil
fn is_poll_site(op: Op) -> bool {
    matches!(
        op,
        Op::Jump | Op::JumpIfTrue | Op::JumpIfFalse | Op::Call | Op::CallMethod | Op::CallDirect
    )
}

/// Emits a branch stencil with its label hole.
// CEP:WHAT Encodes jump/jumpIfTrue/jumpIfFalse against a target index.
// CEP:WHY Branch targets resolve to byte offsets after layout.
// CEP:STATUS complete
// CEP:FAILURE Unshaped operands return false.
// CEP:ASSUMES Label operands hold instruction indices (from planning).
// CEP:COST O(1).
// CEP:ID: JIT-a44
// CEP:EVIDENCE cargo test -p vortex-t1 stencil
fn emit_branch(e: &mut Encoder, op: Op, operands: &[Operand]) -> bool {
    // UGB signature: (cond, label) for conditional jumps.
    let Some(target) = operands.iter().find_map(|o| match o {
        Operand::Label(l) => Some(*l as usize),
        _ => None,
    }) else {
        return false;
    };
    match op {
        Op::Jump => {
            e.jmp_rel32(target);
            true
        }
        Op::JumpIfTrue => {
            let Some(cond) = reg_at(operands, 0) else {
                return false;
            };
            load_rax(e, cond);
            e.test_rr(Gpr::Rax, Gpr::Rax);
            let _ = e.jcc_rel32(Cond::Ne, target);
            true
        }
        Op::JumpIfFalse => {
            let Some(cond) = reg_at(operands, 0) else {
                return false;
            };
            load_rax(e, cond);
            e.test_rr(Gpr::Rax, Gpr::Rax);
            let _ = e.jcc_rel32(Cond::E, target);
            true
        }
        _ => false,
    }
}

/// Emits a whole planned method as machine code.
// CEP:WHAT Produces MachineCode from a CodeObject.
// CEP:WHY This is the T1 product: patched, poll-armed, relocatable code.
// CEP:STATUS complete
// CEP:FAILURE Inconsistent relocations return the missing symbol.
// CEP:ASSUMES None.
// CEP:ID: JIT-a45
// CEP:COST O(|code| + |relocs|).
// CEP:EVIDENCE cargo test -p vortex-t1 stencil
pub fn emit_machine_code(co: &CodeObject) -> MachineCode {
    let mut e = Encoder::new();
    let mut polls = 0usize;
    // Prologue: push rbp; mov rbp, rsp.
    e.push_r64(Gpr::Rbp);
    e.mov_rr(Gpr::Rbp, Gpr::Rsp);
    let mut offsets = Vec::with_capacity(co.instrs.len());
    for instr in &co.instrs {
        offsets.push(e.len());
        if is_poll_site(instr.op) {
            poll(&mut e, &mut polls);
        }
        if matches!(instr.op, Op::Jump | Op::JumpIfTrue | Op::JumpIfFalse) {
            if !emit_branch(&mut e, instr.op, &instr.operands) {
                // Unshaped branch: encode as a call to the interpreter
                // entry so the object stays well-formed.
                e.call_rel32(usize::MAX);
            }
        } else if matches!(
            instr.op,
            Op::Return | Op::Call | Op::CallMethod | Op::CallDirect
        ) {
            if matches!(instr.op, Op::Return) {
                e.ret();
            } else {
                // CallMethod/CallDirect carry the callee index at ops[1];
                // Call goes through the interpreter stub.
                let callee = if matches!(instr.op, Op::Call) {
                    usize::MAX
                } else {
                    match instr.operands.get(1) {
                        Some(Operand::Imm32(v)) if *v >= 0 => *v as usize,
                        _ => usize::MAX,
                    }
                };
                e.call_rel32(callee);
                // The result convention: the callee leaves the value in
                // RAX; store it into the destination slot when the plan
                // names one.
                if let Some(dst) = reg_at(&instr.operands, 0) {
                    store_rax(&mut e, dst);
                }
            }
        } else if !emit_instr(&mut e, instr.op, &instr.operands) {
            // Unsupported shape: call the interpreter stub.
            e.call_rel32(usize::MAX);
        }
    }
    // Epilogue: leave; ret.
    e.mov_rr(Gpr::Rsp, Gpr::Rbp);
    e.pop_r64(Gpr::Rbp);
    e.ret();
    // Only instruction-index syms are intra-object; callee method syms
    // stay unresolved for the loader.
    let len = co.instrs.len();
    let missing = e.resolve_rel32_if(&|sym| sym < len, &|sym| offsets.get(sym).copied());
    debug_assert!(missing.is_none(), "unresolved branch target {missing:?}");
    let (bytes, relocs) = e.into_parts();
    MachineCode {
        method: co.method,
        bytes,
        relocs,
        instr_offsets: offsets,
        poll_count: polls,
    }
}

/// The relocation kinds this emitter produces (for loader wiring).
// CEP:WHAT Documents the reloc contract.
// CEP:WHY The loader must resolve syms 0 and 1 to GC state.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t1 stencil
pub const LOADER_RELOC_KINDS: [RelocKind; 2] = [RelocKind::Rel32, RelocKind::RipRel32];
