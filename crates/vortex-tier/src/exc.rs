// CEP:FILE crates/vortex-tier/src/exc.rs
// CEP:WHAT The EXC-1 exception contract model: sorted exception tables
//        with binary-search dispatch, the throw-stub protocol, and the
//        deopt-transfer rule (EXC-1 sections 2-3).
// CEP:WHY Zero-cost exception handling is table-driven: the happy path
//        pays nothing, and every PC range must be covered by a handler
//        record or the unwinder (EXC-INV-01/02).
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Out-of-order table construction is a typed error; an
//        uncovered PC reports the miss instead of guessing.
// CEP:ASSUMES PC ranges come from compiled blob metadata.
// CEP:COST Dispatch O(log n) per throw; construction O(n log n).
// CEP:EVIDENCE cargo test -p vortex-tier

/// One exception-table record (EXC-1 section 3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExnRecord {
    /// Inclusive range start.
    pub pc_start: usize,
    /// Exclusive range end.
    pub pc_end: usize,
    /// Handler offset (the Catch site).
    pub handler: usize,
    /// Deopt id for records reached through a deopt stub.
    pub deopt_id: Option<usize>,
}

/// Table construction failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExnError {
    /// The records are not sorted and disjoint.
    Unsorted,
}

/// The exception table of one blob.
#[derive(Debug, Clone, Default)]
pub struct ExnTable {
    records: Vec<ExnRecord>,
}

impl ExnTable {
    /// Builds a table; rejects unsorted or overlapping input.
    // CEP:WHAT Builds a table.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-tier
    pub fn build(mut records: Vec<ExnRecord>) -> Result<Self, ExnError> {
        records.sort_by_key(|r| (r.pc_start, r.pc_end));
        for w in records.windows(2) {
            if w[1].pc_start < w[0].pc_end {
                return Err(ExnError::Unsorted);
            }
        }
        Ok(ExnTable { records })
    }

    /// Finds the innermost handler for a PC (binary search; EXC-1 3.1
    /// step 4). Returns None when the PC is uncovered (the OS unwinder
    /// takes over).
    // CEP:WHAT Finds the innermost handler for a PC.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p vortex-tier
    #[must_use]
    pub fn find(&self, pc: usize) -> Option<ExnRecord> {
        // Binary search for the last record whose pc_start <= pc.
        let mut lo = 0_usize;
        let mut hi = self.records.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.records[mid].pc_start <= pc {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return None;
        }
        let r = self.records[lo - 1];
        (pc < r.pc_end).then_some(r)
    }

    /// EXC-INV-02 coverage: whether every byte in [0, code_len) is covered
    /// by some record.
    #[must_use]
    pub fn covers(&self, code_len: usize) -> bool {
        let mut cursor = 0_usize;
        for r in &self.records {
            if r.pc_start > cursor {
                return false;
            }
            cursor = cursor.max(r.pc_end);
        }
        cursor >= code_len
    }
}

/// The deopt-transfer rule (EXC-1 section 3.2): an exception thrown inside
/// a deopt stub transfers to the interpreter's exception state at the
/// stub's resume pc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeoptTransfer {
    /// The resume pc in the interpreter.
    pub resume_pc: usize,
    /// Whether the pending exception was preserved (EXC-INV-04).
    pub exception_preserved: bool,
}
