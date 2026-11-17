// CEP:FILE crates/vortex-ugb/src/verify.rs
// CEP:WHAT The UGB verifier: O(n) enforcement of Valid_UGB subset 6.9 -
//        bytecode well-formedness, branch-target validity, register and
//        index bounds, capability compliance, FFI arity, resource bounds.
// CEP:WHY Unverified bytecode never executes (UGB 6.1: verifiable by
//        construction); the verifier is that gate.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Every violation is a typed VerifyError with method, offset,
//        and rule; the verifier never panics on malformed input.
// CEP:ASSUMES Wire format is little-endian; operand widths are the codec's.
// CEP:COST O(|bytecode|) per method, O(|program|) overall.
// CEP:EVIDENCE cargo test -p vortex-ugb
//! UGB verifier (Valid_UGB, design 6.9).

use crate::instr::{CodecError, Instr, Operand};
use crate::method::Program;
use crate::ops::{CapReq, Op};
use std::collections::BTreeSet;
use std::fmt;

/// Verifier failures, each naming the rule that rejected the image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    /// The method's byte stream did not decode.
    Decode {
        /// Method index.
        method: usize,
        /// Byte offset of the failure.
        offset: usize,
        /// Underlying codec error.
        error: CodecError,
    },
    /// A branch target is not an instruction start.
    BadBranchTarget {
        /// Method index.
        method: usize,
        /// Byte offset of the branch.
        offset: usize,
        /// The invalid target.
        target: i32,
    },
    /// A register index exceeds the method's register count.
    RegisterOutOfRange {
        /// Method index.
        method: usize,
        /// Byte offset.
        offset: usize,
        /// The offending register.
        reg: u16,
    },
    /// A local slot exceeds the method's local count.
    LocalOutOfRange {
        /// Method index.
        method: usize,
        /// Byte offset.
        offset: usize,
        /// The offending local slot.
        local: u16,
    },
    /// A constant-pool index exceeds the pool size.
    ConstantOutOfRange {
        /// Method index.
        method: usize,
        /// Byte offset.
        offset: usize,
        /// The offending constant index.
        index: u32,
    },
    /// An FFI descriptor index exceeds the table size.
    FfiOutOfRange {
        /// Method index.
        method: usize,
        /// Byte offset.
        offset: usize,
        /// The offending descriptor index.
        index: u32,
    },
    /// A language identifier is not declared by the program.
    UnknownLanguage {
        /// Method index.
        method: usize,
        /// Byte offset.
        offset: usize,
        /// The offending language id.
        lang: u8,
    },
    /// The instruction requires a capability the environment lacks.
    CapabilityViolation {
        /// Method index.
        method: usize,
        /// Byte offset.
        offset: usize,
        /// The missing capability.
        required: CapReq,
    },
    /// An FFI call's argument count disagrees with the descriptor arity.
    FfiArityMismatch {
        /// Method index.
        method: usize,
        /// Byte offset.
        offset: usize,
        /// Declared arity.
        declared: u16,
        /// Call-site count.
        found: u16,
    },
    /// A method exceeds a documented resource bound.
    ResourceBound {
        /// Method index.
        method: usize,
        /// The bound that was exceeded.
        rule: &'static str,
    },
    /// The method never returns.
    NoReturn {
        /// Method index.
        method: usize,
    },
}

impl fmt::Display for VerifyError {
    // CEP:WHAT Renders a verifier failure with method, offset, and rule.
    // CEP:WHY Operators and front-end authors need the exact site.
    // CEP:STATUS complete
    // CEP:FAILURE Formatter errors only.
    // CEP:ASSUMES None.
    // CEP:COST O(fields).
    // CEP:EVIDENCE cargo test -p vortex-ugb
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyError::Decode { method, error, .. } => {
                write!(f, "method {method}: decode failed: {error}")
            }
            VerifyError::BadBranchTarget {
                method,
                offset,
                target,
            } => {
                write!(f, "method {method}: branch at {offset} targets {target}, not an instruction start")
            }
            VerifyError::RegisterOutOfRange {
                method,
                offset,
                reg,
            } => {
                write!(
                    f,
                    "method {method}: register r{reg} out of frame at {offset}"
                )
            }
            VerifyError::LocalOutOfRange {
                method,
                offset,
                local,
            } => {
                write!(f, "method {method}: local {local} out of frame at {offset}")
            }
            VerifyError::ConstantOutOfRange {
                method,
                offset,
                index,
            } => {
                write!(
                    f,
                    "method {method}: constant {index} out of pool at {offset}"
                )
            }
            VerifyError::FfiOutOfRange {
                method,
                offset,
                index,
            } => {
                write!(
                    f,
                    "method {method}: ffi descriptor {index} out of table at {offset}"
                )
            }
            VerifyError::UnknownLanguage {
                method,
                offset,
                lang,
            } => {
                write!(
                    f,
                    "method {method}: language {lang} not declared at {offset}"
                )
            }
            VerifyError::CapabilityViolation {
                method,
                offset,
                required,
            } => {
                write!(
                    f,
                    "method {method}: capability {required:?} required at {offset}"
                )
            }
            VerifyError::FfiArityMismatch {
                method,
                offset,
                declared,
                found,
            } => {
                write!(f, "method {method}: ffi arity {found} disagrees with declared {declared} at {offset}")
            }
            VerifyError::ResourceBound { method, rule } => {
                write!(f, "method {method}: exceeds bound {rule}")
            }
            VerifyError::NoReturn { method } => {
                write!(f, "method {method}: no RETURN instruction")
            }
        }
    }
}

impl std::error::Error for VerifyError {}

/// Instruction starts of a method, for branch-target validation.
struct Layout {
    starts: BTreeSet<usize>,
}

/// Computes the instruction-start set of a byte stream.
// CEP:WHAT Decodes the whole stream and records instruction starts.
// CEP:WHY Branch targets must land on instruction boundaries (6.9 item 1).
// CEP:STATUS complete
// CEP:FAILURE Propagates CodecError from the instruction decoder.
// CEP:ASSUMES None.
// CEP:COST O(|code|).
// CEP:EVIDENCE cargo test -p vortex-ugb
fn layout(code: &[u8]) -> Result<Layout, CodecError> {
    let mut starts = BTreeSet::new();
    let mut pos = 0;
    while pos < code.len() {
        starts.insert(pos);
        let (_, size) = Instr::decode(code, pos)?;
        pos += size;
    }
    Ok(Layout { starts })
}

/// Verifies one method against the program tables and bounds.
// CEP:WHAT Checks well-formedness, bounds, capabilities, and FFI arity of
//        one method's instruction stream.
// CEP:WHY 6.9 items 1, 2, 3, 5, 7, 8 are per-method properties.
// CEP:STATUS complete
// CEP:FAILURE First violation is returned; ordering is deterministic.
// CEP:ASSUMES None.
// CEP:COST O(|code| + |caps| log |caps|) with the env as a set.
// CEP:EVIDENCE cargo test -p vortex-ugb
// CEP:ID: IR-a10
fn verify_method(program: &Program, mi: usize) -> Result<(), VerifyError> {
    let method = &program.methods[mi];
    let bounds = &program.bounds;
    if method.code.len() > bounds.max_code_bytes {
        return Err(VerifyError::ResourceBound {
            method: mi,
            rule: "max_code_bytes",
        });
    }
    if method.reg_count > bounds.max_registers {
        return Err(VerifyError::ResourceBound {
            method: mi,
            rule: "max_registers",
        });
    }
    let lay = layout(&method.code).map_err(|e| VerifyError::Decode {
        method: mi,
        offset: 0,
        error: e,
    })?;
    let env: BTreeSet<CapReq> = method.env_caps.iter().copied().collect();
    let mut returns = false;
    let mut pos = 0;
    while pos < method.code.len() {
        let (instr, size) = Instr::decode(&method.code, pos).map_err(|e| VerifyError::Decode {
            method: mi,
            offset: pos,
            error: e,
        })?;
        check_operands(program, mi, pos, &instr, &env)?;
        match instr.op {
            Op::Jump | Op::JumpIfTrue | Op::JumpIfFalse | Op::SuperCmpBranch => {
                let label = label_of(&instr).ok_or(VerifyError::Decode {
                    method: mi,
                    offset: pos,
                    error: CodecError::BadOperand {
                        offset: pos,
                        expected: "label",
                    },
                })?;
                let target = pos as i64 + label as i64;
                if target < 0 || !lay.starts.contains(&(target as usize)) {
                    return Err(VerifyError::BadBranchTarget {
                        method: mi,
                        offset: pos,
                        target: label,
                    });
                }
            }
            Op::Return => returns = true,
            // Data, arithmetic, memory, and call instructions carry no
            // branch targets and no return semantics in this verifier pass.
            _ => {
                let _ = instr;
            }
        }
        pos += size;
    }
    if !returns {
        return Err(VerifyError::NoReturn { method: mi });
    }
    Ok(())
}

/// The label operand of a branch instruction.
// CEP:WHAT Extracts the label operand from a branch instruction.
// CEP:WHY Branch validation needs the relative offset.
// CEP:STATUS complete
// CEP:FAILURE None when no operand is a label.
// CEP:ASSUMES The codec already enforced the signature.
// CEP:COST O(operands).
// CEP:EVIDENCE cargo test -p vortex-ugb
fn label_of(instr: &Instr) -> Option<i32> {
    instr.operands.iter().find_map(|o| match o {
        Operand::Label(l) => Some(*l),
        _ => None,
    })
}

/// Operand-level checks: bounds, capabilities, and FFI arity.
// CEP:WHAT Validates one instruction's operands against program tables.
// CEP:WHY 6.9 item 1 bounds and item 2 capability compliance are
//        per-instruction properties.
// CEP:STATUS complete
// CEP:FAILURE First violation is returned.
// CEP:ASSUMES The operand list matches the opcode signature.
// CEP:COST O(operands + |env caps|).
// CEP:EVIDENCE cargo test -p vortex-ugb
fn check_operands(
    program: &Program,
    mi: usize,
    offset: usize,
    instr: &Instr,
    env: &BTreeSet<CapReq>,
) -> Result<(), VerifyError> {
    let info = instr.op.info();
    for cap in info.caps {
        if !env.contains(cap) {
            return Err(VerifyError::CapabilityViolation {
                method: mi,
                offset,
                required: *cap,
            });
        }
    }
    for (kind, operand) in info.sig.iter().zip(instr.operands.iter()) {
        match (kind, operand) {
            (crate::ops::OperandKind::Reg, Operand::Reg(r)) => {
                if *r >= program.methods[mi].reg_count {
                    return Err(VerifyError::RegisterOutOfRange {
                        method: mi,
                        offset,
                        reg: *r,
                    });
                }
            }
            (crate::ops::OperandKind::Local, Operand::Local(l)) => {
                if *l >= program.methods[mi].local_count {
                    return Err(VerifyError::LocalOutOfRange {
                        method: mi,
                        offset,
                        local: *l,
                    });
                }
            }
            (crate::ops::OperandKind::ConstIdx, Operand::ConstIdx(c)) => {
                if (*c as usize) >= program.constants.len() {
                    return Err(VerifyError::ConstantOutOfRange {
                        method: mi,
                        offset,
                        index: *c,
                    });
                }
            }
            (crate::ops::OperandKind::FfiDesc, Operand::FfiDesc(d)) => {
                let desc =
                    program
                        .ffi_table
                        .get(*d as usize)
                        .ok_or(VerifyError::FfiOutOfRange {
                            method: mi,
                            offset,
                            index: *d,
                        })?;
                let count = instr.operands.iter().find_map(|o| match o {
                    Operand::Count(n) => Some(*n),
                    _ => None,
                });
                if let Some(n) = count {
                    if n != desc.arity {
                        return Err(VerifyError::FfiArityMismatch {
                            method: mi,
                            offset,
                            declared: desc.arity,
                            found: n,
                        });
                    }
                }
            }
            (crate::ops::OperandKind::LangId, Operand::LangId(l))
                if !program.languages.contains(l) =>
            {
                return Err(VerifyError::UnknownLanguage {
                    method: mi,
                    offset,
                    lang: *l,
                });
            }
            // Remaining operand kinds reference no program tables.
            _ => {
                let _ = info;
            }
        }
    }
    Ok(())
}

/// Verifies a whole program image (Valid_UGB subset, design 6.9).
// CEP:WHAT Verifies every method and the program-level bounds.
// CEP:WHY Unverified programs never enter the interpreter or the JIT.
// CEP:STATUS complete
// CEP:FAILURE The first violation, deterministically ordered by method.
// CEP:ASSUMES The program carries its bounds.
// CEP:COST O(|program|).
// CEP:EVIDENCE cargo test -p vortex-ugb
pub fn verify(program: &Program) -> Result<(), VerifyError> {
    let bounds = &program.bounds;
    if program.methods.len() > bounds.max_methods {
        return Err(VerifyError::ResourceBound {
            method: usize::MAX,
            rule: "max_methods",
        });
    }
    if program.constants.len() > bounds.max_constants {
        return Err(VerifyError::ResourceBound {
            method: usize::MAX,
            rule: "max_constants",
        });
    }
    if program.ffi_table.len() > bounds.max_ffi_descriptors {
        return Err(VerifyError::ResourceBound {
            method: usize::MAX,
            rule: "max_ffi_descriptors",
        });
    }
    for mi in 0..program.methods.len() {
        verify_method(program, mi)?;
    }
    Ok(())
}
