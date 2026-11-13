// CEP:FILE crates/vortex-ugb/src/op.rs
// CEP:WHAT UGB opcode set (a faithful subset of the UGB-1 116-opcode taxonomy,
//        covering all 12 functional domains) with operand signatures.
// CEP:WHY The bytecode is the stable contract between language frontends and
//        the execution tiers; opcode numbering follows UGB-1 section 5.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Unknown opcode bytes are rejected by the decoder, never guessed.
// CEP:ASSUMES Operand signatures match the tables in UGB-1 section 5; the
//        implemented subset documents every deviation in its doc comment.
// CEP:COST enum discriminants are the UGB opcode bytes; conversion is a match.
// CEP:EVIDENCE cargo test -p vortex-ugb

/// UGB opcodes. Discriminants are the canonical UGB-1 opcode bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Op {
    /// 0x00: `dst = Null`.
    LoadNull = 0x00,
    /// 0x02: `dst = True`.
    LoadTrue = 0x02,
    /// 0x03: `dst = False`.
    LoadFalse = 0x03,
    /// 0x06: `dst = sext(imm32)`.
    LoadInt32 = 0x06,
    /// 0x08: `dst = binary64(imm64)`.
    LoadFloat = 0x08,
    /// 0x09: `dst = ConstantPool[pool_idx]`.
    LoadConst = 0x09,
    /// 0x10: `dst = src`.
    Move = 0x10,
    /// 0x20: `dst = a + b` (int fast path, float path, or trap per UGB semantics).
    Add = 0x20,
    /// 0x21: `dst = a - b`.
    Sub = 0x21,
    /// 0x22: `dst = a * b`.
    Mul = 0x22,
    /// 0x23: `dst = a / b`.
    Div = 0x23,
    /// 0x26: `dst = -a`.
    Neg = 0x26,
    /// 0x30: `dst = (a == b)` abstract equality.
    Eq = 0x30,
    /// 0x32: `dst = (a < b)`.
    Lt = 0x32,
    /// 0x33: `dst = (a <= b)`.
    Le = 0x33,
    /// 0x36: `dst = (a == Null)`.
    IsNull = 0x36,
    /// 0x40: `PC += offset`.
    Jump = 0x40,
    /// 0x41: `if (truthy(cond)) PC += offset`.
    JumpTrue = 0x41,
    /// 0x42: `if (!truthy(cond)) PC += offset`.
    JumpFalse = 0x42,
    /// 0x47: `dst = target(argc args...)`; `argc` is a u8 immediate (subset
    /// limit; the full ISA encodes extended call forms separately).
    Call = 0x47,
    /// 0x49: return `val` to caller.
    Return = 0x49,
    /// 0x51: `dst = Array(len)`; `len` is a u32 immediate.
    AllocArray = 0x51,
    /// 0x56: `dst = arr[idx]`, bounds-checked; load barrier on tagged arrays.
    LoadIndex = 0x56,
    /// 0x57: `arr[idx] = val`, bounds-checked; write barrier per layout.
    StoreIndex = 0x57,
    /// 0x58: `dst = obj.length`.
    LoadLength = 0x58,
    /// 0x60: `dst = Capability { auth: imm32, handle: imm32 }`.
    CapCreate = 0x60,
    /// 0x61: throw SecurityError unless `cap` carries `req_auth: imm32`.
    CapCheck = 0x61,
    /// 0x63: revoke `cap` and its delegates (bumps the version epoch).
    CapRevoke = 0x63,
    /// 0x70: throw `val`.
    Throw = 0x70,
    /// 0x71: push handler at `catch_offset`.
    TryBegin = 0x71,
    /// 0x72: pop the innermost handler.
    TryEnd = 0x72,
    /// 0x73: `dst = thrown_exception`.
    Catch = 0x73,
    /// 0xB0: explicit GC yield point.
    Safepoint = 0xB0,
    /// 0xB5: no operation.
    Nop = 0xB5,
    /// 0xB6: halt the machine.
    Halt = 0xB6,
}

/// Operand signature: how many fixed operands of each kind follow the opcode
/// byte in the standard (1-byte register) encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sig {
    /// Register operands (1 byte each in standard encoding, 2 bytes extended).
    pub regs: u8,
    /// Branch offsets (i16, relative to the end of the instruction).
    pub branches: u8,
    /// Signed i32 immediates (LoadInt32).
    pub imm32: u8,
    /// Unsigned u32 immediates (CapCreate auth/handle, AllocArray len,
    /// CapCheck req_auth, Call argc).
    pub imm32u: u8,
    /// Binary64 immediates (LoadFloat).
    pub imm64: u8,
    /// Constant-pool indices (u16; LoadConst).
    pub pool: u8,
}

/// Operand signature lookup. Total coverage of the enum: adding an opcode
/// without a signature is a compile error.
#[must_use]
pub fn sig(op: Op) -> Sig {
    use Op::*;
    let (regs, branches, imm32, imm32u, imm64, pool) = match op {
        LoadNull | LoadTrue | LoadFalse => (1, 0, 0, 0, 0, 0),
        LoadInt32 => (1, 0, 1, 0, 0, 0),
        LoadFloat => (1, 0, 0, 0, 1, 0),
        LoadConst => (1, 0, 0, 0, 0, 1),
        Move | Neg | IsNull | LoadLength => (2, 0, 0, 0, 0, 0),
        Throw | Catch | CapRevoke => (1, 0, 0, 0, 0, 0),
        Add | Sub | Mul | Div | Eq | Lt | Le | LoadIndex | StoreIndex => (3, 0, 0, 0, 0, 0),
        Call => (2, 0, 0, 1, 0, 0),
        Return => (1, 0, 0, 0, 0, 0),
        AllocArray => (1, 0, 0, 1, 0, 0),
        // CapCreate carries auth + handle; CapCheck carries req_auth + site.
        CapCreate | CapCheck => (1, 0, 0, 2, 0, 0),
        Jump => (0, 1, 0, 0, 0, 0),
        JumpTrue | JumpFalse => (1, 1, 0, 0, 0, 0),
        TryBegin => (0, 1, 0, 0, 0, 0),
        TryEnd | Safepoint | Nop | Halt => (0, 0, 0, 0, 0, 0),
    };
    Sig {
        regs,
        branches,
        imm32,
        imm32u,
        imm64,
        pool,
    }
}

/// Size in bytes of an instruction in the standard (1-byte register) encoding.
#[must_use]
pub fn standard_size(op: Op) -> usize {
    let s = sig(op);
    1 + usize::from(s.regs)
        + 2 * usize::from(s.branches)
        + 4 * usize::from(s.imm32)
        + 4 * usize::from(s.imm32u)
        + 8 * usize::from(s.imm64)
        + 2 * usize::from(s.pool)
}

/// Maps a raw opcode byte to `Op`, or `None` when the byte is not part of the
/// implemented subset (the decoder must never guess).
#[must_use]
pub fn from_byte(b: u8) -> Option<Op> {
    match b {
        0x00 => Some(Op::LoadNull),
        0x02 => Some(Op::LoadTrue),
        0x03 => Some(Op::LoadFalse),
        0x06 => Some(Op::LoadInt32),
        0x08 => Some(Op::LoadFloat),
        0x09 => Some(Op::LoadConst),
        0x10 => Some(Op::Move),
        0x20 => Some(Op::Add),
        0x21 => Some(Op::Sub),
        0x22 => Some(Op::Mul),
        0x23 => Some(Op::Div),
        0x26 => Some(Op::Neg),
        0x30 => Some(Op::Eq),
        0x32 => Some(Op::Lt),
        0x33 => Some(Op::Le),
        0x36 => Some(Op::IsNull),
        0x40 => Some(Op::Jump),
        0x41 => Some(Op::JumpTrue),
        0x42 => Some(Op::JumpFalse),
        0x47 => Some(Op::Call),
        0x49 => Some(Op::Return),
        0x51 => Some(Op::AllocArray),
        0x56 => Some(Op::LoadIndex),
        0x57 => Some(Op::StoreIndex),
        0x58 => Some(Op::LoadLength),
        0x60 => Some(Op::CapCreate),
        0x61 => Some(Op::CapCheck),
        0x63 => Some(Op::CapRevoke),
        0x70 => Some(Op::Throw),
        0x71 => Some(Op::TryBegin),
        0x72 => Some(Op::TryEnd),
        0x73 => Some(Op::Catch),
        0xB0 => Some(Op::Safepoint),
        0xB5 => Some(Op::Nop),
        0xB6 => Some(Op::Halt),
        _ => None,
    }
}
