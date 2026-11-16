// CEP:FILE crates/vortex-t1/src/fallback.rs
// CEP:WHAT The T0 fallback path: methods whose opcodes lack stencils stay
//        on the interpreter until every frame has drained (T1 fallback
//        policy; old design T1c install gate).
// CEP:WHY The baseline tier must never execute an unsupported stencil;
//        partial coverage is safe only with explicit fallback.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Cannot fail; classification is total.
// CEP:ASSUMES Programs are verified.
// CEP:COST O(|program|).
// CEP:EVIDENCE cargo test -p vortex-t1
//! T0 fallback classification.

use crate::plan::{plan_method, CodeObject, PlanError};
use vortex_ugb::Program;

/// Which methods the baseline tier can install.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Coverage {
    /// Methods with planned code objects.
    pub compiled: Vec<u32>,
    /// Methods that must stay on T0, with the failing opcode.
    pub fallback: Vec<(u32, &'static str)>,
}

/// Classifies the whole program into installable and fallback methods.
// CEP:WHAT Plans every method and partitions by stencil coverage.
// CEP:WHY Installation is all-or-nothing per method; partial coverage
//        must be explicit.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(|program|).
// CEP:EVIDENCE cargo test -p vortex-t1
pub fn classify(program: &Program) -> Coverage {
    let mut coverage = Coverage::default();
    for mi in 0..program.methods.len() {
        match plan_method(program, mi as u32) {
            Ok(CodeObject { .. }) => coverage.compiled.push(mi as u32),
            Err(PlanError::UnsupportedOpcode { opcode, .. }) => {
                coverage.fallback.push((mi as u32, opcode));
            }
            Err(_) => coverage.fallback.push((mi as u32, "plan")),
        }
    }
    coverage
}
