//! vortex-opt: the T2/T3 optimizing pipeline over the unified SSA graph.
// CEP:FILE crates/vortex-opt/src/lib.rs
// CEP:WHAT Crate root: SSA graph construction (dominators, minimal phi
//        placement, renaming), the pass pipeline with T2/T3 budgets
//        (constant folding, GVN, LICM, ARS, proven BCE, SROA), the
//        LLIR/MIR lowering with explicit representation conversions and
//        linear-scan allocation, and the differential plan executor over
//        the shared T0 semantics.
// CEP:WHY T2/T3 are the optimizing tiers: the pipeline turns verified UGB
//        into representation-committed, register-allocated plans whose
//        observable behavior the differential tests hold equal to T0.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE See per-module docs; unsupported opcodes reject compilation
//        with typed errors (the tier controller keeps such methods on T1).
// CEP:ASSUMES Methods pass vortex-ugb verification before compilation.
// CEP:COST Build O(values), passes O(values) each, lowering O(values),
//        allocation O(values log values).
// CEP:EVIDENCE cargo test -p vortex-opt

/// The differential plan executor.
pub mod exec;
/// The LLIR/MIR lowering and physical allocation.
pub mod lower;
/// The optimization passes and tier budgets.
pub mod passes;
/// The unified SSA graph (CFG, phi placement, renaming, deopt states).
pub mod son;

pub use exec::{run_plan_with_deopt, Bin, Cmp, ExecOutcome};
pub use lower::{lower, ExecOp, Plan, PlanBlock, PlanValue, SafepointMap, FPR_COUNT, GPR_COUNT};
pub use passes::{
    assign_reps, bce, constant_fold, gvn, licm, run_pipeline, sroa, Pass, PassStats, TierBudget,
};
pub use son::{build, build_with_index, GuardKind, Rep, SonError, SonFunc, SonOp};

/// Compilation failure of the optimizing pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptError {
    /// The graph build rejected the method.
    Son(SonError),
}

/// Compiles a method to an executable plan with the given tier budget.
// CEP:WHAT Compiles a method to an executable plan with the given tier budget.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn compile_plan(
    program: &[vortex_ugb::Method],
    method_index: usize,
    budget: TierBudget,
) -> Result<Plan, OptError> {
    let method = program
        .get(method_index)
        .ok_or(OptError::Son(SonError::Decode { offset: 0 }))?;
    let mut func = build(method).map_err(OptError::Son)?;
    let stats = run_pipeline(&mut func, budget);
    let mut plan = lower(&func);
    plan.stats = stats;
    plan.sroa_slots = plan
        .values
        .iter()
        .filter_map(|v| match v.op {
            ExecOp::SroaLoad(s) | ExecOp::SroaStore(s) => Some(s + 1),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    Ok(plan)
}
