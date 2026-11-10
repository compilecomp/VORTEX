// CEP:FILE crates/vortex-ugb/src/method.rs
// CEP:WHAT Method representation: register counts, bytecode stream, constant
//        pool, per-site metadata (epoch caches, loop profile slots), and the
//        T1 stencil/OSR metadata hooks.
// CEP:WHY Methods are the unit the interpreter executes, the verifier checks,
//        and the tiering system profiles (UGB-1 section 3, OSR-1 section 6).
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Const-pool loads with out-of-range indices trap at execution;
//        the verifier reports them statically where provable.
// CEP:ASSUMES register_count <= 65536 (extended encoding limit).
// CEP:COST One allocation per method; code stream is compact bytes.
// CEP:EVIDENCE cargo test -p vortex-ugb
use crate::instr::Instr;

/// Constant-pool entry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Const {
    /// Signed 64-bit constant (may exceed the i48 immediate domain).
    Int(i64),
    /// Binary64 constant (already NV-1-canonical at build time).
    Float(f64),
}

/// Per-loop profile slot backing the sampled back-edge counter (PROF-1 v1.1
/// section 3.2): thread-local counters live in method metadata, never in the
/// instruction stream (OSR-INV-06 stencil purity).
#[derive(Debug, Clone, Copy, Default)]
pub struct LoopSlot {
    /// Iterations accumulated in the current sampling window.
    pub window: u32,
    /// Total folded iteration count.
    pub total: u64,
    /// Set when the T4 trace-recording request has been enqueued.
    pub trace_requested: bool,
}

/// Per-capability-check-site epoch cache (UGB-1 section 6.3): one slot per
/// `CapCheck`/`CapInvoke` site, shared across activations of the method.
#[derive(Debug, Clone, Copy, Default)]
pub struct EpochCache {
    /// Cached epoch of the capability last observed at this site.
    pub cached_epoch: Option<u16>,
}

/// A UGB method.
#[derive(Debug, Clone)]
pub struct Method {
    /// Method name (diagnostics only; execution uses IDs).
    pub name: String,
    /// Number of arguments; arguments occupy registers `0..arg_count`.
    pub arg_count: u16,
    /// Total virtual registers (arguments, locals, temporaries).
    pub register_count: u16,
    /// Bytecode stream (hybrid variable-length encoding).
    pub code: Vec<u8>,
    /// Constant pool.
    pub consts: Vec<Const>,
    /// Back-edge sample slots, one per loop header site, in first-appearance
    /// order of backward jumps in the stream.
    pub loop_slots: Vec<LoopSlot>,
    /// Capability epoch caches, one per capability-consuming site, in
    /// first-appearance order in the stream.
    pub epoch_caches: Vec<EpochCache>,
}

impl Method {
    /// Creates an empty method with the given name and register budget.
    #[must_use]
    pub fn new(name: impl Into<String>, arg_count: u16, register_count: u16) -> Self {
        Method {
            name: name.into(),
            arg_count,
            register_count,
            code: Vec::new(),
            consts: Vec::new(),
            loop_slots: Vec::new(),
            epoch_caches: Vec::new(),
        }
    }

    /// Number of decoded instructions (linear scan; diagnostics only).
    #[must_use]
    pub fn instruction_count(&self) -> usize {
        let mut n = 0;
        let mut p = 0;
        while p < self.code.len() {
            match crate::instr::size_at(&self.code, p) {
                Ok(size) => p += size,
                Err(_) => return n,
            }
            n += 1;
        }
        n
    }

    /// Computes the target byte offset of a decoded branch instruction.
    /// Branch offsets are relative to the end of the branch instruction.
    #[must_use]
    pub fn branch_target(&self, instr: &Instr, instr_offset: usize, instr_len: usize) -> usize {
        let off = i64::from(instr.branches[0]);
        (instr_offset + instr_len).wrapping_add_signed(off as isize)
    }
}
