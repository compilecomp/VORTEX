// CEP:FILE crates/vortex-ugb/src/lib.rs
// CEP:WHAT Crate root for the Universal Guest Bytecode: opcode space,
//        instruction codec, program containers, verifier, and assembler.
// CEP:WHY UGB is the universal execution substrate consumed by T0 and T1;
//        this crate owns its wire format and validation gate.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE See per-module docs.
// CEP:ASSUMES None.
// CEP:COST Library crate.
// CEP:EVIDENCE cargo test -p vortex-ugb
//! Universal Guest Bytecode (UGB).
//!
//! Register-based, capability-gated bytecode per the UGB design document:
//! 49 defined operations in six categories, signature-driven variable
//! length encoding, an O(n) verifier, and a text assembly front end.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod asm;
pub mod instr;
pub mod method;
pub mod ops;
pub mod verify;

pub use asm::{assemble, disassemble, AsmError};
pub use instr::{CodecError, Instr, Operand};
pub use method::{Bounds, Constant, FfiDescriptor, Method, Program};
pub use ops::{CapReq, CmpKind, Op};
pub use verify::{verify, VerifyError};
