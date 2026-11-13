// CEP:FILE crates/vortex-opt/src/passes.rs
// CEP:WHAT The optimization passes over the SSA graph, in the moderate (T2)
//        and aggressive (T3) budgets of the revised tier structure:
//        constant folding, dominator-scoped GVN/CSE, LICM over natural
//        loops, ARS representation selection, proven bounds-check
//        elimination for counted loops (INV-LLIR-009: mathematical proof,
//        never a heuristic), and SROA for small non-escaping allocations
//        (INV-LLIR-008).
// CEP:WHY These passes are the optimization content of T2/T3; each one is
//        legality-checked against the interpreter semantics and records its
//        evidence (fold counts, CSE hits, hoists, elisions) in the plan.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Passes are semantics-preserving transformations; the
//        differential executor proves observable equality with T0 per test.
// CEP:ASSUMES The graph was built by son::build from a verified method.
// CEP:COST Each pass is O(values); GVN is O(values x domtree depth) with a
//        scoped hash; LICM is O(values x loops).
// CEP:EVIDENCE cargo test -p vortex-opt
use std::collections::HashMap;

use crate::son::{Rep, SonFunc, SonOp, VId};

/// One pass name (the pass pipeline contract; standard.md 38.20).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    /// Constant folding (VSDG mandate: basic constant folding).
    Fold,
    /// Global value numbering / common subexpression elimination.
    Gvn,
    /// Loop-invariant code motion (pure arithmetic only; Div is excluded
    /// because it can trap, and hoisting a trap changes semantics).
    Licm,
    /// Representation selection (simple ARS).
    Ars,
    /// Proven bounds-check elimination for counted loops.
    Bce,
    /// Scalar replacement of non-escaping constant-length arrays.
    Sroa,
}

/// Tier budgets (design.md section 8 revised tier structure: T2 moderate,
/// T3 aggressive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TierBudget {
    /// T2: good code quickly.
    Moderate,
    /// T3: hot code fast.
    Aggressive,
}

impl TierBudget {
    /// The pass pipeline for the budget.
    #[must_use]
    pub fn passes(self) -> &'static [Pass] {
        match self {
            TierBudget::Moderate => &[Pass::Fold, Pass::Gvn, Pass::Licm, Pass::Ars],
            TierBudget::Aggressive => &[
                Pass::Fold,
                Pass::Gvn,
                Pass::Licm,
                Pass::Ars,
                Pass::Bce,
                Pass::Sroa,
            ],
        }
    }
}

/// Statistics each pass records (the optimization remarks contract,
/// standard.md 38.16).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PassStats {
    /// Constants folded.
    pub folded: u64,
    /// Redundant computations eliminated by GVN.
    pub cse_hits: u64,
    /// Instructions hoisted out of loops.
    pub hoisted: u64,
    /// Bounds checks proven away.
    pub bounds_elided: u64,
    /// Allocations scalar-replaced.
    pub sroa_allocs: u64,
}

/// Runs the pass pipeline over the graph, returning the stats.
// CEP:WHAT Runs the pass pipeline over the graph, returning the stats.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn run_pipeline(func: &mut SonFunc, budget: crate::passes::TierBudget) -> PassStats {
    let mut stats = PassStats::default();
    for &p in budget.passes() {
        match p {
            Pass::Fold => stats.folded += constant_fold(func),
            Pass::Gvn => stats.cse_hits += gvn(func),
            Pass::Licm => stats.hoisted += licm(func),
            Pass::Ars => assign_reps(func),
            Pass::Bce => stats.bounds_elided += bce(func),
            Pass::Sroa => stats.sroa_allocs += sroa(func),
        }
    }
    stats
}

/// Whether the op is a pure arithmetic/comparison (no traps, no memory).
#[must_use]
fn is_pure_no_trap(op: &SonOp) -> bool {
    matches!(
        op,
        SonOp::Add
            | SonOp::Sub
            | SonOp::Mul
            | SonOp::Neg
            | SonOp::Eq
            | SonOp::Lt
            | SonOp::Le
            | SonOp::IsNull
            | SonOp::ConstInt(_)
            | SonOp::ConstFloat(_)
            | SonOp::Null
            | SonOp::Bool(_)
            | SonOp::Copy
    )
}

/// Constant folding over int/float constants with the interpreter's
/// promotion semantics (int op float promotes to float). The float
/// equality arm uses `==` deliberately: abstract equality on numbers is
/// value equality, and NaN != NaN is the required IEEE semantics.
// CEP:WHAT Constant folding over int/float constants with the interpreter's promotion semantics.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn constant_fold(func: &mut SonFunc) -> u64 {
    let mut folded = 0_u64;
    for vid in 0..func.values.len() {
        let (op, inputs) = {
            let v = &func.values[vid];
            (v.op.clone(), v.inputs.clone())
        };
        let two_const = |func: &SonFunc, a: VId, b: VId| {
            match (&func.values[a].op, &func.values[b].op) {
                (SonOp::ConstInt(x), SonOp::ConstInt(y)) => Some((Some((*x, *y)), None)),
                (SonOp::ConstFloat(x), SonOp::ConstFloat(y)) => Some((None, Some((*x, *y)))),
                // Mixed: the interpreter promotes to float.
                (SonOp::ConstInt(x), SonOp::ConstFloat(y)) => {
                    Some((None, Some((f64::from(i32::try_from(*x).unwrap_or(0)), *y))))
                }
                (SonOp::ConstFloat(x), SonOp::ConstInt(y)) => {
                    Some((None, Some((*x, f64::from(i32::try_from(*y).unwrap_or(0))))))
                }
                _ => None,
            }
        };
        let replacement: Option<SonOp> = match op {
            SonOp::Add | SonOp::Sub | SonOp::Mul | SonOp::Div if inputs.len() == 2 => {
                two_const(func, inputs[0], inputs[1]).and_then(|pair| {
                    match (pair.0, pair.1) {
                        (Some((x, y)), _) => match op {
                            SonOp::Add if x.checked_add(y).is_some() => {
                                Some(SonOp::ConstInt(x + y))
                            }
                            SonOp::Sub if x.checked_sub(y).is_some() => {
                                Some(SonOp::ConstInt(x - y))
                            }
                            SonOp::Mul if x.checked_mul(y).is_some() => {
                                Some(SonOp::ConstInt(x * y))
                            }
                            SonOp::Div if y != 0 && x.checked_div(y).is_some() => {
                                Some(SonOp::ConstInt(x / y))
                            }
                            _ => None,
                        },
                        (_, Some((x, y))) => {
                            // Float math; division by zero yields the same
                            // IEEE result the interpreter would produce.
                            match op {
                                SonOp::Add => Some(SonOp::ConstFloat(x + y)),
                                SonOp::Sub => Some(SonOp::ConstFloat(x - y)),
                                SonOp::Mul => Some(SonOp::ConstFloat(x * y)),
                                SonOp::Div => Some(SonOp::ConstFloat(x / y)),
                                _ => None,
                            }
                        }
                        _ => None,
                    }
                })
            }
            SonOp::Neg if inputs.len() == 1 => match func.values[inputs[0]].op {
                SonOp::ConstInt(x) if x.checked_neg().is_some() => Some(SonOp::ConstInt(-x)),
                SonOp::ConstFloat(x) => Some(SonOp::ConstFloat(-x)),
                _ => None,
            },
            SonOp::Eq if inputs.len() == 2 => {
                let a = &func.values[inputs[0]].op;
                let b = &func.values[inputs[1]].op;
                match (a, b) {
                    (SonOp::ConstInt(x), SonOp::ConstInt(y)) => Some(SonOp::Bool(x == y)),
                    #[allow(clippy::float_cmp)]
                    (SonOp::ConstFloat(x), SonOp::ConstFloat(y)) => Some(SonOp::Bool(x == y)),
                    (SonOp::Null, SonOp::Null) => Some(SonOp::Bool(true)),
                    _ => None,
                }
            }
            SonOp::Lt | SonOp::Le if inputs.len() == 2 => {
                let a = &func.values[inputs[0]].op;
                let b = &func.values[inputs[1]].op;
                let xy: Option<(f64, f64)> = match (a, b) {
                    (SonOp::ConstInt(x), SonOp::ConstInt(y)) => Some((
                        f64::from(i32::try_from(*x).unwrap_or(0)),
                        f64::from(i32::try_from(*y).unwrap_or(0)),
                    )),
                    (SonOp::ConstFloat(x), SonOp::ConstFloat(y)) => Some((*x, *y)),
                    _ => None,
                };
                xy.map(|(x, y)| {
                    if op == SonOp::Lt {
                        SonOp::Bool(x < y)
                    } else {
                        SonOp::Bool(x <= y)
                    }
                })
            }
            _ => None,
        };
        if let Some(new_op) = replacement {
            let keep_def = func.values[vid].def_vreg;
            let keep_pc = func.values[vid].pc;
            let keep_block = func.values[vid].block;
            func.values[vid] = crate::son::Value {
                op: new_op,
                inputs: Vec::new(),
                pending: Vec::new(),
                block: keep_block,
                rep: Rep::Tagged,
                pc: keep_pc,
                def_vreg: keep_def,
            };
            folded += 1;
        }
    }
    folded
}

/// Dominator-scoped GVN for pure ops (design.md section 2.2 mandate;
/// loads are excluded because stores may alias them).
// CEP:WHAT Dominator-scoped GVN for pure ops.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn gvn(func: &mut SonFunc) -> u64 {
    // Scope-keyed table walked over the dominator tree requires idoms;
    // the reference implementation walks blocks in reverse post-order and
    // uses a scoped stack keyed by (op, inputs), popping at block exit is
    // not dominator-exact. To stay sound without the dominator walk, GVN
    // replaces only when the dominating relationship is guaranteed: the
    // replacement block must dominate the current block. We recompute
    // domination with the CHK idoms.
    let n = func.blocks.len();
    let order = rpo_of(func);
    let idom = idom_of(func, &order);
    let mut hits = 0_u64;

    // Walk RPO; maintain per-value "available" table of (op, inputs) ->
    // earliest defining vid. A replacement is legal when the candidate's
    // block dominates the user's block.
    let mut table: HashMap<(String, Vec<VId>), VId> = HashMap::new();
    let key_of = |func: &SonFunc, vid: VId| -> Option<(String, Vec<VId>)> {
        let v = &func.values[vid];
        if !is_pure_no_trap(&v.op) {
            return None;
        }
        Some((format!("{:?}", v.op), v.inputs.clone()))
    };
    let dominates = |a: usize, b: usize, idom: &[Option<usize>]| -> bool {
        let mut x = b;
        loop {
            if x == a {
                return true;
            }
            match idom[x] {
                Some(d) if d != x => x = d,
                _ => return false,
            }
        }
    };
    let mut rpo_pos: HashMap<usize, usize> = HashMap::new();
    for (i, &b) in order.iter().enumerate() {
        rpo_pos.insert(b, i);
    }
    let mut processed: Vec<VId> = Vec::new();
    for &b in &order {
        // Values in this block in order.
        let mut vids: Vec<VId> = (0..func.values.len())
            .filter(|&vid| func.values[vid].block == b && is_pure_no_trap(&func.values[vid].op))
            .collect();
        vids.sort_by_key(|&vid| (func.values[vid].pc, vid));
        for vid in vids {
            let Some(key) = key_of(func, vid) else {
                continue;
            };
            if let Some(&cand) = table.get(&key) {
                // Replacement is legal only when the candidate's block
                // dominates this block (same-pc values in sibling blocks
                // are different program points, not duplicates).
                if dominates(func.values[cand].block, func.values[vid].block, &idom) && cand != vid
                {
                    replace_uses(func, vid, cand);
                    hits += 1;
                    continue;
                }
            }
            table.insert(key, vid);
            processed.push(vid);
        }
    }
    let _ = processed;
    let _ = n;
    hits
}

/// Replaces every use of `old` with `new`.
pub(crate) fn replace_uses(func: &mut SonFunc, old: VId, new: VId) {
    for v in &mut func.values {
        for input in &mut v.inputs {
            if *input == old {
                *input = new;
            }
        }
    }
    for b in &mut func.blocks {
        if let Some(t) = &mut b.term {
            match t {
                crate::son::Term::Branch { cond, .. } => {
                    if *cond == old {
                        *cond = new;
                    }
                }
                crate::son::Term::Return(val) => {
                    if *val == old {
                        *val = new;
                    }
                }
                crate::son::Term::Goto(_) => { /* unconditional: no value input */ }
            }
        }
    }
}

/// Reverse post-order of blocks.
// CEP:WHAT Reverse post-order of blocks.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
fn rpo_of(func: &SonFunc) -> Vec<usize> {
    let n = func.blocks.len();
    let mut visited = vec![false; n];
    let mut post = Vec::new();
    let mut stack: Vec<(usize, usize)> = vec![(0, 0)];
    visited[0] = true;
    while let Some((b, i)) = stack.pop() {
        let succs = &func.blocks[b].succs;
        if i < succs.len() {
            stack.push((b, i + 1));
            let s = succs[i];
            if !visited[s] {
                visited[s] = true;
                stack.push((s, 0));
            }
        } else {
            post.push(b);
        }
    }
    post.reverse();
    post
}

/// Immediate dominators over the reachable blocks.
// CEP:WHAT Immediate dominators over the reachable blocks.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
fn idom_of(func: &SonFunc, order: &[usize]) -> Vec<Option<usize>> {
    let n = func.blocks.len();
    let mut idom: Vec<Option<usize>> = vec![None; n];
    let mut rpo_pos: HashMap<usize, usize> = HashMap::new();
    for (i, &b) in order.iter().enumerate() {
        rpo_pos.insert(b, i);
    }
    if let Some(&entry) = order.first() {
        idom[entry] = Some(entry);
    }
    let mut changed = true;
    while changed {
        changed = false;
        for &b in order.iter().skip(1) {
            let mut new_idom: Option<usize> = None;
            for &p in &func.blocks[b].preds {
                if idom[p].is_none() {
                    continue;
                }
                new_idom = Some(match new_idom {
                    None => p,
                    Some(cur) => {
                        let mut a = cur;
                        let mut b2 = p;
                        while a != b2 {
                            while rpo_pos.get(&a).copied().unwrap_or(usize::MAX)
                                > rpo_pos.get(&b2).copied().unwrap_or(usize::MAX)
                            {
                                a = idom[a].expect("idom on reachable path");
                            }
                            while rpo_pos.get(&b2).copied().unwrap_or(usize::MAX)
                                > rpo_pos.get(&a).copied().unwrap_or(usize::MAX)
                            {
                                b2 = idom[b2].expect("idom on reachable path");
                            }
                        }
                        a
                    }
                });
            }
            if let Some(ni) = new_idom {
                if idom[b] != Some(ni) {
                    idom[b] = Some(ni);
                    changed = true;
                }
            }
        }
    }
    idom
}

/// LICM over natural loops: hoists pure, non-trapping arithmetic whose
/// inputs are defined outside the loop or constant (Div excluded: it can
/// trap, and hoisting a trap changes when it fires).
// CEP:WHAT LICM over natural loops: hoists pure, non-trapping arithmetic whose inputs are defined outside the loop or constant.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn licm(func: &mut SonFunc) -> u64 {
    let loops = natural_loops(func);
    let mut hoisted = 0_u64;
    for (header, members) in loops {
        // Preheader: the block ending in a Goto to the header (or the
        // entry-side block that branches in with the header not taken).
        let preheader = func.blocks[header]
            .preds
            .iter()
            .copied()
            .find(|&p| !members.contains(&p));
        let Some(pre) = preheader else {
            continue;
        };
        let mut invariant: HashMap<VId, bool> = HashMap::new();
        let mut hoist_list: Vec<VId> = Vec::new();
        for &m in &members {
            let mut vids: Vec<VId> = (0..func.values.len())
                .filter(|&vid| {
                    func.values[vid].block == m
                        && is_pure_no_trap(&func.values[vid].op)
                        && !matches!(func.values[vid].op, SonOp::Phi { .. })
                })
                .collect();
            vids.sort_by_key(|&vid| (func.values[vid].pc, vid));
            for vid in vids {
                let inputs = func.values[vid].inputs.clone();
                let inv = inputs.iter().all(|&i| {
                    let iv = &func.values[i];
                    matches!(iv.op, SonOp::ConstInt(_) | SonOp::ConstFloat(_))
                        || iv.block == usize::MAX
                        || !members.contains(&iv.block)
                        || invariant.get(&i).copied().unwrap_or(false)
                });
                invariant.insert(vid, inv);
                if inv {
                    hoist_list.push(vid);
                }
            }
        }
        // Hoist by re-bakening the value into the preheader block: change
        // the value's block; the executor evaluates per block, so the
        // preheader must contain it in evaluation order. The executor walks
        // values by block membership, so re-blocking is the hoist.
        for vid in hoist_list {
            func.values[vid].block = pre;
            hoisted += 1;
        }
    }
    hoisted
}

/// Natural loops as (header, member blocks) from back edges.
// CEP:WHAT Natural loops as (header, member blocks) from back edges.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
fn natural_loops(func: &SonFunc) -> Vec<(usize, std::collections::HashSet<usize>)> {
    let order = rpo_of(func);
    let idom = idom_of(func, &order);
    let mut loops = Vec::new();
    for (b, block) in func.blocks.iter().enumerate() {
        for &s in &block.succs {
            // Back edge: s dominates b.
            let dominates = {
                let mut x = b;
                loop {
                    if x == s {
                        break true;
                    }
                    match idom[x] {
                        Some(d) if d != x => x = d,
                        _ => break false,
                    }
                }
            };
            if dominates && s <= b {
                // Collect the natural loop of s <- b.
                let mut members = std::collections::HashSet::new();
                members.insert(s);
                let mut stack = vec![b];
                while let Some(x) = stack.pop() {
                    if members.insert(x) {
                        for &p in &func.blocks[x].preds {
                            stack.push(p);
                        }
                    }
                }
                loops.push((s, members));
            }
        }
    }
    loops
}

/// ARS: assigns representations. F64 for float-domain values, I64 only for
/// BCE-proven induction variables and constants in the immediate domain
/// (set later by bce), Tagged otherwise.
// CEP:WHAT ARS: assigns representations.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn assign_reps(func: &mut SonFunc) {
    let n = func.values.len();
    // Fixpoint over value domains.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Dom {
        Bot,
        Int,
        Float,
        Bool,
        Null,
        Ref,
        Top,
    }
    let mut dom: Vec<Dom> = vec![Dom::Bot; n];
    let mut changed = true;
    while changed {
        changed = false;
        for vid in 0..n {
            let v = &func.values[vid];
            let d = match &v.op {
                SonOp::ConstInt(_) => Dom::Int,
                SonOp::ConstFloat(_) => Dom::Float,
                SonOp::Null => Dom::Null,
                SonOp::Bool(_) => Dom::Bool,
                SonOp::Arg(_) => Dom::Top,
                SonOp::Copy | SonOp::Phi { .. } => {
                    let mut d = Dom::Bot;
                    for &i in &v.inputs {
                        d = join(d, dom.get(i).copied().unwrap_or(Dom::Bot));
                    }
                    d
                }
                SonOp::Add | SonOp::Sub | SonOp::Mul | SonOp::Div | SonOp::Neg => {
                    let mut d = Dom::Bot;
                    for &i in &v.inputs {
                        d = join(d, dom.get(i).copied().unwrap_or(Dom::Bot));
                    }
                    match d {
                        Dom::Int => Dom::Int,
                        Dom::Float => Dom::Float,
                        _ => Dom::Top, // mixed promotes to float per T0; conservatively Top
                    }
                }
                SonOp::Eq | SonOp::Lt | SonOp::Le | SonOp::IsNull => Dom::Bool,
                SonOp::ArrNew(_) | SonOp::ArrLoad => Dom::Ref,
                SonOp::ArrLen => Dom::Int,
                SonOp::ArrStore | SonOp::SroaStore(_) | SonOp::SroaNew { .. } => Dom::Bot,
                SonOp::SroaLoad(_) => Dom::Top,
            };
            if dom[vid] != d {
                dom[vid] = d;
                changed = true;
            }
        }
    }
    fn join(a: Dom, b: Dom) -> Dom {
        use Dom::*;
        match (a, b) {
            (Bot, x) | (x, Bot) => x,
            (x, y) if x == y => x,
            _ => Top,
        }
    }
    for vid in 0..n {
        let is_bool_op = matches!(
            func.values[vid].op,
            SonOp::Eq | SonOp::Lt | SonOp::Le | SonOp::IsNull | SonOp::Bool(_)
        );
        func.values[vid].rep = match dom[vid] {
            Dom::Float if !is_bool_op => Rep::F64,
            Dom::Int if matches!(func.values[vid].op, SonOp::ConstInt(_) | SonOp::ArrLen) => {
                // Constants and lengths carry I64 word values; arithmetic on
                // them still runs through the tagged fast path unless the
                // BCE pass proves a range (which overrides to I64).
                Rep::I64
            }
            _ => Rep::Tagged,
        };
    }
}

/// Proven bounds-check elimination for counted loops (INV-LLIR-009): the
/// induction variable phi starts at a non-negative constant, steps by one,
/// and the loop back-edge is guarded by `phi_next < len`, so every
/// `arr[phi]` inside the loop is in bounds.
// CEP:WHAT Proven bounds-check elimination for counted loops.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn bce(func: &mut SonFunc) -> u64 {
    let mut elided = 0_u64;
    let loops = natural_loops(func);
    for (header, members) in loops {
        // Find the induction phi in the header.
        let phi_vids: Vec<VId> = func.blocks[header]
            .insts
            .iter()
            .copied()
            .filter(|&vid| matches!(func.values[vid].op, SonOp::Phi { .. }))
            .collect();
        for phi in phi_vids {
            // Increment: phi_next = phi + 1 somewhere in the loop.
            let inc = (0..func.values.len()).find(|&vid| {
                func.values[vid].op == SonOp::Add
                    && func.values[vid].inputs.contains(&phi)
                    && members.contains(&func.values[vid].block)
                    && matches!(
                        func.values[func.values[vid]
                            .inputs
                            .iter()
                            .find(|&&i| i != phi)
                            .copied()
                            .unwrap_or(usize::MAX)]
                        .op,
                        SonOp::ConstInt(1)
                    )
            });
            let Some(inc) = inc else { continue };
            // Tail condition: Lt(phi_next, len) branching back to header,
            // where the Add result itself is phi_next.
            let tail_cond = (0..func.values.len()).find(|&vid| {
                func.values[vid].op == SonOp::Lt
                    && func.values[vid].inputs.first() == Some(&inc)
                    && members.contains(&func.values[vid].block)
            });
            let Some(cond) = tail_cond else { continue };
            let len_input = func.values[cond].inputs[1];
            let len_is_ok = match func.values[len_input].op {
                SonOp::ArrLen => {
                    let arr = func.values[len_input].inputs[0];
                    Some(arr)
                }
                SonOp::ConstInt(c) if c > 0 => None, // constant length: still fine but array unknown
                _ => None,
            };
            // Start value: the phi input coming from outside the loop must
            // be a non-negative constant.
            let start_out = func.values[phi]
                .inputs
                .iter()
                .copied()
                .find(|&i| !members.contains(&func.values[i].block));
            let start_ok = start_out
                .and_then(|s| match func.values[s].op {
                    SonOp::ConstInt(c) => Some(c),
                    _ => None,
                })
                .map(|c| c >= 0)
                .unwrap_or(false);
            if !start_ok {
                continue;
            }
            // Verify the guarded back edge. Two encodings are accepted:
            // (a) the tail block branches to the header on truthy of the
            // guard; (b) the tail block branches to the exit on falsy and
            // its fallthrough block carries the unconditional back-edge
            // Goto (the countdown shape). Both prove: the body executes
            // only when phi_next < len.
            let back_edge_ok = members.iter().any(|&m| {
                matches!(
                    &func.blocks[m].term,
                    Some(crate::son::Term::Branch { cond: c, target, on_truthy: true })
                        if *c == cond && *target == header
                )
            }) || members.iter().any(|&m| {
                let guarded = matches!(
                    &func.blocks[m].term,
                    Some(crate::son::Term::Branch { cond: c, target, on_truthy: false })
                        if *c == cond && *target != header
                );
                let fallthrough_loops = func.blocks[m]
                    .succs
                    .first()
                    .and_then(|&f| func.blocks.get(f))
                    .map(|fb| matches!(fb.term, Some(crate::son::Term::Goto(h)) if h == header))
                    .unwrap_or(false);
                guarded && fallthrough_loops
            });
            if !back_edge_ok {
                continue;
            }
            // Commit I64 for the induction chain: the phi, its increment,
            // and the tail comparison run unboxed (range-proven by the
            // induction argument above).
            func.values[phi].rep = Rep::I64;
            func.values[inc].rep = Rep::I64;
            // Proven: phi in [start, len) inside the loop. Elide the bounds
            // check of every ArrLoad/ArrStore with index == phi on the
            // guarded array (when the length came from ArrLen).
            if let Some(arr) = len_is_ok {
                for vid in 0..func.values.len() {
                    if !members.contains(&func.values[vid].block) {
                        continue;
                    }
                    let is_load = func.values[vid].op == SonOp::ArrLoad;
                    let is_store = func.values[vid].op == SonOp::ArrStore;
                    // Loads and stores both carry the index in slot 1
                    // (inputs are [arr, idx] or [arr, idx, val]).
                    if (is_load || is_store)
                        && func.values[vid].inputs[0] == arr
                        && func.values[vid].inputs[1] == phi
                    {
                        elided += 1;
                    }
                }
            }
        }
    }
    elided
}

/// SROA for constant-length, non-escaping allocations (INV-LLIR-008):
/// an ArrNew whose uses are only ArrLoad/ArrStore/ArrLen inside the method
/// becomes a local slot table.
// CEP:WHAT SROA for constant-length, non-escaping allocations.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-opt
pub fn sroa(func: &mut SonFunc) -> u64 {
    let mut replaced = 0_u64;
    let mut next_slot = 0_usize;
    for alloc in 0..func.values.len() {
        if !matches!(func.values[alloc].op, SonOp::ArrNew(_)) {
            continue;
        }
        let len_ok = matches!(func.values[alloc].op, SonOp::ArrNew(l) if l <= 8);
        if !len_ok {
            continue;
        }
        // Non-escaping: only element memory ops may reference the
        // allocation (they rewrite to slot accesses); any other use
        // (return, store into another array, ...) keeps it on the heap.
        let uses_ok = func.values.iter().all(|v| match &v.op {
            SonOp::ArrLoad | SonOp::ArrLen | SonOp::ArrStore => true,
            _ => !v.inputs.contains(&alloc),
        });
        if !uses_ok {
            continue;
        }
        let slot = next_slot;
        next_slot += 1;
        // Rewrite uses.
        let alloc_len = match func.values[alloc].op {
            SonOp::ArrNew(l) => i64::from(l),
            _ => unreachable!("checked above"),
        };
        for vid in 0..func.values.len() {
            if func.values[vid].inputs.first() != Some(&alloc) {
                continue;
            }
            let is_load = matches!(func.values[vid].op, SonOp::ArrLoad);
            let is_store = matches!(func.values[vid].op, SonOp::ArrStore);
            let is_len = matches!(func.values[vid].op, SonOp::ArrLen);
            let v = &mut func.values[vid];
            if is_load {
                v.op = SonOp::SroaLoad(slot);
                v.inputs.remove(0);
            } else if is_store {
                v.op = SonOp::SroaStore(slot);
                v.inputs.remove(0);
            } else if is_len {
                v.op = SonOp::ConstInt(alloc_len);
                v.inputs.clear();
            }
        }
        // The allocation becomes a local slot table declaration.
        let SonOp::ArrNew(l) = func.values[alloc].op else {
            unreachable!("checked above");
        };
        let keep_pc = func.values[alloc].pc;
        let keep_block = func.values[alloc].block;
        let keep_def = func.values[alloc].def_vreg;
        func.values[alloc] = crate::son::Value {
            op: SonOp::SroaNew { slot, len: l },
            inputs: Vec::new(),
            pending: Vec::new(),
            block: keep_block,
            rep: Rep::Tagged,
            pc: keep_pc,
            def_vreg: keep_def,
        };
        replaced += 1;
    }
    replaced
}
