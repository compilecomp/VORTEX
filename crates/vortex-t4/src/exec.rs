// CEP:FILE crates/vortex-t4/src/exec.rs
// CEP:WHAT The trace driver: executes one compiled-trace iteration, and on
//        a guard failure resumes interpretation at the failing pc with the
//        reconstructed tagged frame (OSR-1 section 7.1; design.md T4
//        section 4.4), completing the rest of the method on T0.
// CEP:WHY Guarded execution must preserve T0 semantics exactly: a failed
//        speculation is not an error, it is a transfer of control to the
//        semantic reference at the precise bytecode pc.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Traps propagate as the typed Trap taxonomy; deopts transfer
//        control, never swallow state.
// CEP:ASSUMES The compiled trace came from compile::compile_trace.
// CEP:COST One interpreter resume per deopt; the trace window is O(length).
// CEP:EVIDENCE cargo test -p vortex-t4

use vortex_runtime::Machine;
use vortex_t0::Interp;

/// Outcome of one trace-driven execution attempt.
#[derive(Debug)]
pub enum TraceOutcome {
    /// The trace (or its T0 continuation) produced the method's return
    /// word; the full method completed.
    Returned(u64),
}

/// Runs the compiled trace for `method_index` with the given arguments:
/// the trace executes its window; on a deopt the interpreter resumes at
/// the failing pc; for a closed trace the interpreter then continues from
/// the exit pc (the side exit) to completion.
// CEP:WHAT Runs the compiled trace for method_index with the given arguments.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t4
pub fn run_traced(
    program: &[vortex_ugb::Method],
    method_index: usize,
    compiled: &crate::compile::CompiledTrace,
    m: &mut Machine,
    args: &[u64],
) -> Result<TraceOutcome, vortex_runtime::Trap> {
    let interp = Interp::new(program.to_vec());
    // Fresh executions overlay the caller's arguments on the recorded
    // entry frame; the remaining registers keep their OSR-style entry
    // values.
    let mut run_frame = compiled.entry_frame.clone();
    let arg_count = usize::from(program[method_index].arg_count);
    if run_frame.len() < usize::from(program[method_index].register_count) {
        run_frame.resize(
            usize::from(program[method_index].register_count),
            vortex_nv1::undefined_word(),
        );
    }
    for (i, &a) in args.iter().enumerate().take(arg_count) {
        run_frame[i] = a;
    }
    match vortex_opt::exec::run_plan_from_frame(&compiled.plan, program, m, &run_frame)? {
        vortex_opt::ExecOutcome::Deoptimized { pc, frame, .. } => {
            // Deopt: resume T0 exactly at the failing pc (OSR-INV-05
            // bookkeeping is the tier controller's concern; the driver
            // just transfers control).
            let word = interp.resume(m, method_index, pc, frame)?;
            Ok(TraceOutcome::Returned(word))
        }
        vortex_opt::ExecOutcome::Returned { word, frame } => {
            match compiled.exit_pc {
                // Open trace: the window contained the Return.
                None => Ok(TraceOutcome::Returned(word)),
                // Closed trace: resume T0 at the side-exit pc with the
                // trace's final frame; T0 re-executes the branch and
                // finishes the method.
                Some(exit_pc) => {
                    let w = interp.resume(m, method_index, exit_pc, frame)?;
                    Ok(TraceOutcome::Returned(w))
                }
            }
        }
    }
}
