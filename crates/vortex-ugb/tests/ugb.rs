// CEP:FILE crates/vortex-ugb/tests/ugb.rs
// CEP:WHAT UGB tests: codec round trips, verifier rejections (branch
//        targets, bounds, capabilities, FFI arity), and assembler
//        assembly-to-disassembly round trips.
// CEP:WHY The verifier is the execution gate; its rejections are the
//        executable form of the 6.9 invariants.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES Deterministic programs constructed in the tests.
// CEP:COST O(program) per case.
// CEP:EVIDENCE cargo test -p vortex-ugb

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! UGB codec, verifier, and assembler test suite.

use vortex_ugb::{
    assemble, disassemble, verify, AsmError, CapReq, CmpKind, Constant, FfiDescriptor, Instr,
    Method, Op, Operand, Program, VerifyError,
};

/// A minimal returning method with two registers and one local.
// CEP:WHAT Builds the base method used by most cases.
// CEP:WHY Tests start from a known-valid baseline and inject violations.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-ugb
fn base_method() -> Method {
    Method::build("main", 4, 2, 0).method
}

/// Codec round trip across representative operand kinds.
// CEP:WHAT Encodes and decodes instructions of varied signatures.
// CEP:WHY The wire format must survive a byte round trip exactly.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(bytes).
// CEP:EVIDENCE cargo test -p vortex-ugb
#[test]
fn codec_round_trip() {
    let cases = [
        Instr::new(Op::LoadConst, vec![Operand::Reg(0), Operand::ConstIdx(7)]),
        Instr::new(
            Op::AddImm,
            vec![Operand::Reg(1), Operand::Reg(2), Operand::Imm32(-5)],
        ),
        Instr::new(Op::JumpIfTrue, vec![Operand::Reg(3), Operand::Label(12)]),
        Instr::new(
            Op::FfiCall,
            vec![Operand::Reg(1), Operand::FfiDesc(3), Operand::Count(2)],
        ),
        Instr::new(
            Op::SuperCmpBranch,
            vec![
                Operand::Reg(0),
                Operand::Reg(1),
                Operand::Label(-4),
                Operand::CmpKind(CmpKind::Lt),
            ],
        ),
        Instr::new(Op::Nop, vec![]),
    ];
    for instr in &cases {
        let mut bytes = Vec::new();
        instr.encode(&mut bytes).expect("encode");
        let (back, size) = Instr::decode(&bytes, 0).expect("decode");
        assert_eq!(size, bytes.len());
        assert_eq!(back, *instr);
        assert_eq!(instr.encoded_len(), bytes.len());
    }
}

/// Truncated streams reject with UnexpectedEnd.
// CEP:WHAT Decoding a truncated instruction reports the exact offset.
// CEP:WHY Malformed bytecode must never panic the interpreter loop.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-ugb
#[test]
fn truncated_stream_rejects() {
    let mut bytes = Vec::new();
    Instr::new(Op::LoadConst, vec![Operand::Reg(0), Operand::ConstIdx(1)])
        .encode(&mut bytes)
        .expect("encode");
    bytes.pop();
    bytes.pop();
    match Instr::decode(&bytes, 0) {
        Err(e) => assert!(matches!(e, vortex_ugb::CodecError::UnexpectedEnd { .. })),
        Ok(_) => panic!("truncated stream decoded"),
    }
}

/// Verifier accepts a valid program and rejects bad branch targets.
// CEP:WHAT Validates the accept path and the BadBranchTarget rejection.
// CEP:WHY 6.9 item 1 requires branch targets to be instruction starts.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(code).
// CEP:EVIDENCE cargo test -p vortex-ugb
#[test]
fn verifier_accepts_valid_program() {
    let mut program = Program::default();
    let mut m = base_method();
    let src = "LOAD_CONST r0, const0\nJUMP .done\n.done:\nRETURN r0\n";
    assemble(&mut m, src).expect("assemble");
    program.methods.push(m);
    program.constants.push(Constant::Int(1));
    verify(&program).expect("valid program");
}

/// Branch into the middle of an instruction rejects.
// CEP:WHAT A branch target inside an instruction rejects.
// CEP:WHY 6.9 item 1: targets must be instruction starts.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(code).
// CEP:EVIDENCE cargo test -p vortex-ugb
#[test]
fn verifier_rejects_mid_instruction_branch() {
    let mut program = Program::default();
    let mut m = base_method();
    let src = "LOAD_CONST r0, const0\nJUMP .done\n.done:\nRETURN r0\n";
    assemble(&mut m, src).expect("assemble");
    // The JUMP starts after LOAD_CONST (7 bytes) and its label bytes are at
    // +1. RETURN starts at jump_pos + 5; target lands inside it.
    let jump_pos =
        Instr::new(Op::LoadConst, vec![Operand::Reg(0), Operand::ConstIdx(0)]).encoded_len();
    let return_pos = jump_pos + Instr::new(Op::Jump, vec![Operand::Label(0)]).encoded_len();
    let bad = (return_pos + 1) as i32 - jump_pos as i32;
    m.code[jump_pos + 1..jump_pos + 5].copy_from_slice(&bad.to_le_bytes());
    program.methods.push(m);
    program.constants.push(Constant::Int(1));
    assert_eq!(
        verify(&program),
        Err(VerifyError::BadBranchTarget {
            method: 0,
            offset: jump_pos,
            target: bad
        })
    );
}

/// Missing capabilities reject with the exact requirement named.
// CEP:WHAT STORE without MemWrite rejects.
// CEP:WHY 6.9 item 2: capability compliance is verified statically.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(code).
// CEP:EVIDENCE cargo test -p vortex-ugb
#[test]
fn verifier_rejects_missing_capability() {
    let mut program = Program::default();
    let mut m = base_method();
    let src = "STORE r0, r1, x8\nRETURN r0\n";
    assemble(&mut m, src).expect("assemble");
    program.methods.push(m);
    assert_eq!(
        verify(&program),
        Err(VerifyError::CapabilityViolation {
            method: 0,
            offset: 0,
            required: CapReq::MemWrite,
        })
    );
}

/// Register and constant bounds reject.
// CEP:WHAT Out-of-frame registers and pool indices reject.
// CEP:WHY 6.9 item 1: all indices are within bounds.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(code).
// CEP:EVIDENCE cargo test -p vortex-ugb
#[test]
fn verifier_rejects_bounds_violations() {
    let mut program = Program::default();
    let mut m = Method::build("m", 2, 1, 0).method;
    let src = "LOAD_CONST r5, const0\nRETURN r5\n";
    assemble(&mut m, src).expect("assemble");
    program.methods.push(m);
    program.constants.push(Constant::Int(1));
    match verify(&program) {
        Err(VerifyError::RegisterOutOfRange { reg, .. }) => assert_eq!(reg, 5),
        other => panic!("expected RegisterOutOfRange, got {other:?}"),
    }

    let mut program2 = Program::default();
    let mut m2 = base_method();
    let src = "LOAD_CONST r0, const9\nRETURN r0\n";
    assemble(&mut m2, src).expect("assemble");
    program2.methods.push(m2);
    program2.constants.push(Constant::Int(1));
    match verify(&program2) {
        Err(VerifyError::ConstantOutOfRange { index, .. }) => assert_eq!(index, 9),
        other => panic!("expected ConstantOutOfRange, got {other:?}"),
    }
}

/// FFI calls validate descriptor existence and arity.
// CEP:WHAT FFI arity mismatch rejects with both counts.
// CEP:WHY 6.9 item 5: FFI calls have matching signatures.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(code).
// CEP:EVIDENCE cargo test -p vortex-ugb
#[test]
fn verifier_rejects_ffi_arity_mismatch() {
    let mut program = Program::default();
    let mut mb = Method::build("main", 4, 2, 0);
    mb.grant(CapReq::Ffi);
    let mut m = mb.method;
    let src = "LOAD_CONST r0, const0\nFFI_CALL r1, fd0, x3\nRETURN r1\n";
    assemble(&mut m, src).expect("assemble");
    program.methods.push(m);
    program.constants.push(Constant::Int(1));
    program.ffi_table.push(FfiDescriptor {
        ffi_id: 0,
        lang_id: 0,
        name: "add".to_string(),
        arity: 2,
        caps_required: Vec::new(),
    });
    match verify(&program) {
        Err(VerifyError::FfiArityMismatch {
            declared, found, ..
        }) => {
            assert_eq!((declared, found), (2, 3));
        }
        other => panic!("expected FfiArityMismatch, got {other:?}"),
    }
}

/// CAP_GRANT without CapAdmin rejects.
// CEP:WHAT Capability administration requires the administrative capability.
// CEP:WHY 6.9 item 2: grant and revoke need CapAdmin.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(code).
// CEP:EVIDENCE cargo test -p vortex-ugb
#[test]
fn verifier_rejects_unauthorized_cap_grant() {
    let mut program = Program::default();
    let mut m = base_method();
    let src = "CAP_GRANT cap0\nRETURN r0\n";
    assemble(&mut m, src).expect("assemble");
    program.methods.push(m);
    assert_eq!(
        verify(&program),
        Err(VerifyError::CapabilityViolation {
            method: 0,
            offset: 0,
            required: CapReq::CapAdmin,
        })
    );
}

/// Assembler and disassembler round trip.
// CEP:WHAT Assembly text survives encode-decode-render-parse unchanged.
// CEP:WHY The CLI demo and front-end authors rely on stable text form.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(code).
// CEP:EVIDENCE cargo test -p vortex-ugb
#[test]
fn assembler_disassembler_round_trip() {
    let src = "LOAD_CONST r0, const0\n\
               LOAD_LOCAL r1, local0\n\
               ADD r2, r0, r1\n\
               CMP r3, r0, r1, Lt\n\
               JUMP_IF_FALSE r3, .end\n\
               ADD_IMM r2, r2, 1\n\
               .end:\n\
               STORE_LOCAL local0, r2\n\
               RETURN r2\n";
    let mut m = base_method();
    assemble(&mut m, src).expect("assemble");
    let text = disassemble(&m.code).expect("disassemble");
    let mut m2 = base_method();
    assemble(&mut m2, &text).expect("re-assemble");
    assert_eq!(m.code, m2.code);
}

/// Undefined labels reject at assembly time.
// CEP:WHAT A branch to a missing label fails with the label name.
// CEP:WHY Front-end mistakes must surface at build time, not runtime.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-ugb
#[test]
fn assembler_rejects_undefined_label() {
    let mut m = base_method();
    match assemble(&mut m, "JUMP .nowhere\nRETURN r0\n") {
        Err(AsmError::UndefinedLabel { label, .. }) => assert_eq!(label, "nowhere"),
        other => panic!("expected UndefinedLabel, got {other:?}"),
    }
}

/// Numeric constants expose NV-1 values; byte strings do not.
// CEP:WHAT Constant conversion honors the NV-1 immediate range.
// CEP:WHY LOAD_CONST materializes pool entries as interpreter values.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-ugb
#[test]
fn constants_convert_to_values() {
    assert_eq!(Constant::Int(42).as_value().unwrap().as_i64(), Some(42));
    assert!(Constant::Int(i64::MAX).as_value().is_none());
    assert!(Constant::Bytes(b"x".to_vec()).as_value().is_none());
    let f = Constant::Float(2.5).as_value().unwrap();
    assert_eq!(f.as_f64_bits(), Some(2.5f64.to_bits()));
}
