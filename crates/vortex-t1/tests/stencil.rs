// CEP:FILE crates/vortex-t1/tests/stencil.rs
// CEP:WHAT Machine-code stencil tests: pinned byte sequences for the
//        prologue, load/add stencils, epilogue, poll density at backward
//        branches and calls, and rel32 label resolution.
// CEP:WHY Copy-and-patch is only real if the bytes are pinned.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE A byte mismatch aborts the test binary.
// CEP:ASSUMES Frame slot k lives at [rbp - 8*(k+1)].
// CEP:COST O(cases).
// CEP:EVIDENCE cargo test -p vortex-t1 --test stencil

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! T1 machine-code stencil test suite.

use vortex_t1::plan::{CodeObject, PatchedInstr};
use vortex_t1::{emit_machine_code, MachineCode};
use vortex_ugb::{CapReq, CmpKind, Op, Operand};

/// Builds a hand-planned code object.
// CEP:WHAT Constructs patched instruction lists directly.
// CEP:WHY Tests control operand shapes exactly.
// CEP:STATUS complete
// CEP:FAILURE Panics on invalid construction.
// CEP:ASSUMES Labels hold instruction indices as planning produces.
// CEP:COST O(instrs).
// CEP:EVIDENCE cargo test -p vortex-t1 --test stencil
fn co(method: u32, instrs: Vec<PatchedInstr>) -> CodeObject {
    CodeObject {
        method,
        instrs,
        caps: Vec::<CapReq>::new(),
        reg_count: 4,
        local_count: 0,
    }
}

/// One planned instruction shorthand.
// CEP:WHAT Wraps PatchedInstr construction.
// CEP:WHY Test tables stay readable.
// CEP:STATUS complete
// CEP:FAILURE Panics on invalid construction.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t1 --test stencil
fn i(op: Op, operands: Vec<Operand>) -> PatchedInstr {
    PatchedInstr { op, operands }
}

#[test]
// CEP:WHAT LoadConst/Add/Return stencils emit pinned frame-slot bytes.
// CEP:WHY The whole straight-line body is byte-pinned here.
// CEP:STATUS complete
// CEP:FAILURE A byte mismatch aborts the test binary.
// CEP:ASSUMES slot(k) = -8*(k+1).
// CEP:COST O(code).
// CEP:EVIDENCE cargo test -p vortex-t1 --test stencil
fn straight_line_bytes() {
    let c = co(
        3,
        vec![
            i(Op::LoadConst, vec![Operand::Reg(0), Operand::Imm32(7)]),
            i(Op::LoadConst, vec![Operand::Reg(1), Operand::Imm32(5)]),
            i(Op::Add, vec![Operand::Reg(0), Operand::Reg(1)]),
            i(Op::Return, vec![]),
        ],
    );
    let mc: MachineCode = emit_machine_code(&c);
    let parts: [&[u8]; 8] = [
        &[0x55, 0x48, 0x89, 0xE5], // prologue: push rbp; mov rbp, rsp
        &[
            0x48, 0xC7, 0x85, 0xF8, 0xFF, 0xFF, 0xFF, 0x07, 0x00, 0x00, 0x00,
        ], // [rbp-8] = 7
        &[
            0x48, 0xC7, 0x85, 0xF0, 0xFF, 0xFF, 0xFF, 0x05, 0x00, 0x00, 0x00,
        ], // [rbp-16] = 5
        &[0x48, 0x8B, 0x85, 0xF8, 0xFF, 0xFF, 0xFF], // mov rax, [rbp-8]
        &[0x48, 0x03, 0x85, 0xF0, 0xFF, 0xFF, 0xFF], // add rax, [rbp-16]
        &[0x48, 0x89, 0x85, 0xF8, 0xFF, 0xFF, 0xFF], // mov [rbp-8], rax
        &[0xC3],                   // ret
        &[0x48, 0x89, 0xEC, 0x5D, 0xC3], // leave-style epilogue + ret
    ];
    let expected: Vec<u8> = parts.concat();
    assert_eq!(mc.bytes, expected, "stencil bytes must be exact");
    assert_eq!(mc.poll_count, 0, "no branches or calls: no polls");
    assert_eq!(mc.instr_offsets, vec![4, 15, 26, 47]);
}

#[test]
// CEP:WHAT Backward branches arm a safepoint poll and resolve to the
//        target instruction's byte offset.
// CEP:WHY Loops are the primary poll sites (GC Interaction 6.3).
// CEP:STATUS complete
// CEP:FAILURE A byte mismatch aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(code).
// CEP:EVIDENCE cargo test -p vortex-t1 --test stencil
fn backward_branch_polls_and_resolves() {
    // A two-instruction loop: body (0), JumpIfFalse r0 -> 0 (1).
    let c = co(
        1,
        vec![
            i(Op::LoadConst, vec![Operand::Reg(0), Operand::Imm32(3)]),
            i(Op::JumpIfFalse, vec![Operand::Reg(0), Operand::Label(0)]),
        ],
    );
    let mc = emit_machine_code(&c);
    assert_eq!(mc.poll_count, 1, "the backward branch must poll");
    // Poll = 16 bytes: 48 8B 05 .. 48 85 C0 0F 85 ..
    let off = mc.instr_offsets[1];
    let poll = &mc.bytes[off..off + 16];
    assert_eq!(&poll[0..3], &[0x48, 0x8B, 0x05]);
    assert_eq!(&poll[7..10], &[0x48, 0x85, 0xC0]);
    assert_eq!(&poll[10..12], &[0x0F, 0x85]);
    // Branch: load cond, test, jz rel32 -> instruction 0.
    let br = &mc.bytes[off + 16..];
    assert_eq!(&br[0..7], &[0x48, 0x8B, 0x85, 0xF8, 0xFF, 0xFF, 0xFF]);
    assert_eq!(&br[7..10], &[0x48, 0x85, 0xC0]);
    assert_eq!(&br[10..12], &[0x0F, 0x84]);
    let rel = i32::from_le_bytes([br[12], br[13], br[14], br[15]]);
    assert_eq!(
        (off + 16 + 12 + 4) as i64 + rel as i64,
        mc.instr_offsets[0] as i64,
        "rel32 must point at instruction 0"
    );
}

#[test]
// CEP:WHAT Calls carry the callee symbol and poll; results store to dst.
// CEP:WHY Call sites are poll sites; the RAX convention needs a store.
// CEP:STATUS complete
// CEP:FAILURE A byte mismatch aborts the test binary.
// CEP:ASSUMES callee index lives at operand 1.
// CEP:COST O(code).
// CEP:EVIDENCE cargo test -p vortex-t1 --test stencil
fn call_arms_poll_and_stores_result() {
    let c = co(
        2,
        vec![i(Op::CallDirect, vec![Operand::Reg(0), Operand::Imm32(9)])],
    );
    let mc = emit_machine_code(&c);
    assert_eq!(mc.poll_count, 1);
    let off = mc.instr_offsets[0];
    let body = &mc.bytes[off..];
    assert_eq!(body[16], 0xE8, "call rel32 after the 16-byte poll");
    // Callee symbol 9 lands in the reloc table.
    let has_call_reloc = mc
        .relocs
        .iter()
        .any(|r| r.kind == vortex_opt::x64::RelocKind::Rel32 && r.sym == 9);
    assert!(has_call_reloc, "callee 9 must appear as a Rel32 reloc");
    // Result store: mov [rbp-8], rax right after the call.
    assert_eq!(&body[21..28], &[0x48, 0x89, 0x85, 0xF8, 0xFF, 0xFF, 0xFF]);
}

#[test]
// CEP:WHAT Cmp maps the selector byte to the right condition code.
// CEP:WHY CmpKind must drive setcc, not a fixed predicate.
// CEP:STATUS complete
// CEP:FAILURE A byte mismatch aborts the test binary.
// CEP:ASSUMES UGB signature (dst, lhs, rhs, kind).
// CEP:COST O(code).
// CEP:EVIDENCE cargo test -p vortex-t1 --test stencil
fn cmp_selector_drives_condition() {
    let c = co(
        4,
        vec![i(
            Op::Cmp,
            vec![
                Operand::Reg(2),
                Operand::Reg(0),
                Operand::Reg(1),
                Operand::CmpKind(CmpKind::Lt),
            ],
        )],
    );
    let mc = emit_machine_code(&c);
    let off = mc.instr_offsets[0];
    let body = &mc.bytes[off..];
    // mov rax, [rbp-8]; cmp rax, [rbp-16]; setl al; movzx rax, al; store.
    assert_eq!(&body[0..7], &[0x48, 0x8B, 0x85, 0xF8, 0xFF, 0xFF, 0xFF]);
    assert_eq!(&body[7..14], &[0x48, 0x3B, 0x85, 0xF0, 0xFF, 0xFF, 0xFF]);
    assert_eq!(&body[14..17], &[0x0F, 0x9C, 0xC0], "setl al");
    assert_eq!(&body[17..21], &[0x48, 0x0F, 0xB6, 0xC0], "movzx rax, al");
}

#[test]
// CEP:WHAT Wide constants go through movabs instead of the imm32 form.
// CEP:WHY Immediates beyond 32 bits must not silently truncate.
// CEP:STATUS complete
// CEP:FAILURE A byte mismatch aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(code).
// CEP:EVIDENCE cargo test -p vortex-t1 --test stencil
fn wide_consts_use_movabs() {
    let c = co(
        5,
        vec![i(
            Op::LoadConst,
            vec![Operand::Reg(0), Operand::Imm64(0x1_0000_0000)],
        )],
    );
    let mc = emit_machine_code(&c);
    let off = mc.instr_offsets[0];
    let body = &mc.bytes[off..];
    assert_eq!(&body[0..2], &[0x48, 0xB8], "movabs rax");
    let imm = i64::from_le_bytes(body[2..10].try_into().expect("imm"));
    assert_eq!(imm, 0x1_0000_0000);
    assert_eq!(&body[10..17], &[0x48, 0x89, 0x85, 0xF8, 0xFF, 0xFF, 0xFF]);
}

#[test]
// CEP:WHAT instr_offsets are strictly increasing and in bounds.
// CEP:WHY The loader maps offsets to patched instructions.
// CEP:STATUS complete
// CEP:FAILURE A failed assert aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(instrs).
// CEP:EVIDENCE cargo test -p vortex-t1 --test stencil
fn layout_is_monotonic() {
    let c = co(
        6,
        vec![
            i(Op::LoadConst, vec![Operand::Reg(0), Operand::Imm32(1)]),
            i(Op::Mov, vec![Operand::Reg(1), Operand::Reg(0)]),
            i(Op::Return, vec![]),
        ],
    );
    let mc = emit_machine_code(&c);
    for w in mc.instr_offsets.windows(2) {
        assert!(w[0] < w[1]);
    }
    assert!(mc.instr_offsets[0] >= 4, "prologue precedes instructions");
    assert!(*mc.instr_offsets.last().expect("last") < mc.bytes.len());
}
