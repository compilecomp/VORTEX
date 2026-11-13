// CEP:FILE crates/vortex-t4/tests/t4.rs
// CEP:WHAT T4 integration tests: trace recording from profile requests,
//        guarded trace compilation with guard-fusion evidence, and
//        differential execution - traced runs match T0 exactly, guard
//        failures deopt to T0 at the failing pc, and the closed-trace
//        side exit resumes T0 with the trace's frame.
// CEP:WHY These are the executable proofs of the T4 frontend contract:
//        speculation preserves T0 semantics through the deopt path.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES Demo methods from vortex-ugb; deterministic inputs.
// CEP:COST O(program) per case.
// CEP:EVIDENCE cargo test -p vortex-t4

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! T4 trace compiler test suite.

use vortex_nv1 as nv1;
use vortex_runtime::Machine;
use vortex_t0::Interp;
use vortex_t4::{compile_trace, record_trace, run_traced, TraceOutcome};

/// Runs a program on T0 and returns the result word.
fn run_t0(program: &[vortex_ugb::Method], args: &[u64]) -> u64 {
    let mut m = Machine::new();
    let interp = Interp::new(program.to_vec());
    interp.run(&mut m, 0, args).expect("t0 runs")
}

/// Records, compiles, and runs the traced path; returns (word, trace,
/// compiled) for evidence assertions.
// CEP:WHAT Records, compiles, and runs the traced path; returns (word, trace, compiled) for evidence assertions.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t4
fn traced(
    program: &[vortex_ugb::Method],
    start_pc: usize,
    args: &[u64],
) -> (u64, vortex_t4::Trace, vortex_t4::CompiledTrace) {
    // Record on a separate machine (recording is an observation, not the
    // execution under test).
    let mut rec = Machine::new();
    let trace = record_trace(program, 0, start_pc, &mut rec, args).expect("records");
    let compiled = compile_trace(program, 0, &trace).expect("compiles");
    let mut m = Machine::new();
    let r = run_traced(program, 0, &compiled, &mut m, args).expect("runs");
    match r {
        TraceOutcome::Returned(w) => (w, trace, compiled),
    }
}

/// The countdown loop's hot path records as a closed trace, compiles with
/// guards from the recorded domains, and the traced execution matches T0
/// exactly.
// CEP:WHAT The countdown loop's hot path records as a closed trace, compiles with guards from the recorded domains, and the traced execution matches T0 exactly.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t4
#[test]
fn countdown_trace_matches_t0() {
    let program = vec![vortex_ugb::countdown_method(1_000)];
    let expected = run_t0(&program, &[]);
    // The hot loop head: the JumpFalse at pc 16 (countdown's entry block
    // is 3 instructions: LoadConst 4 bytes, LoadInt32 6 bytes x2).
    let (got, trace, compiled) = traced(&program, 16, &[]);
    assert_eq!(got, expected, "differential traced equality");
    assert_eq!(nv1::decode_int(got), 1_000 * 1_001 / 2);
    assert!(compiled.exit_pc.is_some(), "the loop trace closed");
    assert!(compiled.guards_emitted > 0, "guards from recorded domains");
    assert!(
        compiled.guards_kept <= compiled.guards_emitted,
        "GVN fused or kept every guard"
    );
    assert!(trace.entries.len() <= vortex_t4::TRACE_LENGTH_LIMIT);
}

/// A float-heavy trace guards float domains and produces bit-exact results.
// CEP:WHAT A float-heavy trace guards float domains and produces bit-exact results.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t4
#[test]
fn float_trace_is_bit_exact() {
    // r0 = 1.5; r1 = 2.25; r2 = r0 + r1; r3 = r2 * 4.0; return r3.
    let mut a = vortex_ugb::Asm::new("ftrace", 0, 8);
    let c15 = a.push_const(vortex_ugb::Const::Float(1.5));
    let c225 = a.push_const(vortex_ugb::Const::Float(2.25));
    let c4 = a.push_const(vortex_ugb::Const::Float(4.0));
    let mut l1 = vortex_ugb::mk(vortex_ugb::Op::LoadConst, &[0]);
    l1.pool = c15;
    a.emit(&l1);
    let mut l2 = vortex_ugb::mk(vortex_ugb::Op::LoadConst, &[1]);
    l2.pool = c225;
    a.emit(&l2);
    a.emit(&vortex_ugb::mk(vortex_ugb::Op::Add, &[2, 0, 1]));
    let mut l3 = vortex_ugb::mk(vortex_ugb::Op::LoadConst, &[3]);
    l3.pool = c4;
    a.emit(&l3);
    a.emit(&vortex_ugb::mk(vortex_ugb::Op::Mul, &[4, 2, 3]));
    a.emit(&vortex_ugb::mk(vortex_ugb::Op::Return, &[4]));
    let program = vec![a.finish()];
    let expected = run_t0(&program, &[]);
    let (got, _, compiled) = traced(&program, 0, &[]);
    assert_eq!(got, expected, "float bits identical through the trace");
    assert_eq!(f64::from_bits(got), (1.5 + 2.25) * 4.0);
    assert!(compiled.guards_emitted > 0, "float domain guards emitted");
}

/// A guard failure deoptimizes to T0 at the failing pc: feeding the
/// traced-and-compiled window different data than was recorded still
/// produces the exact T0 result through the deopt path.
// CEP:WHAT A guard failure deoptimizes to T0 at the failing pc: feeding the traced-and-compiled window different data than was recorded still produces the exact T0 result through the deopt path.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t4
#[test]
fn guard_failure_deopts_to_t0() {
    // A method taking an argument and multiplying: record with an int
    // argument, then execute with a float argument - the int guard fails
    // and T0 completes through the float path.
    let mut a = vortex_ugb::Asm::new("mul2", 1, 8);
    let mut two = vortex_ugb::mk(vortex_ugb::Op::LoadInt32, &[1]);
    two.imm32 = 2;
    a.emit(&two);
    a.emit(&vortex_ugb::mk(vortex_ugb::Op::Mul, &[0, 0, 1]));
    a.emit(&vortex_ugb::mk(vortex_ugb::Op::Return, &[0]));
    let program = vec![a.finish()];

    let int_arg = nv1::encode_int(21).expect("encodes");
    let mut rec = Machine::new();
    let trace = record_trace(&program, 0, 0, &mut rec, &[int_arg]).expect("records");
    let compiled = compile_trace(&program, 0, &trace).expect("compiles");

    // Trace execution with the recorded domain: no deopt needed.
    let mut m = Machine::new();
    let r = run_traced(&program, 0, &compiled, &mut m, &[int_arg]).expect("runs");
    match r {
        TraceOutcome::Returned(w) => assert_eq!(nv1::decode_int(w), 42),
    }

    // Trace execution with a float: the int guard fails, T0 resumes at the
    // failing pc, and the result is the exact T0 float semantics.
    let float_arg = nv1::encode_f64(21.5);
    let mut m2 = Machine::new();
    let r2 =
        run_traced(&program, 0, &compiled, &mut m2, &[float_arg]).expect("deopt completes on T0");
    match r2 {
        TraceOutcome::Returned(w) => {
            assert_eq!(f64::from_bits(w), 43.0, "T0 float path through deopt");
        }
    }
}

/// The trace length limit truncates long windows; the truncated prefix
/// still compiles and executes correctly.
// CEP:WHAT The trace length limit truncates long windows; the truncated prefix still compiles and executes correctly.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t4
#[test]
fn trace_length_limit_truncates() {
    // A long straight-line method: 40 additions.
    let mut a = vortex_ugb::Asm::new("long", 0, 41);
    for r in 0..40_u16 {
        let mut li = vortex_ugb::mk(vortex_ugb::Op::LoadInt32, &[r]);
        li.imm32 = 1;
        a.emit(&li);
    }
    let mut acc = vortex_ugb::mk(vortex_ugb::Op::LoadInt32, &[40]);
    acc.imm32 = 0;
    a.emit(&acc);
    for r in 0..40_u16 {
        a.emit(&vortex_ugb::mk(vortex_ugb::Op::Add, &[40, 40, r]));
    }
    a.emit(&vortex_ugb::mk(vortex_ugb::Op::Return, &[40]));
    let program = vec![a.finish()];
    let expected = run_t0(&program, &[]);
    let (got, trace, _) = traced(&program, 0, &[]);
    assert_eq!(got, expected);
    assert!(
        trace.entries.len() <= vortex_t4::TRACE_LENGTH_LIMIT,
        "the window respects the limit"
    );
    // A truncated window carries a resume pc: the driver hands the frame
    // back to T0 after the window, and T0 finishes the method.
    assert!(trace.exit_pc.is_some(), "truncation records a resume pc");
}

/// Guard fusion: the same value checked twice in a row compiles to one
/// guard (GVN collapses identical (kind, value) guards).
// CEP:WHAT Guard fusion: the same value checked twice in a row compiles to one guard (GVN collapses identical (kind, value) guards).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t4
#[test]
fn guards_fuse_through_gvn() {
    // r0 = arg(int); r1 = r0 + 1; r2 = r0 + 2: two int guards on r0.
    let mut a = vortex_ugb::Asm::new("fuse", 1, 8);
    let mut one = vortex_ugb::mk(vortex_ugb::Op::LoadInt32, &[1]);
    one.imm32 = 1;
    a.emit(&one);
    let mut two = vortex_ugb::mk(vortex_ugb::Op::LoadInt32, &[2]);
    two.imm32 = 2;
    a.emit(&two);
    a.emit(&vortex_ugb::mk(vortex_ugb::Op::Add, &[3, 0, 1]));
    a.emit(&vortex_ugb::mk(vortex_ugb::Op::Add, &[4, 0, 2]));
    let mut acc = vortex_ugb::mk(vortex_ugb::Op::LoadInt32, &[5]);
    acc.imm32 = 0;
    a.emit(&acc);
    a.emit(&vortex_ugb::mk(vortex_ugb::Op::Add, &[5, 5, 3]));
    a.emit(&vortex_ugb::mk(vortex_ugb::Op::Add, &[5, 5, 4]));
    a.emit(&vortex_ugb::mk(vortex_ugb::Op::Return, &[5]));
    let program = vec![a.finish()];
    let arg = nv1::encode_int(7).expect("encodes");
    let (got, _, compiled) = traced(&program, 0, &[arg]);
    let expected = run_t0(&program, &[arg]);
    assert_eq!(got, expected);
    // r3 = 7 + 1 = 8; r4 = 7 + 2 = 9; r5 = 0 + 8 + 9 = 17.
    assert_eq!(nv1::decode_int(got), 17);
    assert!(
        compiled.guards_kept < compiled.guards_emitted,
        "identical guards fused: emitted {} kept {}",
        compiled.guards_emitted,
        compiled.guards_kept
    );
}

/// Unsupported trace opcodes reject compilation cleanly (the trace stays
/// on T0; nothing silently degrades).
// CEP:WHAT Unsupported trace opcodes reject compilation cleanly (the trace stays on T0; nothing silently degrades).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t4
#[test]
fn unsupported_trace_ops_reject() {
    let program = vortex_ugb::call_demo_program(1);
    let mut rec = Machine::new();
    let trace = record_trace(&program, 0, 0, &mut rec, &[]).expect("records");
    let e = compile_trace(&program, 0, &trace).expect_err("Call is outside the guarded subset");
    assert!(matches!(
        e,
        vortex_t4::TraceError::Unsupported {
            op: vortex_ugb::Op::Call,
            ..
        }
    ));
}

/// An array trace with bounds guards matches T0 including the AOSS store
/// policy (stores through the traced path go through the same semantic
/// core).
// CEP:WHAT An array trace with bounds guards matches T0 including the AOSS store policy (stores through the traced path go through the same semantic core).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t4
#[test]
fn array_trace_matches_t0() {
    let program = vec![vortex_ugb::array_roundtrip_method()];
    let expected = run_t0(&program, &[]);
    let (got, _, _) = traced(&program, 0, &[]);
    assert_eq!(got, expected);
    assert_eq!(nv1::decode_int(got), 41);
}
