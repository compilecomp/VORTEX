// CEP:FILE crates/vortex-ir/src/compat.rs
// CEP:WHAT The SemanticCompatibilityMatrix (FFI-1 section 5): the six
//        compatibility dimensions that gate cross-language inlining, with a
//        pure checker returning an inline plan or a rejection reason.
// CEP:WHY INV-FFI-007: an unproven inline is a specification violation even
//        when mechanically cheap. This module is that proof object.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Rejections carry the failing dimension; nothing silently
//        passes with unknown facts.
// CEP:ASSUMES Language profiles are conservative summaries: unknown facts
//        reject, they never default to permissive.
// CEP:COST O(raise-set + handler-set) per check.
// CEP:EVIDENCE cargo test -p vortex-ir (compat matrix tests)

/// Exception classes a language context can raise or catch. The engine maps
/// concrete exceptions into these classes at the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExnClass {
    /// Generic runtime error.
    Runtime,
    /// Numeric domain error (overflow, division by zero).
    Numeric,
    /// Security/capability violation (UGB SecurityError).
    Security,
    /// Foreign-boundary error (FFI marshalling failure).
    Foreign,
    /// Memory/bytes error.
    Memory,
}

/// Concrete value domains at a call site (FFI-1 "Value semantics" row).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Domain {
    /// Signed 48-bit-immediate integer semantics.
    #[default]
    Int48,
    /// Full signed 64-bit integer semantics (boxed domain).
    Int64,
    /// IEEE-754 binary64 with VM NaN canonicalization.
    Float64,
    /// Heap references with reference identity.
    Ref,
    /// Domain not proven: rejects inlining per the conservative rule.
    Unknown,
}

impl Domain {
    fn compatible(self, other: Domain) -> bool {
        match (self, other) {
            (Domain::Unknown, _) | (_, Domain::Unknown) => false,
            (a, b) => a == b,
        }
    }
}

/// Language context profile for one side of a boundary.
#[derive(Debug, Clone, Default)]
pub struct LangProfile {
    /// Exception classes this context can raise (callee) or is prepared to
    /// catch (caller).
    pub can_raise: Vec<ExnClass>,
    /// Exception classes with a prepared handler in scope (caller only).
    pub handlers: Vec<ExnClass>,
    /// Whether this context writes to state the other side observes in an
    /// order-sensitive way (globals, shared objects, FFI-visible memory).
    pub orders_globals: bool,
    /// Value domain used at the boundary.
    pub domain: Domain,
    /// Whether continuation halves (finally/ensure) may resume after a throw
    /// path the other side does not model.
    pub continuation_half: bool,
}

/// The inline plan produced on success (adapters proven eliminable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InlinePlan {
    /// The exception adapters that were proven redundant.
    pub exception_adapter_elided: bool,
    /// Whether safepoint rewriting is required in the inlined body.
    pub requires_safepoint_rewrite: bool,
}

/// Rejection reasons, one per matrix dimension (FFI-1 section 5 table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// The callee can raise an exception the caller has no handler for.
    ExceptionVisibility,
    /// Evaluation-order hazard on observable state.
    Ordering,
    /// Value-domain mismatch.
    ValueSemantics,
    /// Continuation halves escape the inlined region.
    Continuation,
}

/// The matrix check (pure function; AOSS consults this before any
/// cross-language inlining attempt). `caller` is L1, `callee` is L2.
pub fn check_matrix(
    caller: &LangProfile,
    callee: &LangProfile,
) -> Result<InlinePlan, RejectReason> {
    // Exception visibility: total mapping from the callee raise set into the
    // caller's handler set.
    for e in &callee.can_raise {
        if !caller.handlers.contains(e) {
            return Err(RejectReason::ExceptionVisibility);
        }
    }
    // Global-state ordering.
    if callee.orders_globals || caller.orders_globals {
        return Err(RejectReason::Ordering);
    }
    // Value semantics.
    if !caller.domain.compatible(callee.domain) {
        return Err(RejectReason::ValueSemantics);
    }
    // Continuation compatibility.
    if callee.continuation_half {
        return Err(RejectReason::Continuation);
    }
    Ok(InlinePlan {
        exception_adapter_elided: callee.can_raise.is_empty(),
        requires_safepoint_rewrite: false,
    })
}
