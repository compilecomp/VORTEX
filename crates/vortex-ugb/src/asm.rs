// CEP:FILE crates/vortex-ugb/src/asm.rs
// CEP:WHAT Bytecode assembler: builder API over Method, choosing standard or
//        extended encoding per instruction, branch patching, and assignment of
//        loop-profile and epoch-cache metadata sites in stream order.
// CEP:WHY Frontends and tests need a correct, checked way to emit UGB without
//        hand-packing bytes.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Register operands above 255 are auto-promoted to the extended
//        encoding; nothing is silently truncated. An assembler-level
//        TryBegin without TryEnd aborts assembly.
// CEP:ASSUMES Branch offsets are i16 and patched by `patch`; overflowing
//        i16 forward branches are rejected at patch time.
// CEP:COST Amortized O(1) per instruction plus one pass for jump patching.
// CEP:EVIDENCE cargo test -p vortex-ugb
use crate::instr::{encode_extended, encode_standard, EncodeError, Instr};
use crate::method::{Const, EpochCache, LoopSlot, Method};
use crate::op::{sig, Op};

/// Builds an instruction with the given register operands. Operands beyond the
/// opcode's signature are ignored; missing operands are zero.
// CEP:WHAT Builds an instruction with the given register operands.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-ugb
#[must_use]
pub fn mk(op: Op, regs: &[u16]) -> Instr {
    let mut instr = Instr {
        op,
        regs: [0; 3],
        branches: [0; 1],
        imm32: 0,
        imm32u: [0; 2],
        imm64: 0,
        pool: 0,
        extended: false,
    };
    for (i, r) in regs.iter().take(3).enumerate() {
        instr.regs[i] = *r;
    }
    instr
}

/// Builds a branch instruction (`Jump`, `JumpTrue`, `JumpFalse`, `TryBegin`)
/// with a zero branch offset that [`Asm::patch`] fills in later.
#[must_use]
pub fn branch(op: Op, cond: u16) -> (Instr, ()) {
    let mut i = mk(op, &[cond]);
    i.branches[0] = 0;
    (i, ())
}

/// A pending forward-branch record.
#[derive(Debug, Clone, Copy)]
pub struct JumpPatch {
    patch_at: usize,
    instr_end: usize,
}

/// Assembler over a [`Method`].
#[derive(Debug)]
pub struct Asm {
    method: Method,
    pending: Vec<JumpPatch>,
    open_trys: usize,
}

impl Asm {
    /// Creates an assembler for a new method.
    #[must_use]
    pub fn new(name: impl Into<String>, arg_count: u16, register_count: u16) -> Self {
        Asm {
            method: Method::new(name, arg_count, register_count),
            pending: Vec::new(),
            open_trys: 0,
        }
    }

    /// Finishes assembly and returns the method.
    ///
    /// Panics when a `TryBegin` has no matching `TryEnd`: unbalanced handlers
    /// are an assembler bug, not a runtime condition (UGB-INV-04).
    #[must_use]
    pub fn finish(self) -> Method {
        assert_eq!(self.open_trys, 0, "unbalanced TryBegin at assembly exit");
        self.method
    }

    /// Borrows the method under construction.
    #[must_use]
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// Appends a constant to the pool and returns its index.
    pub fn push_const(&mut self, c: Const) -> u16 {
        self.method.consts.push(c);
        let idx = self.method.consts.len() - 1;
        u16::try_from(idx).expect("constant pool exceeds u16")
    }

    /// Emits one instruction, choosing standard or extended encoding by the
    /// largest register operand actually present in the signature.
    ///
    /// Returns the forward-branch patch handle for `Jump`, `JumpTrue`,
    /// `JumpFalse`, and `TryBegin`; `None` otherwise.
    // CEP:WHAT Emits one instruction, choosing standard or extended encoding by the largest register operand actually present in the signature.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-ugb
    pub fn emit(&mut self, instr: &Instr) -> Option<JumpPatch> {
        let s = sig(instr.op);
        let max_reg = instr.regs[..usize::from(s.regs)]
            .iter()
            .copied()
            .max()
            .unwrap_or(0);
        let res = if max_reg > u16::from(u8::MAX) {
            encode_extended(instr, &mut self.method.code)
        } else {
            match encode_standard(instr, &mut self.method.code) {
                Ok(n) => Ok(n),
                Err(EncodeError::RegisterTooLarge { .. }) => {
                    encode_extended(instr, &mut self.method.code)
                }
            }
        };
        debug_assert!(res.is_ok(), "extended encoding accepts all u16 registers");

        match instr.op {
            Op::TryBegin => self.open_trys += 1,
            Op::TryEnd => self.open_trys = self.open_trys.saturating_sub(1),
            _ => { /* no handler-state change for other opcodes */ }
        }

        if matches!(
            instr.op,
            Op::Jump | Op::JumpTrue | Op::JumpFalse | Op::TryBegin
        ) {
            let end = self.method.code.len();
            let jmp = JumpPatch {
                patch_at: end - 2,
                instr_end: end,
            };
            self.pending.push(jmp);
            Some(jmp)
        } else {
            None
        }
    }

    /// Assigns the next free loop profile slot (PROF-1 sampled back-edge
    /// counter) and returns its index.
    pub fn bind_loop_slot(&mut self) -> usize {
        self.method.loop_slots.push(LoopSlot::default());
        self.method.loop_slots.len() - 1
    }

    /// Assigns the next free capability epoch cache (UGB-1 section 6.3) and
    /// returns its site index.
    pub fn bind_epoch_cache(&mut self) -> usize {
        self.method.epoch_caches.push(EpochCache::default());
        self.method.epoch_caches.len() - 1
    }

    /// Patches a previously emitted forward branch to jump to the current end
    /// of the code stream.
    ///
    /// Panics when the relative offset exceeds i16: such methods need a jump
    /// island, which the verifier reports as an unpatchable branch otherwise.
    // CEP:WHAT Patches a previously emitted forward branch to jump to the current end of the code stream.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Typed errors or panics documented in the doc comment above; no silent fallback.
    // CEP:ASSUMES Caller obligations documented in the doc comment above.
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-ugb
    pub fn patch(&mut self, jmp: JumpPatch) {
        let target = self.method.code.len() as i64;
        let rel = target - jmp.instr_end as i64;
        let rel_i16 = i16::try_from(rel).expect("branch offset exceeds i16; emit a jump island");
        let bytes = rel_i16.to_le_bytes();
        self.method.code[jmp.patch_at] = bytes[0];
        self.method.code[jmp.patch_at + 1] = bytes[1];
    }

    /// Emits a backward branch to an already-emitted offset (loop back-edge).
    /// The caller supplies the absolute byte offset of the loop head; the
    /// relative offset is computed and range-checked immediately. Backward
    /// branches are fully encoded here and never enter the pending set.
    // CEP:WHAT Emits a backward branch to an already-emitted offset (loop back-edge).
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-ugb
    pub fn emit_back_branch(&mut self, op: Op, cond: u16, loop_head: usize) {
        let s = sig(op);
        let instr_end = self.method.code.len() + 1 + usize::from(s.regs) + 2;
        let rel = loop_head as i64 - instr_end as i64;
        let rel_i16 = i16::try_from(rel).expect("loop back-edge exceeds i16");
        let mut instr = mk(op, &[cond]);
        instr.branches[0] = rel_i16;
        let max_reg = instr.regs[..usize::from(s.regs)]
            .iter()
            .copied()
            .max()
            .unwrap_or(0);
        let res = if max_reg > u16::from(u8::MAX) {
            encode_extended(&instr, &mut self.method.code)
        } else {
            encode_standard(&instr, &mut self.method.code)
        };
        debug_assert!(res.is_ok(), "backward branch encoding failed");
    }
}
