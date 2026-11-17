// CEP:FILE crates/vortex-opt/src/x64.rs
// CEP:WHAT A real x86-64 byte encoder: REX/ModRM construction, register,
//        memory, and immediate forms, rip-relative addressing, rel32
//        branches with relocations, and the GC safepoint poll macro.
// CEP:WHY The backend and the T1 stencils both emit machine bytes; one
//        encoder keeps every encoding verified in a single place.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Out-of-range condition codes surface as EncodeError;
//        displacements are always encoded in the 32-bit form.
// CEP:ASSUMES Long mode (64-bit); no 16/32-bit modes are emitted.
// CEP:COST O(1) per instruction; O(|code|) total.
// CEP:EVIDENCE cargo test -p vortex-opt x64
//! x86-64 machine-code encoder.

use std::fmt;

/// Encoder failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// A conditional code is out of range.
    BadCondition {
        /// The offending condition.
        cc: u8,
    },
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncodeError::BadCondition { cc } => write!(f, "condition code {cc} out of range"),
        }
    }
}

/// General-purpose 64-bit registers (encoding order matters).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Gpr {
    /// RAX.
    Rax = 0,
    /// RCX.
    Rcx = 1,
    /// RDX.
    Rdx = 2,
    /// RBX.
    Rbx = 3,
    /// RSP.
    Rsp = 4,
    /// RBP.
    Rbp = 5,
    /// RSI.
    Rsi = 6,
    /// RDI.
    Rdi = 7,
    /// R8.
    R8 = 8,
    /// R9.
    R9 = 9,
    /// R10.
    R10 = 10,
    /// R11.
    R11 = 11,
    /// R12.
    R12 = 12,
    /// R13.
    R13 = 13,
    /// R14.
    R14 = 14,
    /// R15.
    R15 = 15,
}

impl Gpr {
    fn code(self) -> u8 {
        self as u8
    }

    fn high(self) -> bool {
        self.code() >= 8
    }
}

/// Arithmetic opcodes by ModRM /digit (ADD=0 .. CMP=7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Alu {
    /// ADD.
    Add = 0,
    /// OR.
    Or = 1,
    /// AND.
    And = 4,
    /// SUB.
    Sub = 5,
    /// XOR.
    Xor = 6,
    /// CMP.
    Cmp = 7,
}

impl Alu {
    /// The reg-reg opcode (dst <- dst op src) in the 32/64-bit form.
    fn op_rr(self) -> u8 {
        0x01 | ((self as u8) << 3)
    }

    /// The mem-reg opcode (dst <- dst op [mem]) in the 32/64-bit form.
    fn op_rm(self) -> u8 {
        0x03 | ((self as u8) << 3)
    }
}

/// Shift opcodes by ModRM /digit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Shift {
    /// SHL.
    Shl = 4,
    /// SHR (logical).
    Shr = 5,
    /// SAR (arithmetic).
    Sar = 7,
}

/// Condition codes for JCC (low nibble of 0F 8x).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Cond {
    /// Overflow.
    O = 0,
    /// Not overflow.
    No = 1,
    /// Below (unsigned <).
    B = 2,
    /// Above or equal.
    Ae = 3,
    /// Equal.
    E = 4,
    /// Not equal.
    Ne = 5,
    /// Below or equal.
    Be = 6,
    /// Above (unsigned >).
    A = 7,
    /// Sign.
    S = 8,
    /// Not sign.
    Ns = 9,
    /// Less (signed <).
    L = 12,
    /// Greater or equal.
    Ge = 13,
    /// Less or equal (signed <=).
    Le = 14,
    /// Greater (signed >).
    G = 15,
}

/// Relocation kinds carried next to the emitted bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelocKind {
    /// 32-bit PC-relative (branches, calls): value = target - (site + 4).
    Rel32,
    /// 32-bit displacement in a rip-relative memory operand.
    RipRel32,
}

/// One pending relocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reloc {
    /// Byte offset of the relocatable field inside the code buffer.
    pub at: usize,
    /// The kind of relocation.
    pub kind: RelocKind,
    /// Symbol the relocation refers to (loader-assigned id or, for
    /// intra-object branch fixups, an instruction index).
    pub sym: usize,
}

/// The byte emitter. Byte order is always [REX][opcode][ModRM][disp][imm].
// CEP:WHAT Accumulates encoded bytes and relocations.
// CEP:WHY All machine emission flows through one verified path.
// CEP:STATUS complete
// CEP:FAILURE EncodeError for malformed operands.
// CEP:ASSUMES Long mode.
// CEP:COST O(1) per instruction.
// CEP:EVIDENCE cargo test -p vortex-opt x64
#[derive(Debug, Default)]
pub struct Encoder {
    buf: Vec<u8>,
    relocs: Vec<Reloc>,
}

impl Encoder {
    /// Creates an empty encoder.
    // CEP:WHAT Initializes the buffer.
    // CEP:WHY Composition of encoders builds whole functions.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn new() -> Encoder {
        Encoder::default()
    }

    /// Consumes the encoder into raw bytes and relocations.
    // CEP:WHAT Finishes emission.
    // CEP:WHY Callers map or patch the bytes afterwards.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(|code|).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn into_parts(self) -> (Vec<u8>, Vec<Reloc>) {
        (self.buf, self.relocs)
    }

    /// Current byte length.
    // CEP:WHAT Reports the buffer size.
    // CEP:WHY Layouts compute instruction offsets.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Whether the buffer is empty.
    // CEP:WHAT Reports emptiness.
    // CEP:WHY Companion predicate for len (clippy convention).
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    // CEP:WHAT Builds the REX prefix byte when any bit is set.
    // CEP:WHY 64-bit width and high-register access need REX.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; plain 0x40 is suppressed.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    fn rex(&mut self, w: bool, r: bool, b: bool) {
        let mut byte = 0x40u8;
        if w {
            byte |= 0x8;
        }
        if r {
            byte |= 0x4;
        }
        if b {
            byte |= 0x1;
        }
        if byte != 0x40 {
            self.buf.push(byte);
        }
    }

    fn modrm(&mut self, mode: u8, reg: u8, rm: u8) {
        self.buf.push((mode << 6) | ((reg & 7) << 3) | (rm & 7));
    }

    fn imm32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn imm64(&mut self, v: i64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn rel32_slot(&mut self, kind: RelocKind, sym: usize) {
        let at = self.buf.len();
        self.relocs.push(Reloc { at, kind, sym });
        self.buf.extend_from_slice(&[0, 0, 0, 0]);
    }

    fn patch_reloc(&mut self, at: usize, value: i32) {
        self.buf[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    /// PUSH r64.
    // CEP:WHAT Encodes 50+r with REX.B for r8+.
    // CEP:WHY Prologues save callee-saved registers.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn push_r64(&mut self, r: Gpr) {
        self.rex(false, false, r.high());
        self.buf.push(0x50 | (r.code() & 7));
    }

    /// POP r64.
    // CEP:WHAT Encodes 58+r with REX.B for r8+.
    // CEP:WHY Epilogues restore callee-saved registers.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn pop_r64(&mut self, r: Gpr) {
        self.rex(false, false, r.high());
        self.buf.push(0x58 | (r.code() & 7));
    }

    /// MOV r64, r64 (dst <- src): REX.W 89 /r.
    // CEP:WHAT Encodes the register move.
    // CEP:WHY Register moves are the stencil fast path.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn mov_rr(&mut self, dst: Gpr, src: Gpr) {
        self.rex(true, src.high(), dst.high());
        self.buf.push(0x89);
        self.modrm(3, src.code(), dst.code());
    }

    /// MOV r64, imm64 (movabs): REX.W B8+r io.
    // CEP:WHAT Encodes a full 64-bit constant load.
    // CEP:WHY Constants wider than 32 bits need movabs.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn mov_ri64(&mut self, dst: Gpr, imm: i64) {
        self.rex(true, false, dst.high());
        self.buf.push(0xB8 | (dst.code() & 7));
        self.imm64(imm);
    }

    /// MOV r64, [rbp + disp32]: REX.W 8B /r, mod=10.
    // CEP:WHAT Encodes a frame-slot load.
    // CEP:WHY Frame slots hold the value word registers.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES disp fits 32 bits.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn mov_rm_disp(&mut self, dst: Gpr, disp: i32) {
        self.rex(true, dst.high(), false);
        self.buf.push(0x8B);
        self.modrm(2, dst.code(), Gpr::Rbp.code());
        self.imm32(disp);
    }

    /// MOV [rbp + disp32], r64: REX.W 89 /r, mod=10.
    // CEP:WHAT Encodes a frame-slot store.
    // CEP:WHY Results land back in frame slots.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES disp fits 32 bits.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn mov_disp_rm(&mut self, disp: i32, src: Gpr) {
        self.rex(true, src.high(), false);
        self.buf.push(0x89);
        self.modrm(2, src.code(), Gpr::Rbp.code());
        self.imm32(disp);
    }

    /// MOV [rbp + disp32], imm32: REX.W C7 /0, mod=10.
    // CEP:WHAT Encodes a sign-extended constant store.
    // CEP:WHY LoadConst stencils materialize constants in place.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES disp fits 32 bits.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn mov_disp_imm32(&mut self, disp: i32, imm: i32) {
        self.rex(true, false, false);
        self.buf.push(0xC7);
        self.modrm(2, 0, Gpr::Rbp.code());
        self.imm32(disp);
        self.imm32(imm);
    }

    /// MOV r64, [rip + disp32]: REX.W 8B /r, mod=00 rm=101.
    // CEP:WHAT Encodes a rip-relative load with a RipRel32 reloc.
    // CEP:WHY Safepoint flags and globals load rip-relative.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn mov_rip_rel(&mut self, dst: Gpr, disp_sym: usize) {
        self.rex(true, dst.high(), false);
        self.buf.push(0x8B);
        self.modrm(0, dst.code(), 5);
        self.rel32_slot(RelocKind::RipRel32, disp_sym);
    }

    /// ALU r64, r64 (dst op= src): REX.W <op> /r, mod=11.
    // CEP:WHAT Encodes a register-register ALU op.
    // CEP:WHY Pure arithmetic maps to one ALU instruction.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn alu_rr(&mut self, op: Alu, dst: Gpr, src: Gpr) {
        self.rex(true, src.high(), dst.high());
        self.buf.push(op.op_rr());
        self.modrm(3, src.code(), dst.code());
    }

    /// ALU r64, [rbp + disp32] (load-op): REX.W <op> /r, mod=10.
    // CEP:WHAT Encodes an ALU op with a frame-slot source.
    // CEP:WHY Two-operand stencils fold the load into the ALU op.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES disp fits 32 bits.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn alu_rm_disp(&mut self, op: Alu, dst: Gpr, disp: i32) {
        self.rex(true, dst.high(), false);
        self.buf.push(op.op_rm());
        self.modrm(2, dst.code(), Gpr::Rbp.code());
        self.imm32(disp);
    }

    /// ALU r64, imm32: REX.W 81 /digit, mod=11.
    // CEP:WHAT Encodes an immediate ALU op.
    // CEP:WHY Immediates fold into ALU stencils (AddImm et al).
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn alu_ri32(&mut self, op: Alu, dst: Gpr, imm: i32) {
        self.rex(true, false, dst.high());
        self.buf.push(0x81);
        self.modrm(3, op as u8, dst.code());
        self.imm32(imm);
    }

    /// SHIFT r64, imm8: REX.W C1 /digit, mod=11.
    // CEP:WHAT Encodes an immediate shift.
    // CEP:WHY Shift stencils carry immediate counts.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn shift_ri8(&mut self, op: Shift, dst: Gpr, imm: u8) {
        self.rex(true, false, dst.high());
        self.buf.push(0xC1);
        self.modrm(3, op as u8, dst.code());
        self.buf.push(imm);
    }

    /// NEG r64: REX.W F7 /3.
    // CEP:WHAT Encodes a negation.
    // CEP:WHY Neg stencils map to NEG.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn neg_r64(&mut self, dst: Gpr) {
        self.rex(true, false, dst.high());
        self.buf.push(0xF7);
        self.modrm(3, 3, dst.code());
    }

    /// NOT r64: REX.W F7 /2.
    // CEP:WHAT Encodes a bitwise complement.
    // CEP:WHY Not stencils map to NOT.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn not_r64(&mut self, dst: Gpr) {
        self.rex(true, false, dst.high());
        self.buf.push(0xF7);
        self.modrm(3, 2, dst.code());
    }

    /// TEST r64, r64: REX.W 85 /r.
    // CEP:WHAT Encodes a register test.
    // CEP:WHY Safepoint polls test the flag against itself.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn test_rr(&mut self, a: Gpr, b: Gpr) {
        self.rex(true, b.high(), a.high());
        self.buf.push(0x85);
        self.modrm(3, b.code(), a.code());
    }

    /// JMP rel32 to a symbol: E9 with a Rel32 reloc.
    // CEP:WHAT Encodes an unconditional branch.
    // CEP:WHY Branch stencils need inter-instruction targets.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn jmp_rel32(&mut self, sym: usize) {
        self.buf.push(0xE9);
        self.rel32_slot(RelocKind::Rel32, sym);
    }

    /// JCC rel32 to a symbol: 0F 8x with a Rel32 reloc.
    // CEP:WHAT Encodes a conditional branch.
    // CEP:WHY Conditional branches carry the condition code.
    // CEP:STATUS complete
    // CEP:FAILURE BadCondition for cc >= 16.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn jcc_rel32(&mut self, cc: Cond, sym: usize) -> Result<(), EncodeError> {
        let c = cc as u8;
        if c >= 16 {
            return Err(EncodeError::BadCondition { cc: c });
        }
        self.buf.push(0x0F);
        self.buf.push(0x80 | c);
        self.rel32_slot(RelocKind::Rel32, sym);
        Ok(())
    }

    /// CALL rel32 to a symbol: E8 with a Rel32 reloc.
    // CEP:WHAT Encodes a call.
    // CEP:WHY Calls target runtime entries or other methods.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn call_rel32(&mut self, sym: usize) {
        self.buf.push(0xE8);
        self.rel32_slot(RelocKind::Rel32, sym);
    }

    /// IMUL r64, r64: REX.W 0F AF /r (dst <- dst * src).
    // CEP:WHAT Encodes a two-operand integer multiply.
    // CEP:WHY Mul stencils map to IMUL.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn imul_rr(&mut self, dst: Gpr, src: Gpr) {
        self.rex(true, src.high(), dst.high());
        self.buf.push(0x0F);
        self.buf.push(0xAF);
        self.modrm(3, src.code(), dst.code());
    }

    /// CQO: REX.W 99 (sign-extend RAX into RDX:RAX).
    // CEP:WHAT Encodes the divide sign extension.
    // CEP:WHY IDIV needs RDX:RAX set up.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn cqo(&mut self) {
        self.rex(true, false, false);
        self.buf.push(0x99);
    }

    /// IDIV r/m64 at [rbp + disp32]: REX.W F7 /7, mod=10.
    // CEP:WHAT Encodes a signed divide by a frame slot.
    // CEP:WHY Div/Rem stencils divide through frame slots.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES RDX:RAX already extended; divide-by-zero traps.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn idiv_rm_disp(&mut self, disp: i32) {
        self.rex(true, false, false);
        self.buf.push(0xF7);
        self.modrm(2, 7, Gpr::Rbp.code());
        self.imm32(disp);
    }

    /// SETcc AL + MOVZX r64, AL for the given condition.
    // CEP:WHAT Materializes a boolean comparison result in r64.
    // CEP:WHY Cmp stencils produce value-word booleans.
    // CEP:STATUS complete
    // CEP:FAILURE BadCondition for cc >= 16.
    // CEP:ASSUMES Flags are live from the preceding compare.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn set_bool(&mut self, cc: Cond, dst: Gpr) -> Result<(), EncodeError> {
        let c = cc as u8;
        if c >= 16 {
            return Err(EncodeError::BadCondition { cc: c });
        }
        // SETcc AL: 0F 9x C0.
        self.buf.push(0x0F);
        self.buf.push(0x90 | c);
        self.buf.push(0xC0);
        // MOVZX r64, AL: REX.W 0F B6 /r.
        self.rex(true, false, dst.high());
        self.buf.push(0x0F);
        self.buf.push(0xB6);
        self.modrm(3, dst.code(), Gpr::Rax.code());
        Ok(())
    }

    /// RET: C3.
    // CEP:WHAT Encodes the return.
    // CEP:WHY Epilogues end in RET.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn ret(&mut self) {
        self.buf.push(0xC3);
    }

    /// The GC safepoint poll macro (GC Interaction 6.3): load the global
    /// flag rip-relative, test it, branch to the slow path when set.
    // CEP:WHAT Emits mov/test/jne against the safepoint word.
    // CEP:WHY Compiled code must poll for GC and deopt safepoints.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES The loader resolves SAFEPOINT_WORD_SYM and the slow sym.
    // CEP:COST O(1); 16 bytes.
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn safepoint_poll(&mut self, slow_path_sym: usize) {
        self.mov_rip_rel(Gpr::Rax, SAFEPOINT_WORD_SYM);
        self.test_rr(Gpr::Rax, Gpr::Rax);
        let _ = self.jcc_rel32(Cond::Ne, slow_path_sym);
    }

    /// Resolves Rel32 relocations selected by `should_resolve`, leaving
    /// the rest (e.g., external callee symbols) for the loader.
    // CEP:WHAT Patches only intra-object branch fields.
    // CEP:WHY Call relocations target other objects, not layout offsets.
    // CEP:STATUS complete
    // CEP:FAILURE Returns the first selected-but-unresolvable symbol.
    // CEP:ASSUMES targets covers every selected sym.
    // CEP:COST O(relocs).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn resolve_rel32_if(
        &mut self,
        should_resolve: &dyn Fn(usize) -> bool,
        targets: &dyn Fn(usize) -> Option<usize>,
    ) -> Option<usize> {
        for r in self.relocs.clone() {
            if r.kind == RelocKind::Rel32 && should_resolve(r.sym) {
                let Some(t) = targets(r.sym) else {
                    return Some(r.sym);
                };
                let rel = (t as i64) - ((r.at + 4) as i64);
                self.patch_reloc(r.at, rel as i32);
            }
        }
        None
    }

    /// Resolves all Rel32 relocations given per-symbol byte offsets.
    // CEP:WHAT Patches rel32 fields in place.
    // CEP:WHY Intra-object branch targets are known only after layout.
    // CEP:STATUS complete
    // CEP:FAILURE Returns the first unresolvable symbol.
    // CEP:ASSUMES targets covers every referenced sym.
    // CEP:COST O(relocs).
    // CEP:EVIDENCE cargo test -p vortex-opt x64
    pub fn resolve_rel32(&mut self, targets: &dyn Fn(usize) -> Option<usize>) -> Option<usize> {
        for r in self.relocs.clone() {
            if r.kind == RelocKind::Rel32 {
                let Some(t) = targets(r.sym) else {
                    return Some(r.sym);
                };
                let rel = (t as i64) - ((r.at + 4) as i64);
                self.patch_reloc(r.at, rel as i32);
            }
        }
        None
    }
}

/// Loader-assigned symbol id for the global safepoint word.
pub const SAFEPOINT_WORD_SYM: usize = 0;

/// The slow-path symbol id convention for safepoint polls.
pub const SAFEPOINT_SLOW_SYM: usize = 1;
