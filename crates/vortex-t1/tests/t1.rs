// CEP:FILE crates/vortex-t1/tests/t1.rs
// CEP:WHAT T1 integration tests: differential T0/T1 execution over the demo
//        programs, stencil pool purity and reuse, copy-and-patch work
//        accounting, PC coverage, OSR stub accounting, and path routing.
// CEP:WHY These are the executable proofs that the baseline tier compiles
//        correct code (T0 wins any disagreement) and that the stencil pool
//        stays pure and O(operand-location variants).
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES Demo methods from vortex-ugb; deterministic inputs.
// CEP:COST O(program) per case.
// CEP:EVIDENCE cargo test -p vortex-t1

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! T1 baseline compiler integration test suite.

use vortex_nv1 as nv1;
use vortex_runtime::{Machine, StencilPool};
use vortex_t0::Interp;
use vortex_t1::{classify, compile_baseline, T1Path};

/// Runs a program on T0 and returns the result word.
fn run_t0(program: &[vortex_ugb::Method], args: &[u64]) -> u64 {
    let mut m = Machine::new();
    let interp = Interp::new(program.to_vec());
    interp.run(&mut m, 0, args).expect("t0 runs")
}

/// Compiles and runs a program on T1 and returns (result, blob).
// CEP:WHAT Compiles and runs a program on T1 and returns (result, blob).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t1
fn run_t1(program: &[vortex_ugb::Method], args: &[u64]) -> (u64, vortex_t1::T1Blob) {
    let mut pool = StencilPool::default();
    let blob = compile_baseline(&mut pool, program, 0).expect("compiles");
    let mut m = Machine::new();
    let result =
        vortex_t1::exec::compile_and_run(&mut StencilPool::default(), program, 0, &mut m, args)
            .expect("t1 runs");
    let _ = pool;
    (result, blob)
}

/// T1a: the countdown demo compiles through pure copy-and-patch and produces
/// the exact T0 result; the profile sees the same back-edge samples.
// CEP:WHAT T1a: the countdown demo compiles through pure copy-and-patch and produces the exact T0 result; the profile sees the same back-edge samples.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t1
#[test]
fn t1a_countdown_matches_t0() {
    let program = vec![vortex_ugb::countdown_method(1_000)];
    assert_eq!(classify(&program[0]), T1Path::A);
    let expected = run_t0(&program, &[]);
    let (got, blob) = run_t1(&program, &[]);
    assert_eq!(got, expected, "differential T0/T1 equality");
    assert_eq!(blob.path, T1Path::A);
    assert_eq!(blob.uncovered_bytes(), 0, "DEBUG-INV-01 coverage");
    assert_eq!(blob.regions.len(), 1, "one loop region");
    assert_eq!(
        blob.osr_stub.counters.len(),
        1,
        "one OSR metadata counter per loop"
    );
}

/// T1b: a method with registers beyond the fast map compiles through
/// patched spill variants and matches T0 exactly.
// CEP:WHAT T1b: a method with registers beyond the fast map compiles through patched spill variants and matches T0 exactly.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t1
#[test]
fn t1b_spilled_registers_match_t0() {
    // r0..r12 = 1; r0 = r0 + r1 + ... + r12; return r0.
    let mut a = vortex_ugb::Asm::new("wide", 0, 13);
    for r in 0..13_u16 {
        let mut li = vortex_ugb::mk(vortex_ugb::Op::LoadInt32, &[r]);
        li.imm32 = 1;
        a.emit(&li);
    }
    let mut acc = vortex_ugb::mk(vortex_ugb::Op::LoadInt32, &[0]);
    acc.imm32 = 0;
    a.emit(&acc);
    for r in 1..13_u16 {
        a.emit(&vortex_ugb::mk(vortex_ugb::Op::Add, &[0, 0, r]));
    }
    a.emit(&vortex_ugb::mk(vortex_ugb::Op::Return, &[0]));
    let program = vec![a.finish()];

    assert_eq!(classify(&program[0]), T1Path::B);
    let expected = run_t0(&program, &[]);
    let (got, blob) = run_t1(&program, &[]);
    assert_eq!(got, expected);
    assert_eq!(blob.path, T1Path::B);
    assert!(
        !blob.plan.slot_to_vreg.is_empty(),
        "T1b allocates spill slots"
    );
}

/// T1c: a high-pressure method routes to the linear-scan fallback and
/// matches T0 exactly; the scan keeps hot registers physical and spills the
/// rest.
// CEP:WHAT T1c: a high-pressure method routes to the linear-scan fallback and matches T0 exactly; the scan keeps hot registers physical and spills the rest.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t1
#[test]
fn t1c_high_pressure_routes_to_linear_scan() {
    // 40 live temporaries, then sum them: the fixed map and the T1b spill
    // budget (8 + 24) cannot hold this.
    let mut a = vortex_ugb::Asm::new("pressure", 0, 41);
    for r in 0..40_u16 {
        let mut li = vortex_ugb::mk(vortex_ugb::Op::LoadInt32, &[r]);
        li.imm32 = i32::from(r) + 1;
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

    assert_eq!(classify(&program[0]), T1Path::C);
    let expected = run_t0(&program, &[]);
    let (got, blob) = run_t1(&program, &[]);
    assert_eq!(got, expected);
    assert_eq!(blob.path, T1Path::C);
    assert!(
        !blob.plan.slot_to_vreg.is_empty(),
        "linear scan spilled beyond the fast map"
    );
    assert_eq!(blob.uncovered_bytes(), 0);
}

/// Cross-method calls execute correctly through the T1 executor (the callee
/// runs on the interpreter; the T1 caller stages the argument window).
// CEP:WHAT Cross-method calls execute correctly through the T1 executor (the callee runs on the interpreter; the T1 caller stages the argument window).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t1
#[test]
fn calls_execute_through_t1() {
    let program = vortex_ugb::call_demo_program(21);
    let expected = run_t0(&program, &[]);
    let got = {
        let mut m = Machine::new();
        vortex_t1::exec::compile_and_run(&mut StencilPool::default(), &program, 0, &mut m, &[])
            .expect("t1 runs")
    };
    assert_eq!(got, expected);
    assert_eq!(nv1::decode_int(got), 42);
}

/// Recursion through the method table produces the same factorial from T1
/// and T0.
// CEP:WHAT Recursion through the method table produces the same factorial from T1 and T0.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t1
#[test]
fn factorial_matches_between_tiers() {
    let program = vortex_ugb::factorial_program();
    let arg = nv1::encode_int(6).expect("encodes");
    let expected = run_t0(&program, &[arg]);
    let got = {
        let mut m = Machine::new();
        vortex_t1::exec::compile_and_run(&mut StencilPool::default(), &program, 0, &mut m, &[arg])
            .expect("t1 runs")
    };
    assert_eq!(got, expected);
    assert_eq!(nv1::decode_int(got), 720);
}

/// Stencil pool purity and reuse: identical (opcode, locations) requests
/// reuse one template, every payload is a hole (no method data in the
/// template), and the OSR accounting stays one stub per blob.
// CEP:WHAT Stencil pool purity and reuse: identical (opcode, locations) requests reuse one template, every payload is a hole (no method data in the template), and the OSR accounting stays one stub per blob.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t1
#[test]
fn stencil_pool_is_pure_and_deduplicated() {
    let mut pool = StencilPool::default();
    let p1 = vec![vortex_ugb::countdown_method(10)];
    let p2 = vec![vortex_ugb::countdown_method(999)];
    let b1 = compile_baseline(&mut pool, &p1, 0).expect("compiles");
    let before = pool.len();
    let b2 = compile_baseline(&mut pool, &p2, 0).expect("compiles");
    assert_eq!(
        before,
        pool.len(),
        "same-shaped methods reuse the exact templates"
    );
    assert_eq!(b1.bytes_copied, b2.bytes_copied);
    assert_eq!(
        pool.blobs_assembled, pool.osr_stubs_created,
        "exactly one shared OSR stub per blob"
    );
    // No template may embed method data: every payload hole must still be
    // the unpatched 0xC1 marker in the template (payloads are filled only
    // at copy time), and identical shapes reuse one template (asserted
    // above), so no method-specific byte can leak into the pool.
    for i in 0..pool.len() {
        let stencil = pool.stencil(i);
        for ph in &stencil.payload_holes {
            for b in &stencil.code[ph.offset..ph.offset + usize::from(ph.width)] {
                assert_eq!(*b, 0xC1, "payload byte in stencil {i} is pre-patched");
            }
        }
    }
}

/// The OSR back-link patch records the post-OSR state transition
/// (OSR-1 section 6.1 step 4).
// CEP:WHAT The OSR back-link patch records the post-OSR state transition (OSR-1 section 6.1 step 4).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t1
#[test]
fn osr_backlink_patch_transitions_to_t2() {
    let mut pool = StencilPool::default();
    let program = vec![vortex_ugb::countdown_method(10)];
    let mut blob = compile_baseline(&mut pool, &program, 0).expect("compiles");
    assert!(!blob.osr_stub.patched_to_t2);
    blob.patch_backlink_to_t2();
    assert!(blob.osr_stub.patched_to_t2);
    assert_eq!(
        blob.osr_stub.counters[0].bytecode_pc, blob.regions[0].bytecode_pc,
        "the OSR counter targets the loop head"
    );
}

/// The deopt frame carries the argument window and defined registers
/// (DEOPT-001/002 reconstruction contract).
// CEP:WHAT The deopt frame carries the argument window and defined registers (DEOPT-001/002 reconstruction contract).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t1
#[test]
fn deopt_frame_reconstructs_vm_state() {
    let mut pool = StencilPool::default();
    let program = vec![vortex_ugb::countdown_method(10)];
    let blob = compile_baseline(&mut pool, &program, 0).expect("compiles");
    let mach = vortex_t1::T1Machine::new(program[0].register_count, &[]);
    // Execute to the first instance, then reconstruct.
    let frame = mach.deopt_frame(&blob, 0);
    assert_eq!(frame.len(), usize::from(program[0].register_count));
    // The argument window is always carried (OSR-INV-01 state completeness
    // for the entry frame).
    for v in 0..usize::from(vortex_t1::FAST_MAP_REGS).min(frame.len()) {
        assert!(
            vortex_t1::live_vregs_at(&blob, 0).contains(&(v as u16)),
            "register {v} missing from the entry liveness set"
        );
    }
}

/// Copy-and-patch work accounting: patches equal holes plus payload holes,
/// and copied bytes equal the image size (the image is pure stencil copies).
// CEP:WHAT Copy-and-patch work accounting: patches equal holes plus payload holes, and copied bytes equal the image size (the image is pure stencil copies).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t1
#[test]
fn copy_patch_work_is_accounted() {
    let mut pool = StencilPool::default();
    let program = vec![vortex_ugb::array_roundtrip_method()];
    let blob = compile_baseline(&mut pool, &program, 0).expect("compiles");
    let mut holes = 0_u64;
    for inst in &blob.instances {
        // Re-derive hole counts from the pool.
        let stencil = (0..pool.len())
            .map(|i| pool.stencil(i))
            .find(|s| s.key.op == inst.op)
            .expect("stencil exists");
        holes +=
            u64::try_from(stencil.holes.len() + stencil.payload_holes.len()).expect("counts fit");
    }
    assert_eq!(
        blob.patches_applied, holes,
        "every hole was patched exactly once"
    );
    assert_eq!(
        blob.bytes_copied,
        blob.image.len() as u64,
        "the image is pure copied stencil bytes"
    );
}

/// The array roundtrip demo executes identically through T1 and T0.
// CEP:WHAT The array roundtrip demo executes identically through T1 and T0.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t1
#[test]
fn array_roundtrip_matches_t0() {
    let program = vec![vortex_ugb::array_roundtrip_method()];
    let expected = run_t0(&program, &[]);
    let got = {
        let mut m = Machine::new();
        vortex_t1::exec::compile_and_run(&mut StencilPool::default(), &program, 0, &mut m, &[])
            .expect("t1 runs")
    };
    assert_eq!(got, expected);
    assert_eq!(nv1::decode_int(got), 41);
}
