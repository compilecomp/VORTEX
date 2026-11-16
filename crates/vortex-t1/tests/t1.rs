// CEP:FILE crates/vortex-t1/tests/t1.rs
// CEP:WHAT T1 tests: plan/execute differential against T0, fallback
//        classification, patched call frames, and loop label patching.
// CEP:WHY T1 must be observationally equivalent to T0 on covered opcodes.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES Programs are verified before planning.
// CEP:COST O(instructions) per case.
// CEP:EVIDENCE cargo test -p vortex-t1

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! T1 baseline tier test suite.

use vortex_t1::{classify, plan_method, Executor};
use vortex_ugb::{CapReq, Constant, Method, Program};

/// Builds a two-method program: main calls dbl and returns the result.
// CEP:WHAT Assembles the shared test program.
// CEP:WHY Calls, constants, and returns cover the common stencils.
// CEP:STATUS complete
// CEP:FAILURE Panics on assembly errors; shapes are fixed.
// CEP:ASSUMES None.
// CEP:COST O(code).
// CEP:EVIDENCE cargo test -p vortex-t1
fn build_program() -> Program {
    let mut p = Program::default();
    let mut main = Method::build("main", 4, 0, 0).method;
    main.env_caps.push(CapReq::Call);
    vortex_ugb::assemble(
        &mut main,
        "LOAD_CONST r0, const0\nLOAD_CONST r1, const2\nCALL_METHOD r2, const1, x2\nRETURN r2\n",
    )
    .expect("assemble");
    let mut dbl = Method::build("dbl", 3, 0, 2).method;
    vortex_ugb::assemble(&mut dbl, "ADD r2, r0, r1\nRETURN r2\n").expect("assemble");
    p.methods.push(main);
    p.methods.push(dbl);
    p.constants = vec![Constant::Int(20), Constant::Int(1), Constant::Int(22)];
    vortex_ugb::verify(&p).expect("verify");
    p
}

/// Plans both methods and executes them; result matches the T0 result.
// CEP:WHAT Differential T1 versus T0 execution.
// CEP:WHY T1 6.2 correctness: same observable values.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(instructions).
// CEP:EVIDENCE cargo test -p vortex-t1
#[test]
fn t1_matches_t0() {
    let p = build_program();
    let mut interp = vortex_t0::Interp::new(p.clone());
    let t0_result = match interp.run(0, &[]).expect("t0 run") {
        vortex_t0::Outcome::Returned(v) => v,
        other => panic!("t0: {other:?}"),
    };
    let coverage = classify(&p);
    assert_eq!(coverage.compiled, vec![0, 1]);
    assert!(coverage.fallback.is_empty());
    let objects: Vec<vortex_t1::CodeObject> = coverage
        .compiled
        .iter()
        .map(|m| plan_method(&p, *m).expect("plan"))
        .collect();
    let mut ex = Executor::new();
    ex.install(objects, p.constants.clone());
    assert!(ex.installed(0) && ex.installed(1));
    let t1_result = ex.run(0, &[], &mut interp.heap).expect("t1 run");
    assert_eq!(t0_result.as_i64(), Some(42));
    assert_eq!(t1_result.as_i64(), t0_result.as_i64());
}

/// Methods with unsupported opcodes fall back to T0.
// CEP:WHAT Classification routes uncovered opcodes to the interpreter.
// CEP:WHY Fallback keeps partial stencil coverage safe.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(code).
// CEP:EVIDENCE cargo test -p vortex-t1
#[test]
fn unsupported_falls_back() {
    let mut p = Program::default();
    let mut m = Method::build("ffi", 3, 0, 0).method;
    m.env_caps.push(CapReq::Ffi);
    vortex_ugb::assemble(&mut m, "FFI_CALL r0, fd0, x0\nRETURN r0\n").expect("assemble");
    p.methods.push(m);
    let coverage = classify(&p);
    assert!(coverage.compiled.is_empty());
    assert_eq!(coverage.fallback.len(), 1);
    assert_eq!(coverage.fallback[0].1, "FFI_CALL");
}

/// Branches execute through patched labels (instruction indices).
// CEP:WHAT A counting loop in T1 terminates with the right accumulator.
// CEP:WHY Label patching is the trickiest stencil step.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(iterations).
// CEP:EVIDENCE cargo test -p vortex-t1
#[test]
fn loop_executes() {
    let mut p = Program::default();
    let mut m = Method::build("main", 4, 0, 0).method;
    vortex_ugb::assemble(
        &mut m,
        "LOAD_CONST r1, const0\n.top:\nADD_IMM r1, r1, 1\nLOAD_CONST r2, const1\n\
         CMP r3, r1, r2, Lt\nJUMP_IF_TRUE r3, .top\nRETURN r1\n",
    )
    .expect("assemble");
    p.methods.push(m);
    p.constants = vec![Constant::Int(0), Constant::Int(5)];
    vortex_ugb::verify(&p).expect("verify");
    let obj = plan_method(&p, 0).expect("plan");
    let mut ex = Executor::new();
    ex.install(vec![obj], p.constants.clone());
    let result = ex
        .run(
            0,
            &[],
            &mut vortex_runtime::Heap::new(vortex_runtime::HeapConfig::default()),
        )
        .expect("run");
    assert_eq!(result.as_i64(), Some(5));
}
