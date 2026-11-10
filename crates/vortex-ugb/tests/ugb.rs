// CEP:FILE crates/vortex-ugb/tests/ugb.rs
// CEP:WHAT Tests for the UGB codec, assembler, disassembler, and verifier:
//        roundtrips, extended encoding, and UGB-INV-02/03/04 enforcement.
// CEP:WHY The bytecode contract must reject malformed streams before
//        execution; these tests are the executable form of that requirement.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES None.
// CEP:COST O(bytes) per case.
// CEP:EVIDENCE cargo test -p vortex-ugb

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! UGB codec/assembler/verifier test suite.

use vortex_ugb::{disasm, instr, mk, verify, Asm, Const, Method, Op};

/// Every built-in demo method must decode, verify clean, and disassemble.
#[test]
fn demo_methods_verify_and_roundtrip() {
    for m in [
        vortex_ugb::countdown_method(5),
        vortex_ugb::capability_loop_method(0b101),
        vortex_ugb::array_roundtrip_method(),
    ] {
        assert!(verify(&m).is_empty(), "method {} failed verify", m.name);
        let text = disasm::disassemble(&m);
        assert!(text.contains(&m.name));
        assert!(m.instruction_count() > 0);
    }
}

/// Countdown has exactly one backward jump bound to one loop slot, and no
/// counter bytes inside the instruction stream (OSR-INV-06 stencil purity of
/// the bytecode form: counters live in Method metadata).
// CEP:WHAT Countdown has exactly one backward jump bound to one loop slot, and no counter bytes inside the instruction stream (OSR-INV-06 stencil purity of the bytecode form: counters live in Method metadata).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-ugb
#[test]
fn countdown_loop_metadata_is_out_of_stream() {
    let m = vortex_ugb::countdown_method(10);
    assert_eq!(m.loop_slots.len(), 1, "one loop slot");
    assert!(m.epoch_caches.is_empty());
    // The only backward branch is the Jump back-edge; its offset is negative.
    let mut back = 0;
    let mut p = 0;
    while p < m.code.len() {
        let (i, next) = instr::decode_at(&m.code, p).expect("decodable");
        if matches!(i.op, Op::Jump | Op::JumpTrue | Op::JumpFalse) && i.branches[0] < 0 {
            back += 1;
        }
        p = next;
    }
    assert_eq!(back, 1, "exactly one back-edge");
}

/// Standard/extended encoding roundtrip with high register numbers.
// CEP:WHAT Standard/extended encoding roundtrip with high register numbers.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-ugb
#[test]
fn extended_encoding_roundtrip() {
    let mut a = Asm::new("wide", 0, 300);
    // r290 = r291 + r292 (all > 255: forces extended encoding)
    a.emit(&mk(Op::Add, &[290, 291, 292]));
    a.emit(&mk(Op::Return, &[290]));
    let m = a.finish();

    let (i0, next) = instr::decode_at(&m.code, 0).expect("decodable");
    assert!(i0.extended, "wide registers must use the extended form");
    assert_eq!(i0.regs, [290, 291, 292]);
    assert_eq!(m.code[0], instr::EXTENDED_PREFIX);
    let (i1, end) = instr::decode_at(&m.code, next).expect("decodable");
    assert_eq!(i1.op, Op::Return);
    assert_eq!(end, m.code.len());
}

/// Truncated and unknown-byte streams are rejected with offsets (never guessed).
// CEP:WHAT Truncated and unknown-byte streams are rejected with offsets (never guessed).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-ugb
#[test]
fn decoder_rejects_malformed_streams() {
    assert_eq!(
        instr::decode_at(&[], 0).err().map(|e| e.cause),
        Some("empty stream")
    );
    let err = instr::decode_at(&[0x7D], 0).expect_err("unknown opcode");
    assert_eq!(err.cause, "unknown opcode");
    let err = instr::decode_at(&[0x20, 0x00, 0x01], 0).expect_err("truncated Add");
    assert_eq!(err.cause, "truncated regs");
}

/// The verifier catches: branch out of range, register overflow, and
/// unbalanced handlers.
// CEP:WHAT The verifier catches: branch out of range, register overflow, and unbalanced handlers.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-ugb
#[test]
fn verifier_enforces_ugb_invariants() {
    // Branch beyond the code.
    let mut a = Asm::new("bad_target", 0, 2);
    let mut j = mk(Op::Jump, &[]);
    j.branches[0] = 100;
    a.emit(&j);
    a.emit(&mk(Op::Return, &[0]));
    let v = verify(&a.finish());
    assert!(v.iter().any(|x| x.rule == "UGB-INV-03"));

    // Register beyond register_count.
    let mut a = Asm::new("bad_reg", 0, 2);
    a.emit(&mk(Op::Move, &[5, 1]));
    a.emit(&mk(Op::Return, &[0]));
    let v = verify(&a.finish());
    assert!(v.iter().any(|x| x.rule == "UGB-INV-02"));

    // Unbalanced TryBegin: hand-built (the assembler aborts on this case).
    let bad_try = Method {
        name: "bad_try".into(),
        arg_count: 0,
        register_count: 2,
        code: vec![0x71, 0x06, 0x00, 0x49, 0x00], // TryBegin +6; Return r0
        consts: Vec::new(),
        loop_slots: Vec::new(),
        epoch_caches: Vec::new(),
    };
    let v = verify(&bad_try);
    assert!(v.iter().any(|x| x.rule == "UGB-INV-04"));

    // Balanced try/catch verifies clean.
    let mut a = Asm::new("good_try", 0, 3);
    let mut tb = mk(Op::TryBegin, &[]);
    tb.branches[0] = 0;
    let tb_patch = a.emit(&tb).expect("TryBegin yields patch");
    a.emit(&mk(Op::Throw, &[0]));
    a.patch(tb_patch);
    a.emit(&mk(Op::Catch, &[1]));
    a.emit(&mk(Op::TryEnd, &[]));
    a.emit(&mk(Op::Return, &[1]));
    assert!(verify(&a.finish()).is_empty());
}

/// Constant pool roundtrip through LoadConst with an above-i48 value.
// CEP:WHAT Constant pool roundtrip through LoadConst with an above-i48 value.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-ugb
#[test]
fn const_pool_indices_survive() {
    // 2^47 is outside the i48 immediate domain and must live in the pool.
    const BIG: i64 = 140_737_488_355_328;
    let mut a = Asm::new("pool", 0, 2);
    let big = a.push_const(Const::Int(BIG));
    let mut li = mk(Op::LoadConst, &[0]);
    li.pool = big;
    a.emit(&li);
    a.emit(&mk(Op::Return, &[0]));
    let m = a.finish();
    match m.consts[usize::from(big)] {
        Const::Int(v) => assert_eq!(v, BIG),
        _ => panic!("wrong const kind"),
    }
}
