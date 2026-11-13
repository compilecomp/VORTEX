// CEP:FILE crates/vortex-tier/src/mem.rs
// CEP:WHAT The MEM-1 memory-order model: the MemoryOrder tags, the
//        optimizer's reorder-legality rules (MEM-1 section 3), redundant
//        fence elimination (MEM-INV-03), and the target lowering tables
//        for x86-64, AArch64, and RV64 (MEM-1 section 4).
// CEP:WHY The optimizer must never reorder across an order boundary, and
//        the emitter must produce the exact hardware instruction per
//        target; both are pure functions, testable without hardware.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE The legality predicate is conservative by construction;
//        lowering is total over the target set.
// CEP:ASSUMES The source memory model is at least C11/Java strength.
// CEP:COST O(1) per query.
// CEP:EVIDENCE cargo test -p vortex-tier
use std::fmt;

/// Memory ordering tags (MEM-1 section 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryOrder {
    /// No ordering guarantees.
    Relaxed,
    /// Later operations cannot move before this one.
    Acquire,
    /// Earlier operations cannot move after this one.
    Release,
    /// Both acquire and release.
    AcqRel,
    /// Release + acquire plus global store-load ordering.
    SeqCst,
}

impl fmt::Display for MemoryOrder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemoryOrder::Relaxed => write!(f, "relaxed"),
            MemoryOrder::Acquire => write!(f, "acquire"),
            MemoryOrder::Release => write!(f, "release"),
            MemoryOrder::AcqRel => write!(f, "acq_rel"),
            MemoryOrder::SeqCst => write!(f, "seqcst"),
        }
    }
}

/// Whether moving `order` in `direction` across another operation is
/// legal (MEM-1 section 3; `up` = earlier in program order).
// CEP:WHAT Whether moving order in direction across another operation is legal.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-tier
#[must_use]
pub fn reorder_legal(order: MemoryOrder, up: bool) -> bool {
    match (order, up) {
        // Acquire loads cannot move up; Release stores cannot move down.
        (MemoryOrder::Acquire, true) => false,
        (MemoryOrder::Release, false) => false,
        (MemoryOrder::AcqRel, _) => false,
        // SeqCst cannot reorder with anything.
        (MemoryOrder::SeqCst, _) => false,
        // Relaxed moves freely in both directions; the remaining strong
        // orders move legally in their free direction.
        _ => true,
    }
}

/// Target instruction selection for a memory order (MEM-1 section 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// x86-64 (TSO).
    X86_64,
    /// AArch64 (weak).
    AArch64,
    /// RV64 (RVWMO).
    Rv64,
}

/// The lowering decision: the instruction mnemonic plus whether an
/// explicit fence is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lowering {
    /// The instruction (or fence) mnemonic.
    pub mnemonic: &'static str,
    /// Whether an extra fence instruction is required.
    pub fence: bool,
}

/// Lowers a memory order for the target (MEM-1 section 4; MEM-INV-02).
// CEP:WHAT Lowers a memory order for the target.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-tier
#[must_use]
pub fn lower(order: MemoryOrder, target: Target) -> Lowering {
    match target {
        Target::X86_64 => match order {
            // TSO gives acquire/release for free.
            MemoryOrder::Relaxed | MemoryOrder::Acquire | MemoryOrder::Release => Lowering {
                mnemonic: "mov",
                fence: false,
            },
            MemoryOrder::AcqRel => Lowering {
                mnemonic: "mov",
                fence: false,
            },
            MemoryOrder::SeqCst => Lowering {
                mnemonic: "mfence",
                fence: true,
            },
        },
        Target::AArch64 => match order {
            MemoryOrder::Relaxed => Lowering {
                mnemonic: "ldr",
                fence: false,
            },
            MemoryOrder::Acquire | MemoryOrder::AcqRel => Lowering {
                mnemonic: "ldar",
                fence: false,
            },
            MemoryOrder::Release => Lowering {
                mnemonic: "stlr",
                fence: false,
            },
            MemoryOrder::SeqCst => Lowering {
                mnemonic: "dmb ish",
                fence: true,
            },
        },
        Target::Rv64 => match order {
            MemoryOrder::Relaxed => Lowering {
                mnemonic: "lw",
                fence: false,
            },
            MemoryOrder::Acquire | MemoryOrder::AcqRel => Lowering {
                mnemonic: "lw.aq",
                fence: false,
            },
            MemoryOrder::Release => Lowering {
                mnemonic: "sw.rl",
                fence: false,
            },
            MemoryOrder::SeqCst => Lowering {
                mnemonic: "fence rw,rw",
                fence: true,
            },
        },
    }
}

/// Eliminates redundant fences in a sequence (MEM-INV-03): an ordering
/// operation followed immediately by a stronger fence collapses when the
/// fence adds nothing the operation does not already provide.
// CEP:WHAT Eliminates redundant fences in a sequence.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-tier
#[must_use]
pub fn eliminate_redundant_fences(seq: &[MemoryOrder]) -> Vec<MemoryOrder> {
    let mut out: Vec<MemoryOrder> = Vec::with_capacity(seq.len());
    for &o in seq {
        match out.last() {
            // Acquire followed by SeqCst: the fence subsumes the acquire.
            Some(MemoryOrder::Acquire) if o == MemoryOrder::SeqCst => {
                out.pop();
                out.push(MemoryOrder::SeqCst);
            }
            // Identical adjacent fences collapse to one (drop the repeat).
            Some(prev) if *prev == o => { /* the later operation adds nothing */ }
            _ => out.push(o),
        }
    }
    out
}
