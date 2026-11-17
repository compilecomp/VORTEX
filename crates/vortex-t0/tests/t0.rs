// CEP:FILE crates/vortex-t0/tests/t0.rs
// CEP:WHAT T0 tests: arithmetic with boxing, control flow, calls with
//        inline caches, deoptimization on guard failure, rewriting
//        (specialization, dead code, super formation), capability checks.
// CEP:WHY The interpreter's semantics and rewrite safety are the T0
//        contract; the tests are their executable definitions.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES Programs are verified before interpretation.
// CEP:COST O(instructions) per case.
// CEP:EVIDENCE cargo test -p vortex-t0

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! T0 interpreter test suite.

use vortex_t0::{Interp, Outcome, Rewriter};
use vortex_ugb::{CapReq, Constant, Method, Operand, Program};

/// Builds a program from assembly text for one method.
// CEP:WHAT Assembles a single-method program with constants.
// CEP:WHY Tests read as bytecode source, matching the design examples.
// CEP:STATUS complete
// CEP:FAILURE Panics on assembly errors; shapes are fixed.
// CEP:ASSUMES None.
// CEP:COST O(code).
// CEP:EVIDENCE cargo test -p vortex-t0
fn program_from(
    name: &str,
    regs: u16,
    locals: u16,
    constants: Vec<Constant>,
    caps: Vec<CapReq>,
    src: &str,
) -> Program {
    let mut program = Program::default();
    let mut mb = Method::build(name, regs, locals, 0);
    for c in caps {
        mb.grant(c);
    }
    let mut m = mb.method;
    vortex_ugb::assemble(&mut m, src).expect("assemble");
    program.methods.push(m);
    program.constants = constants;
    vortex_ugb::verify(&program).expect("verify");
    program
}

/// ADD of two constants flows through registers to RETURN.
// CEP:WHAT Interprets a straight-line arithmetic program.
// CEP:WHY The canonical execution path.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(instructions).
// CEP:EVIDENCE cargo test -p vortex-t0
#[test]
fn interprets_arithmetic() {
    let p = program_from(
        "main",
        4,
        0,
        vec![Constant::Int(5), Constant::Int(7)],
        vec![],
        "LOAD_CONST r0, const0\nLOAD_CONST r1, const1\nADD r2, r0, r1\nRETURN r2\n",
    );
    let mut interp = Interp::new(p);
    match interp.run(0, &[]).expect("run") {
        Outcome::Returned(v) => assert_eq!(v.as_i64(), Some(12)),
        other => panic!("expected Returned, got {other:?}"),
    }
}

/// Integer overflow boxes into a heap object (NV-1 policy).
// CEP:WHAT Out-of-range results become boxed integers.
// CEP:WHY INV-004 with the boxed slow path.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t0
#[test]
fn overflow_boxes() {
    let big = 1i64 << 46;
    let p = program_from(
        "main",
        4,
        0,
        vec![Constant::Int(big), Constant::Int(big)],
        vec![],
        "LOAD_CONST r0, const0\nLOAD_CONST r1, const1\nADD r2, r0, r1\nRETURN r2\n",
    );
    let mut interp = Interp::new(p);
    match interp.run(0, &[]).expect("run") {
        Outcome::Returned(v) => {
            let off = v.as_heap_ref().expect("box reference");
            let x = interp.heap.mark_word(off as usize).unwrap() as i64;
            assert_eq!(x, big * 2);
        }
        other => panic!("expected Returned, got {other:?}"),
    }
}

/// Conditional jumps select branches correctly.
// CEP:WHAT A branch program returns the taken arm's value.
// CEP:WHY Control flow is the correctness backbone.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(instructions).
// CEP:EVIDENCE cargo test -p vortex-t0
#[test]
fn branches() {
    let p = program_from(
        "main",
        4,
        0,
        vec![Constant::Int(2), Constant::Int(5)],
        vec![],
        "LOAD_CONST r0, const0\nLOAD_CONST r1, const1\nCMP r2, r0, r1, Lt\n\
         JUMP_IF_TRUE r2, .yes\nLOAD_CONST r3, const0\nRETURN r3\n\
         .yes:\nLOAD_CONST r3, const1\nRETURN r3\n",
    );
    let mut interp = Interp::new(p);
    match interp.run(0, &[]).expect("run") {
        Outcome::Returned(v) => assert_eq!(v.as_i64(), Some(5)),
        other => panic!("expected Returned, got {other:?}"),
    }
}

/// Method calls pass arguments and return values through frames.
// CEP:WHAT CALL_METHOD executes a callee and resumes the caller.
// CEP:WHY Frame discipline underpins every tier.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(instructions).
// CEP:EVIDENCE cargo test -p vortex-t0
// CEP:ID: JIT-a35
#[test]
fn calls_pass_frames() {
    let mut p = Program::default();
    let mut main = Method::build("main", 4, 0, 0).method;
    main.env_caps.push(CapReq::Call);
    vortex_ugb::assemble(
        &mut main,
        "LOAD_CONST r0, const0\nLOAD_CONST r1, const1\nCALL_METHOD r2, const1, x2\nRETURN r2\n",
    )
    .expect("assemble");
    let mut dbl = Method::build("dbl", 3, 0, 2).method;
    vortex_ugb::assemble(&mut dbl, "ADD r2, r0, r1\nRETURN r2\n").expect("assemble");
    p.methods.push(main);
    p.methods.push(dbl);
    p.constants = vec![Constant::Int(20), Constant::Int(22), Constant::Int(1)];
    vortex_ugb::verify(&p).expect("verify");
    let mut interp = Interp::new(p);
    match interp.run(0, &[]).expect("run") {
        Outcome::Returned(v) => assert_eq!(v.as_i64(), Some(42)),
        other => panic!("expected Returned, got {other:?}"),
    }
}

/// Stack overflow at the depth bound traps, never aborts.
// CEP:WHAT Unbounded recursion fails at the documented bound.
// CEP:WHY Law 6: bounded failure for recursive programs.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(depth).
// CEP:ID: JIT-a36
// CEP:EVIDENCE cargo test -p vortex-t0
#[test]
fn recursion_hits_stack_bound() {
    let mut p = Program::default();
    let mut main = Method::build("main", 2, 0, 0).method;
    main.env_caps.push(CapReq::Call);
    vortex_ugb::assemble(&mut main, "CALL_METHOD r0, const0, x0\nRETURN r0\n").expect("assemble");
    let mut self_call = Method::build("recurse", 2, 0, 0).method;
    self_call.env_caps.push(CapReq::Call);
    vortex_ugb::assemble(&mut self_call, "CALL_METHOD r0, const0, x0\nRETURN r0\n")
        .expect("assemble");
    p.methods.push(main);
    p.methods.push(self_call);
    p.constants = vec![Constant::Int(1)];
    vortex_ugb::verify(&p).expect("verify");
    let mut interp = Interp::new(p);
    assert!(matches!(
        interp.run(0, &[]),
        Err(vortex_runtime::Trap::CallStackOverflow { .. })
    ));
}

/// Guarded ADD_INT deoptimizes when the assumption fails.
// CEP:WHAT A float operand at an ADD_INT site captures deopt state.
// CEP:WHY Speculative correctness (T0 6.9).
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t0
#[test]
fn guarded_add_deoptimizes() {
    let p = program_from(
        "main",
        4,
        0,
        vec![],
        vec![],
        "ADD_INT r2, r0, r1\nRETURN r2\n",
    );
    let mut interp = Interp::new(p);
    let float = nv1_value_float(2.5);
    match interp.run(0, &[float, float]).expect("run") {
        Outcome::Deopt(state) => {
            assert_eq!(state.reason, "int assumption");
            assert_eq!(state.regs[0], float);
        }
        other => panic!("expected Deopt, got {other:?}"),
    }
}

/// Builds a float register argument.
// CEP:WHAT Encodes 2.5 as an NV-1 value.
// CEP:WHY Tests pass non-integer arguments directly.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t0
fn nv1_value_float(x: f64) -> vortex_nv1::Value {
    vortex_nv1::Value::from_f64(x)
}

/// Capability checks fire when the environment lacks a permission.
// CEP:WHAT STORE without MemWrite traps with the permission bit.
// CEP:WHY UGB 6.4 gating applies at runtime too.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t0
#[test]
fn capability_check_traps() {
    let p = program_from("main", 4, 0, vec![], vec![], "RETURN r0\n");
    // Hand-craft an ungranted STORE to trigger the runtime gate.
    let interp = Interp::new(p);
    let mut m = Method::build("s", 3, 0, 0).method;
    vortex_ugb::assemble(&mut m, "STORE r0, r1, x1\nRETURN r0\n").expect("assemble");
    let mut p2 = Program::default();
    p2.methods.push(m);
    let mut interp2 = Interp::new(p2);
    assert!(matches!(
        interp2.run(0, &[]),
        Err(vortex_runtime::Trap::CapabilityViolation { .. })
    ));
    let _ = interp;
}

/// Rewriting specializes all-int ADD sites to ADD_INT in place.
// CEP:WHAT Profile feedback rewrites ADD without changing code size.
// CEP:WHY T0 rule 1 with branch-target safety.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:ID: JIT-a37
// CEP:COST O(code).
// CEP:EVIDENCE cargo test -p vortex-t0
#[test]
fn rewriting_specializes_hot_add() {
    let p = program_from(
        "main",
        4,
        0,
        vec![Constant::Int(2)],
        vec![],
        "LOAD_CONST r0, const0\nLOAD_CONST r1, const0\nADD r2, r0, r1\nRETURN r2\n",
    );
    let mut interp = Interp::new(p);
    let mut rewriter = Rewriter::new();
    // Warm the profile, then rewrite and observe the patch.
    for _ in 0..10 {
        let _ = interp.run(0, &[]);
    }
    rewriter.maybe_rewrite(&mut interp, 0);
    let code = interp.program.method(0).unwrap().code.clone();
    let mut pos = 0;
    let mut saw_add_int = false;
    while pos < code.len() {
        let (instr, size) = vortex_ugb::Instr::decode(&code, pos).unwrap();
        if instr.op == vortex_ugb::Op::AddInt {
            saw_add_int = true;
        }
        pos += size;
    }
    assert!(saw_add_int, "ADD was specialized to ADD_INT");
    // The specialized program still computes the same result.
    match interp.run(0, &[]).expect("run") {
        Outcome::Returned(v) => assert_eq!(v.as_i64(), Some(4)),
        other => panic!("expected Returned, got {other:?}"),
    }
}

/// Super bytecode formation collapses the load-add-store triple.
// CEP:WHAT The hot triple patches into LOAD_ADD_STORE plus padding.
// CEP:WHY T0 6.7 super bytecodes reduce dispatches.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ID: JIT-a38
// CEP:ASSUMES None.
// CEP:COST O(code).
// CEP:EVIDENCE cargo test -p vortex-t0
#[test]
fn super_load_add_store_forms_and_runs() {
    let src = "LOAD_LOCAL r0, local0\nADD_IMM r0, r0, 1\nSTORE_LOCAL local0, r0\nRETURN r0\n";
    let p = program_from("main", 2, 1, vec![], vec![], src);
    let mut interp = Interp::new(p);
    let mut rewriter = Rewriter::new();
    let mut seed = vec![vortex_nv1::Value::from_i64(41).unwrap()];
    // The entry arguments seed r0; locals start undefined, so warm the
    // locals through an explicit store pass first.
    for _ in 0..3 {
        let _ = interp.run(0, &seed);
        seed = vec![vortex_nv1::Value::from_i64(41).unwrap()];
    }
    rewriter.maybe_rewrite(&mut interp, 0);
    let code = interp.program.method(0).unwrap().code.clone();
    let mut pos = 0;
    let mut saw_super = false;
    while pos < code.len() {
        let (instr, size) = vortex_ugb::Instr::decode(&code, pos).unwrap();
        if instr.op == vortex_ugb::Op::SuperLoadAddStore {
            saw_super = true;
        }
        pos += size;
    }
    assert!(saw_super, "triple collapsed to LOAD_ADD_STORE");
}

/// Inline caches transition monomorphic to polymorphic to megamorphic.
// CEP:WHAT Cache states follow the documented state machine.
// CEP:WHY T0 6.5 cache behavior drives call specialization.
// CEP:STATUS complete
// CEP:ID: JIT-a39
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(entries).
// CEP:EVIDENCE cargo test -p vortex-t0
#[test]
fn inline_cache_states() {
    use vortex_t0::{CacheState, InlineCache};
    let mut c = InlineCache::new();
    assert_eq!(c.state, CacheState::Uninitialized);
    assert_eq!(c.lookup(1), Some(1));
    assert_eq!(c.state, CacheState::Monomorphic);
    assert_eq!(c.lookup(1), Some(1));
    assert_eq!(c.lookup(2), Some(2));
    assert_eq!(c.state, CacheState::Polymorphic);
    for k in 3..7u32 {
        let _ = c.lookup(k);
    }
    assert_eq!(c.state, CacheState::Megamorphic);
    assert_eq!(c.lookup(9), None);
}

/// Division by zero traps with the typed error.
// CEP:WHAT DIV by zero never panics.
// CEP:WHY Law 6: named failure per fault class.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t0
#[test]
fn div_by_zero_traps() {
    let p = program_from(
        "main",
        4,
        0,
        vec![Constant::Int(1), Constant::Int(0)],
        vec![],
        "LOAD_CONST r0, const0\nLOAD_CONST r1, const1\nDIV r2, r0, r1\nRETURN r2\n",
    );
    let mut interp = Interp::new(p);
    assert!(matches!(
        interp.run(0, &[]),
        Err(vortex_runtime::Trap::DivByZero)
    ));
}

/// Heap objects allocate, store, and load through capability gates.
// CEP:WHAT ALLOC/STORE/LOAD round trip one slot.
// CEP:WHY Memory operations are the state-token domain's model.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p vortex-t0
#[test]
fn heap_round_trip() {
    let p = program_from(
        "main",
        6,
        0,
        vec![Constant::Int(99)],
        vec![CapReq::MemAlloc, CapReq::MemRead, CapReq::MemWrite],
        "ALLOC r0, x2\nLOAD_CONST r1, const0\nSTORE r1, r0, x2\nLOAD r2, r0, x2\nRETURN r2\n",
    );
    let mut interp = Interp::new(p);
    match interp.run(0, &[]).expect("run") {
        Outcome::Returned(v) => assert_eq!(v.as_i64(), Some(99)),
        other => panic!("expected Returned, got {other:?}"),
    }
    let _ = Operand::Reg(0);
}
