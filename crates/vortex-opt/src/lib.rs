// CEP:FILE crates/vortex-opt/src/lib.rs
// CEP:WHAT Crate root for the optimizing backend pipeline: ET-SoN passes,
//        Vector IR, Machine IR with selection and allocation, scheduling,
//        ELF emission, DWARF debug info, and the linker.
// CEP:WHY vortex-opt owns everything between the ET-SoN IR and the object
//        file.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE See per-module docs.
// CEP:ASSUMES None.
// CEP:COST Library crate.
// CEP:EVIDENCE cargo test -p vortex-opt
//! VORTEX optimizing backend pipeline.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod dwarf;
pub mod emit;
pub mod link;
pub mod mach;
pub mod passes;
pub mod sched;
pub mod vector;
pub mod x64;

pub use dwarf::{emit_abbrev, emit_info, sleb, uleb, Abbrev, Die, LineHeader, LineProgram};
pub use emit::{
    emit_elf, Elf, EmitError, Object, Reloc, RelocType, Section, SectionFlags, SectionType,
    SymBind, SymType, SymVis, Symbol,
};
pub use link::{
    apply_relocations, apply_relocations_with_plt, assign_addresses, build_got_plt,
    resolve_symbols, GotPlt, Layout, LinkError, SymbolTable,
};
pub use mach::{allocate_registers, select, MInstr, MOp, MachError, MachineFunction, Unit};
pub use passes::{run_pipeline, PassStats};
pub use sched::{
    build_dependencies, list_schedule, modulo_schedule, validate, DepEdge, ScheduleError,
    Scheduled, TargetModel,
};
pub use vector::{Alignment, LaneOp, ReduceOp, ScalarTag, VError, VGraph, VNode, VType};
pub use x64::{Alu, Cond, EncodeError, Encoder, Gpr, RelocKind, Shift};
