// CEP:FILE crates/vortex-ugb/src/instr.rs
// CEP:WHAT Instruction and operand model plus the signature-driven byte
//        codec: encode, decode, and encoded-size computation.
// CEP:WHY UGB is a variable-length register bytecode; one codec keeps the
//        wire format identical for the assembler, verifier, and interpreter.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Decode returns typed CodecError values; it never panics and
//        never reads past the supplied slice.
// CEP:ASSUMES Little-endian wire order; operands are encoded in signature
//        order with fixed widths per kind.
// CEP:COST O(instruction bytes) for decode; O(1) per operand.
// CEP:EVIDENCE cargo test -p vortex-ugb
//! Instruction model and byte codec.

use crate::ops::{CmpKind, Op, OperandKind};
use std::fmt;

/// Codec failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecError {
    /// The buffer ended inside an instruction.
    UnexpectedEnd {
        /// Byte offset where more bytes were required.
        offset: usize,
    },
    /// The byte does not map to a defined opcode.
    UnknownOpcode {
        /// Byte offset of the opcode.
        offset: usize,
        /// The offending byte.
        byte: u8,
    },
    /// An operand byte was outside its kind's domain.
    BadOperand {
        /// Byte offset of the operand.
        offset: usize,
        /// What was expected.
        expected: &'static str,
    },
}

impl fmt::Display for CodecError {
    // CEP:WHAT Renders a codec failure with its offset.
    // CEP:WHY Diagnostics must point at the failing byte.
    // CEP:STATUS complete
    // CEP:FAILURE Formatter errors only.
    // CEP:ASSUMES None.
    // CEP:COST O(fields).
    // CEP:EVIDENCE cargo test -p vortex-ugb
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodecError::UnexpectedEnd { offset } => {
                write!(f, "unexpected end of code at byte {offset}")
            }
            CodecError::UnknownOpcode { offset, byte } => {
                write!(f, "unknown opcode {byte:#04x} at byte {offset}")
            }
            CodecError::BadOperand { offset, expected } => {
                write!(f, "invalid {expected} operand at byte {offset}")
            }
        }
    }
}

impl std::error::Error for CodecError {}

/// One decoded operand value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Operand {
    /// Register index.
    Reg(u16),
    /// 32-bit signed immediate.
    Imm32(i32),
    /// 64-bit signed immediate.
    Imm64(i64),
    /// 64-bit IEEE-754 immediate.
    F64(f64),
    /// Capability identifier.
    CapId(u16),
    /// Branch target as a byte offset into the method code.
    Label(i32),
    /// Type tag.
    TypeTag(u8),
    /// FFI descriptor identifier.
    FfiDesc(u32),
    /// Comparison selector.
    CmpKind(CmpKind),
    /// Argument or size count.
    Count(u16),
    /// Local slot.
    Local(u16),
    /// Constant-pool index.
    ConstIdx(u32),
    /// Language identifier.
    LangId(u8),
}

impl Operand {
    /// The register index if this operand is a register.
    // CEP:WHAT Extracts the register index of a register operand.
    // CEP:WHY The verifier and interpreter pattern-match registers often.
    // CEP:STATUS complete
    // CEP:FAILURE None for non-register operands.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ugb
    #[inline]
    pub fn as_reg(self) -> Option<u16> {
        match self {
            Operand::Reg(r) => Some(r),
            _ => None,
        }
    }

    /// The label offset if this operand is a label.
    // CEP:WHAT Extracts the branch offset of a label operand.
    // CEP:WHY The verifier validates targets against instruction starts.
    // CEP:STATUS complete
    // CEP:FAILURE None for non-label operands.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ugb
    #[inline]
    pub fn as_label(self) -> Option<i32> {
        match self {
            Operand::Label(l) => Some(l),
            _ => None,
        }
    }

    /// Renders the operand in assembly syntax.
    // CEP:WHAT Formats an operand for assembly output.
    // CEP:WHY The disassembler and error messages share one rendering.
    // CEP:STATUS complete
    // CEP:FAILURE Formatter errors only.
    // CEP:ASSUMES None.
    // CEP:COST O(digits).
    // CEP:EVIDENCE cargo test -p vortex-ugb
    pub fn render(self) -> String {
        match self {
            Operand::Reg(r) => format!("r{r}"),
            Operand::Imm32(v) => format!("{v}"),
            Operand::Imm64(v) => format!("{v}L"),
            Operand::F64(v) => format!("{v}D"),
            Operand::CapId(c) => format!("cap{c}"),
            Operand::Label(l) => format!("{l:+#x}"),
            Operand::TypeTag(t) => format!("type{t}"),
            Operand::FfiDesc(d) => format!("fd{d}"),
            Operand::CmpKind(k) => format!("{k:?}"),
            Operand::Count(n) => format!("x{n}"),
            Operand::Local(l) => format!("local{l}"),
            Operand::ConstIdx(c) => format!("const{c}"),
            Operand::LangId(l) => format!("lang{l}"),
        }
    }
}

/// A decoded instruction.
#[derive(Debug, Clone, PartialEq)]
pub struct Instr {
    /// The operation.
    pub op: Op,
    /// Operands in signature order.
    pub operands: Vec<Operand>,
}

impl Instr {
    /// Creates an instruction without validation; constructors that build
    /// wire bytes should route through `encode`, and programs through the
    /// verifier before execution.
    // CEP:WHAT Builds an instruction from an opcode and operand list.
    // CEP:WHY The assembler constructs instructions before validation.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; shape errors surface at encode or verify.
    // CEP:ASSUMES Operand order follows the opcode signature.
    // CEP:COST O(operands) for the vector.
    // CEP:EVIDENCE cargo test -p vortex-ugb
    pub fn new(op: Op, operands: Vec<Operand>) -> Instr {
        Instr { op, operands }
    }

    /// Encodes the instruction to bytes in signature order.
    // CEP:WHAT Encodes an instruction into the wire format.
    // CEP:WHY Methods store a byte array; encoding is the single writer.
    // CEP:STATUS complete
    // CEP:FAILURE CodecError when the operand list shape does not match
    //        the opcode signature.
    // CEP:ASSUMES None.
    // CEP:COST O(bytes written).
    // CEP:EVIDENCE cargo test -p vortex-ugb
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<(), CodecError> {
        let info = self.op.info();
        if info.sig.len() != self.operands.len() {
            return Err(CodecError::BadOperand {
                offset: out.len(),
                expected: "operand count",
            });
        }
        out.push(self.op as u8);
        for (kind, operand) in info.sig.iter().zip(self.operands.iter()) {
            match (kind, operand) {
                (OperandKind::Reg, Operand::Reg(v)) => out.extend_from_slice(&v.to_le_bytes()),
                (OperandKind::Imm32, Operand::Imm32(v)) => out.extend_from_slice(&v.to_le_bytes()),
                (OperandKind::Imm64, Operand::Imm64(v)) => out.extend_from_slice(&v.to_le_bytes()),
                (OperandKind::F64, Operand::F64(v)) => {
                    out.extend_from_slice(&v.to_bits().to_le_bytes())
                }
                (OperandKind::CapId, Operand::CapId(v)) => out.extend_from_slice(&v.to_le_bytes()),
                (OperandKind::Label, Operand::Label(v)) => out.extend_from_slice(&v.to_le_bytes()),
                (OperandKind::TypeTag, Operand::TypeTag(v)) => out.push(*v),
                (OperandKind::FfiDesc, Operand::FfiDesc(v)) => {
                    out.extend_from_slice(&v.to_le_bytes())
                }
                (OperandKind::CmpKind, Operand::CmpKind(v)) => out.push(v.to_byte()),
                (OperandKind::Count, Operand::Count(v)) => out.extend_from_slice(&v.to_le_bytes()),
                (OperandKind::Local, Operand::Local(v)) => out.extend_from_slice(&v.to_le_bytes()),
                (OperandKind::ConstIdx, Operand::ConstIdx(v)) => {
                    out.extend_from_slice(&v.to_le_bytes())
                }
                (OperandKind::LangId, Operand::LangId(v)) => out.push(*v),
                _ => {
                    return Err(CodecError::BadOperand {
                        offset: out.len(),
                        expected: info.name,
                    })
                }
            }
        }
        Ok(())
    }

    /// Encoded byte length of this instruction.
    // CEP:WHAT Computes the encoded length without writing bytes.
    // CEP:WHY The assembler resolves labels with a size pre-pass.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES The operand list matches the signature.
    // CEP:COST O(operands).
    // CEP:EVIDENCE cargo test -p vortex-ugb
    pub fn encoded_len(&self) -> usize {
        let mut n = 1;
        for (kind, operand) in self.op.info().sig.iter().zip(self.operands.iter()) {
            n += match (kind, operand) {
                (OperandKind::TypeTag, _)
                | (OperandKind::CmpKind, _)
                | (OperandKind::LangId, _) => 1,
                (OperandKind::Reg, _)
                | (OperandKind::CapId, _)
                | (OperandKind::Count, _)
                | (OperandKind::Local, _) => 2,
                (OperandKind::Imm32, _)
                | (OperandKind::Label, _)
                | (OperandKind::FfiDesc, _)
                | (OperandKind::ConstIdx, _) => 4,
                (OperandKind::Imm64, _) | (OperandKind::F64, _) => 8,
            };
        }
        n
    }

    /// Decodes one instruction from the front of `code`.
    // CEP:WHAT Decodes one instruction starting at `offset`.
    // CEP:WHY The interpreter dispatch loop decodes lazily at the PC.
    // CEP:STATUS complete
    // CEP:FAILURE Typed CodecError for truncated or malformed encodings.
    // CEP:ASSUMES Little-endian wire order.
    // CEP:COST O(instruction bytes).
    // CEP:EVIDENCE cargo test -p vortex-ugb
    pub fn decode(code: &[u8], offset: usize) -> Result<(Instr, usize), CodecError> {
        let mut pos = offset;
        let need = |pos: usize, n: usize, len: usize| -> Result<(), CodecError> {
            if pos + n > len {
                Err(CodecError::UnexpectedEnd { offset: pos })
            } else {
                Ok(())
            }
        };
        need(pos, 1, code.len())?;
        let byte = code[pos];
        pos += 1;
        let op = Op::from_byte(byte).ok_or(CodecError::UnknownOpcode {
            offset: pos - 1,
            byte,
        })?;
        let info = op.info();
        let mut operands = Vec::with_capacity(info.sig.len());
        for kind in info.sig {
            match kind {
                OperandKind::Reg => {
                    need(pos, 2, code.len())?;
                    operands.push(Operand::Reg(u16::from_le_bytes([code[pos], code[pos + 1]])));
                    pos += 2;
                }
                OperandKind::Imm32 => {
                    need(pos, 4, code.len())?;
                    operands.push(Operand::Imm32(i32::from_le_bytes([
                        code[pos],
                        code[pos + 1],
                        code[pos + 2],
                        code[pos + 3],
                    ])));
                    pos += 4;
                }
                OperandKind::Imm64 => {
                    need(pos, 8, code.len())?;
                    let mut b = [0u8; 8];
                    b.copy_from_slice(&code[pos..pos + 8]);
                    operands.push(Operand::Imm64(i64::from_le_bytes(b)));
                    pos += 8;
                }
                OperandKind::F64 => {
                    need(pos, 8, code.len())?;
                    let mut b = [0u8; 8];
                    b.copy_from_slice(&code[pos..pos + 8]);
                    operands.push(Operand::F64(f64::from_bits(u64::from_le_bytes(b))));
                    pos += 8;
                }
                OperandKind::CapId => {
                    need(pos, 2, code.len())?;
                    operands.push(Operand::CapId(u16::from_le_bytes([
                        code[pos],
                        code[pos + 1],
                    ])));
                    pos += 2;
                }
                OperandKind::Label => {
                    need(pos, 4, code.len())?;
                    operands.push(Operand::Label(i32::from_le_bytes([
                        code[pos],
                        code[pos + 1],
                        code[pos + 2],
                        code[pos + 3],
                    ])));
                    pos += 4;
                }
                OperandKind::TypeTag => {
                    need(pos, 1, code.len())?;
                    operands.push(Operand::TypeTag(code[pos]));
                    pos += 1;
                }
                OperandKind::FfiDesc => {
                    need(pos, 4, code.len())?;
                    operands.push(Operand::FfiDesc(u32::from_le_bytes([
                        code[pos],
                        code[pos + 1],
                        code[pos + 2],
                        code[pos + 3],
                    ])));
                    pos += 4;
                }
                OperandKind::CmpKind => {
                    need(pos, 1, code.len())?;
                    let k = CmpKind::from_byte(code[pos]).ok_or(CodecError::BadOperand {
                        offset: pos,
                        expected: "comparison kind",
                    })?;
                    operands.push(Operand::CmpKind(k));
                    pos += 1;
                }
                OperandKind::Count => {
                    need(pos, 2, code.len())?;
                    operands.push(Operand::Count(u16::from_le_bytes([
                        code[pos],
                        code[pos + 1],
                    ])));
                    pos += 2;
                }
                OperandKind::Local => {
                    need(pos, 2, code.len())?;
                    operands.push(Operand::Local(u16::from_le_bytes([
                        code[pos],
                        code[pos + 1],
                    ])));
                    pos += 2;
                }
                OperandKind::ConstIdx => {
                    need(pos, 4, code.len())?;
                    operands.push(Operand::ConstIdx(u32::from_le_bytes([
                        code[pos],
                        code[pos + 1],
                        code[pos + 2],
                        code[pos + 3],
                    ])));
                    pos += 4;
                }
                OperandKind::LangId => {
                    need(pos, 1, code.len())?;
                    operands.push(Operand::LangId(code[pos]));
                    pos += 1;
                }
            }
        }
        Ok((Instr { op, operands }, pos - offset))
    }
}
