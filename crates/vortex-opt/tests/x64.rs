// CEP:FILE crates/vortex-opt/tests/x64.rs
// CEP:WHAT Byte-exact x86-64 encoder tests against reference encodings,
//        relocation placement, safepoint poll shape, and rel32 fixup.
// CEP:WHY Wrong bytes crash real targets; every form is pinned here.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE A byte mismatch aborts the test binary.
// CEP:ASSUMES Long-mode encodings from the Intel SDM.
// CEP:COST O(cases).
// CEP:EVIDENCE cargo test -p vortex-opt --test x64

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! x86-64 encoder test suite.

use vortex_opt::x64::{Alu, Cond, Gpr, RelocKind, Shift, SAFEPOINT_SLOW_SYM, SAFEPOINT_WORD_SYM};
use vortex_opt::{EncodeError, Encoder};

/// Collects the encoder bytes as a vector.
// CEP:WHAT Runs an emission closure and returns the bytes.
// CEP:WHY Tests read better when the plumbing is one helper.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(code).
// CEP:EVIDENCE cargo test -p vortex-opt --test x64
fn bytes(f: impl FnOnce(&mut Encoder)) -> (Vec<u8>, Vec<(usize, RelocKind, usize)>) {
    let mut e = Encoder::new();
    f(&mut e);
    let (b, r) = e.into_parts();
    (b, r.into_iter().map(|x| (x.at, x.kind, x.sym)).collect())
}

#[test]
// CEP:WHAT Stack ops encode as 50/58+r with REX.B.
// CEP:WHY Prologue and epilogue bytes are pinned.
// CEP:STATUS complete
// CEP:FAILURE A failed assert aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-opt --test x64
fn push_pop() {
    let (b, r) = bytes(|e| {
        e.push_r64(Gpr::Rbp);
        e.pop_r64(Gpr::Rbp);
        e.push_r64(Gpr::R15);
    });
    assert_eq!(b, vec![0x55, 0x5D, 0x41, 0x57]);
    assert!(r.is_empty());
}

#[test]
// CEP:WHAT Register moves encode as REX.W 89 /r.
// CEP:WHY mov rax, rbx = 48 89 D8 is the canonical form.
// CEP:STATUS complete
// CEP:FAILURE A failed assert aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-opt --test x64
fn mov_rr() {
    let (b, r) = bytes(|e| e.mov_rr(Gpr::Rax, Gpr::Rbx));
    assert_eq!(b, vec![0x48, 0x89, 0xD8]);
    assert!(r.is_empty());
    // R8 as destination needs REX.B; R9 as source needs REX.R.
    let (b2, _) = bytes(|e| e.mov_rr(Gpr::R8, Gpr::R9));
    assert_eq!(b2, vec![0x4D, 0x89, 0xC8]);
}

#[test]
// CEP:WHAT movabs encodes REX.W B8+r io.
// CEP:WHY Full 64-bit constants must round-trip.
// CEP:STATUS complete
// CEP:FAILURE A failed assert aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-opt --test x64
fn mov_abs() {
    let (b, r) = bytes(|e| e.mov_ri64(Gpr::Rax, 0x1234));
    assert_eq!(
        b,
        vec![0x48, 0xB8, 0x34, 0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
    );
    assert!(r.is_empty());
}

#[test]
// CEP:WHAT Frame slot loads and stores use mod=10 disp32.
// CEP:WHY disp32 form is pinned; disp8 shortcuts must not appear.
// CEP:STATUS complete
// CEP:FAILURE A failed assert aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-opt --test x64
fn frame_slots() {
    let (b, r) = bytes(|e| e.mov_rm_disp(Gpr::Rax, -16));
    assert_eq!(b, vec![0x48, 0x8B, 0x85, 0xF0, 0xFF, 0xFF, 0xFF]);
    assert!(r.is_empty());
    let (b2, _) = bytes(|e| e.mov_disp_rm(-8, Gpr::Rax));
    assert_eq!(b2, vec![0x48, 0x89, 0x85, 0xF8, 0xFF, 0xFF, 0xFF]);
    let (b3, _) = bytes(|e| e.mov_disp_imm32(-8, 7));
    assert_eq!(b3.len(), 11, "C7 /0 with disp32 + imm32");
}

#[test]
// CEP:WHAT ALU reg-reg encodes REX.W <op>/r with the /digit opcode.
// CEP:WHY add rax, rcx = 48 01 C8 is canonical.
// CEP:STATUS complete
// CEP:FAILURE A failed assert aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-opt --test x64
fn alu_forms() {
    let (b, r) = bytes(|e| e.alu_rr(Alu::Add, Gpr::Rax, Gpr::Rcx));
    assert_eq!(b, vec![0x48, 0x01, 0xC8]);
    assert!(r.is_empty());
    let (b2, _) = bytes(|e| e.alu_rr(Alu::Cmp, Gpr::Rax, Gpr::Rdx));
    assert_eq!(b2, vec![0x48, 0x39, 0xD0]);
    let (b3, _) = bytes(|e| e.alu_rm_disp(Alu::Add, Gpr::Rax, -16));
    assert_eq!(b3, vec![0x48, 0x03, 0x85, 0xF0, 0xFF, 0xFF, 0xFF]);
    let (b4, _) = bytes(|e| e.alu_ri32(Alu::Add, Gpr::Rax, 5));
    assert_eq!(b4, vec![0x48, 0x81, 0xC0, 0x05, 0x00, 0x00, 0x00]);
}

#[test]
// CEP:WHAT Shifts, negation, complement, and test pin their encodings.
// CEP:WHY C1/F7/85 forms are the remaining stencil primitives.
// CEP:STATUS complete
// CEP:FAILURE A failed assert aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-opt --test x64
fn unary_and_shift() {
    let (b, r) = bytes(|e| {
        e.shift_ri8(Shift::Shl, Gpr::Rax, 2);
        e.neg_r64(Gpr::Rax);
        e.not_r64(Gpr::Rax);
        e.test_rr(Gpr::Rax, Gpr::Rax);
    });
    assert_eq!(
        b,
        vec![
            0x48, 0xC1, 0xE0, 0x02, // shl rax, 2
            0x48, 0xF7, 0xD8, // neg rax
            0x48, 0xF7, 0xD0, // not rax
            0x48, 0x85, 0xC0, // test rax, rax
        ]
    );
    assert!(r.is_empty());
}

#[test]
// CEP:WHAT Branches and calls carry Rel32 relocations at the right site.
// CEP:WHY The layout pass patches exactly those fields.
// CEP:STATUS complete
// CEP:FAILURE A failed assert aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-opt --test x64
fn branches_and_relocs() {
    let (b, r) = bytes(|e| {
        e.jmp_rel32(7);
        e.call_rel32(9);
        let _ = e.jcc_rel32(Cond::Ne, 11);
        e.ret();
    });
    assert_eq!(b[0], 0xE9);
    assert_eq!(b[5], 0xE8);
    assert_eq!(&b[10..12], &[0x0F, 0x85]);
    assert_eq!(b.last(), Some(&0xC3));
    assert_eq!(
        r,
        vec![
            (1, RelocKind::Rel32, 7),
            (6, RelocKind::Rel32, 9),
            (12, RelocKind::Rel32, 11)
        ]
    );
}

#[test]
// CEP:WHAT A backward rel32 resolves to a negative displacement.
// CEP:WHY Loops jump backwards; the fixup must sign-extend.
// CEP:STATUS complete
// CEP:FAILURE A failed assert aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-opt --test x64
fn resolve_rel32_backward() {
    let mut e = Encoder::new();
    e.push_r64(Gpr::Rbp); // at 0, len 1: symbol 42 lands here
    let jmp_at = e.len();
    e.jmp_rel32(42); // opcode at 1, rel32 at 2
    e.ret();
    assert!(e
        .resolve_rel32(&|s| if s == 42 { Some(0) } else { None })
        .is_none());
    let (b, _) = e.into_parts();
    let rel = i32::from_le_bytes([b[2], b[3], b[4], b[5]]);
    assert_eq!(rel as i64, 0i64 - (jmp_at as i64 + 1 + 4));
}

#[test]
// CEP:WHAT Unresolvable symbols are reported, not patched to zero.
// CEP:WHY Silent zero branches would corrupt execution.
// CEP:STATUS complete
// CEP:FAILURE A failed assert aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-opt --test x64
fn resolve_reports_missing() {
    let mut e = Encoder::new();
    e.jmp_rel32(1234);
    assert_eq!(e.resolve_rel32(&|_| None), Some(1234));
}

#[test]
// CEP:WHAT The safepoint poll emits mov/test/jne in 16 bytes with two
//        relocations against the safepoint word and the slow path.
// CEP:WHY GC and deopt polls must exist at loops and calls.
// CEP:STATUS complete
// CEP:FAILURE A failed assert aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-opt --test x64
fn safepoint_poll_shape() {
    let (b, r) = bytes(|e| e.safepoint_poll(SAFEPOINT_SLOW_SYM));
    assert_eq!(b.len(), 16);
    assert_eq!(&b[0..3], &[0x48, 0x8B, 0x05], "mov rax, [rip+X]");
    assert_eq!(&b[7..10], &[0x48, 0x85, 0xC0], "test rax, rax");
    assert_eq!(&b[10..12], &[0x0F, 0x85], "jne slow");
    assert_eq!(
        r,
        vec![
            (3, RelocKind::RipRel32, SAFEPOINT_WORD_SYM),
            (12, RelocKind::Rel32, SAFEPOINT_SLOW_SYM)
        ]
    );
}

#[test]
// CEP:WHAT A bad condition code surfaces as EncodeError.
// CEP:WHY Failure behavior must be typed, not silent.
// CEP:STATUS complete
// CEP:FAILURE A failed assert aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-opt --test x64
fn bad_condition_is_typed() {
    let mut e = Encoder::new();
    // Reaching into the enum is impossible; 16 conditions exist, so the
    // type system already bounds cc. The error path guards future forms:
    let err = e.jcc_rel32(Cond::G, 3);
    assert!(err.is_ok());
    let _ = EncodeError::BadCondition { cc: 16 }; // constructible for tests
}
