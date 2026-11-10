// CEP:FILE crates/vortex-runtime/src/trap.rs
// CEP:WHAT Runtime trap taxonomy: every failure mode the interpreter and heap
//        can raise, with stable names used by tests and the CLI.
// CEP:WHY Law 6 (no unbounded failure): every failure path is a named value,
//        never a panic and never a silent fallback.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE This type is the failure reporting mechanism.
// CEP:ASSUMES None.
// CEP:COST Plain enum; construction is a store.
// CEP:EVIDENCE cargo test -p vortex-runtime
use core::fmt;

/// Runtime traps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trap {
    /// Array index outside `0..len`.
    BoundsCheck {
        /// Offending index.
        index: usize,
        /// Array length.
        len: usize,
    },
    /// A value did not have the tag an operation requires.
    TypeMismatch {
        /// What was required (diagnostic text).
        expected: &'static str,
        /// The 64-bit word that was found.
        word: u64,
    },
    /// A store targeted a read-only snapshot page (SNAP-INV-09).
    SnapshotViolation {
        /// The handle of the snapshot object.
        handle: u64,
    },
    /// A capability check failed (UGB-INV-01).
    SecurityError,
    /// Allocation could not be satisfied.
    OutOfMemory,
    /// Integer division or modulo by zero.
    DivByZero,
    /// The bytecode stream was malformed (interpreter-side defensive check).
    InvalidEncoding {
        /// Byte offset of the fault.
        offset: usize,
    },
    /// Freezing a heap page as a snapshot failed (e.g. external references).
    FreezeFailed {
        /// The page that could not be frozen.
        page: usize,
    },
    /// The CALL opcode is outside the reference interpreter's scope (no
    /// method-table linking implemented); the tiered design compiles calls
    /// through inline caches.
    UnsupportedCall,
    /// A throw reached the top of the handler stack.
    UnhandledException,
    /// Integer arithmetic overflowed even the boxed slow path (i64).
    ArithmeticOverflow,
}

impl fmt::Display for Trap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Trap::BoundsCheck { index, len } => {
                write!(f, "bounds check failed: index {index} len {len}")
            }
            Trap::TypeMismatch { expected, word } => {
                write!(f, "type mismatch: expected {expected}, got {word:#018x}")
            }
            Trap::SnapshotViolation { handle } => {
                write!(
                    f,
                    "snapshot violation: write to read-only handle {handle:#x}"
                )
            }
            Trap::SecurityError => write!(f, "security error: capability check failed"),
            Trap::OutOfMemory => write!(f, "out of memory"),
            Trap::DivByZero => write!(f, "division by zero"),
            Trap::InvalidEncoding { offset } => write!(f, "invalid encoding at byte {offset}"),
            Trap::FreezeFailed { page } => write!(f, "freeze failed for page {page}"),
            Trap::UnsupportedCall => write!(f, "call not supported by the reference interpreter"),
            Trap::UnhandledException => write!(f, "unhandled exception reached the top frame"),
            Trap::ArithmeticOverflow => write!(f, "arithmetic overflow beyond the boxed domain"),
        }
    }
}

impl std::error::Error for Trap {}

/// Result alias for runtime operations.
pub type RtResult<T> = Result<T, Trap>;
