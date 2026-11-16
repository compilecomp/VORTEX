// CEP:FILE crates/vortex-runtime/src/trap.rs
// CEP:WHAT The typed trap taxonomy: every failure mode the interpreter,
//        heap, capability system, and collector can raise, with stable
//        names used by tests, the CLI, and the tier controller.
// CEP:WHY Law 6 (no unbounded failure): every failure path is a named
//        value, never a panic and never a silent fallback.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE This type is the failure reporting mechanism.
// CEP:ASSUMES None.
// CEP:COST Plain enum; construction is a store.
// CEP:EVIDENCE cargo test -p vortex-runtime
//! Runtime trap taxonomy.
use core::fmt;

/// Runtime traps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trap {
    /// Array index outside the object bounds.
    BoundsCheck {
        /// Offending index.
        index: usize,
        /// Object length.
        len: usize,
    },
    /// A value did not have the tag an operation requires.
    TypeMismatch {
        /// What was required (diagnostic text).
        expected: &'static str,
        /// The 64-bit word that was found.
        word: u64,
    },
    /// A capability check failed (UGB 6.9 P5).
    CapabilityViolation {
        /// The permission bit that was required.
        perm: u16,
    },
    /// Allocation could not be satisfied after a failed expansion.
    OutOfMemory,
    /// Integer division or modulo by zero.
    DivByZero,
    /// The bytecode stream was malformed at the interpreter boundary.
    InvalidEncoding {
        /// Byte offset of the fault.
        offset: usize,
    },
    /// The call frame stack exceeded its documented depth bound.
    CallStackOverflow {
        /// The depth bound that was exceeded.
        bound: usize,
    },
    /// A throw reached the top of the handler stack.
    UnhandledException,
    /// Integer arithmetic overflowed even the boxed slow path (i64).
    ArithmeticOverflow,
    /// A raw word carried the NV-1 invalid category at a boundary.
    InvalidValue {
        /// The rejected word.
        word: u64,
    },
    /// A collector invariant failed; this is an internal error, not
    /// user-visible (GC 6.14 item 8).
    GcInternal {
        /// Short description of the violated invariant.
        what: &'static str,
    },
    /// Deoptimization state reconstruction found inconsistent metadata.
    DeoptStateCorrupt {
        /// Short description of the inconsistency.
        what: &'static str,
    },
    /// A heap reference escaped its window or pointed outside the arena.
    BadReference {
        /// The offending 48-bit offset.
        offset: u64,
    },
}

impl fmt::Display for Trap {
    // CEP:WHAT Renders a trap for logs and the CLI.
    // CEP:WHY Operators need the failing site and bound without a debugger.
    // CEP:STATUS complete
    // CEP:FAILURE Formatter errors only.
    // CEP:ASSUMES None.
    // CEP:COST O(name length).
    // CEP:EVIDENCE cargo test -p vortex-runtime
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Trap::BoundsCheck { index, len } => {
                write!(f, "bounds check failed: index {index} len {len}")
            }
            Trap::TypeMismatch { expected, word } => {
                write!(f, "type mismatch: expected {expected}, got {word:#018x}")
            }
            Trap::CapabilityViolation { perm } => {
                write!(f, "capability violation: permission {perm:#06x} required")
            }
            Trap::OutOfMemory => write!(f, "out of memory"),
            Trap::DivByZero => write!(f, "division by zero"),
            Trap::InvalidEncoding { offset } => {
                write!(f, "invalid encoding at byte {offset}")
            }
            Trap::CallStackOverflow { bound } => {
                write!(f, "call stack overflow beyond the {bound}-frame bound")
            }
            Trap::UnhandledException => {
                write!(f, "unhandled exception reached the top frame")
            }
            Trap::ArithmeticOverflow => {
                write!(f, "arithmetic overflow beyond the boxed domain")
            }
            Trap::InvalidValue { word } => write!(f, "invalid value word {word:#018x}"),
            Trap::GcInternal { what } => write!(f, "collector internal error: {what}"),
            Trap::DeoptStateCorrupt { what } => {
                write!(f, "deopt state corrupt: {what}")
            }
            Trap::BadReference { offset } => {
                write!(f, "heap reference {offset:#x} outside the arena")
            }
        }
    }
}

impl std::error::Error for Trap {}

/// Result alias for runtime operations.
pub type RtResult<T> = Result<T, Trap>;
