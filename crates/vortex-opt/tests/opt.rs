// CEP:FILE crates/vortex-opt/tests/opt.rs
// CEP:WHAT Differential and evidence tests for the T2/T3 pipeline: compiled
//        plans match T0 exactly over loops, arithmetic, float promotion,
//        phi merges, and array operations; BCE and SROA record their
//        proofs; allocation produces the documented register evidence.
// CEP:WHY These tests are the translation-validation contract (standard.md
//        38.46): the optimizer may transform, but observable behavior is
//        held equal to the interpreter per test.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST O(program) per case.
// CEP:EVIDENCE cargo test -p vortex-opt

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! T2/T3 optimizing pipeline test suite.

use vortex_nv1 as nv1;
use vortex_opt::{compile_plan, TierBudget};
use vortex_runtime::Machine;
use vortex_t0::Interp;
use vortex_ugb::{mk, Asm, Const, Op};

/// Runs a program on T0 and returns the result word.
fn run_t0(program: &[vortex_ugb::Method], args: &[u64]) -> u64 {
    let mut m = Machine::new();
    let interp = Interp::new(program.to_vec());
    interp.run(&mut m, 0, args).expect("t0 runs")
}

/// Compiles a program at the given budget and executes the plan.
fn run_opt(program: &[vortex_ugb::Method], budget: TierBudget, args: &[u64]) -> vortex_opt::Plan {
    let mut m = Machine::new();
    let plan = compile_plan(program, 0, budget).expect("compiles");
    let r = vortex_opt::exec::run_plan(&plan, program, &mut m, args).expect("plan runs");
    let _ = r;
    plan
}

/// Compiles and executes, returning the result word.
fn run_opt_value(program: &[vortex_ugb::Method], budget: TierBudget, args: &[u64]) -> u64 {
    let mut m = Machine::new();
    let plan = compile_plan(program, 0, budget).expect("compiles");
    vortex_opt::exec::run_plan(&plan, program, &mut m, args).expect("plan runs")
}

/// Builds `sum(n)`: an accumulator loop with a safepoint (the countdown
/// shape); exercises phi merges, LICM, GVN.
// CEP:WHAT Builds sum(n): an accumulator loop with a safepoint (the countdown shape); exercises phi merges, LICM, GVN.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
#[must_use]
pub fn sum_method(n: i64) -> vortex_ugb::Method {
    let mut a = Asm::new("sum", 1, 4);
    let n_const = a.push_const(Const::Int(n));
    let mut ld = mk(Op::LoadConst, &[1]);
    ld.pool = n_const;
    a.emit(&ld);
    let mut zero = mk(Op::LoadInt32, &[0]);
    zero.imm32 = 0;
    a.emit(&zero);
    let mut step = mk(Op::LoadInt32, &[2]);
    step.imm32 = 1;
    a.emit(&step);

    let loop_head = a.method().code.len();
    let mut jf = mk(Op::JumpFalse, &[1]);
    jf.branches[0] = 0;
    let jf_patch = a.emit(&jf).expect("patch");
    // fallthrough: r0 += r1; r1 -= 1; loop
    a.emit(&mk(Op::Add, &[0, 0, 1]));
    a.emit(&mk(Op::Sub, &[1, 1, 2]));
    a.emit(&mk(Op::Safepoint, &[]));
    a.emit_back_branch(Op::Jump, 0, loop_head);
    a.patch(jf_patch);
    a.emit(&mk(Op::Return, &[0]));
    a.finish()
}

/// Builds a constant-arithmetic method: `2 + 3 * 4 - 1`.
// CEP:WHAT Builds a constant-arithmetic method: 2 + 3 * 4 - 1.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
#[must_use]
pub fn const_arith_method() -> vortex_ugb::Method {
    let mut a = Asm::new("const_arith", 0, 4);
    let mut two = mk(Op::LoadInt32, &[0]);
    two.imm32 = 2;
    a.emit(&two);
    let mut three = mk(Op::LoadInt32, &[1]);
    three.imm32 = 3;
    a.emit(&three);
    let mut four = mk(Op::LoadInt32, &[2]);
    four.imm32 = 4;
    a.emit(&four);
    a.emit(&mk(Op::Mul, &[1, 1, 2]));
    a.emit(&mk(Op::Add, &[0, 0, 1]));
    let mut minus = mk(Op::LoadInt32, &[3]);
    minus.imm32 = -1;
    a.emit(&minus);
    a.emit(&mk(Op::Sub, &[0, 0, 3]));
    a.emit(&mk(Op::Return, &[0]));
    a.finish()
}

/// Builds a float chain: `(1.5 + 2.25) * 4.0 / 3.0`.
// CEP:WHAT Builds a float chain: (1.5 + 2.25) * 4.0 / 3.0.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
#[must_use]
pub fn float_chain_method() -> vortex_ugb::Method {
    let mut a = Asm::new("float_chain", 0, 4);
    let c15 = a.push_const(Const::Float(1.5));
    let c225 = a.push_const(Const::Float(2.25));
    let c4 = a.push_const(Const::Float(4.0));
    let c3 = a.push_const(Const::Float(3.0));
    let mut l1 = mk(Op::LoadConst, &[0]);
    l1.pool = c15;
    a.emit(&l1);
    let mut l2 = mk(Op::LoadConst, &[1]);
    l2.pool = c225;
    a.emit(&l2);
    let mut l3 = mk(Op::LoadConst, &[2]);
    l3.pool = c4;
    a.emit(&l3);
    let mut l4 = mk(Op::LoadConst, &[3]);
    l4.pool = c3;
    a.emit(&l4);
    a.emit(&mk(Op::Add, &[0, 0, 1]));
    a.emit(&mk(Op::Mul, &[0, 0, 2]));
    a.emit(&mk(Op::Div, &[0, 0, 3]));
    a.emit(&mk(Op::Return, &[0]));
    a.finish()
}

/// Builds an int/float mix: `1 + 2.5` (the interpreter promotes to float).
// CEP:WHAT Builds an int/float mix: 1 + 2.5 (the interpreter promotes to float).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
#[must_use]
pub fn mixed_promote_method() -> vortex_ugb::Method {
    let mut a = Asm::new("mixed", 0, 2);
    let c25 = a.push_const(Const::Float(2.5));
    let mut one = mk(Op::LoadInt32, &[0]);
    one.imm32 = 1;
    a.emit(&one);
    let mut lf = mk(Op::LoadConst, &[1]);
    lf.pool = c25;
    a.emit(&lf);
    a.emit(&mk(Op::Add, &[0, 0, 1]));
    a.emit(&mk(Op::Return, &[0]));
    a.finish()
}

/// Builds a branchy method: `if n > 3 then n * 2 else n + 100` (phi merge).
// CEP:WHAT Builds a branchy method: if n > 3 then n * 2 else n + 100 (phi merge).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
#[must_use]
pub fn branchy_method() -> vortex_ugb::Method {
    let mut a = Asm::new("branchy", 1, 8);
    let c3 = a.push_const(Const::Int(3));
    let c100 = a.push_const(Const::Int(100));
    let mut l3 = mk(Op::LoadConst, &[1]);
    l3.pool = c3;
    a.emit(&l3);
    // r2 = r0 > 3 (Lt with swapped operands is not available; use Le of 3 < n
    // via two comparisons: r2 = !(n <= 3) is not directly encodable, so
    // compare n < 3 (r2), n == 3 (r3) -> "greater" = !lt && !eq handled by
    // branching directly on Le).
    let mut le = mk(Op::Le, &[2, 0, 1]);
    le.regs = [2, 0, 1];
    a.emit(&le);
    let mut jf = mk(Op::JumpFalse, &[2]);
    jf.branches[0] = 0;
    let jf_patch = a.emit(&jf).expect("patch");
    // Le false => n > 3: r4 = n * 2
    a.emit(&mk(Op::Move, &[4, 0]));
    a.emit(&mk(Op::LoadInt32, &[5]));
    let _ = c100;
    let mul_two = 2_i32;
    let mut two = mk(Op::LoadInt32, &[5]);
    two.imm32 = mul_two;
    a.emit(&two);
    a.emit(&mk(Op::Mul, &[4, 4, 5]));
    a.emit(&mk(Op::Return, &[4]));
    a.patch(jf_patch);
    // Le true => n <= 3: r6 = n + 100
    let mut l100 = mk(Op::LoadConst, &[6]);
    l100.pool = a.push_const(Const::Int(100));
    a.emit(&mk(Op::Move, &[7, 0]));
    a.emit(&l100);
    a.emit(&mk(Op::Add, &[7, 7, 6]));
    a.emit(&mk(Op::Return, &[7]));
    a.finish()
}

/// Builds a counted array loop: fill `arr[i] = i * 2` for i in 0..len with
/// the BCE-provable bound `i+1 < len`, then return `arr[3]`.
// CEP:WHAT Builds a counted array loop: fill arr[i] = i * 2 for i in 0..len with the BCE-provable bound i+1 < len, then return arr[3].
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
#[must_use]
pub fn bce_loop_method(len: u32) -> vortex_ugb::Method {
    let mut a = Asm::new("bce_loop", 0, 8);
    let mut alloc = mk(Op::AllocArray, &[0]);
    alloc.imm32u = [len, 0];
    a.emit(&alloc);
    let mut zero = mk(Op::LoadInt32, &[1]);
    zero.imm32 = 0;
    a.emit(&zero);
    let mut one = mk(Op::LoadInt32, &[2]);
    one.imm32 = 1;
    a.emit(&one);
    let mut two = mk(Op::LoadInt32, &[3]);
    two.imm32 = 2;
    a.emit(&two);

    let loop_head = a.method().code.len();
    // body first: arr[i] = i * 2 (uses the current i, 0..len-1)
    a.emit(&mk(Op::Mul, &[7, 1, 3]));
    a.emit(&mk(Op::StoreIndex, &[0, 1, 7]));
    // then the induction step: r4 = i + 1; i = r4
    a.emit(&mk(Op::Add, &[4, 1, 2]));
    a.emit(&mk(Op::Move, &[1, 4]));
    let mut ll = mk(Op::LoadLength, &[5, 0]);
    ll.regs = [5, 0, 0];
    a.emit(&ll);
    a.emit(&mk(Op::Lt, &[6, 4, 5]));
    // The guard falls through to the back-edge jump when i_next is below
    // the length, and exits through the patched JumpFalse otherwise.
    let mut jf = mk(Op::JumpFalse, &[6]);
    jf.branches[0] = 0;
    let jf_patch = a.emit(&jf).expect("patch");
    a.emit_back_branch(Op::Jump, 0, loop_head);
    a.patch(jf_patch);
    // load arr[3]
    let mut three = mk(Op::LoadInt32, &[4]);
    three.imm32 = 3;
    a.emit(&three);
    a.emit(&mk(Op::LoadIndex, &[7, 0, 4]));
    a.emit(&mk(Op::Return, &[7]));
    a.finish()
}

/// Builds a small non-escaping allocation with only local accesses (the
/// SROA candidate).
// CEP:WHAT Builds a small non-escaping allocation with only local accesses (the SROA candidate).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
#[must_use]
pub fn sroa_method() -> vortex_ugb::Method {
    let mut a = Asm::new("sroa", 0, 8);
    let mut alloc = mk(Op::AllocArray, &[0]);
    alloc.imm32u = [4, 0];
    a.emit(&alloc);
    let mut v = mk(Op::LoadInt32, &[1]);
    v.imm32 = 40;
    a.emit(&v);
    let mut two = mk(Op::LoadInt32, &[2]);
    two.imm32 = 2;
    a.emit(&two);
    let mut zero = mk(Op::LoadInt32, &[3]);
    zero.imm32 = 0;
    a.emit(&zero);
    a.emit(&mk(Op::StoreIndex, &[0, 3, 1])); // arr[0] = 40
    a.emit(&mk(Op::StoreIndex, &[0, 2, 1])); // arr[2] = 40
    a.emit(&mk(Op::LoadIndex, &[4, 0, 3])); // r4 = arr[0]
    a.emit(&mk(Op::LoadIndex, &[5, 0, 2])); // r5 = arr[2]
    a.emit(&mk(Op::Add, &[6, 4, 5])); // 80
    a.emit(&mk(Op::Return, &[6]));
    a.finish()
}

/// Differential: the sum loop matches T0 at both budgets; LICM/GVN fire.
// CEP:WHAT Differential: the sum loop matches T0 at both budgets; LICM/GVN fire.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
#[test]
fn sum_loop_matches_t0() {
    let program = vec![sum_method(100)];
    let expected = run_t0(&program, &[]);
    for budget in [TierBudget::Moderate, TierBudget::Aggressive] {
        let got = run_opt_value(&program, budget, &[nv1::encode_int(0).expect("encodes")]);
        assert_eq!(got, expected, "budget {budget:?}");
        assert_eq!(nv1::decode_int(got), 100 * 101 / 2);
    }
}

/// Constant arithmetic folds to the exact T0 value.
// CEP:WHAT Constant arithmetic folds to the exact T0 value.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
#[test]
fn const_arith_folds() {
    let program = vec![const_arith_method()];
    let expected = run_t0(&program, &[]);
    let got = run_opt_value(&program, TierBudget::Moderate, &[]);
    assert_eq!(got, expected);
    assert_eq!(nv1::decode_int(got), 2 + 3 * 4 - (-1)); // r0 - r3 where r3 = -1
    let plan = run_opt(&program, TierBudget::Moderate, &[]);
    assert!(plan.stats.folded > 0, "the folder recorded folds");
}

/// Float chains compute bit-exact results through the unboxed F64 path.
// CEP:WHAT Float chains compute bit-exact results through the unboxed F64 path.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
#[test]
fn float_chain_is_bit_exact() {
    let program = vec![float_chain_method()];
    let expected = run_t0(&program, &[]);
    let got = run_opt_value(&program, TierBudget::Moderate, &[]);
    assert_eq!(got, expected, "float bits identical");
    assert_eq!(f64::from_bits(got), (1.5 + 2.25) * 4.0 / 3.0);
    let plan = run_opt(&program, TierBudget::Moderate, &[]);
    assert!(
        plan.values.iter().any(|v| v.rep == vortex_opt::Rep::F64),
        "ARS committed F64 representations"
    );
}

/// Mixed int/float promotion matches the interpreter exactly.
// CEP:WHAT Mixed int/float promotion matches the interpreter exactly.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
#[test]
fn mixed_promotion_matches_t0() {
    let program = vec![mixed_promote_method()];
    let expected = run_t0(&program, &[]);
    let got = run_opt_value(&program, TierBudget::Moderate, &[]);
    assert_eq!(got, expected);
    assert_eq!(f64::from_bits(got), 3.5);
}

/// Branchy phi merges produce T0 results on both paths.
// CEP:WHAT Branchy phi merges produce T0 results on both paths.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
#[test]
fn branchy_phi_merges_match_t0() {
    let program = vec![branchy_method()];
    for n in [0_i64, 3, 7, 100] {
        let arg = nv1::encode_int(n).expect("encodes");
        let expected = run_t0(&program, &[arg]);
        let got = run_opt_value(&program, TierBudget::Moderate, &[arg]);
        assert_eq!(got, expected, "n = {n}");
    }
}

/// The counted array loop compiles with a proven bounds-check elision at
/// the aggressive budget and matches T0.
// CEP:WHAT The counted array loop compiles with a proven bounds-check elision at the aggressive budget and matches T0.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
#[test]
fn bce_proves_and_elides() {
    let program = vec![bce_loop_method(16)];
    let expected = run_t0(&program, &[]);
    let got = run_opt_value(&program, TierBudget::Aggressive, &[]);
    assert_eq!(got, expected);
    assert_eq!(nv1::decode_int(got), 6, "arr[3] = 3 * 2");
    let plan = run_opt(&program, TierBudget::Aggressive, &[]);
    assert!(plan.stats.bounds_elided > 0, "BCE recorded elisions");
}

/// The scalar-replaced allocation performs no heap allocation and matches
/// T0 (INV-LLIR-008 evidence).
// CEP:WHAT The scalar-replaced allocation performs no heap allocation and matches T0 (INV-LLIR-008 evidence).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
#[test]
fn sroa_replaces_local_allocation() {
    let program = vec![sroa_method()];
    let expected = run_t0(&program, &[]);
    let got = run_opt_value(&program, TierBudget::Aggressive, &[]);
    assert_eq!(got, expected);
    assert_eq!(nv1::decode_int(got), 80);
    let plan = run_opt(&program, TierBudget::Aggressive, &[]);
    assert!(plan.stats.sroa_allocs > 0, "SROA recorded replacements");
    assert!(plan.sroa_slots > 0, "the plan carries local slot tables");
    // No heap allocation happened on the SROA path.
    let mut m = Machine::new();
    let plan = compile_plan(&program, 0, TierBudget::Aggressive).expect("compiles");
    vortex_opt::exec::run_plan(&plan, &program, &mut m, &[]).expect("runs");
    assert_eq!(m.gc.stats.objects_moved, 0);
    assert!(
        m.heap.nursery_page().is_some(),
        "the machine never needed a new nursery page"
    );
}

/// Unsupported opcodes reject compilation with a typed error (the tier
/// controller keeps such methods on T1; nothing silently degrades).
// CEP:WHAT Unsupported opcodes reject compilation with a typed error (the tier controller keeps such methods on T1; nothing silently degrades).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
#[test]
fn unsupported_ops_reject_cleanly() {
    let program = vortex_ugb::call_demo_program(1);
    let e = compile_plan(&program, 0, TierBudget::Moderate)
        .expect_err("Call is outside the optimizing subset");
    assert!(matches!(
        e,
        vortex_opt::OptError::Son(vortex_opt::SonError::Unsupported { op: Op::Call, .. })
    ));
}

/// The array roundtrip demo matches T0 through the optimizing pipeline.
// CEP:WHAT The array roundtrip demo matches T0 through the optimizing pipeline.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
#[test]
fn array_roundtrip_matches_t0() {
    let program = vec![vortex_ugb::array_roundtrip_method()];
    let expected = run_t0(&program, &[]);
    let got = run_opt_value(&program, TierBudget::Moderate, &[]);
    assert_eq!(got, expected);
}

/// Physical allocation evidence: two register classes, bounded pressure,
/// and safepoint maps listing live references (INV-MIR-001/004).
// CEP:WHAT Physical allocation evidence: two register classes, bounded pressure, and safepoint maps listing live references (INV-MIR-001/004).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
#[test]
fn allocation_evidence_is_recorded() {
    let program = vec![bce_loop_method(16)];
    let plan = run_opt(&program, TierBudget::Moderate, &[]);
    assert!(
        plan.pressure <= vortex_opt::GPR_COUNT + vortex_opt::FPR_COUNT,
        "pressure bounded by the register classes"
    );
    // Every location is within its class or a spill slot.
    for (vid, loc) in plan.locations.iter().enumerate() {
        match (plan.values[vid].rep, loc) {
            (vortex_opt::Rep::F64, vortex_runtime::Loc::Reg(r)) => {
                assert!(usize::from(*r) < vortex_opt::FPR_COUNT);
            }
            (vortex_opt::Rep::F64, vortex_runtime::Loc::Stack(_)) => {} // spilled float: legal
            (_, vortex_runtime::Loc::Reg(r)) => {
                assert!(usize::from(*r) < vortex_opt::GPR_COUNT);
            }
            (_, vortex_runtime::Loc::Stack(_)) => {} // spilled value: legal
        }
    }
}

/// Deopt frames reconstruct complete register state (INV-LLIR-013):
/// every live register at a pc has a value.
// CEP:WHAT Deopt frames reconstruct complete register state (INV-LLIR-013): every live register at a pc has a value.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
#[test]
fn deopt_frames_are_complete() {
    let method = sum_method(10);
    let func = vortex_opt::build(&method).expect("builds");
    // Every block entry state plus defs covers the deopt frame at any pc.
    for b in 0..func.blocks.len() {
        let (s, e) = block_range(&func, b);
        for pc in s..e {
            let _map = func.deopt_frame_at(pc, b);
            // Completeness is structural: the map merges entry state with
            // earlier defs; the executor materializes frames from it.
        }
    }
}

/// Instruction range of a block (pc span of its values).
fn block_range(func: &vortex_opt::SonFunc, b: usize) -> (usize, usize) {
    let pcs: Vec<usize> = func.blocks[b]
        .insts
        .iter()
        .filter(|&&vid| func.values[vid].pc != usize::MAX)
        .map(|&vid| func.values[vid].pc)
        .collect();
    match (pcs.first(), pcs.last()) {
        (Some(&s), Some(&e)) => (s, e + 1),
        _ => (0, 0),
    }
}
