// CEP:FILE crates/vortex-ir/src/types.rs
// CEP:WHAT The ET-SoN type lattice, effect lattice, FP flag set, and the
//        subtype relation (design 6.4).
// CEP:WHY Validity invariant 1 (type consistency) and 7 (FP-flag validity)
//        are defined over these lattices; they must be exact.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Subtyping is total over the type enum; no case is silently
//        treated as compatible.
// CEP:ASSUMES Types are structural; user-defined nominal types are
//        represented by an opaque nominal id.
// CEP:COST O(1) per comparison except structural recursion on aggregates.
// CEP:EVIDENCE cargo test -p vortex-ir
//! Type system and effect lattice (ET-SoN design 6.4).

/// Data types of the IR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// Signed integers.
    Int8,
    /// Signed integer.
    Int16,
    /// Signed integer.
    Int32,
    /// Signed integer.
    Int64,
    /// Unsigned integer.
    UInt8,
    /// Unsigned integer.
    UInt16,
    /// Unsigned integer.
    UInt32,
    /// Unsigned integer.
    UInt64,
    /// IEEE binary32.
    Float32,
    /// IEEE binary64.
    Float64,
    /// Boolean.
    Bool,
    /// Control-flow token.
    Ctrl,
    /// State token of one region class.
    State(u32),
    /// Pointer to a pointee type.
    Ptr(Box<Type>),
    /// Aggregate tuple.
    Tuple(Vec<Type>),
    /// Fixed-size array.
    Array(Box<Type>, u32),
    /// Function type: argument types then return type.
    Fn(Vec<Type>, Box<Type>),
    /// Nominal user type.
    Nominal(u32),
}

impl Type {
    /// Whether `self` is a subtype of `other` (structural rules).
    // CEP:WHAT Implements the subtype relation.
    // CEP:WHY Invariant 1 checks every data edge with this relation.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES Standard structural subtyping; nominal types compare by
    //        identifier equality.
    // CEP:COST O(structure) for aggregates and arrays.
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub fn is_subtype(&self, other: &Type) -> bool {
        match (self, other) {
            (a, b) if a == b => true,
            (Type::Ptr(a), Type::Ptr(b)) => a.is_subtype(b),
            (Type::Tuple(a), Type::Tuple(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.is_subtype(y))
            }
            (Type::Array(a, n1), Type::Array(b, n2)) => n1 == n2 && a.is_subtype(b),
            (Type::Fn(a1, r1), Type::Fn(a2, r2)) => {
                a1.len() == a2.len()
                    && a1.iter().zip(a2.iter()).all(|(x, y)| y.is_subtype(x))
                    && r1.is_subtype(r2)
            }
            _ => false,
        }
    }
}

/// Primitive effect kinds (design 6.4); an effect set is a subset of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Effect {
    /// Reads memory.
    MemRead,
    /// Writes memory.
    MemWrite,
    /// Allocates memory.
    Alloc,
    /// Calls code.
    Call,
    /// Traps or throws.
    Trap,
    /// Memory or scheduling barrier.
    Barrier,
}

/// An effect set: a subset of the primitive kinds (powerset lattice).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EffectSet {
    bits: u8,
}

impl EffectSet {
    /// The empty set (pure).
    // CEP:WHAT Returns the bottom effect set.
    // CEP:WHY Pure nodes carry no state-token edges.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub const PURE: EffectSet = EffectSet { bits: 0 };

    /// Single-element set.
    // CEP:WHAT Builds a one-element effect set.
    // CEP:WHY Signatures list exact effects per node.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub const fn of(e: Effect) -> EffectSet {
        EffectSet {
            bits: 1 << (e as u8),
        }
    }

    /// Builds a set from two kinds.
    // CEP:WHAT Builds a two-element effect set.
    // CEP:WHY Atomic nodes carry read+barrier or read+write+barrier.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub const fn of2(a: Effect, b: Effect) -> EffectSet {
        EffectSet {
            bits: (1 << (a as u8)) | (1 << (b as u8)),
        }
    }

    /// Builds a set from three kinds.
    // CEP:WHAT Builds a three-element effect set.
    // CEP:WHY AtomicRMW carries read+write+barrier.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub const fn of3(a: Effect, b: Effect, c: Effect) -> EffectSet {
        EffectSet {
            bits: (1 << (a as u8)) | (1 << (b as u8)) | (1 << (c as u8)),
        }
    }

    /// Whether the set contains a kind.
    // CEP:WHAT Membership test.
    // CEP:WHY Ordering decisions query kinds directly.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub const fn contains(self, e: Effect) -> bool {
        self.bits & (1 << (e as u8)) != 0
    }

    /// Whether the set is empty (pure).
    // CEP:WHAT Reports purity.
    // CEP:WHY E = empty means no state-token edges.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub const fn is_pure(self) -> bool {
        self.bits == 0
    }
}

/// Floating-point relaxation flags (design 6.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FpFlag {
    /// Signed zeros interchangeable.
    Nsz,
    /// Infinities do not arise.
    Ninf,
    /// NaNs do not arise.
    Nnan,
    /// Reciprocal approximation allowed.
    Arcp,
    /// Contraction into FMA allowed.
    Contract,
    /// Reassociation allowed.
    Reassoc,
    /// Approximate functions; implies all others.
    Afn,
}

/// An FP flag set (design 6.5 flag list).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FpSet {
    bits: u8,
}

impl FpSet {
    /// The strict IEEE 754 set (empty).
    // CEP:WHAT Returns the strict FP flag set.
    // CEP:WHY Default source-level semantics are strict (6.5).
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub const STRICT: FpSet = FpSet { bits: 0 };

    /// Adds a flag.
    // CEP:WHAT Adds one flag to the set.
    // CEP:WHY Flag sets are built incrementally by front ends.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub const fn with(self, f: FpFlag) -> FpSet {
        FpSet {
            bits: self.bits | (1 << (f as u8)),
        }
    }

    /// Whether the set contains a flag.
    // CEP:WHAT Membership test.
    // CEP:WHY Transform licensing queries flags.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub const fn contains(self, f: FpFlag) -> bool {
        self.bits & (1 << (f as u8)) != 0
    }

    /// Whether the set satisfies invariant 7 (AFN implies all).
    // CEP:WHAT Validates the AFN-implies-all rule.
    // CEP:WHY Invariant 7 forbids AFN without the other relaxations.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ir
    pub const fn valid(self) -> bool {
        let all_but_afn = (1 << (FpFlag::Afn as u8)) - 1;
        if self.contains(FpFlag::Afn) {
            return self.bits & all_but_afn == all_but_afn;
        }
        true
    }
}
