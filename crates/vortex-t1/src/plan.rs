// CEP:FILE crates/vortex-t1/src/plan.rs
// CEP:WHAT Stencil selection and patch planning: every UGB opcode maps to
//        a stencil template with typed patch points (registers,
//        immediates, branch targets) filled at compile time (T1
//        copy-and-patch; old design T1a).
// CEP:WHY Copy-and-patch front-loads all decoding decisions into template
//        selection so execution never re-decodes wire bytes.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Unsupported opcodes surface as typed PlanError entries and
//        drive the T0 fallback path.
// CEP:ASSUMES The program passed the UGB verifier.
// CEP:COST O(|code|) per method.
// CEP:EVIDENCE cargo test -p vortex-t1
//! Stencil planning (copy-and-patch phase T1a).

use vortex_ugb::{Instr, Operand, Program};

/// Patch planning failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// The opcode has no stencil in this build.
    UnsupportedOpcode {
        /// Method index.
        method: u32,
        /// Byte offset of the instruction.
        offset: usize,
        /// The opcode name.
        opcode: &'static str,
    },
    /// An operand did not fit its patch slot.
    BadPatch {
        /// Method index.
        method: u32,
        /// Byte offset.
        offset: usize,
    },
}

/// One stencil instruction: opcode with pre-resolved operands.
#[derive(Debug, Clone, PartialEq)]
pub struct PatchedInstr {
    /// The operation.
    pub op: vortex_ugb::Op,
    /// Concrete operands: registers resolved to frame slots, immediates
    /// materialized, labels resolved to instruction indices.
    pub operands: Vec<Operand>,
}

/// A compiled code object for one method.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CodeObject {
    /// Method index this object implements.
    pub method: u32,
    /// Linear patched instruction sequence.
    pub instrs: Vec<PatchedInstr>,
    /// Capabilities the method requires at execution time.
    pub caps: Vec<vortex_ugb::CapReq>,
    /// Register count of the frame.
    pub reg_count: u16,
    /// Local count of the frame.
    pub local_count: u16,
}

/// Stencil support per opcode: which operations this build patches.
// CEP:WHAT Reports whether an opcode has a stencil.
// CEP:WHY The baseline tier covers the common opcodes; exotic ones fall
//        back to T0.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t1
fn has_stencil(op: vortex_ugb::Op) -> bool {
    use vortex_ugb::Op as U;
    matches!(
        op,
        U::Nop
            | U::LoadConst
            | U::LoadLocal
            | U::StoreLocal
            | U::Mov
            | U::Add
            | U::Sub
            | U::Mul
            | U::Div
            | U::Rem
            | U::And
            | U::Or
            | U::Xor
            | U::Shl
            | U::Shr
            | U::Neg
            | U::Not
            | U::Cmp
            | U::Jump
            | U::JumpIfTrue
            | U::JumpIfFalse
            | U::CallMethod
            | U::CallDirect
            | U::Call
            | U::Return
            | U::Load
            | U::Store
            | U::Alloc
            | U::AddInt
            | U::AddImm
            | U::SubImm
            | U::SuperLoadAddStore
            | U::SuperCmpBranch
    )
}

/// Plans a method's code object from its wire bytes.
// CEP:WHAT Decodes the method once and resolves all operands.
// CEP:WHY T1a: copy the stencil bodies and compute every patch point.
// CEP:STATUS complete
// CEP:FAILURE PlanError for unsupported opcodes or malformed operands.
// CEP:ASSUMES Verified bytecode.
// CEP:COST O(|code|).
// CEP:EVIDENCE cargo test -p vortex-t1
// CEP:ID: JIT-a42
pub fn plan_method(program: &Program, method: u32) -> Result<CodeObject, PlanError> {
    let m = program
        .method(method)
        .ok_or(PlanError::BadPatch { method, offset: 0 })?;
    let code = m.code.clone();
    let mut instrs = Vec::new();
    let mut pos = 0;
    // First pass: collect instruction starts for label resolution.
    let mut starts: Vec<usize> = Vec::new();
    while pos < code.len() {
        starts.push(pos);
        let (_, size) = Instr::decode(&code, pos).map_err(|_| PlanError::BadPatch {
            method,
            offset: pos,
        })?;
        pos += size;
    }
    let index_of =
        |byte_offset: usize| -> Option<usize> { starts.iter().position(|s| *s == byte_offset) };
    let mut pos = 0;
    while pos < code.len() {
        let (instr, size) = Instr::decode(&code, pos).map_err(|_| PlanError::BadPatch {
            method,
            offset: pos,
        })?;
        if !has_stencil(instr.op) {
            return Err(PlanError::UnsupportedOpcode {
                method,
                offset: pos,
                opcode: instr.op.info().name,
            });
        }
        let mut operands = instr.operands.clone();
        for o in operands.iter_mut() {
            if let Operand::Label(l) = o {
                let target = (pos as i64 + *l as i64).max(0) as usize;
                match index_of(target) {
                    Some(idx) => *o = Operand::Label(idx as i32),
                    None => {
                        return Err(PlanError::BadPatch {
                            method,
                            offset: pos,
                        })
                    }
                }
            }
        }
        instrs.push(PatchedInstr {
            op: instr.op,
            operands,
        });
        pos += size;
    }
    Ok(CodeObject {
        method,
        instrs,
        caps: m.env_caps.clone(),
        reg_count: m.reg_count,
        local_count: m.local_count,
    })
}
