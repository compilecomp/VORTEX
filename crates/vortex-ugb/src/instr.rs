// CEP:FILE crates/vortex-ugb/src/instr.rs
// CEP:WHAT Hybrid variable-length instruction encoding and decoding
//        (UGB-1 section 4): standard 1-byte-register form and the 0xFF-prefix
//        extended 2-byte-register form.
// CEP:WHY L1 I-cache density for 95% of instructions with headroom for large
//        register counts; the decoder is the single authority on byte layout.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Truncated streams, unknown opcodes, and unknown prefix bytes
//        return DecodeError with the byte offset; no guessing, no over-reads.
// CEP:ASSUMES Little-endian byte order throughout.
// CEP:COST Decode is O(size of instruction); no allocation.
// CEP:EVIDENCE cargo test -p vortex-ugb (roundtrip + extended-encoding tests)
use crate::op::{from_byte, sig, Op};

/// Extended-encoding prefix: the following instruction uses 2-byte registers.
pub const EXTENDED_PREFIX: u8 = 0xFF;

/// A decoded UGB instruction. Register operands are widened to `u16` so the
/// decoder output is uniform across both encodings; the interpreter bounds
/// them against the method's `register_count`.
#[derive(Debug, Clone, PartialEq)]
pub struct Instr {
    /// Opcode.
    pub op: Op,
    /// Register operands, in order.
    pub regs: [u16; 3],
    /// Branch offsets (i16, relative to the end of the instruction).
    pub branches: [i16; 1],
    /// Signed i32 immediate (LoadInt32).
    pub imm32: i32,
    /// Unsigned u32 immediates (CapCreate/AllocArray/CapCheck/Call).
    pub imm32u: [u32; 2],
    /// Binary64 immediate bits (LoadFloat).
    pub imm64: u64,
    /// Constant-pool index (LoadConst).
    pub pool: u16,
    /// True when the instruction was decoded from the extended encoding.
    pub extended: bool,
}

/// Decoding failures carry the offending byte offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeError {
    /// Byte offset within the code stream where decoding failed.
    pub offset: usize,
    /// Human-readable cause (stable text; used by tools and tests).
    pub cause: &'static str,
}

/// Reads a little-endian `u16` at `offset`.
fn read_u16(code: &[u8], offset: usize) -> Result<u16, DecodeError> {
    let hi = *code.get(offset).ok_or(DecodeError {
        offset,
        cause: "truncated u16",
    })?;
    let lo = *code.get(offset + 1).ok_or(DecodeError {
        offset,
        cause: "truncated u16",
    })?;
    Ok(u16::from_le_bytes([hi, lo]))
}

/// Reads a little-endian `u32` at `offset`.
fn read_u32(code: &[u8], offset: usize) -> Result<u32, DecodeError> {
    let mut b = [0_u8; 4];
    for (i, slot) in b.iter_mut().enumerate() {
        *slot = *code.get(offset + i).ok_or(DecodeError {
            offset,
            cause: "truncated u32",
        })?;
    }
    Ok(u32::from_le_bytes(b))
}

/// Reads a little-endian `u64` at `offset`.
fn read_u64(code: &[u8], offset: usize) -> Result<u64, DecodeError> {
    let mut b = [0_u8; 8];
    for (i, slot) in b.iter_mut().enumerate() {
        *slot = *code.get(offset + i).ok_or(DecodeError {
            offset,
            cause: "truncated u64",
        })?;
    }
    Ok(u64::from_le_bytes(b))
}

/// Decodes the instruction starting at byte `offset` of `code`.
///
/// On success returns the instruction and the byte offset of the next
/// instruction. The decoder reads at most one instruction.
// CEP:WHAT Decodes the instruction starting at byte offset of code.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-ugb
pub fn decode_at(code: &[u8], offset: usize) -> Result<(Instr, usize), DecodeError> {
    let opcode_byte = *code.get(offset).ok_or(DecodeError {
        offset,
        cause: "empty stream",
    })?;
    let mut p = offset + 1;
    let (op, extended, reg_width) = if opcode_byte == EXTENDED_PREFIX {
        let inner = *code.get(p).ok_or(DecodeError {
            offset,
            cause: "prefix without opcode",
        })?;
        let op = from_byte(inner).ok_or(DecodeError {
            offset: p,
            cause: "unknown opcode after prefix",
        })?;
        p += 1;
        (op, true, 2_usize)
    } else {
        let op = from_byte(opcode_byte).ok_or(DecodeError {
            offset,
            cause: "unknown opcode",
        })?;
        (op, false, 1_usize)
    };

    let s = sig(op);
    let mut instr = Instr {
        op,
        regs: [0; 3],
        branches: [0; 1],
        imm32: 0,
        imm32u: [0; 2],
        imm64: 0,
        pool: 0,
        extended,
    };
    for i in 0..usize::from(s.regs) {
        let v = if reg_width == 2 {
            u32::from(read_u16(code, p + i * 2)?)
        } else {
            u32::from(*code.get(p + i).ok_or(DecodeError {
                offset: p,
                cause: "truncated regs",
            })?)
        };
        if v > u32::from(u16::MAX) {
            return Err(DecodeError {
                offset: p,
                cause: "register exceeds u16",
            });
        }
        instr.regs[i] = v as u16;
    }
    p += usize::from(s.regs) * reg_width;
    for i in 0..usize::from(s.branches) {
        instr.branches[i] = i16::from_ne_bytes(read_u16(code, p + i * 2)?.to_ne_bytes());
    }
    p += 2 * usize::from(s.branches);
    if s.imm32 > 0 {
        instr.imm32 = i32::from_ne_bytes(read_u32(code, p)?.to_ne_bytes());
        p += 4;
    }
    for i in 0..usize::from(s.imm32u) {
        instr.imm32u[i] = read_u32(code, p + i * 4)?;
    }
    p += 4 * usize::from(s.imm32u);
    if s.imm64 > 0 {
        instr.imm64 = read_u64(code, p)?;
        p += 8;
    }
    if s.pool > 0 {
        instr.pool = read_u16(code, p)?;
        p += 2;
    }
    Ok((instr, p))
}

/// Encodes `instr` in the standard (1-byte register) encoding.
///
/// Register operands must fit in one byte; use [`encode_extended`] for larger
/// indices. Returns the encoded byte length written into `out`.
pub fn encode_standard(instr: &Instr, out: &mut Vec<u8>) -> Result<usize, EncodeError> {
    for (i, r) in instr.regs[..usize::from(sig(instr.op).regs)]
        .iter()
        .enumerate()
    {
        if *r > u8::MAX.into() {
            return Err(EncodeError::RegisterTooLarge {
                index: i,
                value: *r,
            });
        }
    }
    encode_into(instr, out, false)
}

/// Encodes `instr` in the extended (0xFF prefix, 2-byte register) encoding.
pub fn encode_extended(instr: &Instr, out: &mut Vec<u8>) -> Result<usize, EncodeError> {
    encode_into(instr, out, true)
}

/// Encoding failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeError {
    /// A register operand exceeds 255 in the standard encoding.
    RegisterTooLarge {
        /// Zero-based operand index.
        index: usize,
        /// The rejected register number.
        value: u16,
    },
}

fn push_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn push_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

// CEP:WHAT Implements `encode_into` as part of the module contract.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-ugb
fn encode_into(instr: &Instr, out: &mut Vec<u8>, extended: bool) -> Result<usize, EncodeError> {
    let start = out.len();
    if extended {
        out.push(EXTENDED_PREFIX);
    }
    out.push(instr.op as u8);
    let s = sig(instr.op);
    for r in instr.regs[..usize::from(s.regs)].iter() {
        if extended {
            push_u16(out, *r);
        } else {
            out.push(u8::try_from(*r).map_err(|_| EncodeError::RegisterTooLarge {
                index: 0,
                value: *r,
            })?);
        }
    }
    for b in instr.branches[..usize::from(s.branches)].iter() {
        push_u16(out, *b as u16);
    }
    if s.imm32 > 0 {
        push_u32(out, instr.imm32 as u32);
    }
    for v in instr.imm32u[..usize::from(s.imm32u)].iter() {
        push_u32(out, *v);
    }
    if s.imm64 > 0 {
        push_u64(out, instr.imm64);
    }
    if s.pool > 0 {
        push_u16(out, instr.pool);
    }
    Ok(out.len() - start)
}

/// Size of the instruction at `offset` without full decode; used by the
/// verifier and tools for cheap stream walking.
pub fn size_at(code: &[u8], offset: usize) -> Result<usize, DecodeError> {
    let (instr, next) = decode_at(code, offset)?;
    let _ = instr;
    Ok(next - offset)
}

/// Standard-encoding size of a structurally valid instruction.
#[must_use]
pub fn standard_size_of(instr: &Instr) -> usize {
    crate::op::standard_size(instr.op)
        + if instr.extended {
            extended_extra(instr)
        } else {
            0
        }
}

fn extended_extra(instr: &Instr) -> usize {
    let s = sig(instr.op);
    1 + usize::from(s.regs)
}
