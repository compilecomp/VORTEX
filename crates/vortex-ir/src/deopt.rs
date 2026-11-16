// CEP:FILE crates/vortex-ir/src/deopt.rs
// CEP:WHAT Deoptimization metadata: the Safepoint payload mapping each
//        live source-level slot to its IR value or a materialized
//        constant (design 6.5 Safepoint; Deoptimization subsystem 6.6).
// CEP:WHY State reconstruction during deopt needs an exact, typed map;
//        correctness of the tiered system depends on it.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Construction is total; interpretation returns typed values
//        without panicking.
// CEP:ASSUMES Slots are caller-defined source-level identifiers.
// CEP:COST O(|slots|) per read.
// CEP:EVIDENCE cargo test -p vortex-ir
//! Deoptimization metadata (Safepoint payload).

use crate::graph::NodeId;

/// One reconstructed value at a deoptimization point.
#[derive(Debug, Clone, PartialEq)]
pub enum DeoptValue {
    /// The value lives in this IR node's result.
    Node(NodeId),
    /// The value is a materialized integer constant.
    Int(i64),
    /// The value is a materialized float constant.
    Float(f64),
    /// The value is a null reference.
    Null,
    /// The value is the NV-1 undefined special.
    Undefined,
}

/// The Safepoint payload: slot-to-value map.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeoptMeta {
    entries: Vec<(u32, DeoptValue)>,
}

impl DeoptMeta {
    /// Builds metadata from slot pairs.
    // CEP:WHAT Constructs the slot map.
    // CEP:WHY Safepoints embed this map as their payload.
    // CEP:STATUS complete
    // CEP:FAILURE Later duplicates of a slot overwrite earlier ones.
    // CEP:ASSUMES None.
    // CEP:COST O(entries).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn new(entries: Vec<(u32, DeoptValue)>) -> DeoptMeta {
        let mut meta = DeoptMeta { entries };
        meta.entries.sort_by_key(|(slot, _)| *slot);
        // Later duplicates overwrite earlier ones per the builder contract.
        let mut unique: Vec<(u32, DeoptValue)> = Vec::with_capacity(meta.entries.len());
        for (slot, value) in meta.entries {
            if unique.last().is_some_and(|(s, _)| *s == slot) {
                unique.pop();
            }
            unique.push((slot, value));
        }
        meta.entries = unique;
        meta
    }

    /// The value bound to a slot.
    // CEP:WHAT Resolves one slot.
    // CEP:WHY State reconstruction reads slots individually.
    // CEP:STATUS complete
    // CEP:FAILURE None for unbound slots.
    // CEP:ASSUMES None.
    // CEP:COST O(entries).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn slot(&self, slot: u32) -> Option<&DeoptValue> {
        self.entries
            .binary_search_by_key(&slot, |(s, _)| *s)
            .ok()
            .map(|i| &self.entries[i].1)
    }

    /// Number of bound slots.
    // CEP:WHAT Reports the map size.
    // CEP:WHY Frame reconstruction bounds its work by this count.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no slots are bound.
    // CEP:WHAT Reports emptiness.
    // CEP:WHY Empty maps occur at method entry safepoints.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterates (slot, value) pairs in slot order.
    // CEP:WHAT Enumerates the map.
    // CEP:WHY Reconstruction walks the map in order.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1) to create.
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn iter(&self) -> impl Iterator<Item = (u32, &DeoptValue)> {
        self.entries.iter().map(|(s, v)| (*s, v))
    }
}
