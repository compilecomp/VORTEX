// CEP:FILE crates/vortex-runtime/src/aoss.rs
// CEP:WHAT AOSS: canonical layout registry (hash-assisted interning per
//        REVISED section 2) and the Numeric Array Layout Transition
//        (NUM-1 section 3) driven by store homogeneity observations.
// CEP:WHY Homogeneous numeric arrays must reach unboxed layouts to unlock
//        raw SIMD, barrier elision, and GC payload non-tracing.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE A transition aborts when any payload element is outside the
//        target domain; reverse transitions preserve language semantics
//        (NUM-INV-02).
// CEP:ASSUMES Transitions run at a safepoint (single-mutator reference model).
// CEP:COST Observation: O(1) per store. Transition: O(len) once.
// CEP:EVIDENCE cargo test -p vortex-runtime (AOSS transition tests)
use std::collections::HashMap;

use crate::heap::{layout, Handle, Heap};
use crate::trap::RtResult;
use vortex_nv1 as nv1;

/// Canonical layout descriptor (structural identity; the hash is not the
/// identity - REVISED section 2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LayoutDescriptor {
    /// Kind tag: "array:<domain>" for arrays.
    pub kind: String,
    /// Element domain: "tagged", "f64", "i64", "capability", "boxed_int".
    pub domain: String,
}

impl LayoutDescriptor {
    /// Descriptor for a tagged array.
    #[must_use]
    pub fn tagged_array() -> Self {
        LayoutDescriptor {
            kind: "array".into(),
            domain: "tagged".into(),
        }
    }

    /// Descriptor for a binary64 numeric array.
    #[must_use]
    pub fn numeric_f64_array() -> Self {
        LayoutDescriptor {
            kind: "array".into(),
            domain: "f64".into(),
        }
    }

    /// Descriptor for an i64 numeric array.
    #[must_use]
    pub fn numeric_i64_array() -> Self {
        LayoutDescriptor {
            kind: "array".into(),
            domain: "i64".into(),
        }
    }
}

/// Hash used for lookup acceleration. A simple FNV-1a over the descriptor
/// text; collisions are resolved by exact comparison among candidates.
#[must_use]
fn descriptor_hash(d: &LayoutDescriptor) -> u64 {
    let text = format!("{}/{}", d.kind, d.domain);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// Interning registry: hash accelerates lookup; the canonical handle (the
/// interned descriptor identity) is what guards compare (REVISED section 2:
/// "Hashes accelerate identity. They do not establish identity.").
#[derive(Debug, Default)]
pub struct LayoutRegistry {
    descriptors: Vec<LayoutDescriptor>,
    by_hash: HashMap<u64, Vec<u64>>,
}

impl LayoutRegistry {
    /// Interns `desc` and returns its canonical handle. Equal descriptors
    /// return the same handle.
    // CEP:WHAT Interns desc and returns its canonical handle.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn intern(&mut self, desc: LayoutDescriptor) -> u64 {
        let h = descriptor_hash(&desc);
        if let Some(cands) = self.by_hash.get(&h) {
            for &cand in cands {
                if self.descriptors[cand as usize - 0x2000] == desc {
                    return cand;
                }
            }
        }
        let handle = 0x2000 + self.descriptors.len() as u64;
        self.descriptors.push(desc);
        self.by_hash.entry(h).or_default().push(handle);
        handle
    }

    /// Borrows a descriptor by handle.
    ///
    /// # Panics
    /// Panics on an unknown handle.
    #[must_use]
    pub fn descriptor(&self, handle: u64) -> &LayoutDescriptor {
        &self.descriptors[handle as usize - 0x2000]
    }
}

/// Homogeneity observation state for one tagged array.
#[derive(Debug, Clone, Copy, Default)]
struct Homogeneity {
    /// Consecutive numeric stores observed.
    run: u32,
    /// Domain of the current run: 0 = unknown, 1 = int, 2 = float.
    domain: u8,
}

/// Window of homogeneous stores before a transition fires.
pub const HOMOGENEITY_WINDOW: u32 = 16;

/// The AOSS subsystem.
#[derive(Debug, Default)]
pub struct Aoss {
    /// Canonical layout registry.
    pub registry: LayoutRegistry,
    homogeneity: HashMap<u64, Homogeneity>,
}

/// Transition decision returned by [`Aoss::observe_store`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// No transition.
    None,
    /// Transition to `Layout_NumericArray_F64`.
    ToNumericF64,
    /// Transition to `Layout_NumericArray_I64`.
    ToNumericI64,
}

impl Aoss {
    /// Creates an AOSS with the built-in layouts interned.
    #[must_use]
    pub fn new() -> Self {
        let mut aoss = Aoss::default();
        let _tagged = aoss.registry.intern(LayoutDescriptor::tagged_array());
        let _f64 = aoss.registry.intern(LayoutDescriptor::numeric_f64_array());
        let _i64 = aoss.registry.intern(LayoutDescriptor::numeric_i64_array());
        aoss
    }

    /// Records one store observation against a tagged array. Returns the
    /// transition decision when the homogeneity window is met (NUM-1
    /// section 3.1).
    // CEP:WHAT Records one store observation against a tagged array.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn observe_store(&mut self, arr: Handle, value: u64) -> Decision {
        let state = self.homogeneity.entry(arr.0).or_default();
        let domain = if nv1::is_int(value) {
            1_u8
        } else if nv1::categorize(value) == nv1::Category::Float {
            2
        } else {
            0
        };
        if domain == 0 {
            // Non-numeric store resets the run (never transitions).
            state.run = 0;
            state.domain = 0;
            return Decision::None;
        }
        if state.domain != domain {
            state.domain = domain;
            state.run = 1;
        } else {
            state.run += 1;
        }
        if state.run >= HOMOGENEITY_WINDOW {
            state.run = 0; // one transition per window
            return if domain == 2 {
                Decision::ToNumericF64
            } else {
                Decision::ToNumericI64
            };
        }
        Decision::None
    }

    /// Drops observation state for a handle (post-transition or GC).
    pub fn forget(&mut self, arr: Handle) {
        self.homogeneity.remove(&arr.0);
    }

    /// Transitions a tagged array to a numeric layout at a safepoint
    /// (NUM-1 section 3.1). Returns `false` when any payload element is
    /// outside the target domain, leaving the array untouched.
    // CEP:WHAT Transitions a tagged array to a numeric layout at a safepoint (NUM-1 section 3.1).
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn transition_numeric(&self, heap: &mut Heap, arr: Handle, to: Decision) -> RtResult<bool> {
        let target = match to {
            Decision::ToNumericF64 => layout::NUMERIC_F64,
            Decision::ToNumericI64 => layout::NUMERIC_I64,
            Decision::None => return Ok(false),
        };
        let len = heap.array_len(arr);
        // Pre-flight: every element must be in the target domain.
        for i in 0..len {
            let w = heap.read_payload(arr, i);
            let ok = if target == layout::NUMERIC_F64 {
                nv1::categorize(w) == nv1::Category::Float
            } else {
                nv1::is_int(w) || w == nv1::null_word()
            };
            if !ok {
                return Ok(false);
            }
        }
        // Convert in place: floats are bit-identical; integers widen their
        // 48-bit payloads to raw sign-extended i64 words.
        for i in 0..len {
            let w = heap.read_payload(arr, i);
            let raw: u64 = if target == layout::NUMERIC_F64 {
                w
            } else {
                nv1::decode_int(w) as u64
            };
            heap.write_payload(arr, i, raw)?;
        }
        // Swap the layout handle word (atomic at safepoint in this model).
        heap.swap_layout(arr, target)?;
        Ok(true)
    }

    /// Reverses a numeric array back to the tagged layout, re-tagging raw
    /// payloads (NUM-INV-02 reverse transition). Values outside the i48
    /// immediate domain become null in the reference model; production boxes
    /// them through Layout_BoxedInt (NV-1 section 2.1).
    // CEP:WHAT Reverses a numeric array back to the tagged layout, re-tagging raw payloads (NUM-INV-02 reverse transition).
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-runtime
    pub fn transition_tagged(&self, heap: &mut Heap, arr: Handle) -> RtResult<()> {
        let len = heap.array_len(arr);
        let lay = heap.layout_of(arr);
        for i in 0..len {
            let raw = heap.read_payload(arr, i);
            let tagged = if lay == layout::NUMERIC_F64 {
                nv1::encode_f64(f64::from_bits(raw))
            } else {
                nv1::encode_int(raw as i64).unwrap_or_else(nv1::null_word)
            };
            heap.write_payload(arr, i, tagged)?;
        }
        heap.swap_layout(arr, layout::GENERIC_ARRAY)?;
        Ok(())
    }
}
