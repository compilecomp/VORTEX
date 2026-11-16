// CEP:FILE crates/vortex-nv1/src/lib.rs
// CEP:WHAT NV-1 value word: NaN-boxed 64-bit encoding with immediate integer,
//        heap-relative reference, special-immediate, and binary64 categories;
//        classification, validation, and checked integer arithmetic.
// CEP:WHY T0 registers, UGB constants, and the heap need one unforgeable
//        representation that survives GC relocation without embedding native
//        pointers (NV-1 sections 2-4, invariant list INV-001..INV-015).
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Constructors return typed errors for out-of-range payloads;
//        classification never panics; the invalid category can never be
//        stored in a `Value` (INV-002, INV-015).
// CEP:ASSUMES IEEE-754 binary64 bit layout; heap references are 48-bit
//        unsigned byte offsets from a runtime heap base; offset zero and the
//        0xFFFF prefix group are reserved.
// CEP:COST O(1) per operation, no allocation, no hidden cost (Law 1).
// CEP:EVIDENCE cargo test -p vortex-nv1
//! NV-1: the NaN-boxed 64-bit value word shared by T0 registers, UGB
//! constants, and the runtime heap; encoding, classification, validation.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use core::fmt;

/// Mask selecting the 14 upper reserved bits of the tag region.
pub const RESERVED_MASK: u64 = 0xFFFC_0000_0000_0000;
/// The 14 upper bits shared by all four reserved categories.
pub const RESERVED_PREFIX: u64 = 0xFFFC_0000_0000_0000;
/// Prefix of the immediate-integer category (tag bits 00).
pub const INT_PREFIX: u64 = 0xFFFC_0000_0000_0000;
/// Prefix of the heap-reference category (tag bits 01).
pub const REF_PREFIX: u64 = 0xFFFD_0000_0000_0000;
/// Prefix of the special-immediate category (tag bits 10).
pub const SPECIAL_PREFIX: u64 = 0xFFFE_0000_0000_0000;
/// Prefix of the reserved-invalid category (tag bits 11).
pub const INVALID_PREFIX: u64 = 0xFFFF_0000_0000_0000;
/// The canonical VM NaN for arithmetic results (NV-1 section 4).
pub const CANONICAL_NAN_BITS: u64 = 0x7FF8_0000_0000_0000;
/// One past the largest valid 48-bit payload.
pub const PAYLOAD_LIMIT: u64 = 1 << 48;
/// One past the smallest immediate integer payload.
pub const INT_MIN: i64 = -(1i64 << 47);
/// One past the largest immediate integer payload.
pub const INT_MAX: i64 = (1i64 << 47) - 1;

/// Defined special immediates (prefix `0xFFFE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Special {
    /// Payload 0: the uninitialized register value; reads before writes are
    /// observable and must trap at the interpreter boundary, never here.
    Undefined,
}

/// The category of a raw 64-bit word, including the invalid category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classification {
    /// Immediate signed 48-bit integer.
    Int(i64),
    /// Heap-relative reference: byte offset from the heap base.
    HeapRef(u64),
    /// Special immediate.
    Special(Special),
    /// A binary64 value outside the reserved tag region.
    FloatBits(u64),
    /// Reserved-invalid word; must be rejected at every runtime boundary.
    Invalid,
}

/// Validation failures for raw words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidValue {
    /// The word carries the reserved-invalid prefix (INV-002).
    InvalidPrefix,
    /// A heap reference uses the reserved offset zero (INV-006).
    ZeroOffsetRef,
    /// A special immediate carries an undefined payload.
    ReservedSpecial,
    /// A reference payload exceeds the 48-bit window.
    RefOutOfRange,
}

impl fmt::Display for InvalidValue {
    // CEP:WHAT Renders a validation failure for logs and the CLI.
    // CEP:WHY Operators need the failing category and payload without a
    //        debugger; formatting is bounded and allocation-light.
    // CEP:STATUS complete
    // CEP:FAILURE Writes to the formatter; cannot fail beyond the formatter.
    // CEP:ASSUMES Formatter accepts UTF-8 (Rust contract).
    // CEP:COST O(name length); at most three digits groups.
    // CEP:EVIDENCE cargo test -p vortex-nv1
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InvalidValue::InvalidPrefix => write!(f, "reserved invalid prefix 0xFFFF"),
            InvalidValue::ZeroOffsetRef => write!(f, "heap reference with reserved offset zero"),
            InvalidValue::ReservedSpecial => write!(f, "special immediate with undefined payload"),
            InvalidValue::RefOutOfRange => write!(f, "reference payload beyond the 48-bit window"),
        }
    }
}

impl std::error::Error for InvalidValue {}

/// Outcome of checked integer arithmetic (INV-004: overflow is explicit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntOpOutcome {
    /// The result is an immediate integer.
    Immediate(Value),
    /// The exact result needs a boxed 64-bit integer object.
    NeedsBox(i64),
    /// The exact result needs a wide (beyond i64) integer object.
    NeedsBox128(i128),
    /// An operand was not an immediate integer; the caller must take the
    /// generic path. No silent reinterpretation happens here.
    NotImmediate,
}

/// A valid NV-1 value word (INV-001: exactly one valid representation).
///
/// The invalid category is unrepresentable: every constructor validates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Value(u64);

impl Value {
    /// Classifies a raw word without constructing a `Value`.
    // CEP:WHAT Classifies any raw 64-bit word into its NV-1 category.
    // CEP:WHY Bytecode constants and memory words are untrusted; the
    //        boundary layer needs the invalid category visible to reject.
    // CEP:STATUS complete
    // CEP:FAILURE Never fails; invalid words yield the Invalid variant.
    // CEP:ASSUMES Word obeys the NV-1 bit allocation (section 1.2).
    // CEP:COST O(1): one mask, one shift, one compare chain.
    // CEP:EVIDENCE cargo test -p vortex-nv1
    #[inline]
    pub fn classify_word(w: u64) -> Classification {
        if w & RESERVED_MASK != RESERVED_PREFIX {
            return Classification::FloatBits(w);
        }
        let p = w & 0x0000_FFFF_FFFF_FFFF;
        match (w >> 48) & 3 {
            0 => Classification::Int(((p as i64) << 16) >> 16),
            1 => Classification::HeapRef(p),
            2 if p == 0 => Classification::Special(Special::Undefined),
            2 => Classification::Invalid,
            _ => Classification::Invalid,
        }
    }

    /// Validates a raw word and returns it as a `Value` (INV-002, INV-006).
    // CEP:WHAT Validates a raw word and wraps it in a checked Value.
    // CEP:WHY Runtime boundaries (bytecode load, memory read, FFI return)
    //        must reject invalid words before they become values.
    // CEP:STATUS complete
    // CEP:FAILURE Returns the typed InvalidValue for every rejected shape.
    // CEP:ASSUMES None beyond the bit-allocation assumptions.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-nv1
    #[inline]
    pub fn from_word(w: u64) -> Result<Value, InvalidValue> {
        match Self::classify_word(w) {
            Classification::Int(_) | Classification::FloatBits(_) => Ok(Value(w)),
            Classification::HeapRef(0) => Err(InvalidValue::ZeroOffsetRef),
            Classification::HeapRef(_) => Ok(Value(w)),
            Classification::Special(_) => Ok(Value(w)),
            Classification::Invalid => {
                if (w >> 48) & 3 == 2 {
                    Err(InvalidValue::ReservedSpecial)
                } else {
                    Err(InvalidValue::InvalidPrefix)
                }
            }
        }
    }

    /// Encodes an in-range integer; returns `None` outside the immediate
    /// range so the caller can box without silent truncation (INV-004).
    // CEP:WHAT Encodes a signed 64-bit integer as an immediate when in range.
    // CEP:WHY NV-P01: small integers avoid the heap; the None escape keeps
    //        overflow explicit at the call site.
    // CEP:STATUS complete
    // CEP:FAILURE None when n is outside [-2^47, 2^47-1].
    // CEP:ASSUMES Two's-complement 48-bit payload arithmetic.
    // CEP:COST O(1): one compare pair, one mask, one or.
    // CEP:EVIDENCE cargo test -p vortex-nv1
    #[inline]
    pub fn from_i64(n: i64) -> Option<Value> {
        if !(INT_MIN..=INT_MAX).contains(&n) {
            return None;
        }
        Some(Value(INT_PREFIX | ((n as u64) & 0x0000_FFFF_FFFF_FFFF)))
    }

    /// Encodes a heap-relative reference; offset must be 1..=2^48-1.
    // CEP:WHAT Encodes a heap byte offset as a reference value.
    // CEP:WHY INV-006 reserves offset zero as corruption detection.
    // CEP:STATUS complete
    // CEP:FAILURE Typed error for zero or out-of-window offsets.
    // CEP:ASSUMES The runtime heap base plus the offset is a valid address.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-nv1
    #[inline]
    pub fn from_heap_ref(offset: u64) -> Result<Value, InvalidValue> {
        if offset == 0 {
            return Err(InvalidValue::ZeroOffsetRef);
        }
        if offset >= PAYLOAD_LIMIT {
            return Err(InvalidValue::RefOutOfRange);
        }
        Ok(Value(REF_PREFIX | offset))
    }

    /// Encodes the defined special immediate.
    // CEP:WHAT Encodes Special::Undefined as the 0xFFFE payload-zero word.
    // CEP:WHY Uninitialized registers must be distinguishable from every
    //        numeric and reference value.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; one defined payload exists.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-nv1
    #[inline]
    pub fn undefined() -> Value {
        Value(SPECIAL_PREFIX)
    }

    /// Encodes a binary64 bit pattern for bitwise transport (load, store,
    /// copy). Collisions with the reserved tag region canonicalize (INV-008,
    /// INV-009); permitted payloads pass through unchanged.
    // CEP:WHAT Transports a raw binary64 pattern as a value word.
    // CEP:WHY Bit-preserving operations must not lose non-colliding NaN
    //        payloads, but no stored word may enter the reserved region.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; collisions canonicalize.
    // CEP:ASSUMES Input is a binary64 bit pattern.
    // CEP:COST O(1): one mask compare, one or.
    // CEP:EVIDENCE cargo test -p vortex-nv1
    #[inline]
    pub fn transport_f64_bits(bits: u64) -> Value {
        if bits & RESERVED_MASK == RESERVED_PREFIX {
            Value(CANONICAL_NAN_BITS)
        } else {
            Value(bits)
        }
    }

    /// Encodes a binary64 value; NaN results of arithmetic canonicalize.
    // CEP:WHAT Encodes an f64, canonicalizing NaN to the VM canonical NaN.
    // CEP:WHY INV-007: arithmetic NaN results use one canonical pattern so
    //        cross-platform behavior is identical.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES IEEE-754 binary64.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-nv1
    #[inline]
    pub fn from_f64(x: f64) -> Value {
        Self::transport_f64_bits(x.to_bits())
    }

    /// Checked addition: an immediate result, or the exact sum with a
    /// boxing instruction (NV-1 section 2.2: 64-bit arithmetic cannot
    /// overflow for in-range operands, so only the 48-bit test decides).
    // CEP:WHAT Adds two immediate integers with an explicit overflow path.
    // CEP:WHY INV-004 forbids silent truncation; the caller boxes on
    //        NeedsBox and takes the generic path on NotImmediate.
    // CEP:STATUS complete
    // CEP:FAILURE Explicit outcome for every non-immediate shape.
    // CEP:ASSUMES None; non-immediates are reported, never reinterpreted.
    // CEP:COST O(1): one add, two compares.
    // CEP:EVIDENCE cargo test -p vortex-nv1
    #[inline]
    pub fn checked_add(a: Value, b: Value) -> IntOpOutcome {
        let (x, y) = match (a.as_i64(), b.as_i64()) {
            (Some(x), Some(y)) => (x, y),
            _ => return IntOpOutcome::NotImmediate,
        };
        match Value::from_i64(x.wrapping_add(y)) {
            Some(v) => IntOpOutcome::Immediate(v),
            None => IntOpOutcome::NeedsBox(x.wrapping_add(y)),
        }
    }

    /// Checked subtraction with the same contract as addition.
    // CEP:WHAT Subtracts two immediate integers with an explicit boxing path.
    // CEP:WHY Symmetry with addition; the difference fits in i64 for
    //        in-range operands (NV-1 section 2.2).
    // CEP:STATUS complete
    // CEP:FAILURE Explicit outcome for every non-immediate shape.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-nv1
    #[inline]
    pub fn checked_sub(a: Value, b: Value) -> IntOpOutcome {
        let (x, y) = match (a.as_i64(), b.as_i64()) {
            (Some(x), Some(y)) => (x, y),
            _ => return IntOpOutcome::NotImmediate,
        };
        match Value::from_i64(x.wrapping_sub(y)) {
            Some(v) => IntOpOutcome::Immediate(v),
            None => IntOpOutcome::NeedsBox(x.wrapping_sub(y)),
        }
    }

    /// Checked multiplication via the full i128 product (NV-1 section 2.3):
    /// no intermediate can overflow, and the 48-bit test decides boxing.
    // CEP:WHAT Multiplies two immediate integers with an explicit boxing path.
    // CEP:WHY Products reach 2^94, beyond i64; the i128 product makes the
    //        range decision exact without target-specific high halves.
    // CEP:STATUS complete
    // CEP:FAILURE Explicit outcome, including NeedsBox128 for wide products.
    // CEP:ASSUMES None.
    // CEP:COST O(1): one widening multiply.
    // CEP:EVIDENCE cargo test -p vortex-nv1
    #[inline]
    pub fn checked_mul(a: Value, b: Value) -> IntOpOutcome {
        let (x, y) = match (a.as_i64(), b.as_i64()) {
            (Some(x), Some(y)) => (x, y),
            _ => return IntOpOutcome::NotImmediate,
        };
        let full = (x as i128) * (y as i128);
        if full < (INT_MIN as i128) || full > (INT_MAX as i128) {
            return IntOpOutcome::NeedsBox128(full);
        }
        match Value::from_i64(full as i64) {
            Some(v) => IntOpOutcome::Immediate(v),
            None => IntOpOutcome::NeedsBox128(full),
        }
    }

    /// The raw word.
    // CEP:WHAT Returns the raw 64-bit word of the value.
    // CEP:WHY Encoders (bytecode constants, snapshotting) need the bits.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-nv1
    #[inline]
    pub fn bits(self) -> u64 {
        self.0
    }

    /// The immediate integer, if this is one.
    // CEP:WHAT Decodes an immediate integer with sign extension.
    // CEP:WHY INV-003: immediates decode to their exact signed value.
    // CEP:STATUS complete
    // CEP:FAILURE None for non-integer categories.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-nv1
    #[inline]
    pub fn as_i64(self) -> Option<i64> {
        match Self::classify_word(self.0) {
            Classification::Int(n) => Some(n),
            _ => None,
        }
    }

    /// The heap offset, if this is a reference.
    // CEP:WHAT Decodes the 48-bit heap byte offset of a reference.
    // CEP:WHY NV-P06: decode offsets only when addressing is needed.
    // CEP:STATUS complete
    // CEP:FAILURE None for non-reference categories.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-nv1
    #[inline]
    pub fn as_heap_ref(self) -> Option<u64> {
        match Self::classify_word(self.0) {
            Classification::HeapRef(o) => Some(o),
            _ => None,
        }
    }

    /// The binary64 bits, if this is a float category word.
    // CEP:WHAT Decodes the raw binary64 bit pattern of a float value.
    // CEP:WHY FPR transport and comparison lowering need the exact bits.
    // CEP:STATUS complete
    // CEP:FAILURE None for non-float categories.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-nv1
    #[inline]
    pub fn as_f64_bits(self) -> Option<u64> {
        match Self::classify_word(self.0) {
            Classification::FloatBits(b) => Some(b),
            _ => None,
        }
    }

    /// The special, if this is a special immediate.
    // CEP:WHAT Decodes a special immediate.
    // CEP:WHY The interpreter checks for uninitialized registers explicitly.
    // CEP:STATUS complete
    // CEP:FAILURE None for non-special categories.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-nv1
    #[inline]
    pub fn as_special(self) -> Option<Special> {
        match Self::classify_word(self.0) {
            Classification::Special(s) => Some(s),
            _ => None,
        }
    }
}

impl fmt::Display for Value {
    // CEP:WHAT Renders the value with its category for diagnostics.
    // CEP:WHY Logs and the CLI must show the category, not just the word.
    // CEP:STATUS complete
    // CEP:FAILURE Formatter errors only.
    // CEP:ASSUMES None.
    // CEP:COST O(digits).
    // CEP:EVIDENCE cargo test -p vortex-nv1
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match Self::classify_word(self.0) {
            Classification::Int(n) => write!(f, "int({n})"),
            Classification::HeapRef(o) => write!(f, "ref(+{o:#x})"),
            Classification::Special(s) => write!(f, "special({s:?})"),
            Classification::FloatBits(b) => {
                write!(f, "float({})", f64::from_bits(b))
            }
            Classification::Invalid => write!(f, "invalid({:#018x})", self.0),
        }
    }
}
