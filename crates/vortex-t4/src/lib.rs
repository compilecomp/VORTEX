//! vortex-t4: the trace compiler over the shared optimizing pipeline.
// CEP:FILE crates/vortex-t4/src/lib.rs
// CEP:WHAT T4 crate root: trace recording through the reference
//        interpreter's recording hook, guarded straight-line trace
//        compilation (guards from the recorded operand domains, fusion
//        through GVN), and the trace driver with deopt-to-T0 semantics.
// CEP:WHY T4 specializes hot linear paths that method compilation handles
//        poorly (design.md section 5 revised tier structure); the guard
//        density is the specialization, and the deopt contract preserves
//        T0 semantics under speculation.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE See per-module docs; unsupported trace opcodes reject
//        compilation with typed errors (the trace stays on T0).
// CEP:ASSUMES Methods pass vortex-ugb verification before recording.
// CEP:COST Recording is one hook call per instruction; compilation matches
//        the T2 pipeline cost on the window.
// CEP:EVIDENCE cargo test -p vortex-t4

/// Guarded trace compilation through the shared pipeline.
pub mod compile;
/// The trace driver with deopt-to-T0 semantics.
pub mod exec;
/// Trace recording through the interpreter's hook.
pub mod recorder;

pub use compile::{compile_trace, CompiledTrace, TraceError};
pub use exec::{run_traced, TraceOutcome};
pub use recorder::{record_trace, Trace, TraceEntry, TRACE_LENGTH_LIMIT};
