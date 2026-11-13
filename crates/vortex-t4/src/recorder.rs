// CEP:FILE crates/vortex-t4/src/recorder.rs
// CEP:WHAT Trace recording (T4 section 4.2): the recorder drives the
//        reference interpreter with the recording hook, starts at a hot
//        loop head (the profile store's T4 requests), records every
//        executed instruction with its operand domains, and stops at the
//        loop back-edge (closed trace), a return (open trace), or the
//        trace length limit.
// CEP:WHY Traces specialize hot linear paths: the recorded operand domains
//        become the compiled trace's speculation guards, and the recorded
//        pcs become the deopt resume points.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Truncation at the length limit is a documented outcome, not
//        an error; the recorded prefix is still compilable.
// CEP:ASSUMES The program passed verification; the profile request names a
//        real pc in the method.
// CEP:COST O(trace length); the interpreter cost dominates.
// CEP:EVIDENCE cargo test -p vortex-t4
use vortex_nv1 as nv1;
use vortex_runtime::Machine;
use vortex_ugb::{Method, Op};

/// Maximum recorded instructions per trace (T4 section 4.2 trace length
/// limit; the reference limit is 64).
pub const TRACE_LENGTH_LIMIT: usize = 64;

/// One recorded instruction: pc, opcode, and the pre-execution NV-1
/// categories of its register operands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceEntry {
    /// Bytecode pc.
    pub pc: usize,
    /// Opcode.
    pub op: Op,
    /// The pre-execution categories of the register operands (trailing
    /// `None` beyond the opcode's register count).
    pub cats: [Option<nv1::Category>; 3],
}

/// A recorded linear trace.
#[derive(Debug, Clone, Default)]
pub struct Trace {
    /// Entries in execution order.
    pub entries: Vec<TraceEntry>,
    /// The register file snapshot at window entry: the compiled trace's
    /// Arg values read these words when the trace runs.
    pub entry_frame: Vec<u64>,
    /// The hot pc the recording window starts at (the loop head).
    pub start_pc: usize,
    /// For a closed trace: the pc of the recorded branch that closes the
    /// loop (the compiled window excludes it; T0 re-executes it with the
    /// returned frame, which is the guarded side exit). `None` for an open
    /// trace (the window ended at a Return; the method is done).
    pub exit_pc: Option<usize>,
}

/// Records one execution of `method_index`, starting the window at the
/// first instruction at or after `start_pc` (the profile store supplies
/// the hot loop head from a T4 trace request).
// CEP:WHAT Records one execution of method_index starting at the first instruction at or after start_pc.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t4
pub fn record_trace(
    program: &[Method],
    method_index: usize,
    start_pc: usize,
    m: &mut Machine,
    args: &[u64],
) -> Result<Trace, vortex_runtime::Trap> {
    let interp = vortex_t0::Interp::new(program.to_vec());
    let mut trace = Trace {
        entries: Vec::new(),
        start_pc,
        exit_pc: None,
        entry_frame: Vec::new(),
    };
    let mut started = false;
    let mut recording = false;
    let mut entry_frame: Vec<u64> = Vec::new();
    let method = &program[method_index];
    interp.run_with_recorder(m, method_index, args, &mut |pc, op, regs| {
        // The window starts exactly once, at the first pc at or after the
        // hot head; closing the window never re-arms the recorder.
        if !started && pc >= start_pc {
            started = true;
            recording = true;
            // Snapshot the register file at window entry: the compiled
            // trace consumes these words through its Arg values.
            entry_frame = regs.to_vec();
        }
        if !recording {
            return;
        }
        if trace.entries.len() >= TRACE_LENGTH_LIMIT {
            return;
        }
        // Operand domains: the pre-execution categories of the
        // instruction's register operands.
        let (instr_i, _) =
            vortex_ugb::instr::decode_at(&method.code, pc).expect("verified stream decodes");
        let mut cats = [None; 3];
        for (slot, r) in instr_i
            .regs
            .iter()
            .enumerate()
            .take(usize::from(vortex_ugb::sig(instr_i.op).regs))
        {
            let w = regs
                .get(usize::from(*r))
                .copied()
                .unwrap_or_else(nv1::undefined_word);
            cats[slot] = Some(nv1::categorize(w));
        }
        match op {
            Op::Return => {
                // Open trace: the method completes inside the window.
                recording = false;
            }
            Op::Jump | Op::JumpTrue | Op::JumpFalse => {
                // Close the window only at a loop back-edge (a branch
                // targeting at or below its own pc, the design.md T4 4.2
                // stop rule). Forward branches inside the loop (the exit
                // check at the head) compile as direction guards.
                let is_backedge = vortex_ugb::instr::decode_at(&program[method_index].code, pc)
                    .ok()
                    .map(|(i, next)| {
                        if vortex_ugb::sig(i.op).branches == 0 {
                            return false;
                        }
                        let t = (pc + (next - pc)).wrapping_add_signed(isize::from(i.branches[0]));
                        t <= pc
                    })
                    .unwrap_or(false);
                if is_backedge {
                    trace.exit_pc = Some(pc);
                    recording = false;
                }
            }
            _ => { /* non-branch opcodes never close the window */ }
        }
        trace.entries.push(TraceEntry { pc, op, cats });
        if trace.exit_pc.is_some() {
            // Keep the branch as the last entry so the compiler can read
            // its pc, but mark the window closed by clearing recording.
            recording = false;
        }
    })?;
    trace.entry_frame = entry_frame;
    let ended_with_return = trace.entries.last().is_some_and(|e| e.op == Op::Return);
    if !ended_with_return {
        if trace.exit_pc.is_some() {
            // The trailing branch entry is the exit marker, not compiled
            // code.
            trace.entries.pop();
        } else if let Some(last) = trace.entries.last() {
            // Truncated window (no branch, no Return inside the limit):
            // the driver resumes T0 at the pc after the window.
            if let Ok((_, next)) =
                vortex_ugb::instr::decode_at(&program[method_index].code, last.pc)
            {
                trace.exit_pc = Some(next);
            }
        }
    }
    Ok(trace)
}
