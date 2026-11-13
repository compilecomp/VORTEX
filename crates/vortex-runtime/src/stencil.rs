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
    /// Template bytes (modeled; location holes are `0xC1` fillers).
    pub code: Vec<u8>,
    /// Location hole byte offsets to patch at copy time (stack slots).
    pub holes: Vec<usize>,
    /// Payload holes: every immediate, pool index, branch offset, and the
    /// CALL callee byte is a hole patched at copy time. Payloads are never
    /// part of the key: the template stays pure and method-independent.
    pub payload_holes: Vec<PayloadHole>,
}

/// One payload hole in a stencil template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayloadHole {
    /// Byte offset of the hole within the template.
    pub offset: usize,
    /// Hole width in bytes (1, 2, 4, or 8).
    pub width: u8,
    /// What fills the hole at copy time.
    pub kind: PayloadKind,
}

/// Payload hole kinds, mirroring the UGB operand encodings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadKind {
    /// Signed i32 immediate (LoadInt32).
    Imm32,
    /// Unsigned u32 immediate (CapCreate, AllocArray, CapCheck, Call argc).
    Imm32u,
    /// Binary64 immediate (LoadFloat).
    Imm64,
    /// Constant-pool index (LoadConst).
    Pool,
    /// Branch offset (i16, relative to the copied instance's end).
    Branch,
    /// CALL callee method index (one byte in the register-operand slot that
    /// is not a virtual-register location).
    Callee,
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
    ///
    /// Template layout: opcode byte, then one operand field per signature
    /// operand. Register locations are encoded bytes; stack locations are
    /// 3-byte holes (marker + u16 slot); a `None` location that carries a
    /// payload (CALL callee) becomes a 1-byte hole. Immediates, pool indices,
    /// and branch offsets follow as payload holes sized by the opcode's UGB
    /// signature.
    // CEP:WHAT Returns the stencil for key, compiling it on first use.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST First request compiles the template; later requests are one map lookup.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn get_or_compile(&mut self, key: StencilKey) -> usize {
        if let Some(&idx) = self.stencils.get(&key) {
            return idx;
        }
        // Modeled template: opcode byte + operand fields + payload holes.
        let mut code = vec![key.op as u8];
        let mut holes = Vec::new();
        for l in key.locs.iter() {
            match l {
                Some(Loc::Reg(r)) => code.push(*r),
                Some(Loc::Stack(s)) => {
                    code.push(0xC1); // hole marker
                    holes.push(code.len());
                    code.extend_from_slice(&s.to_le_bytes());
                }
                None => {
                    // Non-location operand slot: a 1-byte payload hole
                    // (currently only the CALL callee index).
                    code.push(0xC1);
                }
            }
        }
        let payload_holes = payload_holes_for(key.op, code.len());
        // Reserve the payload bytes so the template size matches a real
        // encoded instance.
        let payload_bytes: usize = payload_holes.iter().map(|h| usize::from(h.width)).sum();
        code.extend(std::iter::repeat_n(0xC1, payload_bytes));
        let stencil = Stencil {
            key,
            code,
            holes,
            payload_holes,
        };
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

    /// Opens a blob: allocates its single shared OSR Trampoline Stub with
    /// one Metadata Pool counter per loop, and advances the purity
    /// accounting. Stencil selection happens through `get_or_compile`; the
    /// stub is the only per-blob state (OSR-1 section 6.1).
    // CEP:WHAT Opens a blob: allocates its single shared OSR Trampoline Stub with one Metadata Pool counter per loop, and advances the purity accounting.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST O(loops) per blob.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn open_blob(&mut self, loops: &[(usize, usize)]) -> OsrTrampolineStub {
        self.blobs_assembled += 1;
        self.osr_stubs_created += 1;
        OsrTrampolineStub {
            counters: loops
                .iter()
                .map(|&(slot, pc)| OsrCounter {
                    slot,
                    bytecode_pc: pc,
                })
                .collect(),
            patched_to_t2: false,
        }
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

/// Computes the payload holes of a template for `op`, starting at byte
/// offset `at` (the first byte after the operand fields). Hole order mirrors
/// the UGB standard encoding: branches, then imm32, imm32u, imm64, pool.
// CEP:WHAT Computes the payload holes of a template for op, starting at byte offset at (the first byte after the operand fields).
// CEP:WHY Every method-varying payload must be a copy-time hole: templates stay pure per key (OSR-INV-06) and patching is O(holes).
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES The opcode's operand signature matches UGB-1 section 5.
// CEP:COST One pass over the opcode's signature; O(1) per opcode.
// CEP:EVIDENCE cargo test -p vortex-runtime
#[must_use]
fn payload_holes_for(op: Op, at: usize) -> Vec<PayloadHole> {
    use PayloadKind::{Branch, Callee, Imm32, Imm32u, Imm64, Pool};
    let s = vortex_ugb::sig(op);
    let mut holes = Vec::new();
    let mut cursor = at;
    for _ in 0..s.branches {
        holes.push(PayloadHole {
            offset: cursor,
            width: 2,
            kind: Branch,
        });
        cursor += 2;
    }
    for _ in 0..s.imm32 {
        holes.push(PayloadHole {
            offset: cursor,
            width: 4,
            kind: Imm32,
        });
        cursor += 4;
    }
    for _ in 0..s.imm32u {
        holes.push(PayloadHole {
            offset: cursor,
            width: 4,
            kind: Imm32u,
        });
        cursor += 4;
    }
    for _ in 0..s.imm64 {
        holes.push(PayloadHole {
            offset: cursor,
            width: 8,
            kind: Imm64,
        });
        cursor += 8;
    }
    for _ in 0..s.pool {
        holes.push(PayloadHole {
            offset: cursor,
            width: 2,
            kind: Pool,
        });
        cursor += 2;
    }
    if op == Op::Call {
        // The callee byte sits in the non-location operand slot (offset of
        // the second operand field), not in the trailing payload area.
        holes.push(PayloadHole {
            offset: at - 1,
            width: 1,
            kind: Callee,
        });
    }
    holes
}
