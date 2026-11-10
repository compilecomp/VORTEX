//! UGB (Universal Guest Bytecode): opcodes, codec, assembler, verifier.
//! See design.md UGB-1 for the specification this subset implements.
// CEP:FILE crates/vortex-ugb/src/lib.rs
// CEP:WHAT UGB (Universal Guest Bytecode) crate root: opcode set, hybrid
//        variable-length codec, assembler, disassembler, static verifier, and
//        method metadata model, plus the `countdown` demo builder.
// CEP:WHY UGB is the stable execution contract between language frontends and
//        the tiers of the engine (design.md UGB-1).
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE See per-module docs; all errors are typed, none are swallowed.
// CEP:ASSUMES Opcode bytes follow UGB-1 section 5 for the implemented subset.
// CEP:COST Library crate; hot paths are the codec and the verifier, both O(n).
// CEP:EVIDENCE cargo test -p vortex-ugb

/// Assembler with branch patching and metadata site binding.
pub mod asm;
/// Canonical disassembler.
pub mod disasm;
/// Hybrid variable-length codec.
pub mod instr;
/// Method, constant pool, and metadata slots.
pub mod method;
/// Opcode set and operand signatures.
pub mod op;
/// Static verifier (UGB-INV-02/03/04).
pub mod verify;

pub use asm::{mk, Asm, JumpPatch};
pub use instr::{DecodeError, EncodeError, Instr, EXTENDED_PREFIX};
pub use method::{Const, EpochCache, LoopSlot, Method};
pub use op::{sig, Op, Sig};
pub use verify::{verify, Violation};

/// Builds the `countdown` demo method: `r0 = n; r1 = 0; while r0 != 0
/// { r1 += r0; r0 -= 1; safepoint; } return r1`.
///
/// Exercises the sampled back-edge counter path (PROF-1 section 3.2) and the
/// verifier's loop/branch checks. Registers: r0 counter, r1 accumulator,
/// r2/r3 reserved scratch.
// CEP:WHAT Builds the countdown demo method: r0 = n; r1 = 0; while r0 != 0 { r1 += r0; r0 -= 1; safepoint; } return r1.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-ugb
#[must_use]
pub fn countdown_method(n: i32) -> Method {
    let mut a = Asm::new("countdown", 1, 4);
    let n_const = a.push_const(Const::Int(i64::from(n)));

    // r0 = n ; r1 = 0 ; r2 = 1
    let mut ld = mk(Op::LoadConst, &[0]);
    ld.pool = n_const;
    a.emit(&ld);
    let mut zero = mk(Op::LoadInt32, &[1]);
    zero.imm32 = 0;
    a.emit(&zero);
    let mut one = mk(Op::LoadInt32, &[2]);
    one.imm32 = 1;
    a.emit(&one);

    let loop_head = a.method().code.len();
    let slot = a.bind_loop_slot();
    debug_assert_eq!(slot, 0, "demo method has exactly one loop");

    // while r0 != 0: exit when r0 is falsy.
    let mut jf = mk(Op::JumpFalse, &[0]);
    jf.branches[0] = 0;
    let jf_patch = a.emit(&jf).expect("JumpFalse yields a patch handle");

    // body: r1 = r1 + r0 ; r0 = r0 - r2 ; safepoint
    a.emit(&mk(Op::Add, &[1, 1, 0]));
    a.emit(&mk(Op::Sub, &[0, 0, 2]));
    a.emit(&mk(Op::Safepoint, &[]));
    a.emit_back_branch(Op::Jump, 0, loop_head);

    a.patch(jf_patch);
    a.emit(&mk(Op::Return, &[1]));
    a.finish()
}

/// Builds a method demonstrating the capability epoch protocol:
/// create a capability, check it in a loop (fast path), and return.
///
/// Registers: r0 capability handle, r1 loop counter, r2 result.
// CEP:WHAT Builds a method demonstrating the capability epoch protocol: create a capability, check it in a loop (fast path), and return.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-ugb
#[must_use]
pub fn capability_loop_method(auth: u32) -> Method {
    let mut a = Asm::new("capability_loop", 0, 4);
    let iterations = a.push_const(Const::Int(3));

    // r0 = Capability { auth, handle = 7 }
    let mut cc = mk(Op::CapCreate, &[0]);
    cc.imm32u = [auth, 7];
    a.emit(&cc);
    // r1 = 3 (loop bound)
    let mut li = mk(Op::LoadConst, &[1]);
    li.pool = iterations;
    a.emit(&li);

    let loop_head = a.method().code.len();
    let _slot = a.bind_loop_slot();

    // CAP_CHECK r0, req_auth = auth (site 0 -> epoch cache 0)
    let mut chk = mk(Op::CapCheck, &[0]);
    chk.imm32u = [auth, 0];
    let _ = a.bind_epoch_cache();
    a.emit(&chk);

    // r1 = r1 - 1; loop while r1 truthy
    a.emit(&mk(Op::Sub, &[1, 1, 1]));
    let mut jf = mk(Op::JumpFalse, &[1]);
    jf.branches[0] = 0;
    let jf_patch = a.emit(&jf).expect("JumpFalse yields a patch handle");
    a.emit_back_branch(Op::Jump, 0, loop_head);

    a.patch(jf_patch);
    a.emit(&mk(Op::Return, &[0]));
    a.finish()
}

/// Builds a method demonstrating tagged-array indexing with a load barrier
/// surface: allocate an array, store a value, load it back into r0.
///
/// The interpreter executes the load through the BARRIER-1-corrected barrier.
// CEP:WHAT Builds a method demonstrating tagged-array indexing with a load barrier surface: allocate an array, store a value, load it back into r0.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-ugb
#[must_use]
pub fn array_roundtrip_method() -> Method {
    let mut a = Asm::new("array_roundtrip", 0, 4);
    let v = a.push_const(Const::Int(41));

    // r0 = Array(4)
    let mut alloc = mk(Op::AllocArray, &[0]);
    alloc.imm32u = [4, 0];
    a.emit(&alloc);
    // r1 = 41 ; r2 = 0
    let mut li = mk(Op::LoadConst, &[1]);
    li.pool = v;
    a.emit(&li);
    let mut zero = mk(Op::LoadInt32, &[2]);
    zero.imm32 = 0;
    a.emit(&zero);
    // r0[0] = r1
    a.emit(&mk(Op::StoreIndex, &[0, 2, 1]));
    // r3 = r0[0]
    a.emit(&mk(Op::LoadIndex, &[3, 0, 2]));
    a.emit(&mk(Op::Return, &[3]));
    a.finish()
}
