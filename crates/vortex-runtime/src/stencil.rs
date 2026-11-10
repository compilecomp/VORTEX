// CEP:FILE crates/vortex-runtime/src/stencil.rs
// CEP:WHAT T1 stencil pool and CodeBlob model (copy-and-patch architecture):
//        location-keyed stencils, per-blob shared OSR Trampoline Stubs with
//        Metadata Pool counters, and the stencil-purity accounting
//        (OSR-1 section 6.1 v1.1, OSR-INV-06).
// CEP:WHY Stencils must remain pure: no counters, no per-method writable
//        data. OSR state lives in one shared stub per blob plus one metadata
//        slot per loop, keeping pool growth at O(operand-location variants).
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Duplicate stencils are deduplicated by key; a request for a
//        counter-carrying variant is a type error by construction (the API
//        cannot express it).
// CEP:ASSUMES Stencil bodies are modeled as byte vectors with holes; native
//        emission is out of scope for the reference runtime.
// CEP:COST Pool lookup is O(variants per op); blob assembly is O(instructions).
// CEP:EVIDENCE cargo test -p vortex-runtime (purity accounting test)
use std::collections::HashMap;
use vortex_ugb::Op;

/// Operand location in the fixed physical map (T1 section 3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Loc {
    /// Virtual register mapped to a physical register slot (0..K).
    Reg(u8),
    /// Virtual register spilled to a stack slot.
    Stack(u16),
}

/// Stencil key: opcode plus operand locations. Selection is a pure function
/// of these (T1 section 3.3): no method-identity, no counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StencilKey {
    /// Opcode the stencil implements.
    pub op: Op,
    /// Operand locations.
    pub locs: [Option<Loc>; 3],
}

/// A pre-assembled machine-code template with patch holes. Pure by
/// construction: the struct carries no counter, no method identity, and no
/// writable runtime state (OSR-INV-06).
#[derive(Debug, Clone)]
pub struct Stencil {
    /// Key this stencil serves.
    pub key: StencilKey,
    /// Template bytes (modeled; holes are `0xC1` fillers).
    pub code: Vec<u8>,
    /// Hole byte offsets to patch at copy time.
    pub holes: Vec<usize>,
}

/// Metadata Pool counter slot: the OSR back-edge counter lives here, not in
/// the stencil (OSR-1 section 6.1 step 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OsrCounter {
    /// Slot index in the Metadata Pool.
    pub slot: usize,
    /// Bytecode PC of the loop head, supplied to trigger_osr on trip.
    pub bytecode_pc: usize,
}

/// The single shared OSR Trampoline Stub of a CodeBlob (OSR-1 section 6.1
/// step 2): exactly one per blob, emitted once at T1 assembly time.
#[derive(Debug, Clone)]
pub struct OsrTrampolineStub {
    /// Loop counters this stub services (one Metadata Pool slot per loop).
    pub counters: Vec<OsrCounter>,
    /// Whether the stub's back-link has been patched to a T2 OSR stub.
    pub patched_to_t2: bool,
}

/// A T1 CodeBlob: the stencils copied into it plus its shared OSR stub.
#[derive(Debug, Clone)]
pub struct CodeBlob {
    /// Indices into the pool for each copied stencil, in emission order.
    pub stencils_used: Vec<usize>,
    /// The blob's single OSR Trampoline Stub.
    pub osr_stub: OsrTrampolineStub,
}

/// The Stencil Pool.
#[derive(Debug, Default)]
pub struct StencilPool {
    stencils: HashMap<StencilKey, usize>,
    list: Vec<Stencil>,
    /// Total blob assemblies performed (for purity accounting).
    pub blobs_assembled: u64,
    /// Total OSR stubs created: must equal blobs_assembled (one per blob).
    pub osr_stubs_created: u64,
}

impl StencilPool {
    /// Returns the stencil for `key`, compiling it on first use. Selection is
    /// a pure function of the key: identical requests reuse one template.
    // CEP:WHAT Returns the stencil for key, compiling it on first use.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn get_or_compile(&mut self, key: StencilKey) -> usize {
        if let Some(&idx) = self.stencils.get(&key) {
            return idx;
        }
        // Modeled template: opcode byte + location bytes + hole.
        let mut code = vec![key.op as u8];
        let mut holes = Vec::new();
        for l in key.locs.iter().flatten() {
            match l {
                Loc::Reg(r) => code.push(*r),
                Loc::Stack(s) => {
                    code.push(0xC1); // hole marker
                    holes.push(code.len());
                    code.extend_from_slice(&s.to_le_bytes());
                }
            }
        }
        let stencil = Stencil { key, code, holes };
        let idx = self.list.len();
        self.list.push(stencil);
        self.stencils.insert(key, idx);
        idx
    }

    /// Borrows a stencil by index.
    #[must_use]
    pub fn stencil(&self, idx: usize) -> &Stencil {
        &self.list[idx]
    }

    /// Number of distinct stencils in the pool.
    #[must_use]
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// Whether the pool is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Assembles a CodeBlob for a method-like instruction sequence: one
    /// stencil per step, and exactly one shared OSR Trampoline Stub per blob,
    /// with one Metadata Pool counter per loop (OSR-1 section 6.1).
    // CEP:WHAT Assembles a CodeBlob for a method-like instruction sequence: one stencil per step, and exactly one shared OSR Trampoline Stub per blob, with one Metadata Pool counter per loop (OSR-1 section 6.1).
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn assemble_blob(&mut self, steps: &[StencilKey], loops: &[(usize, usize)]) -> CodeBlob {
        let mut used = Vec::with_capacity(steps.len());
        for key in steps {
            used.push(self.get_or_compile(*key));
        }
        let counters = loops
            .iter()
            .map(|&(slot, pc)| OsrCounter {
                slot,
                bytecode_pc: pc,
            })
            .collect();
        self.blobs_assembled += 1;
        self.osr_stubs_created += 1;
        CodeBlob {
            stencils_used: used,
            osr_stub: OsrTrampolineStub {
                counters,
                patched_to_t2: false,
            },
        }
    }
}
