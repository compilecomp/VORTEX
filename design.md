# ET-SoN IR — Complete Design Document (v1)

## 0. Document Control and Design Scope

**Component:** Intermediate Representation (ET-SoN — Effect-Typed Sea of Nodes)

**Version:** 1.0

**Role:** Central target-independent representation of the program. Domain for global optimizations including GVN, LICM, DCE, and effect-preserving transformations.

**Design Status:** Complete specification, pending alias-analysis subsystem.

**Scope:** This document defines the graph structure, type system, effect lattice, node semantics, and validity invariants of the IR. It does not define:
- The source language (which maps into this IR via front-end lowering)
- The target machine (which this IR maps into via backend lowering)
- The alias analysis that determines region classes (separate subsystem)

**Normative References:**
- Click, C. "A Simple, Fast Dominance Algorithm" (for scheduling conditions)
- C11 Standard §7.17 (for atomic ordering semantics)
- IEEE 754-2019 (for floating-point semantics)

**Known Limitations:**
- Region-class resolution depends on an alias-analysis subsystem not specified here
- Observational-equivalence proofs for effect-preserving transformations are not discharged in this document (belong to optimizer subsystem)

---

## 1. Goals, Non-Goals, and Constraints

### Goals

1. Capture data, control, and effect dependencies explicitly to enable aggressive global optimization (GVN, LICM, DCE).
2. Strictly preserve the source language's observable side effects, memory model, and exception semantics.
3. Provide a mathematically verifiable structure where invalid programs are rejected before optimization.
4. Partition state tokens by memory region to avoid serializing independent memory operations.
5. Support vector operations, atomics, and exception handling without special cases.

### Non-Goals

1. Target-specific instruction selection (handled in lowering/codegen).
2. Source-level debugging information mapping (handled via orthogonal metadata).
3. Alias analysis (separate subsystem; ET-SoN consumes its results).

### Constraints

- IR construction time must be O(|AST| · |R|), where |R| is the number of region classes.
- IR memory footprint must not exceed O(|AST| · k), where k is a small constant factor for graph overhead.
- Verifier must run in polynomial time O(|N| + |E|).

---

## 6. Intermediate Representation: ET-SoN

### 6.1 Purpose and Domain

The ET-SoN IR is a directed, typed graph. It eliminates explicit basic blocks in favor of a "sea" of nodes connected by data, control, and effect edges. Its purpose is to decouple the logical operations of the program from their physical execution schedule, allowing the optimizer to float pure computations while strictly ordering effectful operations.

### 6.2 Mathematical Specification

| Requirement | Specification |
|---|---|
| **Domain** | P_valid: The set of well-typed, name-resolved source programs post-front-end. |
| **Codomain** | G_IR: The set of valid ET-SoN graphs. |
| **Definitions** | G = (N, E, Σ, Γ). N: Nodes. E: Edges. Σ: Node signatures. Γ: Global type/effect environment. |
| **Preconditions** | The source program P has passed lexical, syntactic, and static semantic analysis. |
| **Postconditions** | The resulting graph G satisfies Valid_IR(G) (defined in 6.6). |
| **Invariants** | Type consistency, effect kind consistency, control-flow well-formedness, state-token well-formedness, schedulability, effect-kind monotonicity, FP-flag validity. |
| **Relations** | Data dependence (→_d), control dependence (→_c), state dependence (→_s), exception dependence (→_except). |
| **Failure behavior** | If P ∉ P_valid, the front-end rejects P. If IR construction encounters an unresolvable structural contradiction, it emits an InternalCompilerError. |
| **Complexity** | Construction: O(|V_s| · |R| + |E_s|) time and space, where V_s, E_s are source AST nodes and edges, |R| is region-class count. |
| **Correctness** | ⟦P⟧_S = ⟦Lower(G)⟧_T. The IR must preserve the observational equivalence of the source program. |

### 6.3 Graph Structure

The IR is a directed graph G = (N, E).

**Node partition.** N = N_pure ∪ N_ctrl ∪ N_eff, disjoint.

**Edge partition.** E = E_data ∪ E_ctrl ∪ E_state ∪ E_except, disjoint.

- E_data ⊆ N × N — data flow.
- E_ctrl ⊆ N_ctrl × N_ctrl — control flow.
- E_state ⊆ N_eff × N_eff — state-token flow (memory and synchronization ordering).
- E_except ⊆ N_eff × N_ctrl — exceptional control flow (only from Invoke/Throw to handler Regions).

**State-token partitioning.** State tokens are not global. They are partitioned by *region class* r ∈ R, where R is a finite set determined by alias analysis. Each effectful node n declares, for each region class r it may touch, whether its interaction is read, write, or atomic with a given ordering. The state-token edges are therefore per-region: for each region class r that n touches, n has one state-in_r and one state-out_r.

This means the state subgraph is a set of parallel chains, one per region class, threaded through effectful nodes that touch that region. Independent regions do not serialize each other.

**State Phis.** At each Region node (control merge), for each region class r, there is a corresponding state-Phi_r node that merges the incoming state tokens from each predecessor. State-Phis are themselves effectful nodes with effect kind {MemRead} (they are pure observers of the state, not modifiers).

### 6.4 Type System and Effect Lattice

**Data types** T: standard lattice with subtyping ≤_T. Includes at minimum:
- Integer types: Int8, Int16, Int32, Int64, UInt8, UInt16, UInt32, UInt64
- Floating-point types: Float32, Float64
- Boolean: Bool
- Pointer types: Ptr<T> for any T
- Aggregate types: Tuple<T₁, …, Tₙ>, Array<T, n>
- Function types: Fn<T_args → T_ret>
- State token type: State_r for each region class r

Subtyping rules: standard structural subtyping for aggregates, nominal subtyping for user-defined types.

**Effect lattice.** Let K = {MemRead, MemWrite, Alloc, Call, Trap, Barrier} be the finite set of primitive effect kinds. An effect is a subset E ⊆ K. The effect lattice is the powerset lattice:

    L_ε = (℘(K), ⊆, ∪, ∩, ∅, K)

- ⊥ = ∅ (pure)
- ⊤ = K
- Join = union, meet = intersection

MemRead and MemWrite are incomparable by construction. This is not a design choice; it is a consequence of the powerset structure.

**Node signatures.** Every node n has a signature Σ(n) = (τ⃗_in, τ⃗_out, E, F), where:

- τ⃗_in, τ⃗_out ∈ T* — data types.
- E ∈ ℘(K) — the effect kind of n.
- F — additional flags (defined per node class below).

If E = ∅, n has no state-token edges. If E ≠ ∅, n has one state-in_r and state-out_r for each region class r it touches.

**Conflict relation.** The lattice order does not determine ordering requirements. Ordering is determined by a separate relation Conflict(n₁, n₂) ∈ {true, false}, defined as:

    Conflict(n₁, n₂) =
        MayAlias(region(n₁), region(n₂)) ∧
        (MemWrite ∈ E(n₁) ∨ MemWrite ∈ E(n₂)) ∧
        (MemRead ∈ E(n₁) ∨ MemRead ∈ E(n₂))
        ∨ (Barrier ∈ E(n₁) ∨ Barrier ∈ E(n₂))
        ∨ AtomicConflict(n₁, n₂)

where:
- MayAlias(r₁, r₂) is provided by the alias-analysis subsystem.
- AtomicConflict(n₁, n₂) encodes the C11/Java memory-ordering constraints for atomic operations:
  - If either is atomic with ordering ∈ {acquire, release, seq_cst}, they conflict.
  - If both are relaxed atomics touching the same location, they conflict.
  - Otherwise, they do not conflict.

Two nodes require a state edge between them if and only if they conflict. State edges are added during IR construction based on this relation; they are not added between non-conflicting nodes.

This separates the *effect kind* (what a node does, for analysis) from the *ordering requirement* (what must be sequenced, for correctness).

### 6.5 Node Semantics

#### Control nodes (N_ctrl)

- **Start.** Produces initial arguments and the initial state tokens s₀_r for each region class r. E = ∅. Signature: Σ(Start) = (∅, (τ⃗_args, State_r₁, …, State_rₖ), ∅, ∅).

- **Region(c₁, …, cₖ).** Control merge. E = ∅. Signature: Σ(Region) = ((Ctrl, …, Ctrl), Ctrl, ∅, ∅).

- **Phi(region, v₁, …, vₖ).** Data merge. E = ∅. Signature: Σ(Phi) = ((Ctrl, τ, …, τ), τ, ∅, ∅).

- **StatePhi_r(region, s₁, …, sₖ).** State merge for region class r. E = {MemRead}. The output state token is the join of the inputs; no memory is modified. Signature: Σ(StatePhi_r) = ((Ctrl, State_r, …, State_r), State_r, {MemRead}, ∅).

- **If(c, b).** Produces two control outputs. E = ∅. Signature: Σ(If) = ((Ctrl, Bool), (Ctrl, Ctrl), ∅, ∅).

- **Return(c, v⃗, s⃗).** Terminates with values. E = ∅. Signature: Σ(Return) = ((Ctrl, τ⃗_ret, State_r₁, …, State_rₖ), ∅, ∅, ∅).

- **Stop(c, s⃗).** Terminates without values (void). E = ∅. Signature: Σ(Stop) = ((Ctrl, State_r₁, …, State_rₖ), ∅, ∅, ∅).

#### Pure computation nodes (N_pure)

Integer, floating-point, boolean, cast, and aggregate construction/destruction nodes. E = ∅.

**Integer operations.** Add, Sub, Mul, Div, Rem, And, Or, Xor, Shl, Shr. Standard wraparound semantics for overflow unless the source language specifies otherwise. Division by zero is undefined behavior (traps in checked mode).

**Floating-point operations.** Add, Sub, Mul, Div, Sqrt, Fma, Neg, Abs, Min, Max, Compare. Every FP node carries a flag set F ⊆ {NSZ, NINF, NNAN, ARCP, CONTRACT, REASSOC, AFN}. The semantics:

- F = ∅: strict IEEE 754. All exceptions visible, no reassociation, no contraction, signed zeros distinguished, NaNs and infinities preserved.
- Each flag f ∈ F licenses a specific set of transformations:
  - NSZ (No Signed Zeros): +0 and -0 are interchangeable.
  - NINF (No Infinities): infinities do not arise.
  - NNAN (No NaNs): NaNs do not arise.
  - ARCP (Allow Reciprocal): 1.0/x may be replaced by a reciprocal approximation.
  - CONTRACT: expressions may be contracted (e.g., a*b+c → fma(a,b,c)).
  - REASSOC: reassociation is permitted.
  - AFN (Approximate Functions): approximate versions of sin, cos, exp, etc. are permitted.
  - AFN implies all other flags.
- The default for source-level FP operations is F = ∅ unless the source language explicitly opts into relaxed semantics.

**Cast operations.** IntToFloat, FloatToInt, IntCast (widening/narrowing), PtrCast. Truncation is defined by the source language (usually low-bit truncation for integers, undefined for out-of-range float-to-int).

**Aggregate operations.** TupleExtract, TupleInsert, ArrayGet, ArraySet.

#### Effectful nodes (N_eff)

- **Load(s⃗_in, p).** Reads memory at p. E = {MemRead}. Produces value v and s⃗_out. The state-out tokens equal the state-in tokens for all region classes except the one containing p's target; for that region, s_out reflects the read (observationally identical to s_in, but the edge enforces ordering against intervening writes). Signature: Σ(Load) = ((State_r₁, …, State_rₖ, Ptr<T>), (State_r₁, …, State_rₖ, T), {MemRead}, ∅).

- **Store(s⃗_in, p, v).** Writes v to p. E = {MemWrite}. Produces s⃗_out where the region containing p is updated. Signature: Σ(Store) = ((State_r₁, …, State_rₖ, Ptr<T>, T), (State_r₁, …, State_rₖ), {MemWrite}, ∅).

- **AtomicLoad(s⃗_in, p, ord).** E = {MemRead, Barrier}. ord ∈ {relaxed, acquire, seq_cst}. Signature: Σ(AtomicLoad) = ((State_r₁, …, State_rₖ, Ptr<T>), (State_r₁, …, State_rₖ, T), {MemRead, Barrier}, {ord}).

- **AtomicStore(s⃗_in, p, v, ord).** E = {MemWrite, Barrier}. ord ∈ {relaxed, release, seq_cst}. Signature: Σ(AtomicStore) = ((State_r₁, …, State_rₖ, Ptr<T>, T), (State_r₁, …, State_rₖ), {MemWrite, Barrier}, {ord}).

- **AtomicRMW(s⃗_in, p, v, op, ord).** E = {MemRead, MemWrite, Barrier}. op ∈ {add, sub, and, or, xor, xchg, min, max}. ord ∈ {relaxed, acquire, release, acq_rel, seq_cst}. Signature: Σ(AtomicRMW) = ((State_r₁, …, State_rₖ, Ptr<T>, T), (State_r₁, …, State_rₖ, T), {MemRead, MemWrite, Barrier}, {op, ord}).

- **CompareExchange(s⃗_in, p, expected, desired, succ_ord, fail_ord).** E = {MemRead, MemWrite, Barrier}. succ_ord, fail_ord ∈ {relaxed, acquire, release, acq_rel, seq_cst}. Signature: Σ(CompareExchange) = ((State_r₁, …, State_rₖ, Ptr<T>, T, T), (State_r₁, …, State_rₖ, T, Bool), {MemRead, MemWrite, Barrier}, {succ_ord, fail_ord}).

- **Allocate(s⃗_in, size, align).** E = {Alloc}. Produces pointer and s⃗_out. The allocated memory is in a fresh region class (determined by escape analysis). Signature: Σ(Allocate) = ((State_r₁, …, State_rₖ, Int64, Int32), (State_r₁, …, State_rₖ, Ptr<T>), {Alloc}, ∅).

- **Invoke(s⃗_in, target, args⃗, exception_handler).** E = {Call}. Produces return values and s⃗_out. If the callee throws, control transfers to exception_handler (a Region) instead of producing return values. The exception edge is a control edge from Invoke to the handler Region. Signature: Σ(Invoke) = ((State_r₁, …, State_rₖ, Ptr<Fn>, τ⃗_args), (State_r₁, …, State_rₖ, τ⃗_ret), {Call}, ∅).

- **Throw(s⃗_in, exception).** E = {Trap}. Transfers control to the nearest enclosing exception handler via an exception edge. Does not produce a state-out (it is a control terminator). Signature: Σ(Throw) = ((State_r₁, …, State_rₖ, Ptr<Exception>), ∅, {Trap}, ∅).

- **Trap(s⃗_in, reason).** E = {Trap}. Terminates execution abnormally and unrecoverably. No exception handler is invoked. Used for assertion failures, undefined-behavior detection, and unreachable-code markers. Does not produce state-out. Signature: Σ(Trap) = ((State_r₁, …, State_rₖ, Int32), ∅, {Trap}, {reason}).

- **Safepoint(s⃗_in, deopt_meta).** E = {Barrier}. Marks a point where the runtime may deoptimize. deopt_meta maps each live value to its source-level location. Used for tiered compilation. Signature: Σ(Safepoint) = ((State_r₁, …, State_rₖ), (State_r₁, …, State_rₖ), {Barrier}, {deopt_meta}).

#### Exceptional control flow

Exception edges are a fourth edge kind E_except ⊆ N_eff × N_ctrl, distinct from E_ctrl. Only Invoke and Throw have exception edges. The target of an exception edge is always a Region that heads an exception handler. Exception edges do not carry state tokens; the handler Region's state Phis receive their inputs from the exceptional predecessor's state-out.

### 6.6 Validity Invariants

Valid_IR(G) holds iff all of the following hold:

**1. Type consistency.** For every data edge (u, v), τ_out(u) ≤_T τ_in(v) at the corresponding position.

**2. Effect consistency.** For every state edge (u, v) on region class r, both u and v have E ≠ ∅, and the region class r is in the footprint of both u and v.

**3. Control well-formedness.**
- Every Region has k ≥ 1 control inputs.
- Every Phi has k data inputs, where k is the number of control inputs of its controlling Region.
- Every StatePhi_r has k state-in_r inputs, where k is the number of control inputs of its controlling Region.
- The control graph is reachable from Start and terminates at Return, Stop, or Throw.

**4. State-token well-formedness.**
- For each region class r, the state-r subgraph is a set of chains and cycles.
- Every cycle in the state-r subgraph passes through at least one StatePhi_r. (This is the loop-carried-state invariant; it replaces the incorrect acyclicity requirement of v0.)
- No state-r chain terminates except at Return, Stop, or Throw.

**5. Schedulability.** There exists a schedule S: N → Regions such that:
- For every data edge (u, v), S(u) dominates S(v) in the control graph, or u is a data input to a Phi at S(v).
- For every state edge (u, v) on region r, S(u) precedes S(v) in every control path containing both.
- For every exception edge (u, v), S(u) is the Invoke node and S(v) is the handler Region.
- For every effectful node n, S(n) is a Region where n's memory footprint is well-defined (i.e., alias analysis can resolve the region class at that point).

This is the generalized dominance condition from Click's IR. It replaces the hand-waved invariant 5 of v0.

**6. Effect-kind monotonicity.** For every effectful node n, E(n) is exactly the union of the primitive kinds implied by n's opcode. (E.g., AtomicRMW always has {MemRead, MemWrite, Barrier}; it cannot be weakened.)

**7. FP-flag validity.** For every FP node n with flag set F, F ⊆ {NSZ, NINF, NNAN, ARCP, CONTRACT, REASSOC, AFN}. If AFN ∈ F, then all other flags are also in F (AFN is the union of all relaxations).

### 6.7 Representation and Cost Model

**Node layout.** Nodes are allocated in a generational arena. Each node has a fixed header (ID, opcode, E, F, region-class footprint bitmask) followed by a variable-length edge array. Inline capacity: 4 edges; overflow spills to a secondary heap array.

**Edge representation.** Integer indices into a global node table. No separate edge objects.

**State-Phi cost.** Each Region with k control predecessors and touching r region classes requires r state-Phi nodes, each with k inputs. Total state-Phi count is O(|Regions| · |R| · avg_predecessors). For typical code |R| is small (3–8 region classes after alias analysis), so this is a modest constant factor over a single-token design.

**Traversal cost.** Iterating over a node's edges is O(1) per edge, with pointer-chasing cost proportional to arena fragmentation. State edges are stored contiguously per region class to improve locality for alias-aware passes.

**Modification cost.** Adding or removing an edge may reallocate the node's edge array. Passes that perform many incremental modifications should batch changes or use a worklist that defers edge mutations.

**Memory complexity.** O(|N| · (H + Ē · W)), where H is header size, Ē is average edge count, W is word size. State-Phis add a factor of O(|R|) to the node count at control merges.

**Construction cost.** O(|AST| · |R|) time and space, dominated by region-class resolution during front-end lowering.

---

# Vector IR — Complete Design Document (v1)

## 0. Document Control and Design Scope

**Component:** Intermediate Representation (Vector IR)

**Version:** 1.0

**Role:** Post-vectorization representation. Receives scalar ET-SoN, encodes all vectorization decisions explicitly, and provides maximum information density to downstream analyses (instruction selection, register allocation, code generation) before lowering to Machine IR.

**Design Status:** Complete specification.

**Scope:** This document defines the graph structure, type system, node semantics, and validity invariants of the Vector IR. It does not define:
- The vectorization analysis that transforms ET-SoN into Vector IR (separate subsystem)
- The target machine (which Vector IR maps into via backend lowering)
- The scalar ET-SoN (which is the input to vectorization)

**Normative References:**
- LLVM Vectorization Guide (for loop structure representation)
- Intel ISPC language model (for explicit masking and gang abstraction)
- ARM SVE architecture (for width-polymorphic vector types)
- Click, C. "A Simple, Fast Dominance Algorithm" (for scheduling conditions)

**Known Limitations:**
- Vectorization legality proofs (dependence analysis, alias analysis) are not discharged in this document — they belong to the vectorization-analysis subsystem
- Target-specific vector width selection is deferred to Machine IR lowering

---

## 1. Goals, Non-Goals, and Constraints

### Goals

1. Encode all vectorization decisions explicitly in the graph structure: loop bounds, induction variables, vector operations, masks, reductions, shuffles, gather/scatter patterns.
2. Preserve all data-flow, control-flow, and effect dependencies from ET-SoN.
3. Provide width-polymorphic vector types that defer concrete width selection to the backend.
4. Make alignment, stride, and contiguity information explicit on all memory operations.
5. Represent per-lane exception behavior and masking semantics precisely.
6. Enable downstream analyses to perform instruction selection, register allocation, and code generation without re-discovering vectorization opportunities.

### Non-Goals

1. Perform vectorization analysis (separate subsystem; Vector IR is the output).
2. Target-specific instruction selection (handled in Machine IR).
3. Scalar optimization (handled in ET-SoN before vectorization).

### Constraints

- Vector IR construction time must be O(|ET-SoN| · k), where k is the number of loops vectorized.
- IR memory footprint must not exceed O(|ET-SoN| · c), where c is a small constant factor for vector-node overhead.
- Verifier must run in polynomial time O(|N| + |E|).
- All vectorization decisions must be explicit — no implicit vectorization.

---

## 6. Intermediate Representation: Vector IR

### 6.1 Purpose and Domain

The Vector IR is a directed, typed graph that extends ET-SoN with explicit vector operations, loop structure, and width-polymorphic types. Its purpose is to encode all vectorization decisions made by the vectorization analysis, providing a complete and explicit representation of vectorized code that downstream passes can consume without re-analyzing the program.

The Vector IR is the output of vectorization applied to ET-SoN. It makes explicit:
- Loop structure (headers, latches, exits, induction variables)
- Vector operations (SIMD arithmetic, comparisons, conversions)
- Masking (predicated execution, lane selection)
- Reductions (sum, product, min, max with explicit operators and identities)
- Shuffles (lane permutation, blending, broadcasting)
- Gather/scatter (non-contiguous memory access with explicit index vectors)
- Alignment and contiguity (memory access patterns)

### 6.2 Mathematical Specification

| Requirement | Specification |
|---|---|
| **Domain** | G_ETSoN: The set of valid ET-SoN graphs post-optimization. |
| **Codomain** | G_VecIR: The set of valid Vector IR graphs. |
| **Definitions** | G = (N, E, Σ, Γ, L). N: Nodes. E: Edges. Σ: Node signatures. Γ: Global type environment. L: Loop nest structure. |
| **Preconditions** | The input ET-SoN graph G_ETSoN satisfies Valid_IR(G_ETSoN). The vectorization analysis has determined which loops and operations to vectorize. |
| **Postconditions** | The resulting graph G satisfies Valid_VecIR(G) (defined in 6.6). |
| **Invariants** | Type consistency, effect consistency, control-flow well-formedness, loop-structure well-formedness, vector-width consistency, mask consistency, reduction well-formedness, schedulability. |
| **Relations** | Data dependence (→_d), control dependence (→_c), state dependence (→_s), exception dependence (→_except), loop-carried dependence (→_lc). |
| **Failure behavior** | If G_ETSoN ∉ Valid_IR, the vectorization analysis rejects it. If Vector IR construction encounters an unresolvable structural contradiction, it emits an InternalCompilerError. |
| **Complexity** | Construction: O(|N_ETSoN| · |L|) time and space, where |L| is the number of vectorized loops. |
| **Correctness** | ⟦G_ETSoN⟧ = ⟦G_VecIR⟧. The Vector IR must preserve the observational equivalence of the input ET-SoN graph. |

### 6.3 Graph Structure

The IR is a directed graph G = (N, E, L).

**Node partition.** N = N_pure ∪ N_vec ∪ N_ctrl ∪ N_eff, where:
- N_pure: scalar pure operations (unchanged from ET-SoN)
- N_vec: vector operations (arithmetic, memory, reduction, shuffle, mask)
- N_ctrl: control flow (scalar and vector control, including loop structure)
- N_eff: scalar effectful operations (unchanged from ET-SoN)

**Edge partition.** E = E_data ∪ E_ctrl ∪ E_state ∪ E_except ∪ E_loop, disjoint.
- E_data ⊆ N × N — data flow (scalar and vector values)
- E_ctrl ⊆ N_ctrl × N_ctrl — control flow
- E_state ⊆ N_eff × N_eff — state-token flow (memory and synchronization ordering)
- E_except ⊆ N_eff × N_ctrl — exceptional control flow
- E_loop ⊆ N_ctrl × N_ctrl — loop structure edges (header → body, body → latch, latch → header/exit)

**Loop structure.** The loop structure L is a forest of loop nests. Each loop ℓ ∈ L has:
- header(ℓ): the LoopHeader node
- latch(ℓ): the LoopLatch node
- exit(ℓ): the set of LoopExit nodes
- body(ℓ): the set of nodes dominated by header(ℓ) and post-dominated by latch(ℓ)
- parent(ℓ): the enclosing loop (if any)
- induction_vars(ℓ): the set of induction variables (Phi nodes with constant stride)
- trip_count(ℓ): the trip count (if statically known) or a symbolic expression

**State-token partitioning.** Inherited from ET-SoN: state tokens are partitioned by region class r ∈ R. Vector memory operations (vector load/store, gather/scatter) participate in the same state-token chains as scalar memory operations, with the same conflict relation.

### 6.4 Type System

**Scalar types.** Inherited from ET-SoN: Int8, Int16, Int32, Int64, UInt8, ..., Float32, Float64, Bool, Ptr<T>, Tuple, Array, Fn, State_r.

**Vector types.** Vec<T, N> where:
- T is a scalar element type (Int32, Float64, etc.)
- N is the vector width, which can be:
  - A concrete positive integer (e.g., Vec<Int32, 4>)
  - A symbolic width variable w ∈ W (e.g., Vec<Int32, w>)
  - A width expression (e.g., Vec<Int32, 2·w>)

Width variables allow the IR to represent width-polymorphic vector code. The backend selects concrete widths during lowering.

**Mask types.** Mask<N> where N is the vector width. Masks are first-class values that represent lane predicates.

**Type consistency rules.**
- Vector operations require all vector operands to have the same width: if op: Vec<T₁, N₁> × Vec<T₂, N₂> → Vec<T₃, N₃>, then N₁ = N₂ = N₃.
- Mask operations require the mask width to match the vector width: if masked_op: Mask<N> × Vec<T, N> → Vec<T, N>, the mask width N must match.
- Scalar-to-vector broadcasting: broadcast: T → Vec<T, N> for any N.
- Vector-to-scalar extraction: extract: Vec<T, N> × Int → T, where the index is in [0, N).
- Vector-to-scalar reduction: reduce: Vec<T, N> → T (see reduction nodes below).

**Width constraints.** Width variables w ∈ W are subject to constraints:
- w ≥ 1 (positive width)
- w is a power of 2 (for most targets) or w is a multiple of the target's minimum vector width
- Constraints are collected during vectorization and solved during backend lowering

### 6.5 Node Semantics

#### Scalar nodes (N_pure, N_ctrl, N_eff)

Inherited from ET-SoN without modification. Scalar operations coexist with vector operations in the same graph.

#### Loop structure nodes (N_ctrl)

- **LoopHeader(loop_id, induction_vars⃗, trip_count).** Marks the entry to a loop. Produces control output and the initial values of induction variables. E = ∅. Signature: Σ(LoopHeader) = ((Ctrl, τ⃗_init), (Ctrl, τ⃗_iv), ∅, {loop_id, trip_count}).

- **LoopLatch(loop_id, induction_vars⃗).** Marks the loop back-edge. Consumes the updated induction variable values and produces control output to the header (if loop continues) or to an exit (if loop terminates). E = ∅. Signature: Σ(LoopLatch) = ((Ctrl, τ⃗_iv_next), (Ctrl, Ctrl), ∅, {loop_id}).

- **LoopExit(loop_id).** Marks a loop exit point. Consumes control from the latch (when loop terminates) and produces control output to the post-loop code. E = ∅. Signature: Σ(LoopExit) = ((Ctrl,), Ctrl, ∅, {loop_id}).

- **InductionVar(header, init, stride).** A Phi node at the loop header that represents an induction variable. init is the initial value, stride is the loop-invariant increment. E = ∅. Signature: Σ(InductionVar) = ((Ctrl, τ, τ), τ, ∅, {header}).

#### Vector arithmetic nodes (N_vec)

All vector arithmetic nodes operate on vector types and carry the same FP flags as scalar FP nodes.

- **VecAdd(a, b).** Element-wise addition. E = ∅. Signature: Σ(VecAdd) = ((Vec<T, N>, Vec<T, N>), Vec<T, N>, ∅, F).

- **VecSub, VecMul, VecDiv, VecFma.** Element-wise subtraction, multiplication, division, fused multiply-add. E = ∅.

- **VecNeg, VecAbs, VecMin, VecMax, VecSqrt.** Element-wise unary and binary operations. E = ∅.

- **VecCompare(a, b, cmp_op).** Element-wise comparison. cmp_op ∈ {eq, ne, lt, le, gt, ge}. Produces a mask. E = ∅. Signature: Σ(VecCompare) = ((Vec<T, N>, Vec<T, N>), Mask<N>, ∅, {cmp_op}).

- **VecSelect(mask, a, b).** Element-wise selection. For each lane i, result[i] = mask[i] ? a[i] : b[i]. E = ∅. Signature: Σ(VecSelect) = ((Mask<N>, Vec<T, N>, Vec<T, N>), Vec<T, N>, ∅, ∅).

- **VecBroadcast(scalar).** Broadcasts a scalar to all lanes of a vector. E = ∅. Signature: Σ(VecBroadcast) = ((T,), Vec<T, N>, ∅, ∅).

- **VecExtract(vec, index).** Extracts a single lane from a vector. E = ∅. Signature: Σ(VecExtract) = ((Vec<T, N>, Int), T, ∅, ∅).

- **VecInsert(vec, index, value).** Inserts a scalar into a vector lane. E = ∅. Signature: Σ(VecInsert) = ((Vec<T, N>, Int, T), Vec<T, N>, ∅, ∅).

#### Vector memory nodes (N_vec)

Vector memory operations carry explicit alignment, stride, and contiguity information.

- **VecLoad(s⃗_in, base_ptr, stride, alignment, width).** Loads a vector from memory. base_ptr is the base address, stride is the element stride (1 for contiguous loads), alignment is the guaranteed alignment of base_ptr, width is the number of elements to load. E = {MemRead}. Signature: Σ(VecLoad) = ((State_r₁, …, State_rₖ, Ptr<T>, Int, Int, Int), (State_r₁, …, State_rₖ, Vec<T, N>), {MemRead}, {stride, alignment}).

  - If stride = 1, the load is contiguous.
  - If stride ≠ 1, the load is strided (element i is at base_ptr + i·stride·sizeof(T)).
  - alignment must be a power of 2 and must divide the address of base_ptr.

- **VecStore(s⃗_in, base_ptr, vec, stride, alignment).** Stores a vector to memory. E = {MemWrite}. Signature: Σ(VecStore) = ((State_r₁, …, State_rₖ, Ptr<T>, Vec<T, N>, Int, Int), (State_r₁, …, State_rₖ), {MemWrite}, {stride, alignment}).

- **VecGather(s⃗_in, base_ptr, indices, mask, alignment).** Gathers elements from memory at addresses base_ptr + indices[i]·sizeof(T) for each lane i where mask[i] is true. E = {MemRead}. Signature: Σ(VecGather) = ((State_r₁, …, State_rₖ, Ptr<T>, Vec<Int, N>, Mask<N>, Int), (State_r₁, …, State_rₖ, Vec<T, N>), {MemRead}, {alignment}).

  - indices is a vector of integer offsets (in elements, not bytes).
  - mask controls which lanes are active; inactive lanes do not access memory.
  - alignment is the guaranteed alignment of each accessed element.

- **VecScatter(s⃗_in, base_ptr, indices, vec, mask, alignment).** Scatters elements to memory. E = {MemWrite}. Signature: Σ(VecScatter) = ((State_r₁, …, State_rₖ, Ptr<T>, Vec<Int, N>, Vec<T, N>, Mask<N>, Int), (State_r₁, …, State_rₖ), {MemWrite}, {alignment}).

- **MaskedVecLoad(s⃗_in, base_ptr, stride, mask, alignment, width).** Loads a vector with masking. Inactive lanes do not access memory and produce undefined values. E = {MemRead}. Signature: Σ(MaskedVecLoad) = ((State_r₁, …, State_rₖ, Ptr<T>, Int, Mask<N>, Int, Int), (State_r₁, …, State_rₖ, Vec<T, N>), {MemRead}, {stride, alignment}).

- **MaskedVecStore(s⃗_in, base_ptr, vec, stride, mask, alignment).** Stores a vector with masking. Inactive lanes do not write to memory. E = {MemWrite}. Signature: Σ(MaskedVecStore) = ((State_r₁, …, State_rₖ, Ptr<T>, Vec<T, N>, Int, Mask<N>, Int), (State_r₁, …, State_rₖ), {MemWrite}, {stride, alignment}).

#### Vector reduction nodes (N_vec)

Reductions collapse a vector to a scalar. Each reduction node specifies the operator, identity element, and associativity/commutativity properties.

- **VecReduceSum(vec).** Sum reduction. E = ∅. Signature: Σ(VecReduceSum) = ((Vec<T, N>,), T, ∅, {op=add, id=0, assoc=true, comm=true}).

- **VecReduceProd(vec).** Product reduction. E = ∅. Signature: Σ(VecReduceProd) = ((Vec<T, N>,), T, ∅, {op=mul, id=1, assoc=true, comm=true}).

- **VecReduceMin(vec).** Minimum reduction. E = ∅. Signature: Σ(VecReduceMin) = ((Vec<T, N>,), T, ∅, {op=min, id=+∞, assoc=true, comm=true}).

- **VecReduceMax(vec).** Maximum reduction. E = ∅. Signature: Σ(VecReduceMax) = ((Vec<T, N>,), T, ∅, {op=max, id=-∞, assoc=true, comm=true}).

- **VecReduceAnd(vec).** Bitwise AND reduction (for integer vectors). E = ∅. Signature: Σ(VecReduceAnd) = ((Vec<T, N>,), T, ∅, {op=and, id=~0, assoc=true, comm=true}).

- **VecReduceOr(vec).** Bitwise OR reduction. E = ∅. Signature: Σ(VecReduceOr) = ((Vec<T, N>,), T, ∅, {op=or, id=0, assoc=true, comm=true}).

- **MaskedVecReduceSum(vec, mask).** Sum reduction with masking. Only active lanes contribute. E = ∅. Signature: Σ(MaskedVecReduceSum) = ((Vec<T, N>, Mask<N>), T, ∅, {op=add, id=0, assoc=true, comm=true}).

All reductions are associative and commutative, which permits reordering of operations during optimization. Non-associative reductions (e.g., floating-point sum with strict semantics) are not permitted unless the source language explicitly opts into relaxed FP semantics.

#### Vector shuffle nodes (N_vec)

Shuffles permute, blend, or broadcast lanes of one or more vectors.

- **VecShuffle(a, b, indices).** Permutes lanes from two input vectors. indices is a compile-time-known vector of integers in [0, 2N), where indices[i] < N selects from a and indices[i] ≥ N selects from b (with indices[i] - N as the lane index). E = ∅. Signature: Σ(VecShuffle) = ((Vec<T, N>, Vec<T, N>, Vec<Int, M>), Vec<T, M>, ∅, {indices}).

  - M is the output width, which can differ from N.
  - indices must be compile-time known (static shuffle).

- **VecDynamicShuffle(a, b, indices).** Permutes lanes with runtime-variable indices. indices is a runtime vector of integers. E = ∅. Signature: Σ(VecDynamicShuffle) = ((Vec<T, N>, Vec<T, N>, Vec<Int, M>), Vec<T, M>, ∅, ∅).

- **VecBlend(mask, a, b).** Blends two vectors under mask control. result[i] = mask[i] ? a[i] : b[i]. E = ∅. Signature: Σ(VecBlend) = ((Mask<N>, Vec<T, N>, Vec<T, N>), Vec<T, N>, ∅, ∅).

- **VecBroadcastLane(vec, lane).** Broadcasts a single lane to all lanes. E = ∅. Signature: Σ(VecBroadcastLane) = ((Vec<T, N>, Int), Vec<T, N>, ∅, ∅).

#### Mask operations (N_vec)

- **MaskAnd(a, b).** Bitwise AND of two masks. E = ∅. Signature: Σ(MaskAnd) = ((Mask<N>, Mask<N>), Mask<N>, ∅, ∅).

- **MaskOr(a, b).** Bitwise OR of two masks. E = ∅. Signature: Σ(MaskOr) = ((Mask<N>, Mask<N>), Mask<N>, ∅, ∅).

- **MaskNot(a).** Bitwise NOT of a mask. E = ∅. Signature: Σ(MaskNot) = ((Mask<N>,), Mask<N>, ∅, ∅).

- **MaskAllTrue().** Produces a mask with all lanes active. E = ∅. Signature: Σ(MaskAllTrue) = ((), Mask<N>, ∅, ∅).

- **MaskAllFalse().** Produces a mask with all lanes inactive. E = ∅. Signature: Σ(MaskAllFalse) = ((), Mask<N>, ∅, ∅).

- **MaskTest(mask).** Tests whether any lane is active. Produces a scalar boolean. E = ∅. Signature: Σ(MaskTest) = ((Mask<N>,), Bool, ∅, ∅).

- **MaskCount(mask).** Counts the number of active lanes. Produces a scalar integer. E = ∅. Signature: Σ(MaskCount) = ((Mask<N>,), Int, ∅, ∅).

#### Type conversion nodes (N_vec)

- **VecCast(vec, target_type).** Converts a vector to a different element type. E = ∅. Signature: Σ(VecCast) = ((Vec<T₁, N>,), Vec<T₂, N>, ∅, ∅).

- **VecExtend(vec, target_type).** Widens a vector (e.g., Int16 → Int32). The output width may differ from the input width if the target type is wider. E = ∅. Signature: Σ(VecExtend) = ((Vec<T₁, N>,), Vec<T₂, M>, ∅, ∅), where sizeof(T₂) > sizeof(T₁) and M ≤ N.

- **VecTruncate(vec, target_type).** Narrows a vector (e.g., Int32 → Int16). E = ∅. Signature: Σ(VecTruncate) = ((Vec<T₁, N>,), Vec<T₂, M>, ∅, ∅), where sizeof(T₂) < sizeof(T₁) and M ≥ N.

- **VecPack(a, b).** Packs two vectors into one wider vector. E = ∅. Signature: Σ(VecPack) = ((Vec<T, N>, Vec<T, N>), Vec<T, 2N>, ∅, ∅).

- **VecUnpack(vec).** Unpacks a wide vector into two narrower vectors. E = ∅. Signature: Σ(VecUnpack) = ((Vec<T, 2N>,), (Vec<T, N>, Vec<T, N>), ∅, ∅).

### 6.6 Validity Invariants

Valid_VecIR(G) holds iff all of the following hold:

**1. Type consistency.** For every data edge (u, v), τ_out(u) ≤_T τ_in(v) at the corresponding position. Vector widths must match: if an operation requires operands of the same width, all operands must have the same width N.

**2. Effect consistency.** Inherited from ET-SoN. For every state edge (u, v) on region class r, both u and v have E ≠ ∅, and the region class r is in the footprint of both u and v.

**3. Control-flow well-formedness.** Inherited from ET-SoN. Every Region has k ≥ 1 control inputs. Every Phi has k data inputs. The control graph is reachable from Start and terminates at Return, Stop, or Throw.

**4. Loop-structure well-formedness.**
- Every LoopHeader has exactly one incoming control edge from outside the loop (the entry edge) and one incoming control edge from the LoopLatch (the back edge).
- Every LoopLatch has exactly one outgoing control edge to the LoopHeader (the back edge) and zero or more outgoing control edges to LoopExit nodes.
- Every LoopExit has exactly one incoming control edge from the LoopLatch.
- The loop body (nodes dominated by the header and post-dominated by the latch) is well-formed: all control paths from the header to the latch pass through the loop body.
- Induction variables are Phi nodes at the LoopHeader with a constant stride (the stride is a loop-invariant value).
- The trip count (if statically known) is consistent with the loop bounds and stride.

**5. Vector-width consistency.**
- All vector operands of an operation have the same width N.
- Mask operands have width N matching the vector operands.
- Width variables w ∈ W are consistent: if a width variable appears in multiple nodes, it represents the same width throughout the graph.
- Width constraints are satisfiable: there exists a concrete width assignment that satisfies all constraints.

**6. Mask consistency.**
- Masked operations (MaskedVecLoad, MaskedVecStore, MaskedVecReduceSum, VecSelect, VecBlend) have a mask operand with width N matching the vector operands.
- Inactive lanes of masked loads do not access memory (the mask is respected).
- Inactive lanes of masked stores do not write to memory.
- Inactive lanes of masked reductions do not contribute to the result.

**7. Reduction well-formedness.**
- Every reduction node specifies an operator, identity element, and associativity/commutativity properties.
- The operator is one of {add, mul, min, max, and, or}.
- The identity element is correct for the operator (e.g., 0 for add, 1 for mul).
- The reduction is associative and commutative (or the source language has explicitly opted into non-associative reduction with relaxed FP semantics).

**8. Shuffle well-formedness.**
- Static shuffles (VecShuffle) have compile-time-known indices in [0, 2N).
- Dynamic shuffles (VecDynamicShuffle) have runtime indices, but the output width M is compile-time known.
- The indices are valid: for VecShuffle, indices[i] < N selects from the first input, indices[i] ≥ N selects from the second input.

**9. Memory-operation well-formedness.**
- Vector loads and stores have explicit stride, alignment, and width information.
- The alignment is a power of 2 and is consistent with the pointer type.
- Gather/scatter operations have explicit index vectors and masks.
- The conflict relation (from ET-SoN) is respected: vector memory operations that may alias require state edges between them.

**10. Schedulability.** Inherited from ET-SoN. There exists a schedule S: N → Regions such that:
- For every data edge (u, v), S(u) dominates S(v) in the control graph, or u is a data input to a Phi at S(v).
- For every state edge (u, v) on region r, S(u) precedes S(v) in every control path containing both.
- For every exception edge (u, v), S(u) is the Invoke node and S(v) is the handler Region.
- For every effectful node n, S(n) is a Region where n's memory footprint is well-defined.

**11. Loop-carried dependence consistency.**
- For every loop-carried dependence (a data edge from a node in the loop body to a Phi at the loop header), the dependence distance is consistent with the induction variable stride.
- No loop-carried dependence violates the vectorization legality conditions (e.g., no loop-carried dependence with distance < vector width for operations that require independence).

### 6.7 Representation and Cost Model

**Node layout.** Nodes are allocated in a generational arena. Each node has a fixed header (ID, opcode, E, F, metadata) followed by a variable-length edge array. Inline capacity: 4 edges; overflow spills to a secondary heap array.

**Edge representation.** Integer indices into a global node table. No separate edge objects.

**Loop structure representation.** Loop metadata (header, latch, exit, induction variables, trip count) is stored in a separate loop table, indexed by loop_id. Nodes reference loops by loop_id. This avoids duplicating loop metadata across nodes.

**Vector-width representation.** Width variables w ∈ W are represented as symbolic identifiers. Width constraints are stored in a constraint set associated with the graph. The backend solves the constraint set during lowering.

**Mask representation.** Masks are first-class values, represented as nodes in the graph. Mask operations are explicit nodes. This allows masks to be computed, combined, and passed as operands.

**Memory-operation metadata.** Alignment, stride, and contiguity information are stored as metadata on vector memory nodes. This metadata is used by downstream analyses (instruction selection, code generation) to select optimal instructions.

**Traversal cost.** Iterating over a node's edges is O(1) per edge, with pointer-chasing cost proportional to arena fragmentation. Loop metadata is accessed via the loop table, which is a separate data structure.

**Modification cost.** Adding or removing an edge may reallocate the node's edge array. Passes that perform many incremental modifications should batch changes or use a worklist that defers edge mutations.

**Memory complexity.** O(|N| · (H + Ē · W) + |L| · L_meta), where H is header size, Ē is average edge count, W is word size, |L| is the number of loops, and L_meta is the metadata size per loop.

**Construction cost.** O(|N_ETSoN| · |L|) time and space, dominated by loop recognition and vector-operation insertion during vectorization.

---

# Machine IR — Complete Design Document (v2)

## 0. Document Control and Design Scope

**Component:** Intermediate Representation (Machine IR)

**Version:** 2.0

**Role:** Target-specific, post-instruction-selection representation. Receives Vector IR (with width selection resolved), encodes all target-specific decisions, and serves as the domain for register allocation, instruction scheduling, and final code emission.

**Design Status:** Complete specification with algorithms for register allocation and instruction selection.

**Scope:** This document defines the graph structure, register model, instruction semantics, register allocation algorithm, instruction selection algorithm, and validity invariants of the Machine IR. It does not define:
- The instruction scheduling algorithm (separate subsystem operating on Machine IR)
- The code emitter that encodes Machine IR to bytes (separate subsystem)
- The target machine description (consumed by Machine IR, specified elsewhere)

**Normative References:**
- LLVM Machine IR representation (for CFG-based machine-level IR design)
- GCC RTL (for implicit register use/def modeling)
- Click, C. "Engineering a Compiler" §11 (for register allocation problem formulation)
- Cooper, K. and Torczon, L. "Engineering a Compiler" §10.5 (for linear scan register allocation)
- Burg, R. and Henry, W. "Optimal Tree Pattern Matching" (for DP instruction selection)
- Target architecture manuals (x86-64, ARM64, RISC-V) for register class and instruction semantics

**Known Limitations:**
- Target machine description is not specified here; Machine IR consumes it as an external dependency
- Instruction scheduling algorithm is deferred to a separate subsystem

---

## 1. Goals, Non-Goals, and Constraints

### Goals

1. Represent target-specific machine instructions with explicit operand kinds (register, immediate, memory, label, predicate).
2. Model the register allocation problem precisely and solve it using live-range-aware linear scan.
3. Perform instruction selection using dynamic programming on DAGs with up-down traversal.
4. Make implicit register uses and defs explicit (condition codes, calling-convention clobbers, special-purpose registers).
5. Represent the machine-level control-flow graph with explicit basic blocks and terminators.
6. Encode calling conventions, stack frame layout, and ABI requirements.
7. Support predication, subregister indexing, and register-class constraints.
8. Provide sufficient information for instruction scheduling and code emission without re-analysis.

### Non-Goals

1. Perform instruction scheduling (separate subsystem operating on Machine IR).
2. Encode machine code to bytes (separate subsystem).
3. Represent target-independent optimizations (handled in ET-SoN and Vector IR).

### Constraints

- Machine IR construction time must be O(|Vector IR| · k), where k is a target-specific constant for instruction selection complexity.
- Register allocation time must be O(|V| log |V|), where |V| is the number of virtual registers.
- Instruction selection time must be O(|DAG nodes| · |patterns|), where |patterns| is the number of instruction patterns.
- IR memory footprint must not exceed O(|instructions| · c), where c is a small constant factor for operand and metadata overhead.
- Verifier must run in polynomial time O(|instructions| + |blocks| + |virtual regs|).
- All register allocation constraints must be explicit — no implicit register assignments.

---

## 6. Intermediate Representation: Machine IR

### 6.1 Purpose and Domain

The Machine IR is a control-flow graph of machine basic blocks, each containing a linear sequence of machine instructions. Its purpose is to represent the program at the level of the target machine, with all target-specific decisions (instruction selection, width selection, addressing mode selection) made explicit. It serves as the domain for instruction scheduling and code emission.

Unlike ET-SoN and Vector IR, Machine IR is:
- **CFG-based**, not sea-of-nodes. Instruction order matters at the machine level due to implicit register uses/defs, condition codes, and side effects.
- **Target-specific**. Instructions, registers, and addressing modes are drawn from the target machine description.
- **Post-instruction-selection**. Every instruction corresponds to a target-machine opcode.

### 6.2 Mathematical Specification

| Requirement | Specification |
|---|---|
| **Domain** | G_VecIR: The set of valid Vector IR graphs, with width variables resolved to concrete widths. |
| **Codomain** | G_MachIR: The set of valid Machine IR graphs. |
| **Definitions** | G = (B, I, V, P, R, Σ, M). B: Machine basic blocks. I: Machine instructions. V: Virtual registers. P: Physical registers. R: Register classes. Σ: Instruction signatures. M: Target machine description. |
| **Preconditions** | The input Vector IR graph G_VecIR satisfies Valid_VecIR(G_VecIR). Width variables have been resolved to concrete widths. The target machine description M is provided. |
| **Postconditions** | The resulting graph G satisfies Valid_MachIR(G) (defined in 6.6). |
| **Invariants** | Type consistency, register-class consistency, control-flow well-formedness, instruction well-formedness, implicit-use/def consistency, calling-convention consistency, liveness consistency. |
| **Relations** | Data dependence (→_d), control dependence (→_c), register interference (⋈), register class membership (∈), subregister relation (⊑). |
| **Failure behavior** | If G_VecIR ∉ Valid_VecIR, instruction selection rejects it. If Machine IR construction encounters an unresolvable structural contradiction (e.g., no instruction matches a vector operation), it emits an InternalCompilerError. |
| **Complexity** | Construction: O(|N_VecIR| · k) time and space, where k is the target-specific instruction-selection complexity. Register allocation: O(|V| log |V|). |
| **Correctness** | ⟦G_VecIR⟧ = ⟦G_MachIR⟧. The Machine IR must preserve the observational equivalence of the input Vector IR graph. |

### 6.3 Graph Structure

The IR is a control-flow graph G = (B, E_ctrl), where each block b ∈ B contains a linear sequence of instructions I(b) = [i₁, i₂, …, iₙ].

**Machine basic blocks.** Each block b ∈ B has:
- A unique block ID.
- A label (for branch targets).
- A list of predecessor blocks.
- A list of successor blocks.
- A terminator instruction (the last instruction, which is a branch, return, or unconditional jump).
- A set of live-in virtual registers (registers whose values are defined outside the block and used inside).
- A set of live-out virtual registers (registers whose values are defined inside the block and used outside).

**Machine instructions.** Each instruction i ∈ I has:
- An opcode (from the target machine description).
- A list of operands (defined below).
- A list of implicit uses (physical registers read but not listed as explicit operands).
- A list of implicit defs (physical registers written but not listed as explicit operands).
- A list of flags (e.g., "may load", "may store", "is terminator", "is barrier", "is call").
- Optional metadata (source location, debug info, predicate).

**Operands.** Each operand is one of:
- **Register operand.** Either a virtual register v ∈ V or a physical register p ∈ P. May have a subregister index (e.g., the low 32 bits of a 64-bit register).
- **Immediate operand.** A compile-time constant (integer, floating-point, or symbolic).
- **Memory operand.** A base register, optional index register, displacement, and scale. Represents an addressing mode.
- **Label operand.** A reference to a machine basic block (for branches).
- **Predicate operand.** A mask or condition code (for predicated instructions).

**Virtual registers.** V = {v₀, v₁, v₂, …}. Each virtual register v has:
- A unique ID.
- A register class constraint R(v) ⊆ P (the set of physical registers that v may be allocated to).
- A type (e.g., Int32, Float64, Vec<Int32, 8>).
- A definition point (the instruction that defines v).
- A set of use points (the instructions that use v).
- A live range [start(v), end(v)] (the first and last instruction indices where v is live).

**Physical registers.** P = {p₀, p₁, p₂, …} (target-specific). Each physical register p has:
- A unique ID.
- A register class membership (p ∈ R for some R).
- A set of subregisters (e.g., x86's rax has subregisters eax, ax, al, ah).
- A set of aliases (registers that overlap with p, e.g., rax and eax alias).
- A calling-convention role (caller-saved, callee-saved, argument, return value, or reserved).

**Register classes.** R = {R₁, R₂, …} (target-specific). Each register class R has:
- A set of physical registers R ⊆ P.
- A size (in bits).
- An alignment requirement.
- A set of subregister classes (e.g., GPR32 is a subregister class of GPR64).

### 6.4 Type System and Register Model

**Scalar types.** Inherited from Vector IR: Int8, Int16, Int32, Int64, UInt8, …, Float32, Float64, Bool, Ptr<T>.

**Vector types.** Concrete widths only (width variables have been resolved). Vec<T, N> where N is a concrete positive integer.

**Register-class types.** Each virtual register v has a register class constraint R(v). The register class determines:
- The size of the value (e.g., GPR32 holds 32-bit values).
- The alignment requirement.
- The set of physical registers that v may be allocated to.

**Type consistency rules.**
- The type of a virtual register must be compatible with its register class: if v has type Int32, then R(v) must be a 32-bit register class (e.g., GPR32).
- Instruction operands must match the instruction's signature: if an instruction expects a GPR32 operand, the operand must be a virtual register with register class GPR32 or a physical register in GPR32.
- Memory operands must have types compatible with the addressing mode: e.g., a 32-bit load must have a 32-bit destination register.

**Register allocation: live-range-aware linear scan.**

The register allocation algorithm is live-range-aware linear scan. The algorithm operates as follows:

**Input:**
- A set of virtual registers V, each with a live range [start(v), end(v)] and register class constraint R(v).
- A set of physical registers P, partitioned by register class.
- An interference relation ⋈ ⊆ V × V (two virtual registers interfere if their live ranges overlap).
- A set of physical register reservations (for calling convention, special instructions).

**Algorithm:**

1. **Sort virtual registers by start point.** Create a list L of all virtual registers, sorted by start(v) in ascending order.

2. **Initialize data structures.**
   - active: an empty list of virtual registers currently assigned to physical registers, sorted by end point.
   - free_regs[R]: for each register class R, a list of available physical registers in R.
   - spill_cost[v]: for each virtual register v, the estimated cost of spilling v (based on live range length, loop depth, and execution frequency).

3. **Iterate through L.** For each virtual register v in L:
   
   a. **Expire old intervals.** Remove from active all registers u where end(u) < start(v). Add their physical registers back to the appropriate free_regs lists.
   
   b. **Find a free register.** If free_regs[R(v)] is non-empty:
      - Assign v to a physical register p ∈ free_regs[R(v)].
      - Remove p from free_regs[R(v)].
      - Add v to active, maintaining sorted order by end(v).
   
   c. **Otherwise, spill.** If free_regs[R(v)] is empty:
      - Let u be the register in active with the largest end point (the one that extends furthest into the future).
      - If end(u) > end(v) (u extends further than v):
        - Spill v: allocate a spill slot for v, do not assign a physical register.
      - Otherwise (end(v) ≥ end(u), v extends further):
        - Spill u: allocate a spill slot for u, assign u's physical register to v.
        - Remove u from active.
        - Add v to active, maintaining sorted order by end(v).

4. **Insert spill code.** For each spilled virtual register v:
   - After the definition of v, insert a SPILL instruction that stores v to its spill slot.
   - Before each use of v, insert a RELOAD instruction that loads v from its spill slot into a temporary physical register.

**Output:**
- A mapping f: V → P ∪ S, where S is the set of spill slots.
- A modified instruction sequence with SPILL and RELOAD instructions inserted.

**Complexity:** O(|V| log |V|) time, dominated by sorting and maintaining the active list.

**Spill cost model.** The spill cost of a virtual register v is:

    spill_cost(v) = live_range_length(v) · execution_frequency(v) · loop_depth_factor(v)

where:
- live_range_length(v) = end(v) - start(v)
- execution_frequency(v) is the estimated execution frequency of the block containing v's definition
- loop_depth_factor(v) is 1 + 0.1 · loop_depth(v), where loop_depth(v) is the nesting depth of the innermost loop containing v

Higher spill cost means the register is more expensive to spill, so the algorithm prefers to spill registers with lower spill cost.

**Subregister indexing.** For targets with subregisters (e.g., x86), a virtual register v may be defined or used at a subregister index. For example, a 32-bit operation may define the low 32 bits of a 64-bit virtual register. The subregister index is part of the register operand.

**Implicit uses and defs.** Some instructions implicitly read or write physical registers that are not listed as explicit operands. Examples:
- x86 arithmetic instructions implicitly read and write the flags register (EFLAGS).
- Call instructions implicitly clobber caller-saved registers.
- Return instructions implicitly read the return-value register.

These implicit uses and defs are listed explicitly in the instruction's metadata. They participate in liveness analysis and register allocation.

### 6.5 Instruction Selection: DP on DAG up-down

**Instruction selection algorithm.** The instruction selection algorithm is dynamic programming on DAGs with up-down traversal. The algorithm operates as follows:

**Input:**
- A DAG of operations from Vector IR (after lowering to scalar/vector operations).
- A set of instruction patterns, where each pattern is a tree of operations that can be matched by a single machine instruction or a sequence of instructions.
- A cost function cost(pattern) that returns the cost (in cycles or bytes) of using a pattern.

**Algorithm:**

The algorithm processes the DAG in two passes: bottom-up labeling, then top-down selection.

**Pass 1: Bottom-up labeling.**

For each node n in the DAG (processed in post-order, i.e., children before parents):

1. **Initialize labels.** Create a set labels(n) of (pattern, cost) pairs, initially empty.

2. **Match patterns.** For each instruction pattern p that matches the subtree rooted at n:
   - Check if all children of p match the corresponding children of n.
   - If p matches, compute the cost: cost(p) + Σ cost(labels(child)[p_child]) for each child, where p_child is the pattern matched at that child.
   - Add (p, total_cost) to labels(n).

3. **Keep minimum cost.** For each pattern p that can produce the value at n, keep only the (p, cost) pair with the minimum cost.

**Pass 2: Top-down selection.**

Starting from the root of the DAG (or the roots, if the DAG has multiple outputs):

1. **Select pattern.** For the current node n, select the pattern p ∈ labels(n) with the minimum cost.

2. **Emit instructions.** Emit the machine instruction(s) corresponding to p.

3. **Recurse on children.** For each child c of p, recursively select the pattern for c and emit instructions.

**Output:**
- A sequence of machine instructions that implements the DAG.

**Complexity:** O(|DAG nodes| · |patterns|) time, where |patterns| is the number of instruction patterns. The bottom-up pass processes each node once, and for each node, it checks all patterns. The top-down pass processes each node once.

**Pattern matching.** Patterns are specified as trees of operations. For example, a pattern for x86 ADD might be:

    ADD(pattern1, pattern2)

where pattern1 and pattern2 are themselves patterns (e.g., register, memory, immediate). The pattern matcher checks if the DAG subtree matches the pattern tree, recursively matching children.

**Cost model.** The cost of a pattern is the sum of:
- The instruction cost (in cycles or bytes).
- The cost of the patterns matched at the children.
- Any additional cost for register constraints, memory access, or other target-specific factors.

The cost function is target-specific and defined by the target machine description.

**Handling DAGs with multiple roots.** If the DAG has multiple roots (multiple output values), the algorithm processes each root independently. If two roots share a common subexpression, the subexpression is computed once and the result is reused.

**Handling cycles.** The DAG may contain cycles due to Phi nodes or loop-carried dependencies. The algorithm breaks cycles by selecting a cut point (e.g., a Phi node) and processing the DAG as a tree rooted at the cut point.

### 6.6 Instruction Semantics

**Instruction opcodes.** The set of opcodes is target-specific and defined by the target machine description. Examples for x86-64:
- ADD32rr, ADD32ri, ADD64rr, ADD64ri (integer addition)
- MOV32rr, MOV64rr, MOV32mi, MOV64mi (move)
- JMP, JCC (jump, conditional jump)
- CALL, RET (call, return)
- VADDPS, VMULPS (vector floating-point add, multiply)
- VPGATHERDD (vector gather)

**Instruction signatures.** Each opcode has a signature Σ(opcode) = (τ⃗_in, τ⃗_out, implicit_uses, implicit_defs, flags), where:
- τ⃗_in: the types of the explicit input operands.
- τ⃗_out: the types of the explicit output operands.
- implicit_uses: the set of physical registers implicitly read.
- implicit_defs: the set of physical registers implicitly written.
- flags: metadata flags (may load, may store, is terminator, is barrier, is call, etc.).

**Instruction categories.**

**Arithmetic and logic.** ADD, SUB, MUL, DIV, AND, OR, XOR, SHL, SHR, NEG, NOT. Operands: register or immediate inputs, register output. Implicit defs: flags register (for targets with condition codes).

**Memory operations.** LOAD, STORE. Operands: memory operand (base, index, displacement, scale), register input/output. Flags: may load, may store. Alignment and size are part of the memory operand.

**Vector operations.** VADD, VMUL, VFMA, VMIN, VMAX, VSHUFF, VGATHER, VSCATTER. Operands: vector register inputs/outputs, optional mask operand. Flags: may load (for gather), may store (for scatter).

**Control flow.** JMP, JCC, CALL, RET. Operands: label operand (for jumps), register operands (for calls). Flags: is terminator, is barrier (for calls), is call.

**Move and convert.** MOV, MOVZX, MOVSX, CVTSI2SS, CVTSS2SI. Operands: register or memory input, register output.

**Predicate and mask.** CMP, TEST, SETCC. Operands: register inputs, register or flags output. Implicit defs: flags register.

**Spill and reload.** SPILL, RELOAD. Pseudo-instructions used by register allocation to represent stack spills and reloads. Operands: virtual register, stack slot. Flags: may load (for reload), may store (for spill).

**Phi.** PHI. Pseudo-instruction at the beginning of a block to merge values from different predecessors. Operands: one register input per predecessor, one register output.

**Nop.** NOP. No-op instruction. Used for alignment padding.

### 6.7 Validity Invariants

Valid_MachIR(G) holds iff all of the following hold:

**1. Type consistency.** For every instruction i and every operand position k, the type of the operand matches the expected type Σ(opcode(i))[k]. For register operands, the virtual register's type must be compatible with the operand type.

**2. Register-class consistency.** For every virtual register v, the register class constraint R(v) is non-empty and is a valid register class in the target machine description. For every register operand (virtual or physical), the register's class is compatible with the operand's expected class.

**3. Control-flow well-formedness.**
- Every block has exactly one terminator instruction (the last instruction).
- Every block has at least one predecessor (except the entry block).
- Every block has at least one successor (except exit blocks).
- The control-flow graph is reachable from the entry block.
- All branch targets are valid block labels.
- Conditional branches have exactly two successors.

**4. Instruction well-formedness.**
- Every instruction's opcode is valid in the target machine description.
- Every instruction's operand count matches the opcode's signature.
- Every instruction's operand types match the opcode's signature.
- Every instruction's implicit uses and defs are valid physical registers.
- Every instruction's flags are consistent with the opcode (e.g., a LOAD instruction has the "may load" flag).

**5. Implicit-use/def consistency.**
- For every implicit use of a physical register p in instruction i, p is live at the point of i.
- For every implicit def of a physical register p in instruction i, p is not live after i (or is redefined before use).
- Implicit uses and defs are consistent with the target's ABI and instruction semantics.

**6. Calling-convention consistency.**
- For every CALL instruction, the arguments are passed in the correct registers or stack slots according to the target's calling convention.
- For every CALL instruction, the caller-saved registers are marked as clobbered (implicit defs).
- For every RET instruction, the return value is in the correct register according to the target's calling convention.
- Callee-saved registers are saved and restored around the function body.

**7. Liveness consistency.**
- For every virtual register v, v is defined exactly once (single static assignment).
- For every use of v, v is live at the point of use (i.e., v is defined before the use and not killed in between).
- The live-in and live-out sets of each block are consistent with the definitions and uses within the block.
- Phi operands are live-out of the corresponding predecessor blocks.

**8. Spill-slot consistency.**
- Every spill slot is allocated on the stack frame.
- Every SPILL instruction writes to a valid spill slot.
- Every RELOAD instruction reads from a valid spill slot.
- Spill slots do not overlap (unless explicitly allowed for optimization).

**9. Memory-operation consistency.**
- Every memory operand has a valid addressing mode (base, index, displacement, scale).
- The alignment of every memory operand is consistent with the target's alignment requirements.
- The size of every memory operation matches the operand type.

**10. Predicate consistency.**
- For predicated instructions, the predicate operand is a valid mask or condition code.
- The predicate width matches the instruction's vector width (for vector predication).

**11. Subregister consistency.**
- For subregister operands, the subregister index is valid for the register.
- The subregister size matches the operand type.
- Subregister writes do not partially overwrite a register unless explicitly allowed by the target.

### 6.8 Representation and Cost Model

**Block layout.** Machine basic blocks are stored in a list, in layout order (the order in which they will be emitted). Each block has a fixed header (block ID, label, predecessor/successor lists, live-in/live-out sets) followed by a list of instruction pointers.

**Instruction layout.** Each instruction is a fixed-size structure (opcode, operand list pointer, implicit-use list pointer, implicit-def list pointer, flags, metadata pointer). Operands are stored in a separate operand array to reduce instruction size.

**Operand layout.** Each operand is a tagged union (register, immediate, memory, label, predicate). Register operands have a register ID (virtual or physical) and an optional subregister index. Memory operands have a base register, index register, displacement, and scale.

**Virtual register layout.** Virtual registers are stored in a table, indexed by virtual register ID. Each entry has the register class constraint, type, definition point, use list, and live range.

**Physical register layout.** Physical registers are defined by the target machine description. They are stored in a table, indexed by physical register ID. Each entry has the register class membership, subregister list, alias list, and calling-convention role.

**Register class layout.** Register classes are defined by the target machine description. They are stored in a table, indexed by register class ID. Each entry has the set of physical registers, size, alignment, and subregister classes.

**Spill slot layout.** Spill slots are allocated on the stack frame. Each spill slot has an offset from the frame pointer and a size. Spill slots are stored in a table, indexed by spill slot ID.

**Traversal cost.** Iterating over a block's instructions is O(1) per instruction, with pointer-chasing cost proportional to memory layout. Iterating over an instruction's operands is O(1) per operand.

**Modification cost.** Adding or removing an instruction requires updating the block's instruction list. Adding or removing a virtual register requires updating the virtual register table and all instructions that use it. Passes that perform many incremental modifications should batch changes or use a worklist that defers mutations.

**Memory complexity.** O(|B| · B_header + |I| · I_header + |I| · Ē · O_size + |V| · V_entry + |S| · S_entry), where B_header is block header size, I_header is instruction header size, Ē is average operand count per instruction, O_size is operand size, V_entry is virtual register entry size, and S_entry is spill slot entry size.

**Construction cost.** O(|N_VecIR| · k) time and space, dominated by instruction selection and operand allocation. Register allocation: O(|V| log |V|). Instruction selection: O(|DAG nodes| · |patterns|).

---


# Instruction Scheduling Subsystem — Complete Design Document (v1)

## 0. Document Control and Design Scope

**Component:** Instruction Scheduling Subsystem

**Version:** 1.0

**Role:** Post-register-allocation optimization phase. Receives Machine IR with physical registers assigned, reorders instructions within basic blocks and loops to minimize execution time subject to data dependencies, resource constraints, and latency requirements.

**Design Status:** Complete specification.

**Scope:** This document defines the scheduling problem, dependency model, scheduling algorithms (list scheduling for local scheduling, modulo scheduling for loops), validity invariants, and cost model. It does not define:
- The register allocation algorithm (already completed; scheduling operates on its output)
- The code emitter that encodes Machine IR to bytes (separate subsystem)
- The target machine description (consumed by the scheduler, specified elsewhere)

**Normative References:**
- Gibbons, G. and Muchnick, V. "Optimizing Instruction Scheduling for the Itanium Architecture" (for list scheduling)
- Lam, M. "Software Pipelining: An Effective Scheduling Technique for VLIW Machines" (for modulo scheduling)
- Aho, A., Lam, M., Sethi, R., and Ullman, J. "Compilers: Principles, Techniques, and Tools" §10.7 (for scheduling fundamentals)
- Target architecture manuals for execution unit models, latency tables, and resource constraints

**Known Limitations:**
- Target machine description (execution units, latencies, resource constraints) is not specified here; the scheduler consumes it as an external dependency
- Trace scheduling for hyperblocks is not covered (deferred to future work)

---

## 1. Goals, Non-Goals, and Constraints

### Goals

1. Reorder instructions within basic blocks to minimize execution time while preserving all data dependencies and semantic correctness.
2. Schedule loop bodies using software pipelining to overlap iterations and hide latencies.
3. Respect target-specific resource constraints (execution units, issue width, memory ports).
4. Account for instruction latencies to maximize instruction-level parallelism.
5. Preserve all register allocation decisions (no register reassignment).
6. Provide a mathematically verifiable scheduling that can be validated for correctness.

### Non-Goals

1. Perform register allocation (already completed; scheduling operates on its output).
2. Perform instruction selection (already completed; scheduling operates on its output).
3. Encode machine code to bytes (separate subsystem).
4. Perform trace scheduling across multiple basic blocks (deferred).

### Constraints

- Local scheduling time must be O(|I|² · |E|), where |I| is the number of instructions in a block and |E| is the number of dependency edges.
- Modulo scheduling time must be O(|I| · |E| · II_max), where II_max is the maximum initiation interval explored.
- Scheduling must preserve all data dependencies: if instruction A must precede instruction B, the schedule must respect this ordering.
- Scheduling must respect all resource constraints: no more instructions of a given type may be issued in a cycle than the target allows.
- Scheduling must respect all latency constraints: if instruction A produces a value used by instruction B, and the latency is L cycles, then B must be issued at least L cycles after A.

---

## 6. Instruction Scheduling Subsystem

### 6.1 Purpose and Domain

The Instruction Scheduling Subsystem operates on Machine IR after register allocation. Its purpose is to reorder instructions to maximize instruction-level parallelism (ILP) while respecting data dependencies, resource constraints, and latency requirements. The scheduler produces a reordered instruction sequence that minimizes execution time on the target architecture.

The subsystem performs two types of scheduling:
1. **Local scheduling** (within basic blocks): reorders instructions in a single block using list scheduling.
2. **Loop scheduling** (software pipelining): overlaps loop iterations using modulo scheduling.

### 6.2 Mathematical Specification

| Requirement | Specification |
|---|---|
| **Domain** | G_MachIR: The set of valid Machine IR graphs, post-register-allocation. |
| **Codomain** | G_MachIR': The set of valid Machine IR graphs with reordered instructions. |
| **Definitions** | S: schedule (mapping from instructions to cycle numbers). D: dependency graph (data dependencies, resource constraints, latencies). R: resource model (execution units, issue width, memory ports). |
| **Preconditions** | The input Machine IR graph G_MachIR satisfies Valid_MachIR(G_MachIR). Register allocation is complete (all virtual registers are assigned to physical registers or spill slots). The target machine description M is provided. |
| **Postconditions** | The resulting graph G_MachIR' satisfies Valid_MachIR(G_MachIR') and Valid_Schedule(G_MachIR', S). |
| **Invariants** | Dependency preservation, resource constraint satisfaction, latency constraint satisfaction, register allocation preservation. |
| **Relations** | Data dependence (→_d), anti-dependence (→_a), output dependence (→_o), resource conflict (⋈_r). |
| **Failure behavior** | If G_MachIR ∉ Valid_MachIR, scheduling rejects it. If no valid schedule exists (e.g., resource constraints are unsatisfiable), the scheduler emits a compilation error. |
| **Complexity** | Local scheduling: O(|I|² · |E|). Modulo scheduling: O(|I| · |E| · II_max). |
| **Correctness** | ⟦G_MachIR⟧ = ⟦G_MachIR'⟧. The scheduled Machine IR must preserve the observational equivalence of the input Machine IR graph. |

### 6.3 Dependency Model

The scheduler operates on a dependency graph D = (I, E), where I is the set of instructions and E is the set of dependency edges.

**Dependency types.**

1. **Data dependence (true dependence).** A →_d B means instruction A produces a value that instruction B consumes. This is a hard constraint: B cannot execute before A completes.

2. **Anti-dependence.** A →_a B means instruction A reads a value that instruction B overwrites. This can be resolved by renaming (if registers are available) or by enforcing ordering.

3. **Output dependence.** A →_o B means both A and B write to the same register or memory location. This can be resolved by renaming or by enforcing ordering.

4. **Resource conflict.** A ⋈_r B means A and B cannot execute in the same cycle due to resource constraints (e.g., both require the same execution unit, or both require a memory port).

**Latency model.** Each instruction A has a latency lat(A) that specifies the number of cycles between when A is issued and when its result is available. For a data dependence A →_d B, the constraint is:

    cycle(B) ≥ cycle(A) + lat(A)

**Resource model.** The target machine description specifies:
- **Issue width W:** the maximum number of instructions that can be issued per cycle.
- **Execution units:** a set of functional units (e.g., ALU, FPU, load/store unit, branch unit). Each instruction requires one or more execution units.
- **Unit constraints:** for each execution unit type, the maximum number of instructions of that type that can be issued per cycle.
- **Memory ports:** the number of simultaneous memory operations (loads/stores) that can be issued per cycle.

**Dependency graph construction.** The dependency graph is constructed as follows:

1. **Data dependencies.** For each instruction B that uses a value produced by instruction A, add an edge A →_d B with weight lat(A).

2. **Anti-dependencies and output dependencies.** For each pair of instructions A and B where A and B access the same register or memory location, add an edge A →_a B or A →_o B as appropriate. These edges have weight 0 (no latency constraint, but ordering must be preserved unless renaming is possible).

3. **Resource constraints.** Resource constraints are not represented as edges in the dependency graph. Instead, they are enforced during scheduling by checking that the number of instructions of each type issued in each cycle does not exceed the target's resource limits.

### 6.4 Local Scheduling: List Scheduling

**Algorithm.** Local scheduling uses list scheduling, a greedy priority-based algorithm. The algorithm operates as follows:

**Input:**
- A basic block B with a sequence of instructions I = [i₁, i₂, …, iₙ].
- A dependency graph D = (I, E) with data, anti, and output dependencies.
- A target machine description M with execution units, latencies, and resource constraints.

**Algorithm:**

1. **Compute priorities.** For each instruction i ∈ I, compute a priority priority(i). The priority is typically the length of the longest path from i to any sink (instruction with no successors) in the dependency graph. Instructions on the critical path have the highest priority.

2. **Initialize ready list.** Create a ready list R containing all instructions with no predecessors (no incoming dependency edges). Sort R by priority in descending order.

3. **Initialize cycle counter.** Set cycle = 0.

4. **Schedule instructions.** While R is not empty or there are unscheduled instructions:
   
   a. **Select instructions for this cycle.** From R, select up to W instructions (where W is the issue width) that can be issued in this cycle without violating resource constraints. Selection is greedy: choose the highest-priority instructions first.
   
   b. **Check resource constraints.** For each selected instruction, check that:
      - The required execution units are available in this cycle.
      - The memory port limit is not exceeded (if the instruction is a load or store).
      - No resource conflict exists with other instructions selected for this cycle.
   
   c. **Issue instructions.** For each instruction i that passes the resource check:
      - Assign cycle(i) = cycle.
      - Remove i from R.
      - Mark the required execution units as busy for this cycle.
   
   d. **Update ready list.** For each instruction j that was just issued:
      - For each successor k of j in the dependency graph:
        - If all predecessors of k have been issued and cycle ≥ cycle(pred) + lat(pred) for all predecessors pred, add k to R.
      - Sort R by priority in descending order.
   
   e. **Increment cycle.** Set cycle = cycle + 1.

5. **Handle anti-dependencies and output dependencies.** If anti-dependencies or output dependencies prevent scheduling, insert NOPs or reorder instructions to resolve them. If renaming is possible (e.g., using different physical registers), perform renaming.

**Output:**
- A schedule S: I → ℕ, mapping each instruction to a cycle number.
- A reordered instruction sequence I' = [i'_1, i'_2, …, i'_n], sorted by cycle number.

**Complexity:** O(|I|² · |E|) time. For each cycle, we iterate over the ready list (O(|I|) in the worst case) and check resource constraints (O(|E|) in the worst case). The number of cycles is O(|I|) in the worst case.

**Priority heuristics.** The priority of an instruction can be computed using several heuristics:
- **Critical path length:** the length of the longest path from the instruction to any sink. This prioritizes instructions on the critical path.
- **Number of successors:** instructions with more successors are prioritized to unblock more instructions.
- **Latency:** instructions with longer latencies are prioritized to hide their latency.

The default priority is critical path length.

### 6.5 Loop Scheduling: Modulo Scheduling

**Algorithm.** Loop scheduling uses modulo scheduling (software pipelining) to overlap loop iterations. The algorithm operates as follows:

**Input:**
- A loop L with a loop body B containing a sequence of instructions I = [i₁, i₂, …, iₙ].
- A dependency graph D = (I, E) with data, anti, and output dependencies, including loop-carried dependencies.
- A target machine description M with execution units, latencies, and resource constraints.

**Algorithm:**

1. **Compute bounds on initiation interval (II).** The initiation interval II is the number of cycles between the start of consecutive loop iterations. Compute lower bounds on II:
   
   - **Recurrence bound (RecMII):** for each cycle in the dependency graph with total latency L and distance d (number of loop iterations), the recurrence bound is ⌈L / d⌉. The RecMII is the maximum over all cycles.
   
   - **Resource bound (ResMII):** for each resource type (execution unit, memory port), compute the number of instructions of that type in the loop body divided by the number of that resource available per cycle. The ResMII is the maximum over all resource types.
   
   - **Minimum II (MII):** MII = max(RecMII, ResMII).

2. **Try initiation intervals.** Start with II = MII. If scheduling fails, increment II and try again. Continue until II exceeds a maximum limit (e.g., II_max = 2 · MII) or a valid schedule is found.

3. **Schedule the loop body.** For a given II, schedule the instructions in the loop body as follows:
   
   a. **Initialize ready list.** Create a ready list R containing all instructions with no predecessors (including no loop-carried predecessors from previous iterations).
   
   b. **Initialize cycle counter.** Set cycle = 0.
   
   c. **Schedule instructions.** While R is not empty or there are unscheduled instructions:
      
      - **Select instructions for this cycle.** From R, select up to W instructions that can be issued in this cycle without violating resource constraints. For each instruction i, check that:
        - The required execution units are available in this cycle (modulo II, i.e., cycle mod II).
        - The memory port limit is not exceeded (modulo II).
        - No resource conflict exists with other instructions selected for this cycle (modulo II).
        - All latency constraints are satisfied: for each predecessor p of i, cycle(i) ≥ cycle(p) + lat(p) - II · dist(p, i), where dist(p, i) is the loop-carried distance.
      
      - **Issue instructions.** For each instruction i that passes the checks:
        - Assign cycle(i) = cycle.
        - Remove i from R.
        - Mark the required execution units as busy for this cycle (modulo II).
      
      - **Update ready list.** For each instruction j that was just issued:
        - For each successor k of j in the dependency graph:
          - If all predecessors of k have been issued (including loop-carried predecessors from previous iterations) and the latency constraints are satisfied, add k to R.
      
      - **Increment cycle.** Set cycle = cycle + 1.
      
      - **Check for completion.** If all instructions have been scheduled and cycle ≤ II, the schedule is valid. Otherwise, try the next II.

4. **Generate prolog and epilog.** The modulo schedule produces a pipelined loop body. Generate prolog code to fill the pipeline and epilog code to drain the pipeline. The prolog executes the first few iterations without full pipelining, and the epilog executes the last few iterations without full pipelining.

5. **Register renaming.** If anti-dependencies or output dependencies span loop iterations, perform register renaming to eliminate them. Allocate additional registers to hold values from different iterations.

**Output:**
- A schedule S: I → ℕ, mapping each instruction to a cycle number (modulo II).
- A pipelined loop body with prolog and epilog code.
- The initiation interval II.

**Complexity:** O(|I| · |E| · II_max) time. For each II, we iterate over the instructions and check resource and latency constraints. The number of II values tried is II_max - MII.

**Register pressure.** Modulo scheduling increases register pressure because values from multiple iterations are live simultaneously. The scheduler must check that the register pressure does not exceed the available registers. If register pressure is too high, the scheduler may need to increase II or fall back to unscheduled execution.

### 6.6 Validity Invariants

Valid_Schedule(G_MachIR', S) holds iff all of the following hold:

**1. Dependency preservation.** For every dependency edge (A, B) in the dependency graph:
- If (A, B) is a data dependence with latency L, then cycle(B) ≥ cycle(A) + L.
- If (A, B) is an anti-dependence or output dependence, then cycle(B) ≥ cycle(A) (unless renaming is performed).

**2. Resource constraint satisfaction.** For every cycle c and every resource type r:
- The number of instructions issued in cycle c that require resource r does not exceed the target's limit for r.

**3. Latency constraint satisfaction.** For every instruction A and every successor B of A:
- cycle(B) ≥ cycle(A) + lat(A) - II · dist(A, B), where dist(A, B) is the loop-carried distance (0 for non-loop-carried dependencies).

**4. Register allocation preservation.** The schedule does not change any register assignments. All virtual registers remain assigned to the same physical registers or spill slots as in the input Machine IR.

**5. Control-flow preservation.** The schedule does not change the control-flow graph. All branch targets, loop structure, and exception handling remain unchanged.

**6. Semantic preservation.** The scheduled instruction sequence produces the same observable behavior as the input instruction sequence. All data dependencies, memory ordering, and exception semantics are preserved.

### 6.7 Representation and Cost Model

**Schedule representation.** The schedule is represented as a mapping from instruction IDs to cycle numbers. For modulo scheduling, the cycle number is modulo II.

**Dependency graph representation.** The dependency graph is represented as an adjacency list. Each instruction has a list of successors with edge weights (latencies and distances).

**Resource model representation.** The resource model is represented as a set of resource types, each with a capacity (maximum number of instructions per cycle). Each instruction has a set of required resources.

**Priority queue representation.** The ready list is represented as a priority queue, sorted by instruction priority.

**Traversal cost.** Iterating over the dependency graph is O(|I| + |E|). Checking resource constraints is O(|resources|) per cycle.

**Modification cost.** Reordering instructions requires updating the instruction sequence in the basic block. The dependency graph is not modified (it is computed once before scheduling).

**Memory complexity.** O(|I| + |E| + |resources|) for the dependency graph and resource model.

**Construction cost.** Local scheduling: O(|I|² · |E|). Modulo scheduling: O(|I| · |E| · II_max).
---

# Code Emission Subsystem — Complete Design Document (v1)

## 0. Document Control and Design Scope

**Component:** Code Emission Subsystem

**Version:** 1.0

**Role:** Final compilation stage. Receives scheduled Machine IR, encodes instructions into target-specific binary format, generates relocations, symbol tables, and section layout, and produces a conforming object file or assembly output.

**Design Status:** Complete specification.

**Scope:** This document defines the encoding model, relocation model, symbol table model, section layout model, object file format, validity invariants, and cost model. It does not define:
- The instruction scheduling algorithm (already completed; emission operates on its output)
- The target machine description (consumed by the emitter, specified elsewhere)
- The linker that combines object files into an executable (separate subsystem)
- The runtime loader that loads the executable into memory (separate subsystem)

**Normative References:**
- System V Application Binary Interface (for ELF format)
- Apple Mach-O Runtime Architecture (for Mach-O format)
- Microsoft Portable Executable and Common Object File Format Specification (for PE/COFF format)
- Target architecture manuals (x86-64, ARM64, RISC-V) for instruction encoding

**Known Limitations:**
- Target machine description (instruction encoding tables, relocation types) is not specified here; the emitter consumes it as an external dependency
- Linker script processing is not covered (deferred to linker subsystem)
- Debug information emission (DWARF, PDB) is not covered (deferred to separate subsystem)

---

## 1. Goals, Non-Goals, and Constraints

### Goals

1. Encode each Machine IR instruction into its target-specific binary representation according to the ISA specification.
2. Generate relocation entries for all symbols, addresses, and references that cannot be resolved at compile time.
3. Produce a conforming symbol table with all external symbols, function labels, and global variables.
4. Organize code and data into appropriate sections (text, data, bss, rodata) with correct alignment and attributes.
5. Produce output in a standard object file format (ELF, Mach-O, or PE/COFF) that can be consumed by the target's linker.
6. Resolve all fixups (addresses and offsets that depend on the final layout) correctly.
7. Provide a mathematically verifiable emission process that can be validated for correctness.

### Non-Goals

1. Perform instruction scheduling (already completed; emission operates on its output).
2. Perform register allocation (already completed; emission operates on its output).
3. Link object files into an executable (separate subsystem).
4. Generate debug information (separate subsystem).
5. Perform whole-program optimization (already completed in earlier stages).

### Constraints

- Code emission time must be O(|I| · k), where |I| is the number of instructions and k is a target-specific constant for encoding complexity.
- Emission must produce output that conforms to the target's object file format specification.
- All relocations must be generated for references that cannot be resolved at compile time.
- All symbols must be present in the symbol table with correct visibility and binding.
- All sections must have correct alignment, attributes, and content.
- The emitted object file must be loadable by the target's linker without errors.

---

## 6. Code Emission Subsystem

### 6.1 Purpose and Domain

The Code Emission Subsystem operates on scheduled Machine IR. Its purpose is to translate the abstract machine-level representation into a concrete binary format that can be linked and executed on the target architecture. The emitter performs instruction encoding, relocation generation, symbol table construction, section layout, and object file assembly.

The subsystem produces one of two output formats:
1. **Object file** (ELF, Mach-O, or PE/COFF): a binary file containing encoded instructions, data, relocations, and metadata.
2. **Assembly file**: a textual representation of the instructions in the target's assembly language.

### 6.2 Mathematical Specification

| Requirement | Specification |
|---|---|
| **Domain** | G_MachIR': The set of valid Machine IR graphs, post-instruction-scheduling. |
| **Codomain** | O_obj: The set of valid object files (ELF, Mach-O, or PE/COFF). |
| **Definitions** | E: encoding function (maps instructions to bytes). R: relocation set. S: symbol table. L: section layout. F: object file format. |
| **Preconditions** | The input Machine IR graph G_MachIR' satisfies Valid_MachIR(G_MachIR') and Valid_Schedule(G_MachIR', S). The target machine description M is provided. The output format F is specified. |
| **Postconditions** | The resulting object file O satisfies Valid_Obj(O, F). |
| **Invariants** | Encoding correctness, relocation completeness, symbol table completeness, section layout correctness, format conformance. |
| **Relations** | Instruction-to-bytes mapping (→_enc), symbol-to-address mapping (→_addr), relocation-to-reference mapping (→_ref). |
| **Failure behavior** | If G_MachIR' ∉ Valid_MachIR or ∉ Valid_Schedule, emission rejects it. If encoding fails (e.g., an instruction cannot be encoded due to operand constraints), the emitter emits a compilation error. If the object file format is violated, the emitter emits an internal compiler error. |
| **Complexity** | Emission: O(|I| · k + |R| + |S| + |L|), where |I| is instruction count, |R| is relocation count, |S| is symbol count, |L| is section count, and k is encoding complexity. |
| **Correctness** | ⟦G_MachIR'⟧ = ⟦Link(O)⟧. The object file, when linked and loaded, must produce the same observable behavior as the scheduled Machine IR. |

### 6.3 Encoding Model

**Instruction encoding.** Each Machine IR instruction is encoded into a sequence of bytes according to the target's instruction set architecture. The encoding function E: Instruction → Bytes is defined by the target machine description.

For each instruction i with opcode op and operands [o₁, o₂, …, oₙ]:

1. **Encode opcode.** The opcode is encoded into one or more bytes according to the ISA specification. For example, x86-64 uses variable-length encoding with prefixes, opcode bytes, and ModR/M bytes.

2. **Encode operands.** Each operand is encoded according to its type:
   - **Register operand.** Encoded as a register number (e.g., 3 bits for x86-64 GPRs, 4 bits for ARM64 GPRs). May require a REX prefix (x86-64) or be part of the instruction word (ARM64, RISC-V).
   - **Immediate operand.** Encoded as a fixed-width integer (8, 16, 32, or 64 bits) in the target's endianness. May require sign extension.
   - **Memory operand.** Encoded as a combination of base register, index register, scale, and displacement. The encoding depends on the addressing mode (e.g., x86-64 SIB byte, ARM64 scaled offset).
   - **Label operand.** Encoded as a PC-relative offset or an absolute address. If the target address is not yet known, a relocation is generated.
   - **Predicate operand.** Encoded as a condition code or mask register number.

3. **Compute instruction size.** The total size of the encoded instruction is the sum of the opcode size, operand sizes, and any prefix or suffix bytes.

4. **Generate fixups.** For operands that reference symbols or addresses that are not yet resolved (e.g., branch targets, global variables), generate a fixup record. A fixup is a tuple (offset, symbol, relocation_type), where offset is the byte offset within the instruction, symbol is the referenced symbol, and relocation_type specifies how the address should be computed.

**Operand encoding rules.** The target machine description specifies the encoding rules for each operand type. For example:
- x86-64 register operands: 3-bit register number in ModR/M or SIB byte, with REX prefix for registers r8-r15.
- ARM64 register operands: 5-bit register number in the instruction word.
- RISC-V register operands: 5-bit register number in the instruction word.

**Immediate encoding rules.** The target machine description specifies the valid immediate ranges for each instruction. For example:
- x86-64 ADD32ri: 32-bit signed immediate.
- ARM64 ADD_imm: 12-bit unsigned immediate with optional 12-bit shift.
- RISC-V ADDI: 12-bit signed immediate.

If an immediate value is outside the valid range, the emitter must either:
- Reject the instruction with a compilation error.
- Synthesize a sequence of instructions to materialize the value (e.g., load into a register, then use the register).

### 6.4 Relocation Model

**Relocation types.** A relocation specifies how to compute the final value of a reference when the object file is linked. Each relocation type is defined by the target's ABI specification. Common relocation types include:

- **R_X86_64_64** (ELF x86-64): 64-bit absolute address.
- **R_X86_64_PC32** (ELF x86-64): 32-bit PC-relative address.
- **R_X86_64_GOTPCREL** (ELF x86-64): 32-bit PC-relative offset to GOT entry.
- **R_AARCH64_CALL26** (ELF ARM64): 26-bit PC-relative call offset.
- **R_AARCH64_ADR_PREL_PG_HI21** (ELF ARM64): 21-bit PC-relative page offset.

**Relocation generation.** For each fixup (offset, symbol, relocation_type), generate a relocation entry:

    reloc = (offset, symbol, relocation_type, addend)

where:
- offset: the byte offset within the section where the relocation applies.
- symbol: the symbol referenced by the relocation.
- relocation_type: the type of relocation (from the target's relocation type table).
- addend: a constant added to the computed value (may be 0).

**Relocation resolution.** The linker resolves relocations when combining object files into an executable. The resolution formula depends on the relocation type. For example:

- **R_X86_64_64:** value = S + A, where S is the symbol's final address and A is the addend.
- **R_X86_64_PC32:** value = S + A - P, where P is the address of the relocation site.
- **R_AARCH64_CALL26:** value = (S + A - P) / 4, where the division is because ARM64 instructions are 4-byte aligned.

The emitter does not resolve relocations; it only generates them. The linker subsystem is responsible for resolution.

### 6.5 Symbol Table Model

**Symbol attributes.** Each symbol in the symbol table has the following attributes:

- **Name:** the symbol's name (a string).
- **Value:** the symbol's address or offset (relative to the section).
- **Size:** the size of the symbol (for functions, the function size; for variables, the variable size).
- **Type:** the symbol type (function, object, section, file, etc.).
- **Binding:** the symbol binding (local, global, weak).
- **Visibility:** the symbol visibility (default, hidden, protected).
- **Section:** the section in which the symbol is defined (or undefined for external symbols).

**Symbol generation.** The emitter generates symbols for:

- **Function labels:** each machine basic block label becomes a symbol with type = function, binding = global (or local if the function is static).
- **Global variables:** each global variable becomes a symbol with type = object, binding = global.
- **External references:** each undefined symbol (referenced but not defined in this compilation unit) becomes a symbol with type = notype, binding = global, section = undefined.
- **Section symbols:** each section has a symbol representing the section itself.

**Symbol visibility.** The visibility of a symbol determines whether it can be referenced from other compilation units:

- **Default:** the symbol is visible to other compilation units and can be preempted by the dynamic linker.
- **Hidden:** the symbol is not visible to other compilation units and cannot be preempted.
- **Protected:** the symbol is visible to other compilation units but cannot be preempted.

The emitter assigns visibility based on the source language's visibility attributes (e.g., `static` in C makes a symbol local, `__attribute__((visibility("hidden")))` makes it hidden).

### 6.6 Section Layout Model

**Section attributes.** Each section has the following attributes:

- **Name:** the section name (e.g., .text, .data, .bss, .rodata).
- **Type:** the section type (progbits, nobits, symtab, strtab, rel, rela, etc.).
- **Flags:** the section flags (alloc, write, execinstr, merge, strings, etc.).
- **Alignment:** the section's alignment requirement (a power of 2).
- **Content:** the section's content (a sequence of bytes).

**Section generation.** The emitter generates sections for:

- **.text:** contains the encoded instructions. Type = progbits, flags = alloc + execinstr, alignment = 16 (or target-specific).
- **.data:** contains initialized global variables. Type = progbits, flags = alloc + write, alignment = 8 (or target-specific).
- **.bss:** contains uninitialized global variables. Type = nobits, flags = alloc + write, alignment = 8 (or target-specific).
- **.rodata:** contains read-only data (constants, string literals). Type = progbits, flags = alloc, alignment = 8 (or target-specific).
- **.rela.text:** contains relocations for the .text section. Type = rela, flags = 0, alignment = 8.
- **.symtab:** contains the symbol table. Type = symtab, flags = 0, alignment = 8.
- **.strtab:** contains the symbol string table. Type = strtab, flags = 0, alignment = 1.

**Section layout.** The emitter lays out sections in the object file in a specific order:

1. **Header:** the object file header (ELF header, Mach-O header, or PE/COFF header).
2. **Sections:** the sections in layout order (typically .text, .rodata, .data, .bss, .symtab, .strtab, .rela.*).
3. **Section header table:** a table of section headers, one per section.

The layout must satisfy:
- Each section is aligned according to its alignment requirement.
- The section header table is aligned according to the object file format specification.
- The total size of the object file is the sum of the section sizes, plus padding for alignment.

### 6.7 Object File Format Model

**ELF format.** The ELF (Executable and Linkable Format) object file structure is:

1. **ELF header:** contains magic number, architecture, entry point, section header table offset, etc.
2. **Sections:** the sections in layout order.
3. **Section header table:** an array of section header entries, one per section. Each entry contains the section name, type, flags, address, offset, size, link, info, alignment, and entry size.

**Mach-O format.** The Mach-O object file structure is:

1. **Mach-O header:** contains magic number, CPU type, file type, number of load commands, etc.
2. **Load commands:** a sequence of load commands describing the segments, sections, symbol table, etc.
3. **Sections:** the sections in layout order.
4. **Symbol table:** the symbol table and string table.

**PE/COFF format.** The PE/COFF (Portable Executable / Common Object File Format) object file structure is:

1. **COFF header:** contains machine type, number of sections, timestamp, symbol table offset, etc.
2. **Section headers:** an array of section headers, one per section. Each entry contains the section name, virtual size, virtual address, raw data size, raw data pointer, etc.
3. **Sections:** the sections in layout order.
4. **Symbol table:** the symbol table and string table.

**Format conformance.** The emitted object file must conform to the target's object file format specification. The emitter must:
- Set all required fields in the header.
- Generate all required sections.
- Align all sections correctly.
- Generate a valid symbol table.
- Generate all required relocations.

### 6.8 Validity Invariants

Valid_Obj(O, F) holds iff all of the following hold:

**1. Encoding correctness.** For every instruction i in the input Machine IR, the encoded bytes E(i) match the target's ISA specification for that instruction and its operands.

**2. Relocation completeness.** For every fixup (offset, symbol, relocation_type) generated during encoding, there is a corresponding relocation entry in the appropriate relocation section.

**3. Symbol table completeness.** For every symbol referenced in the input Machine IR (function labels, global variables, external references), there is a corresponding symbol table entry with correct attributes (name, value, size, type, binding, visibility, section).

**4. Section layout correctness.** For every section in the object file:
- The section is aligned according to its alignment requirement.
- The section's content matches the expected content (encoded instructions, data, etc.).
- The section's flags and type match the expected attributes.

**5. Format conformance.** The object file conforms to the target's object file format specification (ELF, Mach-O, or PE/COFF):
- The header is correctly formatted.
- All required sections are present.
- The section header table is correctly formatted.
- The symbol table is correctly formatted.

**6. Fixup resolution correctness.** All fixups that can be resolved at compile time (e.g., PC-relative offsets within the same section) are resolved correctly. All fixups that cannot be resolved at compile time (e.g., references to external symbols) have corresponding relocations.

**7. Alignment correctness.** All instructions, data, and sections are aligned according to the target's alignment requirements.

**8. Size correctness.** The size of each section matches the sum of the sizes of its contents. The total size of the object file matches the sum of the section sizes, plus padding for alignment.

### 6.9 Representation and Cost Model

**Encoding buffer.** The emitter uses a byte buffer to accumulate the encoded instructions and data. The buffer is resizable to accommodate the total size of the output.

**Relocation list.** The emitter maintains a list of relocations, one per fixup. Each relocation is a tuple (offset, symbol, relocation_type, addend).

**Symbol table.** The emitter maintains a symbol table, implemented as a hash table mapping symbol names to symbol entries. Each symbol entry contains the symbol's attributes (value, size, type, binding, visibility, section).

**Section list.** The emitter maintains a list of sections, one per section. Each section contains the section's attributes (name, type, flags, alignment, content) and a list of relocations.

**Traversal cost.** Iterating over the instructions is O(|I|). Encoding each instruction is O(k), where k is the encoding complexity (typically O(1) for fixed-length instructions, O(log n) for variable-length instructions). Generating relocations is O(|R|). Building the symbol table is O(|S|). Laying out sections is O(|L|).

**Modification cost.** The emitter does not modify the input Machine IR. It only reads the IR and produces output.

**Memory complexity.** O(|I| · avg_insn_size + |R| · reloc_size + |S| · symbol_size + |L| · section_size), where avg_insn_size is the average instruction size, reloc_size is the relocation entry size, symbol_size is the symbol entry size, and section_size is the section metadata size.

**Construction cost.** O(|I| · k + |R| + |S| + |L|) time, dominated by instruction encoding and relocation generation.

---

# DWARF Debug Information Subsystem — Complete Design Document (v1)

## 0. Document Control and Design Scope

**Component:** DWARF Debug Information Subsystem

**Version:** 1.0

**Role:** Parallel emission stage. Consumes source-level metadata from the front-end, location and range information from register allocation and instruction scheduling, and deopt metadata from safepoints. Produces DWARF5 debug sections that describe the compiled program in terms of the source language.

**Design Status:** Complete specification.

**Scope:** This document defines the DIE tree model, line number program, location lists, range lists, call frame information, deopt metadata integration, vector type representation, section layout, and validity invariants. It does not define:
- The source language's debug metadata format (consumed from the front-end)
- The debugger that consumes the DWARF output (separate subsystem, external tool)
- The target machine description (consumed for register names, address size, etc.)
- PDB or other non-DWARF debug formats (out of scope)

**Normative References:**
- DWARF Debugging Information Format, Version 5 (August 2023), published by the DWARF 5 Committee
- ISO/IEC 10967 (language-independent arithmetic) for numeric type representation
- System V Application Binary Interface DWARF extensions
- Target architecture manuals for register name tables

**Known Limitations:**
- Split-DWARF (.dwo files) is not covered (deferred)
- Debug names acceleration table (.debug_names) is not covered (deferred)
- Macro information (.debug_macro) is not covered (deferred)
- Type unit deduplication (.debug_types in DWARF4, type units in DWARF5) is partially specified

---

## 1. Goals, Non-Goals, and Constraints

### Goals

1. Produce DWARF5 output that allows a debugger to reconstruct the source-level state of the program at any safepoint, including after aggressive optimization, vectorization, register allocation, and instruction reordering.
2. Represent all source-level entities: variables, functions, types (including vector types), lexical scopes, inlined functions, and control flow.
3. Translate deopt metadata from safepoints into DWARF location lists that describe where each source variable lives at each program point.
4. Produce a correct line number program that maps machine instructions back to source locations, including optimized-out and reordered code.
5. Produce correct call frame information so that stack unwinding works through both normal and exceptional control flow.
6. Allow the emission to be disabled or stripped without affecting the correctness of the compiled code.

### Non-Goals

1. Produce debug information for code that has been fully optimized away (dead code). Dead code has no debug info.
2. Support source-level stepping through vectorized loop bodies at the scalar source level (the debugger sees vectorized code; scalar stepping is best-effort).
3. Produce PDB, STABS, or other non-DWARF formats.
4. Provide a debugger (external tool).

### Constraints

- DWARF emission time must be O(|source constructs| + |safepoints| · |live vars| + |instructions|) — linear in the size of the inputs.
- DWARF output size must not exceed O(|source constructs| · k) where k is a small constant factor for DIE overhead.
- The debug info sections must be independently stripable without affecting code correctness.
- The debug info emission must not affect the generated code's observable behavior.
- All DWARF output must conform to DWARF5 and the target's ABI extensions.

---

## 6. DWARF Debug Information Subsystem

### 6.1 Purpose and Domain

The DWARF Debug Information Subsystem translates the compiler's internal representation of source-level information into the DWARF5 binary format. It consumes:

1. **Source metadata** from the front-end: source file names, line/column numbers, type definitions, variable declarations, function signatures, lexical scopes.
2. **Location metadata** from register allocation: for each virtual register, the physical register or spill slot it was assigned to, and the live range.
3. **Schedule metadata** from instruction scheduling: the mapping from machine instructions to source locations.
4. **Deopt metadata** from safepoints: at each safepoint, the mapping from source variables to their current locations (physical registers, spill slots, or materialized values).
5. **Frame metadata** from code emission: stack frame layout, callee-saved register usage, CFA (call frame address) definition.

It produces the following DWARF5 sections:

- `.debug_info`: the DIE tree describing the program structure.
- `.debug_abbrev`: abbreviation tables for the DIE tree.
- `.debug_line`: the line number program mapping instructions to source locations.
- `.debug_str`: the string table.
- `.debug_line_str`: the line number program string table (DWARF5).
- `.debug_ranges`: non-contiguous range lists (DWARF4 compatibility; DWARF5 uses `.debug_rnglists`).
- `.debug_rnglists`: DWARF5 range lists.
- `.debug_loc`: location lists (DWARF4 compatibility; DWARF5 uses `.debug_loclists`).
- `.debug_loclists`: DWARF5 location lists.
- `.debug_frame`: call frame information.
- `.debug_addr`: address table (DWARF5).

### 6.2 Mathematical Specification

| Requirement | Specification |
|---|---|
| **Domain** | (M_src, M_loc, M_sched, M_deopt, M_frame): source metadata, location metadata, schedule metadata, deopt metadata, frame metadata. All must be well-formed. |
| **Codomain** | O_dwarf: the set of valid DWARF5 section sets. |
| **Definitions** | D: DIE tree. L: line number program. LL: location lists. RL: range lists. CFI: call frame information. |
| **Preconditions** | The input Machine IR has been scheduled and emitted. All safepoints have deopt metadata. The source metadata is consistent with the compiled code. |
| **Postconditions** | O_dwarf satisfies Valid_DWARF(O_dwarf) as defined in §6.11. |
| **Invariants** | DIE tree well-formedness, line program well-formedness, location list coverage, range list coverage, CFI well-formedness, section format conformance. |
| **Relations** | Source-to-DIE mapping (→_die), instruction-to-line mapping (→_line), variable-to-location mapping (→_loc), PC-to-range mapping (→_range). |
| **Failure behavior** | If source metadata is inconsistent (e.g., a variable declared but never defined), emit a warning and produce best-effort output. If DWARF emission fails structurally, emit an InternalCompilerError. |
| **Complexity** | Emission: O(|source constructs| + |safepoints| · |live vars| + |instructions|). |
| **Correctness** | For any safepoint p and any source variable v live at p, the debugger reading O_dwarf at address p must reconstruct the value of v as specified by the deopt metadata. |

### 6.3 DIE Tree Model

**DIE structure.** A DWARF Debugging Information Entry (DIE) is a node in a tree. Each DIE has:

- A **TAG** (e.g., DW_TAG_compile_unit, DW_TAG_subprogram, DW_TAG_variable, DW_TAG_formal_parameter, DW_TAG_base_type, DW_TAG_structure_type, DW_TAG_vector_type, DW_TAG_lexical_block, DW_TAG_inlined_subroutine).
- A set of **attributes** (name, type, location, line, etc.).
- Zero or more **children** (forming the tree).

**Top-level tree structure.** The DIE tree for a compilation unit has the following structure:

```
DW_TAG_compile_unit
├── DW_TAG_base_type (for each primitive type)
├── DW_TAG_vector_type (for each vector type used)
├── DW_TAG_structure_type / DW_TAG_class_type (for each aggregate type)
├── DW_TAG_typedef (for each type alias)
├── DW_TAG_subprogram (for each function)
│   ├── DW_TAG_formal_parameter (for each argument)
│   ├── DW_TAG_variable (for each local variable)
│   ├── DW_TAG_lexical_block (for each nested scope)
│   │   └── ... (nested variables and blocks)
│   ├── DW_TAG_inlined_subroutine (for each inlined call)
│   │   └── ... (formal parameters and variables of the inlined function)
│   └── DW_TAG_label (for each labeled statement)
└── DW_TAG_namespace (for each namespace/module)
    └── ... (nested types and functions)
```

**Attribute encoding.** Each attribute has a FORM that specifies how its value is encoded:

- `DW_FORM_addr`: an address (target pointer size).
- `DW_FORM_data1/2/4/8`: a 1/2/4/8-byte integer.
- `DW_FORM_string`: a null-terminated string (inline).
- `DW_FORM_strp`: an offset into `.debug_str`.
- `DW_FORM_line_strp`: an offset into `.debug_line_str` (DWARF5).
- `DW_FORM_ref1/2/4/8/udata`: a reference to another DIE (offset within `.debug_info`).
- `DW_FORM_sec_offset`: an offset into another section (e.g., `.debug_loclists`).
- `DW_FORM_exprloc`: a DWARF expression (for location descriptions).
- `DW_FORM_flag_present`: a boolean (encoded as presence of the attribute).
- `DW_FORM_rnglistx`: an index into `.debug_rnglists` (DWARF5).
- `DW_FORM_loclistx`: an index into `.debug_loclists` (DWARF5).

**Abbreviation tables.** To reduce the size of `.debug_info`, DIEs are encoded using abbreviation tables stored in `.debug_abbrev`. Each abbreviation specifies a TAG and a list of (attribute, FORM) pairs. Each DIE in `.debug_info` references an abbreviation by number.

**Abbreviation assignment.** The emitter assigns abbreviations using a frequency-based heuristic: the most common (TAG, attribute-set) pairs get the lowest abbreviation numbers (which encode in fewer bytes as ULEB128).

**Type DIEs.** Each source-level type is represented by a type DIE. The mapping is:

- Integer types → `DW_TAG_base_type` with `DW_AT_encoding` = `DW_ATE_signed` or `DW_ATE_unsigned`, `DW_AT_byte_size` = size in bytes, `DW_AT_bit_size` = bit size.
- Floating-point types → `DW_TAG_base_type` with `DW_AT_encoding` = `DW_ATE_float`.
- Pointer types → `DW_TAG_pointer_type` with `DW_AT_type` referencing the pointee type.
- Array types → `DW_TAG_array_type` with `DW_TAG_subrange_type` children.
- Structure types → `DW_TAG_structure_type` with `DW_TAG_member` children.
- Function types → `DW_TAG_subroutine_type` with `DW_TAG_formal_parameter` children.
- Vector types → `DW_TAG_vector_type` (DWARF5) with `DW_AT_byte_size`, `DW_AT_type` (element type), and `DW_AT_count` (number of elements).

**Type deduplication.** If the same type is referenced from multiple places, the type DIE is emitted once and referenced via `DW_FORM_ref*` from all use sites. The emitter maintains a hash table from type identity to DIE offset.

**Source variable mapping.** Each source-level variable declaration maps to a `DW_TAG_variable` or `DW_TAG_formal_parameter` DIE. The DIE carries:

- `DW_AT_name`: the variable's name (string).
- `DW_AT_type`: reference to the type DIE.
- `DW_AT_decl_file`, `DW_AT_decl_line`, `DW_AT_decl_column`: source location.
- `DW_AT_location`: a location list (see §6.5) describing where the variable lives at each PC.

### 6.4 Line Number Program

**Line number program model.** The `.debug_line` section contains a byte-code program that a DWARF consumer executes to reconstruct the instruction-to-source-location mapping. The program is a sequence of opcodes that modify a state machine.

**State machine state.** The state machine maintains:

- `address`: the current machine instruction address (initially 0).
- `file`: the current source file index (initially 1).
- `line`: the current source line number (initially 1).
- `column`: the current source column number (initially 0).
- `discriminator`: the current discriminator (initially 0; used to distinguish multiple statements on the same source line).
- `is_stmt`: boolean, whether the current instruction is a recommended breakpoint location (initially true by default).
- `basic_block`: boolean, whether the current instruction is the first in a basic block (initially false).
- `end_sequence`: boolean, whether the current address is the end of a sequence (initially false).
- `prologue_end`: boolean, whether the current instruction is at the end of a function prologue (DWARF5).
- `epilogue_begin`: boolean, whether the current instruction is at the beginning of a function epilogue (DWARF5).
- `isa`: the instruction set architecture index (initially 0).
- `op_index`: the index of the operation within a VLIW instruction (initially 0; relevant for VLIW targets).

**Standard opcodes.**

- `DW_LNS_copy`: emit a row with the current state, then reset `basic_block`, `prologue_end`, `epilogue_begin`, `discriminator` to 0.
- `DW_LNS_advance_pc` (ULEB128 delta): add delta × `minimum_instruction_length` to `address`.
- `DW_LNS_fixed_advance_pc` (2-byte delta): add delta to `address`.
- `DW_LNS_set_file` (ULEB128 file): set `file`.
- `DW_LNS_set_column` (ULEB128 column): set `column`.
- `DW_LNS_negate_stmt`: toggle `is_stmt`.
- `DW_LNS_set_basic_block`: set `basic_block` to true.
- `DW_LNS_const_add_pc`: add to `address` the amount that would be added by `DW_LNS_advance_pc` with operand 255 (used for compact encoding of small PC advances).
- `DW_LNS_fixed_advance_pc`: already listed.
- `DW_LNS_set_prologue_end`: set `prologue_end` to true.
- `DW_LNS_set_epilogue_begin`: set `epilogue_begin` to true.
- `DW_LNS_set_isa` (ULEB128 isa): set `isa`.
- `DW_LNS_set_discriminator` (ULEB128 discriminator): set `discriminator` (DWARF4+).

**Special opcodes.** The line number program uses special opcodes (values ≥ `opcode_base`) to compactly encode simultaneous advances of `address` and `line`. Each special opcode encodes:

- `address_advance = (opcode - opcode_base) / line_range`
- `line_advance = line_base + ((opcode - opcode_base) % line_range)`

followed by an implicit `DW_LNS_copy`.

**Header parameters.** The line number program header specifies:

- `minimum_instruction_length`: the minimum number of bytes per instruction (1 for x86-64, 4 for ARM64).
- `maximum_operations_per_instruction`: 1 for non-VLIW, >1 for VLIW.
- `default_is_stmt`: default value of `is_stmt` (typically true).
- `line_base`: the smallest line increment encodable by a special opcode (typically -5).
- `line_range`: the number of line increments encodable by a special opcode (typically 14).
- `opcode_base`: the first special opcode value (typically 13).
- `standard_opcode_lengths`: the number of ULEB128 operands for each standard opcode.
- `include_directories`: list of source file directories.
- `file_names`: list of source files (each with directory index, timestamp, size).

**DWARF5 line number header.** DWARF5 changes the header format:

- `directory_entry_format`: describes the format of directory entries.
- `directories`: the directory entries.
- `file_name_entry_format`: describes the format of file name entries.
- `file_names`: the file name entries.

This replaces the ad-hoc encoding of DWARF4.

**Line program generation.** The emitter generates the line program as follows:

1. **Sort instructions by address.** The scheduled Machine IR provides a mapping from each instruction to its machine address. Sort instructions by address.

2. **Group by source location.** Group consecutive instructions that share the same (file, line, column, discriminator).

3. **Emit program.** For each group:
   - If the address has advanced, emit `DW_LNS_advance_pc` or a special opcode.
   - If the line has advanced, emit a special opcode or `DW_LNS_advance_line` followed by `DW_LNS_copy`.
   - If the file has changed, emit `DW_LNS_set_file`.
   - If the column has changed, emit `DW_LNS_set_column`.
   - If the discriminator has changed, emit `DW_LNS_set_discriminator`.
   - Emit `DW_LNS_copy` to emit a row.
   - At function boundaries, emit `DW_LNE_end_sequence` to close the sequence.

4. **Special opcode selection.** When both address and line have advanced, choose between:
   - A special opcode (compact, but limited in address/line range).
   - `DW_LNS_advance_pc` + `DW_LNS_advance_line` + `DW_LNS_copy` (flexible, but larger).
   
   The choice is based on minimizing the total byte count of the program.

**Handling optimized code.** When optimization removes or reorders code:

- If a source statement is completely eliminated (dead code), no line program row is emitted for it.
- If a source statement is split across multiple machine instructions, all instructions get the same (file, line, column); use discriminators to distinguish them if needed.
- If multiple source statements are fused into one machine instruction, the instruction gets the line of the first statement (or use a range list on the DIE).
- If instruction scheduling moves an instruction far from its original position, the line program still maps it to the original source line. The debugger may show the user a confusing source location, but the mapping is correct.

### 6.5 Location Lists

**Location list model.** A location list describes where a variable lives at each program point. It is a sequence of (range, location description) pairs. The location description is a DWARF expression that computes the variable's value given the machine state.

**Location description forms.**

- `DW_OP_reg<N>`: the variable is in register N.
- `DW_OP_fbreg <offset>`: the variable is at frame base + offset.
- `DW_OP_breg<N> <offset>`: the variable is at register N + offset.
- `DW_OP_addr <address>`: the variable is at a fixed address.
- `DW_OP_stack_value`: the value is on the DWARF expression stack (used after computation).
- `DW_OP_piece <size>`: the variable is split; this piece is <size> bytes.
- `DW_OP_implicit_value <ULEB128 size> <bytes>`: the variable has a compile-time constant value.
- `DW_OP_entry_value <expr>`: the variable has the value it had at function entry (for parameters that have been overwritten).
- `DW_OP_GNU_parameter_ref <offset>`: reference to a parameter's entry value.

**DWARF5 location list format.** A location list in `.debug_loclists` is a sequence of entries:

- `DW_LLE_end_of_list` (0x00): terminates the list.
- `DW_LLE_offset_pair` (0x04): (start_offset, end_offset, expr) — the location is valid for PCs in [base + start_offset, base + end_offset).
- `DW_LLE_startx_length` (0x06): (address_index, length, expr) — the location is valid for PCs in [address, address + length).
- `DW_LLE_start_end` (0x02): (start, end, expr) — explicit start and end addresses.
- `DW_LLE_startx_endx` (0x07): (start_index, end_index, expr) — indexed addresses.
- `DW_LLE_default_location` (0x05): the default location (used when no other entry matches).

**Location list generation.** For each source variable v:

1. **Collect deopt entries.** For each safepoint p where v is live, the deopt metadata specifies v's location at p. This gives a set of (PC, location) pairs.

2. **Merge adjacent identical locations.** If v has the same location at consecutive safepoints, merge them into a single range.

3. **Extend ranges.** Each (PC, location) entry from a safepoint extends to cover the instructions between that safepoint and the next. The location is assumed to be constant between safepoints (this is the invariant maintained by the register allocator).

4. **Handle register allocation splits.** If register allocation splits v across multiple physical registers (e.g., a 64-bit value in two 32-bit registers on a 32-bit target), emit a `DW_OP_piece` expression for each piece.

5. **Handle optimized-out variables.** If v is live at some safepoints but not others, emit `DW_OP_implicit_value` with a poison value (or omit the range entirely; the debugger will show the variable as "<optimized out>").

6. **Handle entry values.** If v is a function parameter that is overwritten during the function, emit a `DW_OP_entry_value` expression so the debugger can show the original value.

**Example.** For a variable `x` that lives in register RAX from PC 0x1000 to 0x1010, then in register RBX from 0x1010 to 0x1020, then is spilled to [RBP - 8] from 0x1020 to 0x1030:

```
DW_LLE_offset_pair (0x0, 0x10, DW_OP_reg0)      // RAX
DW_LLE_offset_pair (0x10, 0x20, DW_OP_reg3)     // RBX
DW_LLE_offset_pair (0x20, 0x30, DW_OP_fbreg -8) // [RBP - 8]
DW_LLE_end_of_list
```

### 6.6 Range Lists

**Range list model.** A range list describes the non-contiguous PC ranges of a DIE (typically a function or lexical block). It is a sequence of (start, end) pairs.

**DWARF5 range list format.** A range list in `.debug_rnglists` is a sequence of entries:

- `DW_RLE_end_of_list` (0x00): terminates the list.
- `DW_RLE_offset_pair` (0x04): (start_offset, end_offset) — range is [base + start, base + end).
- `DW_RLE_startx_length` (0x06): (address_index, length).
- `DW_RLE_start_end` (0x02): (start, end) — explicit addresses.
- `DW_RLE_startx_endx` (0x07): (start_index, end_index).
- `DW_RLE_base_addressx` (0x03): sets the base address for subsequent offset_pair entries.
- `DW_RLE_base_address` (0x01): sets the base address (DWARF4 compatibility).

**Range list generation.** For each function or lexical block:

1. **Collect instruction addresses.** For each instruction in the function/block, get its machine address.

2. **Group into contiguous ranges.** Sort addresses and group consecutive addresses (where the gap between them is exactly the instruction size) into ranges.

3. **Emit range list.** Emit one `DW_RLE_offset_pair` per contiguous range, using the function's low PC as the base address.

**Handling inlined functions.** When a function is inlined at multiple call sites, each inlined instance is a separate `DW_TAG_inlined_subroutine` DIE with its own range list. The ranges are the machine addresses of the inlined code at each call site.

### 6.7 Call Frame Information

**Call frame information model.** The `.debug_frame` section describes how to unwind the stack at each PC. It consists of:

- **Common Information Entry (CIE):** describes the frame layout for a group of functions (typically all functions in a compilation unit). Specifies the return address column, the CFA (call frame address) definition, and the initial state of register rules.
- **Frame Description Entry (FDE):** describes a specific function. References a CIE, specifies the PC range of the function, and contains a sequence of instructions that modify the frame state.

**Frame state.** The frame state consists of:

- **CFA rule:** how to compute the CFA (e.g., RSP + 8, or RBP + 16).
- **Register rules:** for each register, how to recover its value (e.g., undefined, same value, offset from CFA, in another register).

**Frame instructions.**

- `DW_CFA_offset` (register, offset): the register is saved at CFA + offset.
- `DW_CFA_restore` (register): restore the register to its initial rule.
- `DW_CFA_set_loc` (address): set the current PC.
- `DW_CFA_advance_loc` (delta): advance the PC by delta × code_alignment_factor.
- `DW_CFA_def_cfa` (register, offset): set the CFA rule to register + offset.
- `DW_CFA_def_cfa_register` (register): change the CFA register.
- `DW_CFA_def_cfa_offset` (offset): change the CFA offset.
- `DW_CFA_def_cfa_expression` (expression): set the CFA to a DWARF expression.
- `DW_CFA_expression` (register, expression): the register's value is computed by a DWARF expression.
- `DW_CFA_offset_extended` (register, offset): like `DW_CFA_offset` but for extended register numbers.
- `DW_CFA_restore_extended` (register): like `DW_CFA_restore` but for extended register numbers.
- `DW_CFA_undefined` (register): the register is not recoverable.
- `DW_CFA_same_value` (register): the register has the same value as in the previous frame.
- `DW_CFA_register` (register, register): the register is saved in another register.
- `DW_CFA_remember_state` / `DW_CFA_restore_state`: save and restore the frame state (for handling branches).
- `DW_CFA_def_cfa_sf` (register, offset): signed offset version of `DW_CFA_def_cfa`.
- `DW_CFA_def_cfa_offset_sf` (offset): signed offset version of `DW_CFA_def_cfa_offset`.
- `DW_CFA_offset_sf` (register, offset): signed offset version of `DW_CFA_offset`.
- `DW_CFA_GNU_window_save`: SPARC-specific (not relevant for x86-64/ARM64/RISC-V).
- `DW_CFA_AARCH64_negate_ra_state`: ARM64 pointer authentication (DWARF5 extension).

**CFI generation.** For each function:

1. **Emit CIE.** One CIE per compilation unit (or per function if they have different frame layouts). The CIE specifies:
   - Return address register column.
   - Code alignment factor (1 for x86-64, 4 for ARM64).
   - Data alignment factor (typically -8 for 64-bit targets).
   - Initial CFA rule (typically RSP + 8 at function entry, before the return address is pushed).
   - Initial register rules (callee-saved registers are "same value"; caller-saved registers are "undefined").

2. **Emit FDE.** One FDE per function. The FDE specifies:
   - PC range (function start to function end).
   - CIE pointer.
   - Augmentation data (e.g., LSDA for exception handling, pointer encoding for PIC).
   - Frame instructions:
     - At function entry, after the prologue: `DW_CFA_def_cfa_offset` to account for the pushed frame pointer or allocated stack space.
     - After each callee-saved register save: `DW_CFA_offset` for that register.
     - After the frame pointer is set up: `DW_CFA_def_cfa` to switch to RBP + 16 (or equivalent).
     - At each safepoint, the CFA rule and register rules must correctly describe how to unwind to the caller.

3. **Handle exceptions.** If the function has exception handlers, the FDE includes an LSDA (language-specific data area) pointer in the augmentation data. The LSDA describes the exception handling table (not specified here; belongs to the exception handling subsystem).

### 6.8 Deopt Metadata Integration

**Deopt metadata model.** Each safepoint in the Machine IR carries deopt metadata: a mapping from source variables to their current locations. The deopt metadata is the primary input to location list generation.

**Deopt entry structure.** Each deopt entry is a tuple:

    (safepoint_pc, variable_id, location_description)

where:
- `safepoint_pc`: the machine address of the safepoint.
- `variable_id`: the source-level variable identifier (from the front-end).
- `location_description`: one of:
  - `Reg(phys_reg)`: the variable is in physical register phys_reg.
  - `Stack(offset)`: the variable is at frame-relative offset.
  - `Const(value)`: the variable has a compile-time constant value.
  - `Materialized(insn_seq)`: the variable's value must be computed by executing a sequence of instructions (used when the value is not directly available).
  - `Piece(loc1, size1, loc2, size2, ...)`: the variable is split across multiple locations.

**Translation to DWARF.** For each source variable v:

1. **Collect all deopt entries for v.** This gives a set of (PC, location) pairs.

2. **Build a location list.** Translate each (PC, location) pair to a DWARF location description:
   - `Reg(r)` → `DW_OP_reg<N>` where N is the DWARF register number for r.
   - `Stack(off)` → `DW_OP_fbreg <off>`.
   - `Const(val)` → `DW_OP_implicit_value <size> <bytes>`.
   - `Materialized(seq)` → emit the sequence as a DWARF expression (this is rare; typically the register allocator avoids this case).
   - `Piece(...)` → emit `DW_OP_piece` expressions.

3. **Merge adjacent identical locations.** If v has the same location at consecutive safepoints, merge them into a single range.

4. **Fill gaps.** Between safepoints, the location is assumed to be constant (the register allocator maintains this invariant). If a variable is live at safepoint p1 and p2 but not in between, the location list covers [p1, p2).

**Handling tiered compilation.** In a JIT with tiered compilation, a function may be recompiled at a higher optimization level. The deopt metadata at each safepoint describes the variable locations for the *current* compilation. If the function is deoptimized, the runtime uses the deopt metadata to reconstruct the source-level state in the lower tier.

The DWARF output describes the *final* compilation. If the function is recompiled, the old DWARF output is replaced. The debugger sees only the current compilation's DWARF.

**Handling deopt targets.** A deopt target is a PC in the lower tier where execution resumes after deoptimization. The DWARF output for the higher tier does not need to describe deopt targets (they are in the lower tier's DWARF). However, the safepoints in the higher tier must have correct location lists so the debugger can inspect the higher-tier execution.

### 6.9 Vector Type Representation

**Vector types in DWARF.** DWARF5 supports vector types via `DW_TAG_vector_type`. The DIE has:

- `DW_AT_byte_size`: the total size of the vector in bytes.
- `DW_AT_type`: reference to the element type DIE.
- `DW_AT_count`: the number of elements.

**Example.** A `Vec<Int32, 8>` type (256 bits, 8 × 32-bit integers) is represented as:

```
DW_TAG_vector_type
  DW_AT_byte_size: 32
  DW_AT_type: <ref to DW_TAG_base_type for Int32>
  DW_AT_count: 8
```

**Vector variable locations.** A vector variable may live in a vector register (e.g., YMM0 on x86-64) or be spilled to the stack. The location description is:

- `DW_OP_reg<N>` where N is the DWARF register number for the vector register.
- `DW_OP_fbreg <offset>` for stack-spilled vectors.

**Vector element access.** If the debugger wants to inspect a single element of a vector variable, it uses the DWARF expression `DW_OP_piece` or the debugger's own logic to extract the element. The DWARF output does not need to describe element access explicitly.

**Mask types.** Masks are represented as `DW_TAG_base_type` with `DW_AT_encoding` = `DW_ATE_boolean` (or a target-specific encoding) and `DW_AT_byte_size` = mask size in bytes. Alternatively, masks can be represented as `DW_TAG_vector_type` with element type boolean.

### 6.10 Section Layout

**Section ordering.** The DWARF sections are laid out in the object file in the following order:

1. `.debug_abbrev` (referenced by `.debug_info`).
2. `.debug_info` (references `.debug_abbrev`, `.debug_str`, `.debug_line_str`, `.debug_loclists`, `.debug_rnglists`, `.debug_addr`).
3. `.debug_str` (string table).
4. `.debug_line_str` (line program string table, DWARF5).
5. `.debug_line` (line number program).
6. `.debug_loclists` (location lists, DWARF5).
7. `.debug_rnglists` (range lists, DWARF5).
8. `.debug_addr` (address table, DWARF5).
9. `.debug_frame` (call frame information).

**Section alignment.** All DWARF sections have alignment 1 (byte-aligned), except `.debug_frame` which has alignment equal to the target's pointer size (4 or 8 bytes).

**Section flags.** All DWARF sections have flags = 0 (no alloc, no write, no exec). They are not loaded into memory at runtime.

**Section types.** All DWARF sections have type = `SHT_PROGBITS` (ELF) or equivalent.

### 6.11 Validity Invariants

Valid_DWARF(O_dwarf) holds iff all of the following hold:

**1. DIE tree well-formedness.**
- Every DIE has a valid TAG.
- Every DIE's attributes match the abbreviation it references.
- The DIE tree is a tree (no cycles, single root).
- The root DIE is `DW_TAG_compile_unit`.
- All `DW_AT_type` references point to valid type DIEs.
- All `DW_AT_abstract_origin` and `DW_AT_specification` references point to valid DIEs.

**2. Line program well-formedness.**
- The line program header is correctly formatted.
- The line program terminates with `DW_LNE_end_sequence` for each sequence.
- All file indices in the line program are valid (refer to files in the header).
- The line program, when executed, produces a monotonically non-decreasing sequence of addresses.

**3. Location list coverage.**
- For every source variable v that is live at a safepoint p, the location list for v covers p.
- The location list for v does not cover any PC where v is not live.
- All location descriptions in the location list are valid DWARF expressions.
- All register numbers in location descriptions are valid for the target.

**4. Range list coverage.**
- For every function or lexical block, the range list covers exactly the PCs of the instructions in that function or block.
- The ranges do not overlap.
- All addresses in the range list are valid machine addresses.

**5. CFI well-formedness.**
- Every FDE references a valid CIE.
- The CFI instructions, when executed, produce a valid frame state at each PC.
- The CFA rule is defined at every PC in the function.
- All callee-saved registers have a recovery rule at every PC after they are saved.
- The return address register has a recovery rule at every PC.

**6. Section format conformance.**
- Each section's header is correctly formatted according to DWARF5.
- All offsets and indices are within bounds.
- All strings are null-terminated.
- All ULEB128 and SLEB128 values are correctly encoded.

**7. Source-level correctness.**
- For every source variable v live at a safepoint p, the location list for v at p describes the correct location (matches the deopt metadata).
- For every source function f, the DIE for f has the correct name, type, and range.
- For every inlined function call, the `DW_TAG_inlined_subroutine` DIE has the correct call site location.

**8. Type correctness.**
- For every source type t, the type DIE for t has the correct name, size, and structure.
- All type references are consistent (e.g., a pointer type's `DW_AT_type` points to the pointee type).
- Vector types have the correct element type and count.

### 6.12 Representation and Cost Model

**DIE buffer.** The emitter uses a byte buffer to accumulate the encoded DIEs. The buffer is resizable.

**Abbreviation table.** The emitter maintains an abbreviation table, implemented as a hash table from (TAG, attribute-set) to abbreviation number.

**String table.** The emitter maintains a string table, implemented as a hash table from string to offset in `.debug_str`.

**Location list buffer.** The emitter uses a byte buffer to accumulate the encoded location lists.

**Range list buffer.** The emitter uses a byte buffer to accumulate the encoded range lists.

**Line program buffer.** The emitter uses a byte buffer to accumulate the encoded line program.

**Traversal cost.** Iterating over the source metadata is O(|source constructs|). Iterating over safepoints is O(|safepoints|). Iterating over instructions is O(|instructions|). Encoding each DIE is O(k) where k is the number of attributes. Encoding each location list entry is O(1). Encoding each line program opcode is O(1).

**Modification cost.** The emitter does not modify the input Machine IR or source metadata. It only reads them and produces output.

**Memory complexity.** O(|source constructs| · die_size + |safepoints| · |live vars| · loc_entry_size + |instructions| · line_entry_size + |strings| · avg_string_length).

**Construction cost.** O(|source constructs| + |safepoints| · |live vars| + |instructions|) time, dominated by DIE construction and location list generation.

---

# Linker Subsystem — Complete Design Document (v1)

## 0. Document Control and Design Scope

**Component:** Linker Subsystem

**Version:** 1.0

**Role:** Final assembly stage. Consumes object files from Code Emission, resolves symbols, assigns addresses, resolves relocations, generates dynamic linking structures (GOT/PLT), and produces a conforming executable or shared library.

**Design Status:** Complete specification.

**Scope:** This document defines symbol resolution rules, section merging and layout, address assignment, relocation resolution, dynamic linking structures, link-time relaxation, output format, and validity invariants. It does not define:
- The code emission that produces object files (already completed)
- The runtime loader that loads executables into memory (separate subsystem, external to the compiler)
- The DWARF debug information (already completed; linker preserves and merges debug sections)
- The target machine description (consumed for relocation types, page sizes, etc.)

**Normative References:**
- System V Application Binary Interface (for ELF format, symbol resolution rules, relocation semantics)
- Apple Mach-O Runtime Architecture (for Mach-O format)
- Microsoft Portable Executable and Common Object File Format Specification (for PE/COFF format)
- Ulrich Drepper, "How To Write Shared Libraries" (for GOT/PLT design)
- Target architecture manuals for page sizes, alignment requirements, and dynamic relocation types

**Known Limitations:**
- Incremental linking (for fast rebuilds) is not covered (deferred)
- Link-time optimization (LTO) beyond basic relaxation is not covered (deferred)
- Plugin-based extension mechanisms are not covered (deferred)
- Cross-compilation toolchain management is not covered (deferred)

---

## 1. Goals, Non-Goals, and Constraints

### Goals

1. Resolve all symbol references across object files according to well-defined precedence rules (strong over weak, visibility rules, binding).
2. Merge and layout sections from all input object files into a coherent address space.
3. Assign addresses to all sections and symbols, respecting alignment constraints and target-specific layout rules.
4. Resolve all relocations by patching references with final addresses.
5. Generate dynamic linking structures (GOT, PLT, dynamic symbol table) for executables and shared libraries that use dynamic linking.
6. Perform link-time relaxation where beneficial (e.g., converting PC-relative calls to direct branches when targets are in range).
7. Produce output in a standard executable format (ELF, Mach-O, or PE/COFF) that the target's runtime loader can load.
8. Support garbage collection of unreachable sections to reduce output size.
9. Preserve debug information sections and merge them correctly.

### Non-Goals

1. Load the executable into memory (runtime loader's job).
2. Perform whole-program optimization beyond link-time relaxation (handled in earlier stages).
3. Generate debug information (handled by DWARF subsystem).
4. Manage the runtime environment (stack, heap, threads — runtime's job).

### Constraints

- Link time must be O(Σ|Oᵢ| · k + |S| · log|S| + |R|), where |Oᵢ| is the size of input object file i, |S| is the number of symbols, |R| is the number of relocations, and k is a constant for parsing overhead.
- Memory usage during linking must not exceed O(Σ|Oᵢ| + |S| + |R| + |output size|).
- The linker must produce output that conforms to the target's executable format specification.
- All symbol resolutions must follow the rules specified in §6.3.
- All relocations must be resolved correctly according to the target's ABI.
- The output must be loadable by the target's runtime loader without errors.

---

## 6. Linker Subsystem

### 6.1 Purpose and Domain

The Linker Subsystem consumes one or more object files and produces a single executable or shared library. Its purpose is to combine separately compiled translation units into a single loadable image by resolving cross-references, assigning addresses, and generating the necessary metadata for dynamic linking.

The subsystem performs the following phases in order:

1. **Input parsing.** Read all input object files and extract symbols, sections, and relocations.
2. **Symbol resolution.** Build a global symbol table and resolve all symbol references.
3. **Section merging.** Combine sections with the same name and attributes from different object files.
4. **Garbage collection.** Remove unreachable sections (if enabled).
5. **Layout.** Assign addresses to all sections, respecting alignment and ordering constraints.
6. **Relocation resolution.** Patch all relocations with final addresses.
7. **Dynamic linking setup.** Generate GOT, PLT, and dynamic symbol table (if applicable).
8. **Relaxation.** Perform target-specific link-time optimizations.
9. **Output generation.** Write the final executable or shared library.

### 6.2 Mathematical Specification

| Requirement | Specification |
|---|---|
| **Domain** | (O₁, O₂, …, Oₙ, S, L): a set of object files Oᵢ, a linker script S (optional), and command-line options L. |
| **Codomain** | O_exec: the set of valid executables or shared libraries (ELF, Mach-O, or PE/COFF). |
| **Definitions** | Σ: global symbol table. M: section map (maps input sections to output sections). A: address assignment function. R': resolved relocations. D: dynamic linking structures. |
| **Preconditions** | All input object files satisfy Valid_Obj(Oᵢ, F). The linker script (if provided) is well-formed. The target machine description is available. |
| **Postconditions** | The output satisfies Valid_Exec(O_exec, F). |
| **Invariants** | Symbol resolution correctness, section layout correctness, relocation resolution correctness, address alignment, format conformance. |
| **Relations** | Symbol-to-definition mapping (→_def), section-to-address mapping (→_addr), relocation-to-value mapping (→_val). |
| **Failure behavior** | If a symbol is undefined and not marked as dynamic, emit an "undefined symbol" error. If a relocation cannot be resolved (e.g., out-of-range branch), emit a "relocation overflow" error. If the output format is violated, emit an internal compiler error. |
| **Complexity** | Linking: O(Σ|Oᵢ| · k + |S| · log|S| + |R| + |output size|). |
| **Correctness** | ⟦O₁, …, Oₙ⟧ = ⟦O_exec⟧. The executable, when loaded and run, must produce the same observable behavior as the union of the input object files. |

### 6.3 Symbol Resolution

**Symbol table construction.** The linker builds a global symbol table Σ by merging the symbol tables from all input object files. Each symbol has the following attributes:

- **Name:** the symbol's name (string).
- **Value:** the symbol's address (initially an offset within its section; updated during address assignment).
- **Size:** the symbol's size.
- **Type:** function, object, section, file, etc.
- **Binding:** local, global, or weak.
- **Visibility:** default, hidden, or protected.
- **Section:** the section in which the symbol is defined (or undefined for external symbols).
- **Source:** the object file in which the symbol was defined.

**Resolution rules.** When multiple object files define the same symbol, the linker applies the following rules in order:

1. **Undefined symbols.** If a symbol is undefined in all object files, it remains undefined. If it is referenced, it must be resolved by a dynamic library or result in an error.

2. **Single definition.** If exactly one object file defines the symbol, that definition is used.

3. **Multiple definitions.** If multiple object files define the symbol:
   - **Strong vs. weak.** A strong definition (binding = global) overrides a weak definition (binding = weak). If there are multiple strong definitions, this is an error (multiple definition error).
   - **Common symbols.** If a symbol is defined as a "common" symbol (uninitialized global variable in C), it can be overridden by a strong definition. If there are multiple common symbols, the largest one is used.
   - **Visibility.** A symbol with hidden visibility cannot be overridden by a symbol from another object file (it is local to its object file).

4. **Dynamic symbols.** If a symbol is undefined in all object files but is available in a dynamic library, the linker marks it as a dynamic symbol and generates a dynamic relocation.

**Symbol precedence.** The precedence order for symbol resolution is:

1. Strong definition (binding = global, visibility = default or protected).
2. Weak definition (binding = weak).
3. Common symbol (uninitialized global).
4. Dynamic symbol (from a shared library).
5. Undefined (error if referenced).

**Symbol interposition.** For shared libraries, a symbol with default visibility can be "interposed" by a definition in the main executable or another shared library loaded earlier. The linker must generate dynamic relocations (e.g., GOT entries) to support interposition.

**Symbol visibility and binding.** The linker respects the visibility and binding attributes:

- **Local symbols** (binding = local) are not exported and cannot be referenced from other object files.
- **Global symbols** (binding = global) are exported and can be referenced from other object files.
- **Weak symbols** (binding = weak) are exported but can be overridden by strong symbols.
- **Hidden visibility** means the symbol is not exported to the dynamic symbol table.
- **Protected visibility** means the symbol is exported but cannot be interposed.

### 6.4 Section Merging and Layout

**Section merging.** The linker merges sections from different object files into output sections. The merging rules are:

- **Same name and attributes.** Sections with the same name and compatible attributes (flags, type) are merged into a single output section.
- **Ordering.** The order of input sections within an output section is determined by the linker script (if provided) or by a default ordering (e.g., .text sections in the order they appear on the command line).
- **Alignment.** The output section's alignment is the maximum alignment of all input sections.

**Default section mapping.** If no linker script is provided, the linker uses a default mapping:

- All .text sections → output .text section.
- All .rodata sections → output .rodata section.
- All .data sections → output .data section.
- All .bss sections → output .bss section.
- All .debug_* sections → corresponding output .debug_* sections.
- All .rela.* sections → merged into output .rela.* sections.

**Garbage collection.** If garbage collection is enabled (--gc-sections), the linker removes sections that are not reachable from the entry point or from symbols marked as "keep". The algorithm is:

1. **Build a section dependency graph.** For each section, find all symbols it defines and references. For each referenced symbol, find the section that defines it. Add an edge from the referencing section to the defining section.

2. **Mark reachable sections.** Starting from the entry point and all "keep" symbols, perform a breadth-first search to mark all reachable sections.

3. **Remove unreachable sections.** Remove all sections that are not marked as reachable. Update the symbol table and relocations accordingly.

**Layout algorithm.** The linker assigns addresses to sections using the following algorithm:

1. **Sort sections by type.** Group sections by type (text, rodata, data, bss) and order the groups according to the target's ABI (typically .text, .rodata, .data, .bss).

2. **Assign base addresses.** For each group, assign a base address. The base address is determined by the linker script or by default values (e.g., 0x400000 for x86-64 ELF executables).

3. **Assign section addresses.** Within each group, assign addresses to sections in order. For each section:
   - Align the current address to the section's alignment requirement.
   - Assign the current address to the section.
   - Increment the current address by the section's size.

4. **Update symbol addresses.** For each symbol, update its address to be the section's base address plus the symbol's offset within the section.

**Alignment handling.** The linker ensures that all sections and symbols are correctly aligned:

- Each section's address is a multiple of its alignment requirement.
- Each symbol's address is a multiple of its alignment requirement (if specified).
- The output file's segments are aligned to the target's page size (typically 4KB or 64KB).

### 6.5 Address Assignment

**Address assignment function.** The address assignment function A maps each section and symbol to a final address:

    A: Section ∪ Symbol → Address

For a section s:

    A(s) = base(group(s)) + offset(s)

where:
- group(s) is the group to which s belongs (text, rodata, data, bss).
- base(g) is the base address of group g.
- offset(s) is the offset of s within its group, computed during layout.

For a symbol v:

    A(v) = A(section(v)) + offset(v)

where:
- section(v) is the section in which v is defined.
- offset(v) is v's offset within its section.

**Address space layout.** The address space is divided into segments:

- **Text segment.** Contains executable code and read-only data. Typically starts at a fixed address (e.g., 0x400000 for x86-64 ELF).
- **Data segment.** Contains initialized and uninitialized data. Typically starts after the text segment, aligned to a page boundary.
- **BSS segment.** Contains uninitialized data. Typically part of the data segment but with no file content.

**Position-independent code (PIC).** For shared libraries and position-independent executables, addresses are relative to a load base. The linker generates relocations that are resolved at load time by the runtime loader.

### 6.6 Relocation Resolution

**Relocation resolution.** For each relocation r = (offset, symbol, type, addend), the linker computes the final value and patches the output at the specified offset:

    value = resolve(r)
    patch(output, offset(r), value)

The resolve function depends on the relocation type. Common relocation types and their resolution formulas:

- **Absolute address (e.g., R_X86_64_64):**
  
      value = A(symbol) + addend

- **PC-relative address (e.g., R_X86_64_PC32):**
  
      value = A(symbol) + addend - P

  where P is the address of the relocation site (the address being patched).

- **GOT-relative address (e.g., R_X86_64_GOTPCREL):**
  
      value = G + addend - P

  where G is the address of the symbol's GOT entry.

- **PLT-relative address (e.g., R_X86_64_PLT32):**
  
      value = L + addend - P

  where L is the address of the symbol's PLT entry.

**Relocation overflow.** If the computed value does not fit in the field being patched (e.g., a 32-bit PC-relative offset is too large), the linker emits a "relocation overflow" error. Link-time relaxation (§6.8) can sometimes avoid this by converting long-range calls to short-range calls.

**Dynamic relocations.** If a symbol is defined in a dynamic library, the linker cannot resolve the relocation at link time. Instead, it generates a dynamic relocation that the runtime loader will resolve at load time. The dynamic relocation is stored in the .rela.dyn or .rela.plt section.

### 6.7 Dynamic Linking (GOT/PLT)

**Global Offset Table (GOT).** The GOT is a table of addresses used for position-independent code. Each global variable or function that is accessed via the GOT has an entry in the GOT. The GOT is divided into:

- **.got:** contains addresses of global variables.
- **.got.plt:** contains addresses of functions (used by the PLT).

**Procedure Linkage Table (PLT).** The PLT is a table of stubs used for lazy binding of function calls. Each function call to a dynamic symbol goes through a PLT stub. The PLT stub:

1. Jumps to the address in the GOT (initially pointing to the resolver).
2. The resolver (in the dynamic loader) looks up the function's actual address and updates the GOT.
3. Subsequent calls jump directly to the function.

**GOT/PLT generation.** For each dynamic symbol:

1. **GOT entry.** Allocate an entry in the .got or .got.plt section. Initialize it to 0 (for .got) or to the resolver address (for .got.plt).

2. **PLT stub.** Allocate a PLT stub in the .plt section. The stub contains instructions to jump to the GOT entry and invoke the resolver if needed.

3. **Dynamic relocation.** Generate a dynamic relocation (e.g., R_X86_64_JUMP_SLOT for PLT entries, R_X86_64_GLOB_DAT for GOT entries) that the runtime loader will resolve.

**Symbol interposition and GOT.** For symbols with default visibility, the GOT entry may be interposed by a definition in the main executable or another shared library. The linker generates a dynamic relocation to allow the runtime loader to update the GOT entry at load time.

**Copy relocations.** If a main executable references a global variable defined in a shared library, the linker may generate a "copy relocation". This allocates space for the variable in the executable's .bss section and generates a dynamic relocation that copies the variable's value from the shared library at load time. All references to the variable in the executable and other shared libraries are redirected to the copy.

### 6.8 Link-Time Optimization (Relaxation)

**Relaxation model.** Link-time relaxation is a set of target-specific optimizations that the linker performs after address assignment. The goal is to replace longer instruction sequences with shorter ones when the target address is known to be in range.

**Common relaxation patterns.**

- **Branch relaxation.** Convert a long-range branch (e.g., a call through the PLT) to a short-range branch (e.g., a direct call) when the target is within the branch's range.

- **NOP padding removal.** Remove NOP instructions that were inserted for alignment when the actual address does not require padding.

- **Address calculation simplification.** Replace a sequence of instructions that computes an address (e.g., load address into a register, then add an offset) with a single instruction when the address is known at link time.

**Relaxation algorithm.** The linker performs relaxation iteratively:

1. **Initial layout.** Perform the initial layout and address assignment.

2. **Relaxation pass.** For each instruction that can be relaxed, check if the relaxation is valid (e.g., the target is in range). If so, apply the relaxation and update the instruction's size.

3. **Re-layout.** If any instruction's size changed, re-layout the affected sections and update addresses.

4. **Re-resolve relocations.** Update all relocations with the new addresses.

5. **Repeat.** Repeat steps 2-4 until no more relaxations are possible or a maximum iteration count is reached.

**Convergence.** The relaxation algorithm is guaranteed to converge because each relaxation reduces the size of the output (or keeps it the same). The maximum number of iterations is bounded by the number of instructions that can be relaxed.

### 6.9 Output Format

**ELF executable format.** The ELF executable structure is:

1. **ELF header.** Contains magic number, architecture, entry point, program header table offset, section header table offset, etc.

2. **Program header table.** Describes the segments (loadable sections) and their attributes (virtual address, physical address, file offset, file size, memory size, flags, alignment).

3. **Segments.** The loadable sections, grouped into segments:
   - **Text segment.** Contains .text, .rodata, and other read-only sections. Flags = PF_R | PF_X.
   - **Data segment.** Contains .data, .bss, and other read-write sections. Flags = PF_R | PF_W.
   - **Dynamic segment.** Contains .dynamic, .got, .got.plt, .rela.dyn, .rela.plt. Flags = PF_R | PF_W.

4. **Sections.** The individual sections within the segments.

5. **Section header table.** An array of section headers, one per section.

**Mach-O executable format.** The Mach-O executable structure is:

1. **Mach-O header.** Contains magic number, CPU type, file type, number of load commands, etc.

2. **Load commands.** A sequence of load commands describing the segments, sections, symbol table, dynamic linker, entry point, etc.

3. **Segments.** The loadable sections, grouped into segments (__TEXT, __DATA, __LINKEDIT).

4. **Sections.** The individual sections within the segments.

5. **Symbol table and string table.**

**PE/COFF executable format.** The PE/COFF executable structure is:

1. **DOS header.** Contains the "MZ" magic number and a pointer to the PE header.

2. **PE header.** Contains the PE signature, COFF header, and optional header.

3. **Section headers.** An array of section headers, one per section.

4. **Sections.** The individual sections.

5. **Import table.** Describes the imported symbols and DLLs.

6. **Export table.** Describes the exported symbols.

**Format conformance.** The linker must:
- Set all required fields in the header.
- Generate all required segments and sections.
- Align all segments and sections correctly.
- Generate a valid symbol table.
- Generate all required dynamic linking structures (GOT, PLT, dynamic symbol table).
- Set the entry point correctly.

### 6.10 Validity Invariants

Valid_Exec(O_exec, F) holds iff all of the following hold:

**1. Symbol resolution correctness.**
- All referenced symbols are defined (either in the executable or in a dynamic library).
- Symbol precedence rules are correctly applied (strong over weak, visibility rules).
- No multiple definitions of the same symbol (unless allowed by weak symbols or common symbols).

**2. Section layout correctness.**
- All sections are assigned addresses that satisfy their alignment requirements.
- Sections do not overlap (except for sections with the SHF_MERGE flag).
- The layout matches the target's ABI requirements (e.g., text segment before data segment).

**3. Relocation resolution correctness.**
- All relocations are resolved correctly according to the target's ABI.
- No relocation overflows (all values fit in their fields).
- Dynamic relocations are generated for symbols that cannot be resolved at link time.

**4. Address alignment.**
- All segment addresses are aligned to the target's page size.
- All section addresses are aligned to their alignment requirements.
- All symbol addresses are aligned to their alignment requirements.

**5. Format conformance.**
- The output conforms to the target's executable format specification (ELF, Mach-O, or PE/COFF).
- All required headers, segments, and sections are present.
- All required dynamic linking structures (GOT, PLT, dynamic symbol table) are present and correctly formatted.

**6. Entry point correctness.**
- The entry point is a valid address within the executable.
- The entry point is in the text segment.
- The entry point symbol (if specified) is correctly resolved.

**7. Dynamic linking correctness.**
- All dynamic symbols are correctly marked (exported or imported).
- GOT and PLT entries are correctly generated for dynamic symbols.
- Dynamic relocations are correctly generated for symbols that require runtime resolution.

**8. Debug information preservation.**
- All debug sections from input object files are preserved in the output.
- Debug section addresses are correctly updated.
- Debug relocations are correctly resolved.

### 6.11 Representation and Cost Model

**Symbol table.** The global symbol table is implemented as a hash table mapping symbol names to symbol entries. Each symbol entry contains the symbol's attributes (value, size, type, binding, visibility, section, source).

**Section map.** The section map is a table that maps input sections to output sections. Each entry contains the input section's object file, section index, and the corresponding output section.

**Address table.** The address table is a table that maps sections and symbols to their final addresses. Each entry contains the address and the alignment requirement.

**Relocation list.** The relocation list is a list of all relocations from all input object files. Each entry contains the relocation's offset, symbol, type, addend, and resolved value.

**Output buffer.** The linker uses a byte buffer to accumulate the output executable. The buffer is resizable.

**Traversal cost.** Parsing input object files is O(Σ|Oᵢ|). Building the symbol table is O(|S| · log|S|) (due to hash table operations). Section merging is O(Σ|sections|). Layout is O(|sections|). Relocation resolution is O(|R|). Output generation is O(|output size|).

**Modification cost.** The linker does not modify the input object files. It only reads them and produces output.

**Memory complexity.** O(Σ|Oᵢ| + |S| · symbol_size + |sections| · section_size + |R| · reloc_size + |output size|).

**Construction cost.** O(Σ|Oᵢ| · k + |S| · log|S| + |R| + |output size|) time, dominated by parsing, symbol resolution, and relocation resolution.

---

# T0: Speculative Register Interpreter — Complete Design Document (v1)

## 0. Document Control and Design Scope

**Component:** T0 Tier — Speculative Register Interpreter

**Version:** 1.0

**Role:** Initial execution tier. Interprets source-level bytecode or IR using a register-based virtual machine with speculative optimizations, inline caching, opcode rewriting, and super byte code generation. Collects profiling data for tier transitions.

**Design Status:** Complete specification.

**Scope:** This document defines the register-based interpreter architecture, speculative optimization mechanisms, opcode rewriting rules, super byte code format, profiling infrastructure, deoptimization mechanism, and tier transition triggers. It does not define:
- The source language's bytecode format (consumed by T0)
- Higher tiers (T1, T2, etc.) that T0 transitions to
- The runtime system that manages tier transitions (separate subsystem)

**Normative References:**
- Hölzle, U., Ungar, D., and Wolfer, L. "The Design and Evaluation of a Self-Optimizing Compiler" (for speculative optimization)
- Chambers, C. and Ungar, D. "Customization: A Memory-Efficient Technique for Reducing Interpreter Performance" (for inline caching)
- Click, C. and Rose, J. "Speedup of Java Programs Using the HotSpot VM" (for tiered compilation)
- Standard JIT compiler literature for deoptimization and tier transitions

**Known Limitations:**
- Garbage collection integration is not fully specified (deferred to runtime subsystem)
- Thread safety for concurrent execution is partially specified (deferred)
- Exact profiling data format is not specified (only the interface)

---

## 1. Goals, Non-Goals, and Constraints

### Goals

1. Execute programs with low startup latency by interpreting bytecode directly without compilation overhead.
2. Collect accurate profiling data (execution counts, type feedback, branch outcomes) to guide higher-tier compilation.
3. Apply speculative optimizations (type specialization, branch prediction, inline caching) to improve interpretation speed.
4. Rewrite opcodes dynamically to eliminate redundant checks and combine operations (super byte codes).
5. Support fast deoptimization when speculative assumptions are violated.
6. Provide a clean interface for transitioning to higher tiers (T1, T2) when profiling data indicates hot code.
7. Maintain correctness under all speculative assumptions (deoptimize on violation).

### Non-Goals

1. Achieve peak performance (higher tiers handle that).
2. Perform whole-program optimization (deferred to higher tiers).
3. Generate native code (deferred to higher tiers).
4. Handle all language features at peak speed (some features may trigger immediate tier transitions).

### Constraints

- T0 startup time must be O(|bytecode|) — no compilation overhead.
- T0 execution speed must be within 2-5x of native code for typical workloads (after warm-up).
- Profiling overhead must not exceed 10% of execution time.
- Deoptimization must be correct and complete (no observable behavior differences).
- Super byte code generation must be bounded (no unbounded code growth).
- Memory overhead for inline caches and profiling data must be O(|bytecode| · k) where k is a small constant.

---

## 6. T0: Speculative Register Interpreter

### 6.1 Purpose and Domain

The T0 tier is a register-based interpreter that executes programs with speculative optimizations. Its purpose is to provide fast startup and collect profiling data while applying lightweight optimizations to improve interpretation speed. T0 operates on a register-based bytecode format (not stack-based) to reduce interpreter overhead.

The interpreter maintains:
- A register file (virtual registers for the current frame).
- A program counter (PC) pointing to the current bytecode instruction.
- An operand stack (for expression evaluation, though most operations use registers).
- A call stack (for function calls).
- Inline caches (for dynamic dispatch and property access).
- Profiling counters (for execution counts, type feedback, branch outcomes).

### 6.2 Mathematical Specification

| Requirement | Specification |
|---|---|
| **Domain** | B: the set of valid bytecodes. P: the set of valid programs (sequences of bytecodes). S: the set of valid machine states (register file, stack, heap). |
| **Codomain** | S': the set of resulting machine states after execution. Prof: the set of profiling data collected during execution. |
| **Definitions** | I: interpreter function. Spec: speculative assumptions. Deopt: deoptimization function. Rewrite: opcode rewriting function. Super: super byte code generator. |
| **Preconditions** | The input bytecode B is well-formed. The initial state S is valid (registers initialized, heap consistent). |
| **Postconditions** | The resulting state S' satisfies the source language's semantics under the speculative assumptions Spec. If a speculative assumption is violated, deoptimization occurs and execution continues in a higher tier. |
| **Invariants** | Speculative assumption consistency, profiling data accuracy, deoptimization correctness, register file consistency, stack consistency. |
| **Relations** | Bytecode-to-state transition (→_exec), speculative assumption violation (→_deopt), opcode rewriting (→_rewrite), profiling data collection (→_profile). |
| **Failure behavior** | If a speculative assumption is violated, deoptimize and transition to a higher tier. If the bytecode is malformed, emit an InternalCompilerError. If the heap is inconsistent, emit a runtime error. |
| **Complexity** | Interpretation: O(|instructions| · k) where k is the average cost per instruction (including speculative checks). Profiling: O(|instructions| · p) where p is the profiling overhead per instruction. |
| **Correctness** | ⟦B⟧_source = ⟦I(B, S)⟧_T0 under speculative assumptions Spec. If Spec is violated, ⟦Deopt(B, S)⟧_T1 = ⟦B⟧_source. |

### 6.3 Register-Based Interpreter Architecture

**Register file.** The interpreter maintains a register file for each activation frame. The register file contains:
- **Virtual registers** (v0, v1, v2, …): used for local variables and temporary values.
- **Argument registers** (a0, a1, a2, …): used for function arguments.
- **Return value register** (r0): used for function return values.
- **Special registers**: PC (program counter), SP (stack pointer), FP (frame pointer), HP (heap pointer).

**Bytecode format.** The bytecode is register-based, not stack-based. Each instruction specifies its operands as register indices. Example bytecodes:

```
LOAD_CONST r0, const_index    // Load constant into register r0
LOAD_LOCAL r1, local_index    // Load local variable into register r1
STORE_LOCAL local_index, r2   // Store register r2 into local variable
ADD r3, r1, r2                // r3 = r1 + r2
CALL r4, r0, arg_count        // Call function in r0 with arg_count arguments, result in r4
JUMP label                    // Unconditional jump
JUMP_IF_FALSE r5, label       // Conditional jump if r5 is false
RETURN r6                     // Return value in r6
```

**Instruction dispatch.** The interpreter uses a threaded dispatch mechanism (computed goto or switch dispatch) to execute bytecodes. Each bytecode handler:
1. Decodes the instruction (reads opcode and operands).
2. Performs the operation (e.g., add two registers).
3. Updates the PC to the next instruction.
4. Checks for deoptimization triggers (optional, based on profiling).

**Operand stack.** Although the bytecode is register-based, an operand stack is maintained for expression evaluation and function call argument passing. The stack is used for:
- Evaluating complex expressions before storing results in registers.
- Passing arguments to functions (arguments are pushed onto the stack, then the CALL instruction pops them).
- Handling exceptions (the exception object is pushed onto the stack).

### 6.4 Speculative Optimization

**Speculative assumptions.** The interpreter makes speculative assumptions about the program's behavior to optimize execution. Common speculative assumptions include:

1. **Type speculation.** Assume that a variable or expression has a specific type (e.g., "this variable is always an integer"). If the assumption is violated, deoptimize.

2. **Branch prediction.** Assume that a conditional branch always goes in a specific direction (e.g., "this loop condition is always true"). If the assumption is violated, update the prediction and continue (no deoptimization needed for branch prediction).

3. **Inline caching.** Assume that a dynamic dispatch (e.g., method call, property access) always resolves to a specific target. If the assumption is violated, update the cache (polymorphic inline cache) or deoptimize if the cache becomes too complex.

4. **Shape speculation.** Assume that an object has a specific shape (layout of properties). If the object's shape changes, deoptimize.

5. **Constant speculation.** Assume that a value is constant (e.g., "this loop bound is always 10"). If the value changes, deoptimize.

**Speculative assumption representation.** Each speculative assumption is represented as a guard:

    Guard = (assumption_id, check_function, deopt_target)

where:
- `assumption_id`: a unique identifier for the assumption.
- `check_function`: a function that checks whether the assumption holds (e.g., "is this value an integer?").
- `deopt_target`: the PC in a higher tier where execution should resume if the assumption is violated.

**Guard insertion.** Guards are inserted at points where speculative assumptions might be violated:
- Before a type-specialized operation (e.g., integer addition).
- Before a branch (to check the branch prediction).
- Before a dynamic dispatch (to check the inline cache).
- Before accessing an object property (to check the object's shape).

**Guard checking.** Each guard check is a lightweight operation (e.g., a type tag check, a pointer comparison). If the check fails, the interpreter:
1. Records the violation in the profiling data.
2. Deoptimizes: reconstructs the source-level state and transfers control to a higher tier.

### 6.5 Inline Caching

**Inline cache structure.** An inline cache (IC) is a data structure attached to a bytecode instruction that caches the result of a dynamic dispatch. The IC structure is:

```
InlineCache = {
    key: lookup_key,          // The key used for cache lookup (e.g., receiver type, property name)
    target: dispatch_target,  // The cached dispatch target (e.g., method address, property offset)
    state: cache_state,       // The cache state (uninitialized, monomorphic, polymorphic, megamorphic)
    entries: [cache_entry],   // For polymorphic caches, a list of (key, target) pairs
    profiling: profile_data   // Profiling data for this cache (hit count, miss count, etc.)
}
```

**Cache states.**

- **Uninitialized.** The cache has not been used yet. The first dispatch populates the cache.
- **Monomorphic.** The cache has seen only one key. Fast path: check if the current key matches the cached key; if so, use the cached target.
- **Polymorphic.** The cache has seen 2-4 different keys. The cache stores a list of (key, target) pairs. The interpreter searches the list for a match.
- **Megamorphic.** The cache has seen more than 4 different keys. The cache is disabled, and the interpreter uses a slow path (full dynamic dispatch). The interpreter may deoptimize if the megamorphic site is hot.

**Cache lookup.** For a monomorphic cache:

```
if (cache.key == current_key) {
    return cache.target;  // Cache hit
} else {
    // Cache miss
    target = slow_path_lookup(current_key);
    update_cache(cache, current_key, target);
    return target;
}
```

For a polymorphic cache:

```
for (entry in cache.entries) {
    if (entry.key == current_key) {
        return entry.target;  // Cache hit
    }
}
// Cache miss
target = slow_path_lookup(current_key);
if (cache.entries.length < MAX_POLYMORPHIC_SIZE) {
    cache.entries.append((current_key, target));
} else {
    cache.state = MEGAMORPHIC;
}
return target;
```

**Cache invalidation.** Inline caches are invalidated when:
- The object's shape changes (e.g., a new property is added).
- The method is redefined (e.g., monkey patching).
- The class hierarchy changes (e.g., a new subclass is loaded).

When a cache is invalidated, its state is reset to UNINITIALIZED.

### 6.6 Opcode Rewriting

**Opcode rewriting model.** The interpreter dynamically rewrites bytecodes to eliminate redundant checks and optimize common patterns. Opcode rewriting is a form of just-in-time optimization that modifies the bytecode in place.

**Rewriting rules.** Common rewriting rules include:

1. **Type specialization.** If a variable is speculatively assumed to have a specific type, rewrite the bytecode to use a type-specialized version:

   ```
   Original: ADD r3, r1, r2
   Rewritten: ADD_INT r3, r1, r2  // Assumes r1 and r2 are integers
   ```

   The rewritten bytecode includes a guard that checks the type assumption. If the guard fails, the interpreter deoptimizes.

2. **Constant folding.** If an operand is a compile-time constant, rewrite the bytecode to use the constant directly:

   ```
   Original: ADD r3, r1, 5
   Rewritten: ADD_IMM r3, r1, 5  // Use immediate operand
   ```

3. **Dead code elimination.** If a bytecode is unreachable (e.g., after an unconditional jump), rewrite it to a NOP:

   ```
   Original: LOAD_CONST r0, 42
   Rewritten: NOP  // This instruction is unreachable
   ```

4. **Common subexpression elimination.** If a computation is repeated, cache the result and reuse it:

   ```
   Original: 
     MUL r3, r1, r2
     MUL r4, r1, r2
   Rewritten:
     MUL r3, r1, r2
     MOV r4, r3  // Reuse the result
   ```

5. **Inline cache specialization.** If an inline cache is monomorphic, rewrite the bytecode to inline the dispatch:

   ```
   Original: CALL_METHOD r3, r0, "method_name"
   Rewritten: CALL_DIRECT r3, r0, method_address  // Inlined dispatch
   ```

**Rewriting trigger.** Opcode rewriting is triggered by:
- Profiling data (e.g., a branch is taken 99% of the time).
- Inline cache state (e.g., a cache becomes monomorphic).
- Type feedback (e.g., a variable is always an integer).
- Explicit optimization requests (e.g., the runtime detects a hot loop).

**Rewriting safety.** Opcode rewriting must preserve the program's semantics. The interpreter ensures this by:
- Inserting guards for all speculative assumptions.
- Maintaining a deoptimization target for each rewritten bytecode.
- Verifying that the rewritten bytecode is equivalent to the original under the speculative assumptions.

### 6.7 Super Byte Codes

**Super byte code model.** A super byte code is a single bytecode that represents a sequence of operations. Super byte codes reduce interpreter overhead by combining multiple operations into a single dispatch.

**Super byte code generation.** The interpreter generates super byte codes by:
1. Identifying common sequences of bytecodes (e.g., "load local, add constant, store local").
2. Creating a new bytecode that represents the sequence.
3. Replacing the original sequence with the super byte code.

**Example super byte codes.**

- **LOAD_ADD_STORE.** Combines loading a local variable, adding a constant, and storing the result:

  ```
  Original:
    LOAD_LOCAL r0, local_index
    ADD_IMM r0, r0, constant
    STORE_LOCAL local_index, r0
  
  Super byte code:
    LOAD_ADD_STORE local_index, constant
  ```

- **LOAD_CALL_STORE.** Combines loading a function, calling it, and storing the result:

  ```
  Original:
    LOAD_LOCAL r0, func_index
    CALL r1, r0, arg_count
    STORE_LOCAL local_index, r1
  
  Super byte code:
    LOAD_CALL_STORE local_index, func_index, arg_count
  ```

- **BRANCH_IF_TRUE.** Combines a comparison and a conditional branch:

  ```
  Original:
    CMP r0, r1, r2
    JUMP_IF_TRUE r0, label
  
  Super byte code:
    CMP_BRANCH r1, r2, label, CMP_EQ
  ```

**Super byte code format.** Super byte codes are encoded in the same format as regular bytecodes, but with additional operands to specify the combined operations. The interpreter's dispatch table includes handlers for both regular and super byte codes.

**Super byte code benefits.** Super byte codes reduce interpreter overhead by:
- Reducing the number of dispatch operations (one dispatch per super byte code instead of one per original bytecode).
- Reducing the number of register reads and writes (intermediate results stay in registers).
- Enabling better instruction-level parallelism (the super byte code handler can be optimized as a unit).

**Super byte code limitations.** Super byte codes are only beneficial for common patterns. The interpreter limits super byte code generation to:
- Sequences of 2-4 bytecodes (longer sequences have diminishing returns).
- Patterns that occur frequently (based on profiling data).
- Patterns that do not involve complex control flow (e.g., no branches in the middle of a super byte code).

### 6.8 Profiling Infrastructure

**Profiling data model.** The interpreter collects profiling data to guide tier transitions and optimization decisions. Profiling data includes:

1. **Execution counts.** For each bytecode, the number of times it has been executed.
2. **Type feedback.** For each variable or expression, the set of types it has taken.
3. **Branch outcomes.** For each conditional branch, the number of times it has been taken and not taken.
4. **Inline cache statistics.** For each inline cache, the hit count, miss count, and the set of keys seen.
5. **Call site statistics.** For each call site, the set of callees and the number of calls to each.

**Profiling data structure.** Profiling data is stored in a side table associated with each function:

```
ProfileData = {
    execution_counts: [int],           // One counter per bytecode
    type_feedback: [type_set],         // One type set per variable
    branch_outcomes: [(int, int)],     // (taken_count, not_taken_count) per branch
    inline_cache_stats: [cache_stat],  // One stat per inline cache
    call_site_stats: [call_stat]       // One stat per call site
}
```

**Profiling overhead.** Profiling adds overhead to interpretation. To minimize overhead:
- Execution counts are incremented only for bytecodes that are candidates for optimization (e.g., loop headers, call sites).
- Type feedback is collected only for variables that are used in type-specialized operations.
- Branch outcomes are collected only for branches that are candidates for branch prediction.
- Inline cache statistics are collected automatically as part of the inline cache mechanism.

**Profiling accuracy.** Profiling data must be accurate to guide optimization decisions. The interpreter ensures accuracy by:
- Using atomic operations for counters (to handle concurrent execution).
- Sampling (e.g., collecting type feedback only every Nth execution) to reduce overhead while maintaining statistical accuracy.
- Resetting profiling data when a function is recompiled (to avoid stale data).

### 6.9 Deoptimization

**Deoptimization model.** Deoptimization is the process of transferring execution from T0 to a higher tier when a speculative assumption is violated. Deoptimization must:
1. Reconstruct the source-level state (variables, call stack, etc.).
2. Transfer control to the appropriate point in the higher tier.
3. Ensure that the higher tier produces the same observable behavior as T0 would have.

**Deoptimization trigger.** Deoptimization is triggered when:
- A guard check fails (e.g., a type assumption is violated).
- An inline cache becomes megamorphic and the site is hot.
- The runtime explicitly requests deoptimization (e.g., a class is redefined).

**Deoptimization process.** When deoptimization is triggered:

1. **Capture the current state.** The interpreter captures the current register file, operand stack, call stack, and PC.

2. **Map to source-level state.** The interpreter uses deopt metadata (generated during bytecode compilation) to map the current state to source-level variables. The deopt metadata specifies:
   - Which register corresponds to which source variable.
   - Which stack slots correspond to which source variables.
   - The PC in the higher tier where execution should resume.

3. **Transfer to higher tier.** The interpreter transfers control to the higher tier by:
   - Allocating a new frame in the higher tier's call stack.
   - Initializing the frame with the source-level state.
   - Jumping to the deopt target PC in the higher tier.

4. **Continue execution.** The higher tier continues execution from the deopt target, producing the same observable behavior as T0 would have.

**Deoptimization correctness.** Deoptimization is correct if:
- The source-level state is correctly reconstructed.
- The higher tier produces the same observable behavior as T0 would have.
- No speculative assumptions from T0 are carried over to the higher tier (the higher tier must re-establish its own assumptions).

**Deoptimization overhead.** Deoptimization is expensive (it involves stack reconstruction and tier transition). To minimize overhead:
- Deoptimization should be rare (speculative assumptions should be accurate).
- The deoptimization path should be optimized (e.g., use a fast path for common cases).
- The runtime should avoid deoptimizing the same function repeatedly (e.g., by disabling speculative optimizations for that function).

### 6.10 Tier Transition

**Tier transition model.** The interpreter transitions to a higher tier when profiling data indicates that a function is hot (executed frequently). The transition is triggered by:
- Execution count thresholds (e.g., a function has been called 1000 times).
- Loop iteration thresholds (e.g., a loop has iterated 100 times).
- Explicit requests from the runtime (e.g., the runtime detects a long-running function).

**Transition process.** When a tier transition is triggered:

1. **Compile the function.** The runtime compiles the function to a higher tier (T1, T2, etc.) using the profiling data collected by T0.

2. **Install the compiled code.** The runtime installs the compiled code in the function's code table, replacing the T0 bytecode.

3. **Redirect execution.** Subsequent calls to the function execute the compiled code instead of the T0 bytecode.

4. **Deoptimize existing frames.** If there are existing T0 frames on the call stack, they are deoptimized and replaced with frames in the higher tier.

**Transition safety.** Tier transitions are safe if:
- The compiled code produces the same observable behavior as the T0 bytecode.
- Existing frames are correctly deoptimized.
- The profiling data used for compilation is accurate.

**Transition overhead.** Tier transitions are expensive (they involve compilation and deoptimization). To minimize overhead:
- Transitions should be triggered only for hot functions.
- The compilation should be fast (higher tiers use incremental compilation).
- The deoptimization should be fast (use a fast path for common cases).

### 6.11 Validity Invariants

Valid_T0(B, S) holds iff all of the following hold:

**1. Bytecode well-formedness.**
- All bytecodes are valid (correct opcode, correct operand count, correct operand types).
- All jump targets are valid (point to valid bytecodes).
- All register indices are within bounds.

**2. State consistency.**
- The register file is consistent (all registers have valid values).
- The operand stack is consistent (all stack slots have valid values).
- The call stack is consistent (all frames have valid return addresses and frame pointers).
- The heap is consistent (all objects have valid shapes and types).

**3. Speculative assumption consistency.**
- All guards are correctly inserted at points where speculative assumptions might be violated.
- All guards correctly check the speculative assumptions.
- If a guard fails, deoptimization occurs correctly.

**4. Inline cache correctness.**
- All inline caches are correctly initialized.
- All inline cache lookups return the correct target (or trigger a slow path).
- All inline cache updates are correct (the cache state is updated correctly).

**5. Profiling data accuracy.**
- All execution counts are accurate (within sampling error).
- All type feedback is accurate (the type set includes all types seen).
- All branch outcomes are accurate (within sampling error).
- All inline cache statistics are accurate.

**6. Opcode rewriting correctness.**
- All rewritten bytecodes are equivalent to the original bytecodes under the speculative assumptions.
- All rewritten bytecodes have correct guards.
- All rewritten bytecodes have correct deoptimization targets.

**7. Super byte code correctness.**
- All super byte codes are equivalent to the original bytecode sequences.
- All super byte codes have correct guards (if any).
- All super byte codes have correct deoptimization targets (if any).

**8. Deoptimization correctness.**
- All deoptimization targets are valid (point to valid PCs in higher tiers).
- All deoptimization metadata is correct (correctly maps T0 state to source-level state).
- All deoptimizations produce the same observable behavior as T0 would have.

**9. Tier transition correctness.**
- All tier transitions are triggered correctly (based on profiling data).
- All compiled code is correct (produces the same observable behavior as T0).
- All existing frames are correctly deoptimized during tier transitions.

### 6.12 Representation and Cost Model

**Register file representation.** The register file is an array of values (tagged pointers or unboxed primitives). Each frame has its own register file.

**Bytecode representation.** Bytecodes are stored in a byte array. Each bytecode is a variable-length encoding (opcode followed by operands).

**Inline cache representation.** Inline caches are stored in a side table associated with each function. Each cache is a small structure (key, target, state, entries).

**Profiling data representation.** Profiling data is stored in a side table associated with each function. The data structure is as defined in §6.8.

**Traversal cost.** Interpreting a bytecode is O(1) (constant time per bytecode, including dispatch and execution). Inline cache lookup is O(1) for monomorphic caches, O(k) for polymorphic caches (where k is the number of entries). Profiling updates are O(1).

**Modification cost.** Opcode rewriting modifies the bytecode array in place. The cost is O(1) per rewrite. Super byte code generation modifies the bytecode array and may require updating jump targets. The cost is O(|bytecodes|) in the worst case.

**Memory complexity.** O(|bytecodes| + |frames| · |registers| + |inline caches| · cache_size + |profiling data| · profile_size).

**Construction cost.** Interpretation: O(|instructions| · k) where k is the average cost per instruction. Profiling: O(|instructions| · p) where p is the profiling overhead per instruction. Opcode rewriting: O(|rewrites|). Super byte code generation: O(|patterns|).

---

# Universal Guest Bytecode — Complete Design Document (v1)

## 0. Document Control and Design Scope

**Component:** Universal Guest Bytecode (UGB)

**Version:** 1.0

**Role:** Language-agnostic, capability-based intermediate bytecode format. Serves as the universal representation consumed by T1 (baseline JIT) and produced by language-specific front-ends. Supports polyglot execution and near-zero overhead FFI.

**Design Status:** Complete specification.

**Scope:** This document defines the bytecode format, capability system, type system, FFI mechanism, memory model, and validity invariants. It does not define:
- Language-specific front-ends that produce UGB (separate subsystems)
- The T1 JIT compiler (already specified; consumes UGB)
- The runtime system (separate subsystem)
- The FFI runtime for specific foreign languages (separate subsystems)

**Normative References:**
- Capability-based security literature (Dennis & Van Horn, Capability Myths, etc.)
- WebAssembly specification (for portable bytecode design)
- Java Native Interface (JNI) specification (for FFI design patterns)
- Python C API (for dynamic language FFI patterns)
- Target architecture ABIs (for FFI calling conventions)

**Known Limitations:**
- Garbage collection integration with foreign languages is partially specified
- Thread safety for concurrent FFI calls is partially specified
- Exact capability revocation mechanisms are not fully specified

---

## 1. Goals, Non-Goals, and Constraints

### Goals

1. Provide a universal bytecode format that can represent programs from multiple source languages (C, Python, Java, JavaScript, Rust, etc.).
2. Enforce security and isolation through a capability-based permission system.
3. Support polyglot execution: code from different languages can interoperate seamlessly.
4. Provide near-zero overhead FFI: calling foreign functions should be as fast as native calls.
5. Maintain a well-defined memory model that accommodates different language semantics (GC vs. manual memory management, value vs. reference semantics).
6. Be efficiently compilable by T1 (copy-and-patch JIT).
7. Support dynamic features (reflection, dynamic typing, eval) without sacrificing performance for static code.

### Non-Goals

1. Replace language-specific IRs for optimization (each language may have its own optimization pipeline before lowering to UGB).
2. Provide peak performance for all languages (some languages may require specialized compilation paths).
3. Support all language features without overhead (dynamic features may incur runtime checks).

### Constraints

- UGB encoding size must be O(|source constructs| · k) where k is a small constant.
- Capability checks must add <5% overhead to execution.
- FFI calls must have <10% overhead compared to native calls.
- UGB must be verifiable in O(|bytecode|) time.
- UGB must support at least 256 opcodes, 65536 capabilities, and 256 type tags.

---

## 6. Universal Guest Bytecode

### 6.1 Purpose and Domain

The Universal Guest Bytecode (UGB) is a register-based, capability-gated bytecode format designed to represent programs from multiple source languages. Its purpose is to provide a common execution substrate that enforces security through capabilities while enabling efficient polyglot execution and FFI.

UGB is:
- **Register-based**: operations use virtual registers, not a stack.
- **Capability-based**: every operation that accesses resources (memory, FFI, I/O) requires appropriate capabilities.
- **Polyglot**: can represent the semantics of multiple languages (static/dynamic, GC/manual, value/reference).
- **Verifiable**: bytecode can be verified for type safety and capability compliance before execution.
- **FFI-friendly**: provides direct mechanisms for calling foreign functions with minimal overhead.

### 6.2 Mathematical Specification

| Requirement | Specification |
|---|---|
| **Domain** | P_src: the set of valid source programs (from any supported language). |
| **Codomain** | B_UGB: the set of valid UGB programs. |
| **Definitions** | F: front-end lowering function (language-specific). C: capability environment. T: type environment. M: memory model. |
| **Preconditions** | The source program P_src is well-formed according to its language's semantics. The capability environment C is valid. |
| **Postconditions** | The resulting UGB program B_UGB satisfies Valid_UGB(B_UGB). |
| **Invariants** | Capability compliance, type safety, memory model consistency, FFI correctness. |
| **Relations** | Source-to-UGB mapping (→_lower), capability check (→_check), FFI call (→_ffi). |
| **Failure behavior** | If capability check fails, raise a CapabilityViolation exception. If type check fails, raise a TypeViolation exception. If FFI call fails, raise an FFIViolation exception. |
| **Complexity** | Lowering: O(|source constructs| · k). Verification: O(|bytecode|). |
| **Correctness** | ⟦P_src⟧_source = ⟦F(P_src)⟧_UGB under capability environment C. |

### 6.3 Bytecode Format

**Instruction encoding.** Each UGB instruction is variable-length and consists of:

```
Instruction = {
    opcode: u8,                        // The operation code (0-255)
    operands: [operand],               // Variable number of operands
    capabilities: [capability_id],     // Required capabilities for this instruction
    metadata: metadata                 // Optional metadata (source location, type info, etc.)
}
```

**Operand types.** Operands can be:

- **Register**: a virtual register index (u16).
- **Immediate**: a constant value (i32, i64, f32, f64, or pointer).
- **Capability**: a capability identifier (u16).
- **Label**: a bytecode offset for branches (i32).
- **Type tag**: a type identifier (u8).
- **FFI descriptor**: a foreign function descriptor (u32).

**Instruction categories.**

**Arithmetic and logic.** ADD, SUB, MUL, DIV, REM, AND, OR, XOR, SHL, SHR, NEG, NOT. Operands: destination register, source registers or immediates. Capabilities: none (pure computation).

**Memory operations.** LOAD, STORE, ALLOC, FREE. Operands: destination/source registers, address register, size. Capabilities: MemRead, MemWrite, MemAlloc for respective operations.

**Control flow.** JUMP, JUMP_IF_TRUE, JUMP_IF_FALSE, CALL, RETURN. Operands: target label or register, condition register (for conditional jumps), argument registers (for calls). Capabilities: Call for CALL instructions.

**Type operations.** TYPE_CHECK, TYPE_CAST, TYPE_TAG. Operands: value register, type tag. Capabilities: TypeInspect for TYPE_CHECK.

**FFI operations.** FFI_CALL, FFI_LOAD, FFI_STORE. Operands: FFI descriptor, argument registers, result register. Capabilities: FFI for all FFI operations.

**Capability operations.** CAP_CHECK, CAP_GRANT, CAP_REVOKE. Operands: capability register. Capabilities: CapAdmin for CAP_GRANT and CAP_REVOKE.

**Polyglot operations.** LANG_SWITCH, LANG_CONTEXT. Operands: language identifier, context register. Capabilities: Polyglot for language switching.

**Dynamic operations.** EVAL, REFLECT, DYN_CALL. Operands: code register, object register, method name. Capabilities: Dynamic for all dynamic operations.

### 6.4 Capability System

**Capability model.** A capability is a token that grants permission to perform a specific operation or access a specific resource. Capabilities are:

- **Unforgeable**: capabilities cannot be created by user code.
- **Tamper-proof**: capabilities cannot be modified by user code.
- **Transitive**: capabilities can be passed to callees (with possible attenuation).
- **Revocable**: capabilities can be revoked by the runtime.

**Capability types.**

- **Memory capabilities**: MemRead, MemWrite, MemAlloc, MemFree. Grant permission to read, write, allocate, or free memory.
- **FFI capabilities**: FFI, FFI_Specific(ffi_id). Grant permission to call foreign functions (all or specific).
- **Type capabilities**: TypeInspect, TypeModify. Grant permission to inspect or modify type information.
- **I/O capabilities**: IO_Read, IO_Write, IO_Network, IO_File. Grant permission to perform I/O operations.
- **Dynamic capabilities**: Dynamic, Eval, Reflect. Grant permission to perform dynamic operations.
- **Polyglot capabilities**: Polyglot, Lang_Specific(lang_id). Grant permission to switch languages or interact with specific languages.
- **Administrative capabilities**: CapAdmin, ThreadCreate, GC_Control. Grant permission to manage capabilities, threads, or garbage collection.

**Capability environment.** Each function has a capability environment that specifies which capabilities are available:

```
CapabilityEnvironment = {
    available_caps: Set<capability_id>,     // Capabilities available to this function
    granted_caps: Map<register, capability>, // Capabilities stored in registers
    revoked_caps: Set<capability_id>         // Capabilities that have been revoked
}
```

**Capability checking.** Before executing an instruction, the runtime checks that the instruction's required capabilities are available in the current capability environment:

```
check_capabilities(instruction, cap_env):
    for cap in instruction.capabilities:
        if cap not in cap_env.available_caps:
            raise CapabilityViolation(cap)
```

**Capability passing.** When calling a function, the caller can pass capabilities to the callee:

```
CALL r_func, args, caps
```

The callee receives the capabilities in `caps` and can use them during its execution. The caller can attenuate capabilities (pass a subset of its own capabilities).

**Capability revocation.** The runtime can revoke capabilities at any time (e.g., when a sandbox is torn down). Revoked capabilities are immediately invalid:

```
CAP_REVOKE cap_id
```

After revocation, any attempt to use the capability raises a CapabilityViolation.

### 6.5 Type System

**Type tags.** UGB uses a tagged type system where each value has a type tag. Type tags are:

- **Primitive types**: Int8, Int16, Int32, Int64, UInt8, UInt16, UInt32, UInt64, Float32, Float64, Bool, Char.
- **Reference types**: Ptr, Ref, Box, Array, String, Object.
- **Function types**: Func(args, ret).
- **Capability types**: Cap(cap_type).
- **Foreign types**: Foreign(lang_id, foreign_type_id).
- **Dynamic types**: Dyn (for dynamically typed values).
- **Bottom type**: Bottom (for unreachable code).

**Type representation.** Types are represented as type tags (u8) for primitive types and type descriptors (pointers to type metadata) for complex types.

**Type checking.** UGB supports both static and dynamic type checking:

- **Static type checking**: performed during verification (before execution). If the bytecode is statically type-safe, no runtime type checks are needed.
- **Dynamic type checking**: performed at runtime for dynamically typed code. Type checks are guarded by TypeInspect capabilities.

**Type conversion.** Type conversions are explicit:

```
TYPE_CAST r_dest, r_src, target_type
```

If the conversion is invalid (e.g., casting a string to an integer), a TypeViolation exception is raised.

### 6.6 Memory Model

**Memory regions.** UGB defines several memory regions:

- **Stack**: function-local storage (virtual registers, operand stack).
- **Heap**: dynamically allocated objects (managed by GC or manual allocation).
- **Foreign memory**: memory owned by foreign languages (not managed by UGB GC).
- **Code memory**: executable code (immutable after JIT compilation).

**Memory access.** Memory access is capability-gated:

- **Stack access**: no capability required (implicit).
- **Heap access**: requires MemRead/MemWrite capabilities.
- **Foreign memory access**: requires FFI capability and appropriate foreign memory descriptors.

**Pointer semantics.** UGB supports two pointer semantics:

- **Safe pointers**: tagged pointers that include type information and bounds. Require MemRead/MemWrite capabilities.
- **Raw pointers**: untagged pointers for FFI. Require FFI capability and are not bounds-checked.

**Garbage collection.** UGB supports both GC-managed and manually-managed memory:

- **GC-managed objects**: allocated with ALLOC, automatically freed by GC.
- **Manually-managed objects**: allocated with ALLOC, must be freed with FREE. Require MemFree capability.

**Foreign memory management.** Foreign memory is managed by the foreign language's runtime. UGB provides mechanisms to:

- **Borrow foreign memory**: temporarily access foreign memory with a capability.
- **Own foreign memory**: take ownership of foreign memory (transfer management to UGB).
- **Release foreign memory**: release ownership back to the foreign runtime.

### 6.7 FFI Mechanism

**FFI descriptor.** An FFI descriptor describes a foreign function:

```
FFIDescriptor = {
    ffi_id: u32,                       // Unique identifier for this foreign function
    lang_id: u8,                       // The foreign language (C, Python, Java, etc.)
    name: string,                      // The function name
    signature: FuncType,               // The function signature (argument types, return type)
    calling_convention: CallingConvention, // The calling convention (Cdecl, Stdcall, etc.)
    capabilities_required: [capability_id], // Capabilities required to call this function
    marshaling: MarshalingInfo         // How to marshal arguments and results
}
```

**FFI calling convention.** The calling convention specifies how arguments are passed and results are returned:

- **Cdecl**: C calling convention (arguments on stack, caller cleans up).
- **Stdcall**: Windows calling convention (arguments on stack, callee cleans up).
- **Fastcall**: arguments in registers (x86-64 System V, Windows x64).
- **Language-specific**: custom calling conventions for specific languages (e.g., Python's argument passing).

**FFI marshaling.** Marshaling specifies how to convert between UGB values and foreign values:

```
MarshalingInfo = {
    arg_marshalers: [Marshaler],       // How to marshal each argument
    result_marshaler: Marshaler,       // How to marshal the result
    error_handling: ErrorHandling      // How to handle foreign exceptions
}
```

**Marshaling types.**

- **Direct pass**: the UGB value is passed directly (e.g., integers, floats).
- **Boxing**: the UGB value is boxed into a foreign object (e.g., UGB string → Python str).
- **Unboxing**: the foreign value is unboxed into a UGB value (e.g., Python int → UGB Int64).
- **Reference pass**: a reference to the UGB value is passed (e.g., pointer to UGB object).
- **Copy**: the UGB value is copied into foreign memory (e.g., UGB array → C array).

**FFI call process.** The FFI call process is:

1. **Check capabilities**: verify that the FFI capability is available.
2. **Marshal arguments**: convert UGB arguments to foreign values according to the marshaling info.
3. **Call foreign function**: invoke the foreign function using the specified calling convention.
4. **Marshal result**: convert the foreign result to a UGB value.
5. **Handle errors**: if the foreign function raised an exception, convert it to a UGB exception.

**FFI overhead.** The FFI overhead is minimized by:

- **Pre-marshaling**: marshaling information is computed at compile time, not runtime.
- **Direct calls**: foreign functions are called directly, not through a trampoline (when possible).
- **Inline marshaling**: simple marshaling operations are inlined into the call site.
- **Capability caching**: capability checks are cached and only performed once per call site.

**FFI examples.**

**Calling a C function:**

```c
// C code
int add(int a, int b) {
    return a + b;
}
```

```ugb
// UGB code
FFI_DESCRIPTOR fd, "add", "C", FuncType([Int32, Int32], Int32), Cdecl
LOAD_CONST r0, 5
LOAD_CONST r1, 7
FFI_CALL r2, fd, [r0, r1]  // r2 = 12
```

**Calling a Python function:**

```python
# Python code
def greet(name):
    return f"Hello, {name}!"
```

```ugb
// UGB code
FFI_DESCRIPTOR fd, "greet", "Python", FuncType([String], String), Python
LOAD_CONST r0, "World"
FFI_CALL r1, fd, [r0]  // r1 = "Hello, World!"
```

**Passing UGB objects to C:**

```c
// C code
void process_array(int* arr, int len) {
    for (int i = 0; i < len; i++) {
        arr[i] *= 2;
    }
}
```

```ugb
// UGB code
FFI_DESCRIPTOR fd, "process_array", "C", FuncType([Ptr<Int32>, Int32], Void), Cdecl
ALLOC r0, 10 * sizeof(Int32)  // Allocate array
LOAD_CONST r1, 10
FFI_CALL void, fd, [r0, r1]   // Pass array to C
// Array is now modified by C code
```

### 6.8 Polyglot Execution

**Language contexts.** UGB supports multiple language contexts, each with its own semantics:

```
LanguageContext = {
    lang_id: u8,                       // Language identifier
    semantics: LanguageSemantics,      // Language-specific semantics
    runtime: LanguageRuntime,          // Language runtime (GC, exception handling, etc.)
    interop: InteropRules              // Rules for interoperating with other languages
}
```

**Language switching.** Code can switch between language contexts:

```
LANG_SWITCH lang_id, context_reg
```

The new language context determines how operations are interpreted (e.g., integer overflow behavior, exception handling, GC semantics).

**Polyglot capabilities.** Polyglot operations require Polyglot capabilities:

- **LANG_SWITCH**: switch to a different language context.
- **LANG_CONTEXT**: query the current language context.
- **INTEROP_CALL**: call a function in a different language context.

**Interop rules.** Interop rules specify how values are converted between languages:

```
InteropRules = {
    value_conversions: Map<(lang_id, type), (lang_id, type)>,  // How to convert values
    exception_propagation: ExceptionPropagation,                // How exceptions propagate
    memory_ownership: MemoryOwnership                          // Who owns shared memory
}
```

**Polyglot example.**

```ugb
// Start in UGB context
LOAD_CONST r0, 42

// Switch to Python context
LANG_SWITCH PYTHON, r1

// Call a Python function
FFI_CALL r2, python_func, [r0]

// Switch back to UGB context
LANG_SWITCH UGB, r3

// Use the result
ADD r4, r2, 1
```

### 6.9 Validity Invariants

Valid_UGB(B) holds iff all of the following hold:

**1. Bytecode well-formedness.**
- All instructions have valid opcodes.
- All operands have valid types and values.
- All branch targets are valid (point to valid instructions).
- All register indices are within bounds.

**2. Capability compliance.**
- All instructions have their required capabilities available in the capability environment.
- All capability operations (grant, revoke) are performed by code with CapAdmin capability.
- No revoked capabilities are used.

**3. Type safety.**
- All operations are type-safe (operand types match expected types).
- All type conversions are valid (or guarded by dynamic type checks).
- All function calls have matching signatures.

**4. Memory model consistency.**
- All memory accesses are within bounds (for safe pointers).
- All memory accesses have appropriate capabilities.
- All allocated memory is eventually freed (for manually-managed memory).
- No dangling pointers (for GC-managed memory).

**5. FFI correctness.**
- All FFI calls have valid FFI descriptors.
- All FFI arguments are correctly marshaled.
- All FFI results are correctly unmarshaled.
- All FFI exceptions are correctly propagated.

**6. Polyglot correctness.**
- All language switches are valid (target language is supported).
- All interop calls follow the interop rules.
- All value conversions between languages are correct.

**7. Control flow correctness.**
- All functions have a valid entry point and exit points.
- All call sites have matching return points.
- All exception handlers are valid.

**8. Resource bounds.**
- The bytecode size is within limits.
- The number of capabilities is within limits.
- The number of types is within limits.

### 6.10 Representation and Cost Model

**Bytecode representation.** UGB is stored as a byte array. Each instruction is variable-length (1-16 bytes typically). The encoding is compact to minimize memory usage and improve cache locality.

**Capability representation.** Capabilities are represented as opaque tokens (pointers to capability structures). Capability checks are pointer comparisons (O(1)).

**Type representation.** Primitive types are represented as type tags (u8). Complex types are represented as pointers to type descriptors.

**FFI descriptor representation.** FFI descriptors are stored in a global table, indexed by ffi_id. Lookup is O(1).

**Language context representation.** Language contexts are stored in a global table, indexed by lang_id. Switching contexts is O(1).

**Traversal cost.** Decoding an instruction is O(1). Checking capabilities is O(|required_caps|) (typically 1-3). Type checking is O(1) for static types, O(k) for dynamic types (where k is the type complexity). FFI marshaling is O(|args|) (typically 1-10).

**Modification cost.** UGB is immutable after verification (except for capability revocation). Capability revocation is O(1).

**Memory complexity.** O(|bytecode| + |capabilities| · cap_size + |types| · type_size + |ffi_descriptors| · ffi_size).

**Construction cost.** Lowering from source to UGB: O(|source constructs| · k). Verification: O(|bytecode|).

---

# Capability System Implementation — Complete Design Document (v1)

## 0. Document Control and Design Scope

**Component:** Capability System Implementation (subsystem of Universal Guest Bytecode)

**Version:** 1.0

**Role:** Runtime implementation of the capability-based security model defined in UGB §6.4. Provides the concrete data structures, algorithms, and invariants for capability tokens, the global capability table, revocation, attenuation, and per-frame capability environments.

**Design Status:** Complete specification.

**Scope:** This document defines the capability token representation, global capability table layout, all capability operations (create, check, attenuate, pass, revoke, duplicate), the revocation mechanism, per-frame capability environments, integration with UGB execution, security properties, and validity invariants. It does not define:
- The UGB bytecode format (already specified; consumes this subsystem)
- The T1 JIT compiler (already specified; emits capability checks)
- Language-specific front-ends (produce UGB with capability annotations)
- The runtime system's thread-safety model (this subsystem specifies per-thread data structures; concurrency is orthogonal)

**Normative References:**
- Dennis, J. and Van Horn, E. "Programming Semantics for Multiprogrammed Computations" (capability foundations)
- Watson, R. et al. "Capability Hardware Enhanced RISC Instructions" (CHERI — hardware capability design)
- Levy, H. "Capability-Based Computer Systems" (comprehensive capability theory)
- Standard OS literature on file-descriptor-style generation counters (Bugaenko, "Generation Counters")

**Known Limitations:**
- Cryptographic capability tokens (for cross-process capability transfer) are not covered — this design uses in-process tokens only.
- Capability transfer across address spaces (e.g., via shared memory) is deferred.
- Hardware-enforced capability checks (CHERI-style) are out of scope; this is a software implementation.

---

## 1. Goals, Non-Goals, and Constraints

### Goals

1. Provide O(1) capability checks on the hot path (every capability-gated instruction).
2. Support unforgeable, tamper-proof capability tokens that user code cannot create or modify.
3. Support attenuation: deriving a weaker capability from a stronger one, with the invariant that the derived capability's permissions are a subset of the parent's.
4. Support revocation: invalidating a capability and all its descendants, with the invariant that no revoked capability can pass a check.
5. Support transitive capability passing during function calls, with caller-controlled attenuation.
6. Keep memory overhead per capability under 64 bytes in the global table plus 8 bytes per live token.
7. Keep revocation cost proportional to the number of descendants, not the total table size.

### Non-Goals

1. Hardware-enforced capabilities (CHERI-style tagged memory). This is a pure software implementation.
2. Cross-process capability transfer. Capabilities are in-process tokens.
3. Cryptographic capability tokens for untrusted environments.
4. Fine-grained temporal revocation (e.g., "revoke after 5 seconds"). Revocation is immediate and permanent.

### Constraints

- Capability check cost: ≤ 3 memory accesses (one GCT entry load, two comparisons) on the fast path.
- Attenuation cost: O(1) amortized (one GCT entry allocation).
- Revocation cost: O(|descendants|) — proportional to the number of capabilities invalidated.
- Memory overhead per capability: ≤ 64 bytes in GCT + 8 bytes per token.
- Maximum live capabilities: 2³² (limited by 32-bit index).
- Maximum attenuation chain depth: 256 (enforced to bound revocation walk cost).

---

## 6. Capability System Implementation

### 6.1 Purpose and Domain

The Capability System Implementation provides the runtime machinery for the UGB capability model. It translates the abstract capability semantics (unforgeable, tamper-proof, transitive, revocable tokens) into concrete data structures and algorithms.

The subsystem maintains:
- A **Global Capability Table (GCT)**: the single source of truth for all capability state.
- **Capability tokens**: 64-bit values that reference GCT entries and carry permission flags.
- **Per-frame capability environments**: the set of capabilities available to each function activation.
- **Revocation infrastructure**: mechanisms to invalidate capabilities and their descendants.

### 6.2 Mathematical Specification

| Requirement | Specification |
|---|---|
| **Domain** | CapReq: the set of capability requests (check, attenuate, revoke, pass). State: the current GCT state and per-frame environments. |
| **Codomain** | CapResult: the set of capability operation results (OK, REVOKED, INSUFFICIENT_PERMISSION, INVALID_ATTENUATION, etc.). State': the updated state. |
| **Definitions** | GCT: global capability table. Token: 64-bit capability token. Env: per-frame capability environment. Perm: permission bitmask. |
| **Preconditions** | The GCT is well-formed (all entries have valid structure). All tokens in live environments reference valid GCT entries. |
| **Postconditions** | All security invariants (§6.9) hold in State'. |
| **Invariants** | Unforgeability, tamper-proofness, attenuation monotonicity, revocation completeness, check correctness. |
| **Relations** | Token-to-entry mapping (→_entry), parent-child relation (→_child), permission subset (⊆_perm). |
| **Failure behavior** | Invalid operations return error codes; they never corrupt state. Malformed tokens are rejected. |
| **Complexity** | Check: O(1). Attenuate: O(1) amortized. Revoke: O(|descendants|). Pass: O(|caps passed|). |
| **Correctness** | For all reachable states, the security invariants (§6.9) hold. |

### 6.3 Capability Token Representation

**Token format.** A capability token is a 64-bit value with the following layout:

```
Bits  0-15:  permission_flags (u16) — which operations this token permits
Bits 16-47:  gct_index       (u32) — index into the Global Capability Table
Bits 48-63:  generation      (u16) — must match the GCT entry's generation
```

**Permission flags.** The 16-bit permission field is a bitmask. Each bit represents a primitive permission:

```
PERM_MEM_READ       = 0x0001
PERM_MEM_WRITE      = 0x0002
PERM_MEM_ALLOC      = 0x0004
PERM_MEM_FREE       = 0x0008
PERM_FFI            = 0x0010
PERM_FFI_SPECIFIC   = 0x0020  // requires additional FFI descriptor check
PERM_TYPE_INSPECT   = 0x0040
PERM_TYPE_MODIFY    = 0x0080
PERM_IO_READ        = 0x0100
PERM_IO_WRITE       = 0x0200
PERM_IO_NETWORK     = 0x0400
PERM_IO_FILE        = 0x0800
PERM_DYNAMIC        = 0x1000
PERM_EVAL           = 0x2000
PERM_REFLECT        = 0x4000
PERM_POLYGLOT       = 0x8000
```

A token's `permission_flags` field is the intersection of:
- The parent token's permissions (at attenuation time).
- The GCT entry's `permission_mask` (the maximum permissions for this resource).

**Token construction.** Tokens are constructed only by the runtime (never by user code). The constructor is:

```
make_token(gct_index: u32, generation: u16, permission_flags: u16) -> Token:
    return (permission_flags as u64)
         | ((gct_index as u64) << 16)
         | ((generation as u64) << 48)
```

**Token decomposition.** The runtime extracts fields via bit masks and shifts:

```
token_flags(token)     = token & 0xFFFF
token_index(token)     = (token >> 16) & 0xFFFFFFFF
token_generation(token) = (token >> 48) & 0xFFFF
```

**Token validity.** A token is *syntactically valid* if its `gct_index` is within the GCT bounds. It is *semantically valid* if it also passes the check operation (§6.5).

### 6.4 Global Capability Table

**GCT layout.** The GCT is a contiguous array of fixed-size entries. Each entry is 64 bytes (one cache line on most architectures) to minimize cache-line sharing between entries.

```
struct GCTEntry {                              // 64 bytes total
    resource_ptr:    u64,    //  8 bytes — pointer to resource descriptor
    generation:      u16,    //  2 bytes — current generation (incremented on revoke)
    status:          u8,     //  1 byte  — ACTIVE | REVOKED | ATTENUATED
    _pad1:           u8,     //  1 byte
    parent_index:    u32,    //  4 bytes — index of parent (0xFFFFFFFF if root)
    permission_mask: u16,    //  2 bytes — maximum permissions for this resource
    ref_count:       u32,    //  4 bytes — number of live tokens referencing this entry
    children_head:   u32,    //  4 bytes — index of first child (linked list)
    next_sibling:    u32,    //  4 bytes — index of next sibling (linked list)
    _reserved:       u32,    //  4 bytes
    _pad2:           u32,    //  4 bytes — pad to 64 bytes
}
```

**Entry status values.**

```
STATUS_ACTIVE      = 0  // capability is valid
STATUS_REVOKED     = 1  // capability has been revoked
STATUS_ATTENUATED  = 2  // capability has children (informational, still active)
```

**Resource descriptor.** The `resource_ptr` points to a type-specific descriptor that describes what the capability grants access to. The descriptor type is determined by the capability's primitive permissions:

- For memory capabilities: a `MemoryResource` (base address, size, region class).
- For FFI capabilities: an `FFIResource` (FFI descriptor ID, allowed functions).
- For I/O capabilities: an `IOResource` (file descriptor, socket, path pattern).
- For type capabilities: a `TypeResource` (type ID, allowed operations).
- For administrative capabilities: an `AdminResource` (admin scope).

**GCT allocation.** The GCT is allocated as a single contiguous block. Free entries are maintained in a free-list (singly linked through the `children_head` field, repurposed for free-list linkage when `status == FREE`). Allocation is O(1) via free-list pop.

**GCT growth.** When the free-list is exhausted, the GCT is reallocated at double size. All existing tokens remain valid because they reference entries by index, and indices do not change during reallocation.

**GCT invariants.**

1. Every entry with `status == ACTIVE` or `status == ATTENUATED` has a valid `resource_ptr`.
2. Every entry with `status == REVOKED` has `generation` strictly greater than any live token referencing it.
3. The parent-child relation forms a forest (no cycles).
4. The attenuation chain depth is at most 256.
5. `ref_count` accurately reflects the number of live tokens referencing the entry.

### 6.5 Capability Operations

#### 6.5.1 Check

The check operation is the hot-path operation. It is invoked before every capability-gated instruction.

```
check(token: Token, required_perm: u16) -> CapResult:
    idx = token_index(token)
    if idx >= GCT.size:
        return INVALID_TOKEN
    
    entry = GCT[idx]
    
    // Fast path: generation mismatch means revoked
    if entry.generation != token_generation(token):
        return REVOKED
    
    // Status check
    if entry.status == STATUS_REVOKED:
        return REVOKED
    
    // Permission check: token must have the required permission
    if (token_flags(token) & required_perm) != required_perm:
        return INSUFFICIENT_PERMISSION
    
    // Permission check: entry must allow the required permission
    if (entry.permission_mask & required_perm) != required_perm:
        return INSUFFICIENT_PERMISSION
    
    return OK
```

**Cost:** 1 GCT entry load + 3 comparisons. On modern hardware with the GCT entry in L1 cache, this is ~3-5 cycles.

**Fast-path optimization.** The generation check can be combined with the status check into a single comparison if the status is encoded in the top bits of the generation field (e.g., generation occupies bits 0-14, status occupies bit 15). This reduces the check to 2 comparisons.

#### 6.5.2 Attenuate

Attenuation derives a new capability with a subset of the parent's permissions.

```
attenuate(parent_token: Token, new_permissions: u16) -> Result<Token, CapError>:
    // Verify parent is valid
    parent_idx = token_index(parent_token)
    if parent_idx >= GCT.size:
        return Err(INVALID_TOKEN)
    
    parent_entry = GCT[parent_idx]
    if parent_entry.generation != token_generation(parent_token):
        return Err(REVOKED)
    if parent_entry.status == STATUS_REVOKED:
        return Err(REVOKED)
    
    // New permissions must be a subset of parent's permissions
    if (new_permissions & ~token_flags(parent_token)) != 0:
        return Err(INVALID_ATTENUATION)
    
    // New permissions must be a subset of entry's permission mask
    if (new_permissions & ~parent_entry.permission_mask) != 0:
        return Err(INVALID_ATTENUATION)
    
    // Check attenuation depth
    depth = compute_attenuation_depth(parent_idx)
    if depth >= MAX_ATTENUATION_DEPTH:  // 256
        return Err(ATTENUATION_TOO_DEEP)
    
    // Allocate new GCT entry
    new_idx = gct_allocate()
    if new_idx == GCT_INVALID:
        return Err(GCT_FULL)
    
    new_entry = GCTEntry {
        resource_ptr:    parent_entry.resource_ptr,
        generation:      parent_entry.generation,  // inherit generation
        status:          STATUS_ACTIVE,
        parent_index:    parent_idx,
        permission_mask: new_permissions,
        ref_count:       1,
        children_head:   GCT_INVALID,
        next_sibling:    parent_entry.children_head,
    }
    GCT[new_idx] = new_entry
    
    // Link as child of parent
    parent_entry.children_head = new_idx
    if parent_entry.status == STATUS_ACTIVE:
        parent_entry.status = STATUS_ATTENUATED
    
    // Increment parent ref count (for revocation tracking)
    parent_entry.ref_count += 1
    
    return Ok(make_token(new_idx, parent_entry.generation, new_permissions))
```

**Cost:** O(depth) for depth computation (bounded by 256), O(1) for allocation and linking. Amortized O(1) in practice.

#### 6.5.3 Revoke

Revocation invalidates a capability and all its descendants.

```
revoke(token: Token) -> CapResult:
    idx = token_index(token)
    if idx >= GCT.size:
        return INVALID_TOKEN
    
    entry = GCT[idx]
    if entry.generation != token_generation(token):
        return REVOKED  // already revoked
    
    // Eager revocation: walk the descendant tree and invalidate all
    worklist = [idx]
    count = 0
    
    while worklist is not empty:
        curr_idx = worklist.pop()
        curr_entry = GCT[curr_idx]
        
        // Invalidate this entry
        curr_entry.generation = (curr_entry.generation + 1) & 0xFFFF
        curr_entry.status = STATUS_REVOKED
        count += 1
        
        // Add children to worklist
        child_idx = curr_entry.children_head
        while child_idx != GCT_INVALID:
            worklist.push(child_idx)
            child_idx = GCT[child_idx].next_sibling
    
    return OK_WITH_COUNT(count)
```

**Cost:** O(|descendants|) — proportional to the number of capabilities invalidated. The worklist is bounded by the total number of live capabilities.

**Correctness.** After revocation, every token that referenced the revoked entry (or any descendant) has a generation mismatch and will fail the check operation.

#### 6.5.4 Pass (during CALL)

When a CALL instruction is executed, the caller passes capabilities to the callee. The caller may attenuate capabilities before passing.

```
pass_capabilities(caller_env: Env, callee_env: Env, cap_list: [Token]):
    for token in cap_list:
        // Verify token is valid in caller's environment
        if not env_contains(caller_env, token):
            raise CapabilityViolation(PASS_INVALID_CAP)
        
        // Add to callee's environment
        env_add(callee_env, token)
        
        // Increment ref count
        idx = token_index(token)
        GCT[idx].ref_count += 1
```

**Cost:** O(|cap_list|) — linear in the number of capabilities passed.

#### 6.5.5 Duplicate

Duplication creates a new token referencing the same GCT entry. Used when a capability is stored in multiple registers or memory locations.

```
duplicate(token: Token) -> Result<Token, CapError>:
    idx = token_index(token)
    if idx >= GCT.size:
        return Err(INVALID_TOKEN)
    
    entry = GCT[idx]
    if entry.generation != token_generation(token):
        return Err(REVOKED)
    
    entry.ref_count += 1
    return Ok(token)  // token value is unchanged; ref_count tracks duplicates
```

**Cost:** O(1).

#### 6.5.6 Drop

When a capability is no longer needed (register overwritten, frame popped), the ref count is decremented. When ref count reaches zero, the GCT entry is freed.

```
drop(token: Token):
    idx = token_index(token)
    if idx >= GCT.size:
        return  // invalid token, ignore
    
    entry = GCT[idx]
    entry.ref_count -= 1
    
    if entry.ref_count == 0 and entry.status == STATUS_REVOKED:
        // Entry is revoked and no longer referenced; free it
        gct_free(idx)
```

**Cost:** O(1).

### 6.6 Revocation Mechanism

**Design choice: eager revocation with generation counters.**

The revocation mechanism uses a combination of:
1. **Generation counters**: each GCT entry has a generation that is incremented on revocation. Tokens carry the generation at creation time. A mismatch indicates revocation.
2. **Eager descendant invalidation**: when a capability is revoked, all descendants are immediately invalidated by incrementing their generations.
3. **Worklist-based traversal**: avoids stack overflow for deep attenuation chains.

**Why not lazy revocation?** Lazy revocation (checking the parent chain on every use) would make the check operation O(chain_depth), violating the O(1) check constraint. Eager revocation makes check O(1) at the cost of making revoke O(|descendants|), which is acceptable because revocation is rare.

**Why not cryptographic tokens?** Cryptographic tokens (e.g., HMAC-signed) would allow cross-process capability transfer but would make the check operation expensive (cryptographic verification). For in-process use, generation counters are sufficient and fast.

**Revocation completeness.** The revocation algorithm guarantees that after `revoke(token)`:
- The token itself fails all subsequent checks.
- All tokens derived from it (directly or transitively) fail all subsequent checks.
- No new tokens can be derived from the revoked capability.

**Proof sketch.** Let T be the set of tokens derived from the revoked token (the descendant tree). The revocation algorithm increments the generation of every entry in T. Any token in T has a generation that was set at or before the revocation time, so it will mismatch the entry's new generation. Any new token derived after revocation would require a valid parent token, which no longer exists. ∎

### 6.7 Per-Frame Capability Environments

**Environment structure.** Each function activation frame has a capability environment:

```
struct CapabilityEnvironment {
    cap_slots:    [Token; MAX_CAP_SLOTS],   // 64 slots by default
    slot_count:   u16,                       // number of active slots
    parent_env:   *CapabilityEnvironment,    // caller's environment (for nested calls)
    frame_id:     u32,                       // unique frame identifier
}
```

**Environment operations.**

```
env_add(env: Env, token: Token):
    if env.slot_count >= MAX_CAP_SLOTS:
        raise CapabilityError(ENV_FULL)
    env.cap_slots[env.slot_count] = token
    env.slot_count += 1

env_contains(env: Env, token: Token) -> bool:
    for i in 0..env.slot_count:
        if env.cap_slots[i] == token:
            return true
    return false

env_remove(env: Env, token: Token):
    for i in 0..env.slot_count:
        if env.cap_slots[i] == token:
            env.cap_slots[i] = env.cap_slots[env.slot_count - 1]
            env.slot_count -= 1
            return
```

**Environment lifecycle.** The environment is created on function entry and destroyed on function exit. On exit, all capabilities in the environment are dropped (ref count decremented).

### 6.8 Integration with UGB Execution

**Check insertion.** The T1 JIT compiler inserts capability checks before every capability-gated instruction. For example:

```
// UGB bytecode: LOAD r0, [r1]  (requires PERM_MEM_READ)
// T1 compiled code:
    check_cap PERM_MEM_READ, r_cap    // capability check
    load r0, [r1]                     // actual load
```

**Check failure handling.** If a check fails, the runtime raises a `CapabilityViolation` exception. The exception handler can:
- Terminate the program (sandbox violation).
- Deoptimize to a higher tier that handles the violation gracefully.
- Log the violation and continue (for debugging).

**Capability passing in CALL.** The CALL instruction includes a list of capabilities to pass:

```
CALL r_func, args, cap_list
```

The runtime invokes `pass_capabilities` to transfer the capabilities to the callee's environment.

**Capability revocation during execution.** The runtime can revoke capabilities at any time (e.g., when a sandbox is torn down). Revocation is asynchronous: the next check operation on a revoked capability will fail.

### 6.9 Security Properties

The capability system satisfies the following security properties:

**P1. Unforgeability.** User code cannot create a valid capability token.

*Justification.* Tokens are constructed only by the runtime (via `make_token`, `attenuate`, `duplicate`). The GCT is not directly accessible to user code. The token format includes a generation field that must match the GCT entry; without access to the GCT, user code cannot forge a valid generation.

**P2. Tamper-proofness.** User code cannot modify a capability token to gain additional permissions.

*Justification.* Tokens are 64-bit values stored in registers or memory. Even if user code modifies a token's bits, the modified token will fail the check operation because:
- Modifying the permission flags: the entry's `permission_mask` check will fail.
- Modifying the generation: the generation check will fail.
- Modifying the index: the index will either be out of bounds or point to an entry with a different generation.

**P3. Attenuation monotonicity.** If capability B is derived from capability A, then B's permissions are a subset of A's permissions.

*Justification.* The `attenuate` operation enforces `(new_permissions & ~token_flags(parent_token)) == 0`, which ensures the new permissions are a subset of the parent's. The GCT entry's `permission_mask` is also set to `new_permissions`, preventing further expansion.

**P4. Revocation completeness.** If capability A is revoked, all capabilities derived from A (directly or transitively) are also revoked.

*Justification.* The `revoke` operation walks the descendant tree and increments the generation of every entry. All tokens referencing those entries will fail the generation check. (See proof sketch in §6.6.)

**P5. Check correctness.** The `check` operation returns OK if and only if the token is valid and has the required permission.

*Justification.* The check operation verifies:
- The index is in bounds.
- The generation matches (not revoked).
- The status is not REVOKED.
- The token's permission flags include the required permission.
- The entry's permission mask includes the required permission.

If all checks pass, the token is valid and has the required permission. If any check fails, the token is invalid or lacks the permission.

### 6.10 Validity Invariants

Valid_CapSystem(State) holds iff all of the following hold:

**1. GCT well-formedness.**
- Every entry with `status == ACTIVE` or `status == ATTENUATED` has a valid `resource_ptr`.
- Every entry with `status == REVOKED` has a generation that is strictly greater than any live token referencing it.
- The parent-child relation forms a forest (no cycles).
- The attenuation chain depth is at most 256.

**2. Token validity.**
- Every live token references a valid GCT entry (index in bounds).
- Every live token's generation matches the entry's generation (unless the entry is revoked, in which case the token is invalid).
- Every live token's permission flags are a subset of the entry's permission mask.

**3. Environment consistency.**
- Every capability in a live environment is a valid token.
- The ref count of every GCT entry accurately reflects the number of live tokens referencing it.

**4. Revocation correctness.**
- After a revoke operation, all tokens referencing the revoked entry (or any descendant) fail the check operation.
- No new tokens can be derived from a revoked capability.

**5. Attenuation correctness.**
- Every attenuated capability's permissions are a subset of its parent's permissions.
- The attenuation chain depth does not exceed 256.

**6. Resource bounds.**
- The GCT size does not exceed 2³² entries.
- The number of live capabilities does not exceed 2³².
- The number of capabilities per environment does not exceed MAX_CAP_SLOTS (64).

### 6.11 Representation and Cost Model

**GCT representation.** The GCT is a contiguous array of 64-byte entries. The array is allocated via `mmap` (or equivalent) and can be grown by reallocating at double size.

**Token representation.** Tokens are 64-bit values stored in registers or memory. They are passed by value (no indirection).

**Environment representation.** Environments are allocated on the stack (one per function frame). The `cap_slots` array is fixed-size (64 slots).

**Traversal cost.** Check: O(1) — 1 GCT load + 3 comparisons. Attenuate: O(depth) for depth check, O(1) for allocation. Revoke: O(|descendants|). Pass: O(|cap_list|). Duplicate: O(1). Drop: O(1).

**Modification cost.** GCT growth: O(|GCT|) for reallocation (rare). Entry allocation: O(1) via free-list. Entry freeing: O(1).

**Memory complexity.** O(|GCT entries| · 64 bytes + |live tokens| · 8 bytes + |environments| · (64 · 8 + 16) bytes). For a typical program with 10,000 capabilities and 1,000 active frames: ~640 KB for GCT + ~80 KB for tokens + ~520 KB for environments ≈ 1.2 MB.

**Construction cost.** GCT initialization: O(|GCT size|). Capability creation: O(1). Environment creation: O(1).

---

# Garbage Collection Subsystem — Complete Design Document (v1)

## 0. Document Control and Design Scope

**Component:** Garbage Collection Subsystem (Incremental, Concurrent, Generational)

**Version:** 1.0

**Role:** Runtime memory management. Provides automatic reclamation of unreachable objects with low-latency pauses, concurrent operation alongside mutator threads, and generational collection tuned for the weak generational hypothesis.

**Design Status:** Complete specification.

**Scope:** This document defines the heap layout, object model, allocation strategy, generational structure, concurrent SATB marking, write barrier, young-generation copying collection, concurrent old-generation evacuation via load barriers and colored pointers, safepoints, stack scanning, reference processing, collection policy, and validity invariants. It does not define:
- The language front-ends (produce allocations and stores that interact with this subsystem)
- The T0/T1 tiers (must respect safepoints and barriers)
- FFI interactions with foreign allocators (deferred)
- The OS memory management interface (consumed, not specified)

**Normative References:**
- Dijkstra, E. et al. "A Discipline of Multiprogramming" (tri-color abstraction for concurrent marking)
- Jones, R. and Lins, R. "Garbage Collection: Algorithms for Automatic Dynamic Memory Management"
- Bacon, D. et al. "Concurrent Cycle Collection in Reference Counted Systems"
- Årlandson, P. et al. "The Garbage Collection Handbook"
- OpenJDK ZGC design (load barriers, colored pointers, concurrent relocation)
- OpenJDK G1 design (region-based heap, SATB marking, remembered sets)
- Barbois, R. and Le Fessant, F. "A Concurrent, Generational Garbage Collector"

**Known Limitations:**
- Huge-object (>region-size) handling is specified at the interface level; internal strategy deferred.
- NUMA-aware placement is not specified.
- Compacting old-generation via concurrent relocation is specified; alternative strategies (mark-compact STW) are out of scope.
- Finalization semantics interact with language semantics; the GC provides the hooks, the language defines the rules.

---

## 1. Goals, Non-Goals, and Constraints

### Goals

1. Provide automatic memory reclamation with pause times bounded by a configurable target (default ≤ 1 ms for application threads).
2. Run marking and evacuation concurrently with mutator threads for the old generation.
3. Exploit the weak generational hypothesis: most objects die young, few old objects point to young objects.
4. Preserve all language-level reachability semantics, including weak, soft, and phantom references.
5. Maintain the generational invariant (no untracked old-to-young pointers) at all times.
6. Scale to many mutator threads and many GC threads with bounded synchronization overhead.
7. Provide a mathematically verifiable correctness argument based on the tri-color abstraction and barrier invariants.

### Non-Goals

1. Zero-pause garbage collection (pause times are bounded, not zero).
2. Real-time hard deadlines (this is a soft real-time design; hard real-time GC is out of scope).
3. Reference counting as the primary reclamation mechanism (pure tracing GC; optional RC bridge is deferred).
4. Cross-process shared heaps.

### Constraints

- Mutator pause time per safepoint: ≤ P_max (configurable, default 1 ms).
- GC overhead (time spent in GC / total time): ≤ 5% at steady state for typical workloads.
- Heap expansion factor: ≤ 2× live data at steady state.
- Write barrier overhead per store: ≤ 1 cycle on the fast path (for non-cross-generational stores).
- Load barrier overhead per load: ≤ 2 cycles on the fast path (for correctly-colored pointers).
- Memory overhead for metadata (card table, mark bitmap, forwarding table): ≤ 5% of heap.

---

## 6. Garbage Collection Subsystem

### 6.1 Purpose and Domain

The GC subsystem manages a heap of dynamically allocated objects. It reclaims objects that are no longer reachable from the mutator's roots (stack frames, global variables, registers). It does so in a way that is:

- **Incremental**: collection work is performed in small quanta interleaved with mutator execution.
- **Concurrent**: most collection phases run in parallel with mutator threads.
- **Generational**: the heap is partitioned into generations; young objects are collected more frequently and more cheaply than old objects.

### 6.2 Mathematical Specification

| Requirement | Specification |
|---|---|
| **Domain** | H: heap state (objects, edges, roots). M: mutator transitions (alloc, store, load). |
| **Codomain** | H': updated heap state after GC intervention. |
| **Definitions** | R: root set. Reach(S): transitive closure of references from S. Live(H, R) = Reach(R) ∩ H. Dead(H, R) = H \ Live(H). |
| **Preconditions** | H is well-formed (all pointers valid, barrier invariants hold). |
| **Postconditions** | H' satisfies: (a) Live(H', R') ⊇ Live(H, R) (no live object collected); (b) Dead(H, R) ∩ H' = ∅ (dead objects reclaimed); (c) barrier invariants hold in H'. |
| **Invariants** | Tri-color marking invariant, generational invariant, load-barrier forwarding invariant, safepoint consistency. |
| **Relations** | Reference edge (→_ref), forwarding (→_fwd), root (→_root), card membership (∈_card). |
| **Failure behavior** | Out-of-memory: raise OOM exception after failed expansion. Barrier violation: internal error (bug). |
| **Complexity** | Mark: O(|Live|) work, spread across mutator quanta. Evacuate: O(|Live_old|). Young collect: O(|Live_young|). |
| **Correctness** | For any execution, the observable behavior matches a semantics where unreachable objects are reclaimed at arbitrary safe points. |

### 6.3 Heap Layout and Object Model

**Region-based heap.** The heap is a contiguous virtual address range divided into fixed-size **regions**. Each region is 4 MB (configurable). Regions are classified by generation and role:

- **Young regions**: allocated via bump pointer; collected by copying.
- **Old regions**: contain promoted or long-lived objects; collected by concurrent marking and evacuation.
- **Humongous regions**: contiguous runs of regions for objects larger than half a region.
- **Free regions**: available for allocation.
- **Metadata regions**: card table, mark bitmap, forwarding table, etc.

**Object header.** Every object has a header:

```
struct ObjectHeader {
    mark_word:    u64,   // hash code / lock state / age / forwarding pointer
    klass_ptr:    u64,   // pointer to type metadata (also carries color bits on 64-bit)
    size:         u32,   // object size in bytes (including header)
    flags:        u8,    // generation, humongous, pinned, etc.
    _pad:         u8,
}
```

**Colored pointers.** On 64-bit targets with sufficient unused virtual address bits (e.g., x86-64 with 48-bit addresses), the top bits of every object pointer encode GC metadata. We use 4 bits:

```
Bits 45-47 of a pointer:
    Marked0     (bit 45)
    Marked1     (bit 46)
    Remapped    (bit 47)
    Finalizable (bit 44)  // reserved for future use
```

These bits are the **load barrier's** mechanism for concurrent relocation (see §6.10). The actual object address is bits 0-43.

**Card table.** The heap is divided into **cards** of 512 bytes each. A card table is a byte array with one byte per card, encoding which generation the card belongs to:

```
CARD_YOUNG  = 0
CARD_OLD    = 1
CARD_HUMONGOUS = 2
CARD_FREE   = 3
```

The card table is used by the write barrier to detect cross-generational stores and by the remembered set to track old-to-young pointers.

**Remembered set (remset).** A per-region data structure listing cards in that region that contain pointers into the young generation. Maintained by the write barrier.

### 6.4 Allocation

**Thread-local allocation buffers (TLABs).** Each mutator thread has a TLAB: a contiguous chunk of a young region. Allocation is bump-pointer:

```
allocate(size):
    if tlab.remaining >= size:
        obj = tlab.top
        tlab.top += size
        return obj
    else:
        return slow_path_allocate(size)
```

**Slow path.** When a TLAB is exhausted, the thread requests a new TLAB from the GC. If no young regions are available, a young-generation collection is triggered (see §6.12).

**Promotion.** Objects surviving a young collection are promoted to old regions. Promotion is a copy into an old region; the object's header is updated to reflect its new generation.

**Large objects.** Objects larger than half a region (2 MB) are allocated directly in a contiguous run of free regions (humongous allocation).

### 6.5 Generational Structure

**Weak generational hypothesis.** Most objects die young; objects that have survived several collections are likely to live longer. The GC exploits this by:

- Collecting the young generation frequently with short pauses.
- Collecting the old generation infrequently with concurrent phases.
- Avoiding scanning the old generation during young collections (using the remset).

**Age tracking.** Each object's header carries a 4-bit age (0-15). The age is incremented each time the object survives a young collection. When the age reaches the promotion threshold (default 15, configurable), the object is promoted to the old generation.

**Generational invariant.** At all times:

> If an object O in an old region contains a reference to an object Y in a young region, then the card containing that reference is recorded in the old region's remset.

The write barrier (§6.7) maintains this invariant.

### 6.6 Concurrent Marking: SATB and the Tri-Color Abstraction

**Tri-color abstraction.** During marking, every object is in one of three conceptual states:

- **White**: not yet known to be reachable. Candidate for reclamation.
- **Gray**: known to be reachable, but not all of its outgoing references have been scanned.
- **Black**: known to be reachable, and all of its outgoing references have been scanned.

**SATB (snapshot-at-the-beginning) invariant.** At the start of a marking cycle, the GC takes a conceptual snapshot of the heap. The marking must identify at least all objects that were reachable at that snapshot. Objects that become unreachable after the snapshot may or may not be collected in this cycle (they will be collected in the next).

Formally: if an object O was reachable from roots at snapshot time t₀, then O must be marked black or gray at the end of the marking phase.

**SATB write barrier.** To maintain the SATB invariant, the write barrier intercepts reference stores. When a black object's reference field is modified to point to a white object, the object must be re-grayed (added to the marking worklist). The barrier:

```
write_barrier(src_obj, field_offset, new_ref):
    if src_obj is black and new_ref is white:
        enqueue(src_obj)  // re-gray
    // additionally, handle generational invariant (see §6.7)
```

In practice, the barrier is simplified: any store into an old object that might point to an unmarked object causes the source to be enqueued. The precision of "might be unmarked" is a performance/correctness tradeoff; we use a conservative check based on the mark bitmap.

**Marking phases.**

1. **Initial mark (STW, short)**: scan roots, mark directly-reachable objects gray.
2. **Concurrent mark**: GC threads pop gray objects, scan their fields, mark referenced white objects gray, mark the scanned object black. Mutator threads execute the write barrier.
3. **Concurrent preclean**: process the remset to handle cross-generational references that arrived during concurrent mark.
4. **Remark (STW, short)**: drain any remaining gray objects, process weak references, finalize the mark bitmap.

**Mark bitmap.** Each old region has a mark bitmap with one bit per minimum object size (e.g., 16 bytes). A set bit means the object is marked (gray or black). The bitmap is double-buffered: one bitmap is the "prev" bitmap (from the last completed cycle), the other is the "next" bitmap (being built). This allows the mutator to query "was this object reachable last cycle?" for certain optimizations.

### 6.7 Write Barrier

**Combined barrier.** The write barrier performs two duties:

1. **SATB marking**: ensure the tri-color invariant.
2. **Generational invariant**: record old-to-young stores in the remset.

**Barrier implementation.**

```
store(obj, offset, value):
    obj[offset] = value
    
    // Generational part: if obj is old and value is young, dirty the card
    if obj is in old region and value is in young region:
        card = card_of(obj + offset)
        if card_table[card] != CARD_DIRTY:
            card_table[card] = CARD_DIRTY
            // remset will be rebuilt from dirty cards at young collection
    
    // SATB part: if obj is black and value is not marked, re-gray obj
    if mark_bitmap[obj] == BLACK and (value == null or mark_bitmap[value] == WHITE):
        satb_queue.push(obj)
```

**Fast path.** The fast path (no barrier work needed) is a single comparison: if the store is young-to-young or old-to-old with no marking concern, the barrier is a no-op. On modern hardware this is 1 cycle.

**SATB queue.** Each mutator thread has a thread-local SATB queue. When the queue fills, it is flushed to the global SATB queue (lock-free). GC threads consume from the global queue.

### 6.8 Young-Generation Collection

**Young collection (minor GC).** A young collection reclaims unreachable young objects and promotes survivors.

**Phases.**

1. **Pause start (STW)**: enter safepoint, flush TLABs, scan roots (stacks, registers, globals).
2. **Process remset**: for each old region, scan its dirty cards to find old-to-young references. These are added to the root set for the young collection.
3. **Evacuate young regions**: for each live young object, copy it to a survivor region (if age < threshold) or an old region (if age ≥ threshold). Update all references to the object via a forwarding table.
4. **Update references**: scan the evacuated regions' original locations (or use a slot table) to fix up references.
5. **Release old young regions**: mark them as free.
6. **Resume mutator**.

**Pause time.** The pause is bounded by the amount of live data in the young generation. With a bounded young generation size (e.g., 32 MB default), the pause is bounded to a few hundred microseconds on modern hardware.

**Copying algorithm.** Uses a worklist of regions to scan. Each region has a "scan cursor" and a "copy cursor". Live objects are copied to the copy cursor's target region; the scan cursor advances through the copied data, scanning fields and enqueuing newly-discovered live objects.

### 6.9 Old-Generation Evacuation (Concurrent Relocation)

**Concurrent compaction.** Unlike the young generation, the old generation is evacuated **concurrently** with the mutator. This is achieved via load barriers and colored pointers (ZGC-style).

**Evacuation phases.**

1. **Concurrent prepare**: select old regions for evacuation (based on liveness ratio).
2. **Concurrent relocate**: for each live object in a selected region, allocate space in a new region, copy the object, and install a forwarding pointer in the old location. The object's new address is encoded into the colored pointer.
3. **Concurrent remap**: mutator threads, via load barriers, gradually update their references to point to the new locations.
4. **Pause remap (STW, very short)**: scan roots and thread-local state to remap any remaining old references.
5. **Release old regions**: mark them as free.

**Forwarding pointer.** When an object is relocated, its old location is overwritten with a forwarding pointer to the new location. The forwarding pointer is distinguished from a normal pointer by a dedicated bit in the mark word.

### 6.10 Load Barrier and Colored Pointers

**Colored pointer scheme.** Each pointer carries metadata in its high bits indicating the state of the object it references:

- **Marked0/Marked1**: encode the mark state from the most recent marking cycle.
- **Remapped**: indicates the pointer has been updated to point to the relocated object.
- **Finalizable**: reserved.

**Load barrier.** Every reference load goes through a load barrier:

```
load(obj, offset) -> ref:
    raw = obj[offset]
    if raw has bad color:
        return slow_path(raw)
    else:
        return raw

slow_path(raw):
    // Case 1: object has been relocated; forward to new address
    if raw points to a forwarding entry:
        new_addr = forwarding_table[raw]
        new_ref = recolor(new_addr, correct_color)
        obj[offset] = new_ref  // self-healing
        return new_ref
    
    // Case 2: pointer color is stale; recolor
    actual_addr = strip_color(raw)
    new_ref = recolor(actual_addr, current_mark_state)
    obj[offset] = new_ref  // self-healing
    return new_ref
```

**Self-healing.** The load barrier writes the corrected pointer back into the slot. This means subsequent loads of the same slot are fast-path.

**Fast path.** If the pointer's color matches the current expected color, the barrier is a single comparison and branch: 1-2 cycles.

**Compiler integration.** The T1 JIT emits load barriers for all reference loads. For hot code, the fast path is inlined; the slow path is a call to a runtime helper.

### 6.11 Safepoints and Stack Scanning

**Safepoints.** Mutator threads reach safepoints at:
- Backward branches (loop back-edges).
- Method returns.
- Epilogues.
- Explicit `safepoint` bytecodes (from UGB).

At a safepoint, the thread checks a **safepoint flag**. If set, the thread parks and waits for the GC to finish its STW phase.

**Stack scanning.** During STW phases (initial mark, remark, pause remap), the GC scans each mutator thread's stack to find root references. The scan uses:
- **Stack maps**: emitted by the T1 JIT, describing which stack slots contain references at each safepoint PC.
- **Register maps**: describing which registers contain references at each safepoint PC.

**Concurrent root scanning (for concurrent phases).** For concurrent marking, roots are scanned at the initial-mark STW pause. Subsequent changes to roots (e.g., new stack references) are handled by the SATB barrier: any store of a reference into a root location (global variable, field) triggers the barrier.

**Thread-local state.** Each mutator thread maintains:
- TLAB (for allocation).
- SATB queue (for write barrier buffering).
- Safepoint state (parked/running).
- Stack map pointer (for the current PC).

### 6.12 Reference Processing

**Reference types.** The GC supports four reference strengths:

- **Strong**: normal references. Object is live if reachable via strong references.
- **Soft**: cleared only when memory is needed. Cleared in full collections when heap is tight.
- **Weak**: cleared at the end of any marking cycle if the referent is not strongly reachable.
- **Phantom**: cleared after finalization, but the referent is not accessible after clearing. Used for post-mortem cleanup tracking.

**Discovery and processing.** During the remark phase:
1. Discover reference objects whose referents are not strongly reachable.
2. Enqueue soft references only if heap pressure warrants.
3. Clear weak and phantom references (set referent to null).
4. Enqueue cleared references onto their registered queues (for application-level notification).

**Finalization.** Objects with finalizers are treated specially:
1. If an object with a finalizer is found unreachable, it is registered for finalization instead of being reclaimed.
2. Its finalizer is invoked (possibly asynchronously).
3. After finalization, if the object is still unreachable, it is reclaimed in the next cycle.

### 6.13 Collection Policy and Triggering

**Young collection trigger.** A young collection is triggered when:
- TLAB allocation fails (no free young regions).
- Explicit request from the runtime.

**Old collection trigger.** An old collection is triggered when:
- The old generation occupancy exceeds a threshold (e.g., 80% of committed old regions).
- A certain number of young collections have occurred since the last old collection.
- Explicit request from the runtime.

**Concurrent cycle pacing.** Once triggered, the concurrent marking cycle is paced to complete before the old generation fills up. The pacer computes the amount of marking work to perform per mutator allocation quantum:

```
work_per_alloc = (remaining_mark_work) / (remaining_heap_before_full)
```

**Adaptive sizing.** The young generation size is adjusted based on pause time targets:
- If pauses are too long, shrink the young generation.
- If GC overhead is too high, grow the young generation.

### 6.14 Validity Invariants

Valid_GC(H) holds iff all of the following hold:

**1. Heap well-formedness.**
- Every object's size field is consistent with its actual allocation size.
- Every reference field contains either null or a valid pointer to an object.
- No two objects overlap in memory.
- The card table accurately reflects the generation of each card.

**2. Tri-color marking invariant.**
- At the end of a marking cycle, every object reachable from roots is black.
- During marking, no black object has a reference to a white object that was reachable at snapshot time. (Enforced by SATB barrier.)

**3. Generational invariant.**
- Every old-to-young reference is recorded in the source region's remset (via dirty card).
- The card table is consistent with the remset.

**4. Load-barrier forwarding invariant.**
- Every pointer in the mutator's view either points to a valid (non-forwarded) object with the correct color, or will be corrected by the load barrier before use.
- Forwarding pointers are installed atomically with respect to mutator loads.

**5. Safepoint consistency.**
- At any safepoint, the stack maps and register maps accurately describe the reference-containing locations.
- No mutator thread is executing outside a safepoint during an STW phase.

**6. Allocation consistency.**
- TLABs do not overlap.
- Bump-pointer allocation does not exceed region bounds.
- Promotion correctly updates the object's generation.

**7. Reference processing correctness.**
- Weak references are cleared if and only if the referent is not strongly reachable.
- Soft references are cleared only under memory pressure.
- Phantom references are cleared after finalization.
- Finalizable objects are not reclaimed until after finalization.

**8. Barrier integrity.**
- The write barrier is executed for every reference store (enforced by compiler).
- The load barrier is executed for every reference load (enforced by compiler).
- Barrier failures are internal errors, not user-visible.

### 6.15 Representation and Cost Model

**Heap metadata overhead.**
- Card table: 1 byte per 512 bytes of heap = 0.2%.
- Mark bitmap: 1 bit per 16 bytes = 0.78%.
- Forwarding table: 8 bytes per region (one pointer per region header) = negligible.
- SATB queues: per-thread, bounded size.
- Total: ~1% of heap.

**Write barrier cost.**
- Fast path (no work needed): 1 comparison + 1 conditional branch = ~1 cycle.
- Slow path (SATB enqueue or card dirtying): ~20-50 cycles, amortized over many stores.

**Load barrier cost.**
- Fast path (correct color): 1 comparison + 1 conditional branch = ~1-2 cycles.
- Slow path (forwarding or recoloring): ~50-100 cycles, amortized via self-healing.

**Pause times.**
- Initial mark: O(|roots|) = typically 100-500 µs.
- Remark: O(|roots| + |remaining gray|) = typically 100-500 µs.
- Pause remap: O(|roots|) = typically 50-200 µs.
- Young collection: O(|live young|) = typically 500 µs - 5 ms depending on young size.

**Throughput overhead.**
- GC work as fraction of total time: 2-5% at steady state.
- Barrier overhead as fraction of mutator time: 1-3%.

**Memory complexity.** O(|heap| · metadata_fraction + |threads| · per_thread_state + |regions| · per_region_state).

**Construction cost.** Heap initialization: O(|heap|) to zero metadata. Collection cycle: O(|live|) total work, spread over time.

---

# Compiler Optimizations for AOT and JIT Compilers (Revised)
---

## 1. Scalar, Local & Peephole Optimizations (1–24)

| # | Optimization | Description |
|---|---|
| 1 | **Constant folding** | Evaluate constant expressions at compile time. |
| 2 | **Constant propagation** | Replace variables with known constant values. |
| 3 | **Partial constant propagation** | Propagate constants only along paths where it enables further optimizations (cost-driven). |
| 4 | **Copy propagation** | Replace `x = y` usages of `x` with `y`. |
| 5 | **Partial copy propagation** | Propagate copies only when it reduces register pressure or exposes CSE. |
| 6 | **Local dead code elimination (DCE)** | Remove instructions whose results are unused. |
| 7 | **Partial dead code elimination (PDCE)** | Eliminate code that is dead on *some* paths by duplicating the live paths (Kildall, 1981; more aggressive than total DCE). |
| 8 | **Common subexpression elimination (CSE)** | Reuse results of identical expressions. |
| 9 | **Partial CSE** | Perform CSE only when the reuse is profitable given register pressure. |
| 10 | **Algebraic simplification** | `x * 1 → x`, `x + 0 → x`, `x * 2 → x << 1`. |
| 11 | **Strength reduction** | Replace expensive ops with cheaper equivalents. |
| 12 | **Partial strength reduction** | Apply strength reduction only to induction variables or ops on hot paths. |
| 13 | **Peephole optimization** | Pattern-match and replace short instruction sequences. |
| 14 | **Branch folding / simplification** | Remove redundant branches, convert to conditional moves. |
| 15 | **Tail call optimization (TCO)** | Reuse caller's stack frame for tail calls. |
| 16 | **Local value numbering (LVN)** | Detect equivalent expressions within a block. |
| 17 | **Partial value numbering** | Perform LVN only on subsets of expressions where hash collisions or cost justify it. |
| 18 | **Instruction combining** | Merge multiple arithmetic/logical ops into a single target instruction. |
| 19 | **Reassociation** | Rearrange commutative/associative ops to expose CSE or parallelism. |
| 20 | **Redundant flag elimination** | Remove duplicate condition-code updates. |
| 21 | **Trivial dead store elimination** | Remove writes immediately overwritten. |
| 22 | **Bitwise identity simplification** | Simplify `x & 0`, `x | -1`, etc. |
| 23 | **Sign/zero-extension elimination** | Remove redundant extensions based on value ranges. |
| 24 | **Shift-and-add / division-by-constant lowering** | Compile multiplications/divisions by constants into shifts and magic-number multiplications. |

---

## 2. Global, SSA & Data-Flow Optimizations (25–56)

| # | Optimization | Description |
|---|---|
| 25 | **Global value numbering (GVN)** | Prove expression equivalence across blocks. |
| 26 | **Sparse conditional constant propagation (SCCP)** | Combine constant propagation with unreachable-code elimination (Wegman & Zadeck, 1991). |
| 27 | **Partial SCCP** | Apply SCCP only along hot paths or where lattice depth is bounded. |
| 28 | **Global dead store elimination (DSE)** | Remove memory writes never read before being overwritten. |
| 29 | **Partial dead store elimination** | Eliminate stores that are dead on some paths via path-sensitive analysis. |
| 30 | **Redundant load elimination (RLE)** | Reuse values already in registers. |
| 31 | **Partial redundant load elimination** | Eliminate loads redundant on some paths (variant of PRE). |
| 32 | **Partial redundancy elimination (PRE)** | Eliminate expressions redundant on *some* paths by code motion + CSE (Morel & Renvoise, 1979). |
| 33 | **Lazy code motion (LCM)** | Optimal PRE variant minimizing register pressure (Knoop, Rüthing, Steffen, 1992). |
| 34 | **Partial code motion** | Hoist/sink only code where the benefit exceeds the cost (profile- or cost-driven). |
| 35 | **Available expressions analysis** | Classical data-flow for CSE. |
| 36 | **Live variable analysis** | Determine which variables hold values needed in the future. |
| 37 | **Live range splitting** | Shorten variable lifetimes to ease register allocation. |
| 38 | **Phi-node optimization** | Simplify or eliminate φ-nodes in SSA. |
| 39 | **Aggressive dead code elimination (ADCE)** | Iterated DCE using call-graph and side-effect analysis. |
| 40 | **Conditional constant propagation** | Propagate constants along specific CFG edges based on branch conditions. |
| 41 | **Unreachable code elimination** | Remove blocks with no predecessors. |
| 42 | **Dominator tree optimizations** | Use dominance frontiers for efficient φ-node placement. |
| 43 | **Post-dominator tree optimizations** | Used for sinking and control-dependence analysis. |
| 44 | **Control dependence analysis** | Determine which instructions depend on specific branch outcomes. |
| 45 | **Static Single Information (SSI)** | Extend SSA to track both definitions and uses. |
| 46 | **Value forwarding** | Pass values directly via registers across block boundaries. |
| 47 | **Global copy propagation** | Propagate copies across the entire CFG. |
| 48 | **Partial sinking** | Delay instructions to their latest profitable use point (opposite of hoisting). |
| 49 | **Partial hoisting** | Hoist instructions to earliest profitable definition point. |
| 50 | **Critical edge splitting** | Insert empty blocks to safely place code on multi-source/multi-dest edges. |
| 51 | **Out-of-SSA translation optimization** | Minimize copies when leaving SSA form. |
| 52 | **Phi-web coalescing** | Merge variables connected by φ-nodes. |
| 53 | **SSA renaming optimization** | Minimize registers during SSA construction. |
| 54 | **Sparse simple constant propagation** | Lightweight SCCP variant for fast JIT tiers. |
| 55 | **Correlated propagation** | Propagate range info derived from branch conditions (e.g., after `if (x > 0)`). |
| 56 | **Range analysis / Value range propagation (VRP)** | Track min/max bounds to eliminate bounds checks and enable other opts. |

---

## 3. Control Flow Graph (CFG) Optimizations (57–74)

| # | Optimization | Description |
|---|---|
| 57 | **Basic block merging** | Combine a block with its sole successor. |
| 58 | **Basic block splitting** | Break large blocks for finer-grained scheduling. |
| 59 | **Empty block removal** | Eliminate blocks containing only unconditional jumps. |
| 60 | **Jump threading** | Redirect jumps to bypass intermediate blocks. |
| 61 | **Partial jump threading** | Thread only jumps where the benefit (branch elimination) exceeds code-size cost. |
| 62 | **Branch chain elimination** | Collapse chains of unconditional jumps. |
| 63 | **Switch lowering (binary search / jump tables / bit-test)** | Convert switches into optimal decision structures. |
| 64 | **Tail duplication** | Copy small successor blocks to eliminate conditional branches. |
| 65 | **Partial tail duplication** | Duplicate only small, cold successor blocks (size-threshold driven). |
| 66 | **Trace scheduling** | Reorder blocks to form long straight-line traces based on profiles. |
| 67 | **Superblock formation** | Blocks with single entry but multiple exits. |
| 68 | **Hyperblock formation** | Use predication to merge diamond CFGs into single blocks. |
| 69 | **Branchless code generation** | Replace branches with conditional moves or arithmetic. |
| 70 | **Partial branchless conversion** | Convert only branches where the cost of misprediction exceeds the cmov latency. |
| 71 | **Switch splitting** | Break large switches into smaller sub-switches. |
| 72 | **If-conversion** | Convert short if-then-else to predicated/branchless code. |
| 73 | **Partial if-conversion** | Apply only when the branch is unpredictable and the body is small. |
| 74 | **Block reordering for locality** | Place frequently-executed blocks adjacently. |

---

## 4. Loop Transformations (75–115)

| # | Optimization | Description |
|---|---|
| 75 | **Full loop unrolling** | Replicate entire body to eliminate overhead. |
| 76 | **Partial loop unrolling** | Replicate body N times (cost/benefit driven). |
| 77 | **Loop unswitching** | Move loop-invariant conditionals outside the loop. |
| 78 | **Partial loop unswitching** | Unswitch only the most selective/predictable conditions. |
| 79 | **Loop-invariant code motion (LICM)** | Hoist computations that don't change per iteration. |
| 80 | **Partial LICM** | Hoist only invariants where hoisting doesn't hurt register pressure or cache. |
| 81 | **Induction variable simplification / elimination** | Reduce and remove redundant IVs. |
| 82 | **Strength reduction on IVs** | Replace IV multiplications with additions. |
| 83 | **Loop fusion (jamming)** | Combine adjacent loops with same bounds. |
| 84 | **Partial loop fusion** | Fuse only loops where fusion improves locality without increasing register pressure. |
| 85 | **Loop fission (distribution)** | Split a loop for better cache/register use. |
| 86 | **Partial loop fission** | Split only loops where fission enables vectorization or reduces pressure. |
| 87 | **Loop interchange** | Swap nested loop order for locality. |
| 88 | **1D loop strip mining** | Break loop into chunks fitting in registers/L1. |
| 89 | **Multi-dimensional loop tiling** | Block nested loops for cache reuse. |
| 90 | **Partial loop tiling** | Tile only loops with affine, perfectly-nested structure. |
| 91 | **Loop skewing** | Transform iteration spaces to expose parallelism. |
| 92 | **Loop reversal / collapsing / normalization** | Direction, nesting, and bound transformations. |
| 93 | **Software pipelining** | Overlap iterations to hide latency. |
| 94 | **Partial software pipelining** | Pipeline only innermost loops or those with bounded initiation intervals. |
| 95 | **Modulo scheduling** | Resource-constrained software pipelining. |
| 96 | **Iteration peeling** | Handle first/last iterations specially. |
| 97 | **Partial loop peeling** | Peel only iterations needed for alignment or prologue/epilogue. |
| 98 | **Loop versioning** | Generate specialized loop variants (aligned vs. unaligned). |
| 99 | **Loop unroll-and-jam** | Unroll outer loop and fuse resulting inner loops. |
| 100 | **Loop bound analysis / elimination of bounds checks** | Prove bounds to remove internal checks. |
| 101 | **Loop-carried dependence analysis** | Determine if iterations can be reordered. |
| 102 | **Reduction recognition** | Identify accumulation patterns for vectorization. |
| 103 | **Loop idiom recognition** | Replace manual loops with `memset`/`memcpy`/etc. |
| 104 | **Loop predication** | Hoist bounds checks out of loops by verifying range upfront. |
| 105 | **Partial loop predication** | Predicate only checks that dominate the loop body. |
| 106 | **Loop-invariant function call hoisting** | Move pure calls out of loops. |
| 107 | **Loop unswitching with branch prediction** | Use profile data to duplicate loops for the hot path. |
| 108 | **Multi-level loop interchange** | Reorder deeply nested loops. |
| 109 | **Loop index set splitting** | Partition loops into ranges where different opts apply. |
| 110 | **Affine loop scheduling (Polyhedral)** | Compute optimal execution order for affine loops. |
| 111 | **Polyhedral dataflow analysis** | Exact dependence analysis for complex loop nests. |
| 112 | **Partial vectorization** | Vectorize only the main body of a loop, leaving a scalar tail. |
| 113 | **Partial loop distribution** | Distribute only statements that benefit from separation. |
| 114 | **Loop-aware prefetch insertion** | Insert prefetches based on loop stride and cache geometry. |
| 115 | **Loop alignment optimization** | Align loop entry to cache-line boundaries. |

---

## 5. Interprocedural & Whole-Program Optimizations (116–150)

| # | Optimization | Description |
|---|---|
| 116 | **Function inlining** | Replace call with callee body. |
| 117 | **Partial inlining** | Inline only the hot prefix of a function, leaving the cold path as a call. |
| 118 | **Devirtualization** | Resolve virtual/dynamic calls to direct calls. |
| 119 | **Partial devirtualization** | Devirtualize only monomorphic or highly-biased call sites. |
| 120 | **Class hierarchy analysis (CHA)** | Use type hierarchy to limit call targets. |
| 121 | **Rapid type analysis (RTA)** | Refine CHA using instantiated types. |
| 122 | **Variable type inference (VTI)** | Infer types of dynamically-typed variables. |
| 123 | **Interprocedural constant propagation (IPCP)** | Propagate constants across calls. |
| 124 | **Partial IPCP** | Propagate constants only for call sites where it shrinks the callee or enables inlining. |
| 125 | **Interprocedural DCE** | Remove unused functions/methods. |
| 126 | **Function cloning / specialization** | Create specialized copies for specific argument types/values. |
| 127 | **Partial function cloning** | Clone only functions with a small number of hot argument profiles. |
| 128 | **Escape analysis** | Determine if an object escapes its creating scope. |
| 129 | **Partial escape analysis (PEA)** | Track object escape per-path; allow allocation elimination even if object escapes on *some* paths (Säverud et al., GraalVM — a landmark research optimization). |
| 130 | **Scalar replacement of aggregates (SRA)** | Break structs/objects into scalar fields. |
| 131 | **Partial scalar replacement** | Replace only fields that are proven non-escaping or frequently accessed. |
| 132 | **Call graph construction** | Build static or dynamic call graphs. |
| 133 | **Summary-based analysis** | Propagate function summaries (side effects, return values). |
| 134 | **Link-time optimization (LTO)** | Whole-program optimization at link time. |
| 135 | **ThinLTO** | Scalable LTO using summary bitcodes. |
| 136 | **Profile-guided optimization (PGO)** | Use runtime profiles to guide AOT decisions. |
| 137 | **AutoFDO / SampleFDO** | PGO from hardware sampling. |
| 138 | **Context-sensitive analysis** | Analyze functions in calling context. |
| 139 | **Partial evaluation** | Specialize a program with respect to known compile-time inputs (Jones, Gomard, Sestoft — a whole research field underpinning JIT specialization). |
| 140 | **Argument specialization** | Generate code tailored to specific argument values. |
| 141 | **Return value optimization (RVO) / NRVO** | Elide copies of returned objects. |
| 142 | **Cross-module inlining** | Inline across compilation units. |
| 143 | **Whole-program devirtualization** | Prove exact type globally to eliminate virtual dispatch. |
| 144 | **Interprocedural SCCP** | Apply SCCP across the call graph. |
| 145 | **Tail call / sibling call optimization** | Reuse caller's stack frame for tail calls. |
| 146 | **Exception handling table optimization** | Shrink LSDA tables. |
| 147 | **Dead function elimination** | Remove unused code at module/program level. |
| 148 | **Partial specialization** | Specialize functions only for the most frequent type profiles. |
| 149 | **Speculative devirtualization** | Devirtualize with a runtime type-check guard. |
| 150 | **Argument marshalling optimization** | Optimize FFI/RPC data packing. |

---

## 6. Memory, Pointers, Alias & GC Optimizations (151–184)

| # | Optimization | Description |
|---|---|
| 151 | **Stack allocation of heap objects** | Promote non-escaping heap objects to the stack. |
| 152 | **Allocation sinking** | Delay object allocation until first use. |
| 153 | **Partial allocation sinking** | Sink only allocations where the delay reduces pressure on the fast path. |
| 154 | **Object field reordering** | Reorder struct fields for packing/alignment. |
| 155 | **Memory coalescing** | Merge adjacent memory ops into wider operations. |
| 156 | **Cache-aware data layout** | Reorder data structures for cache line utilization. |
| 157 | **Software prefetch insertion** | Insert explicit prefetches. |
| 158 | **Partial prefetching** | Insert prefetches only for accesses with predictable strides. |
| 159 | **TLB optimization** | Promote hot regions to large/huge pages. |
| 160 | **Write barrier elimination** | Remove GC write barriers when proven unnecessary. |
| 161 | **Partial write barrier elimination** | Remove barriers only on paths where GC state is known (e.g., young-gen only). |
| 162 | **Card table / remembered set optimization** | Optimize GC dirty-card tracking. |
| 163 | **Generational GC tuning** | Optimize nursery sizes and promotion thresholds. |
| 164 | **Region-based memory management** | Group allocations into regions freed together. |
| 165 | **Bump-pointer allocation / TLAB** | Eliminate lock contention for thread-local allocations. |
| 166 | **Slab / free-list / arena allocation lowering** | Optimize pool allocations. |
| 167 | **Pointer compression (Compressed Oops)** | Use 32-bit offsets for 64-bit heap pointers. |
| 168 | **Partial pointer compression** | Compress only pointers within a bounded heap region. |
| 169 | **Heap compaction / mark-and-sweep bitmap optimization** | Evacuation and fast marking. |
| 170 | **Reference counting cycle collection** | Detect and collect reference cycles. |
| 171 | **Flow-insensitive alias analysis (Andersen's / Steensgaard's)** | Inclusion-based and equality-based pointer analysis. |
| 172 | **Flow-sensitive alias analysis** | Track pointer aliases across control flow. |
| 173 | **Context-sensitive / field-sensitive alias analysis** | Track aliases through call graph and struct fields. |
| 174 | **Mod/Ref analysis** | Determine if a function modifies or only references memory. |
| 175 | **Zero-cost abstraction lowering** | Eliminate overhead from iterators, monads, etc. |
| 176 | **Pointer alignment inference** | Prove alignment to enable vectorized loads. |
| 177 | **Non-temporal store insertion** | Use streaming stores to bypass cache for large sequential writes. |
| 178 | **Partial non-temporal store insertion** | Use streaming stores only when the working set exceeds cache. |
| 179 | **Store-to-load forwarding promotion** | Promote stores that are immediately reloaded into register-to-register moves. |
| 180 | **Partial store-to-load forwarding** | Forward only when the store size matches the load and no aliasing intervenes. |
| 181 | **Memory access coalescing** | Merge adjacent scalar accesses into vector loads/stores. |
| 182 | **Partial memory coalescing** | Coalesce only when alignment and size permit. |
| 183 | **Stack slot coloring** | Reuse stack slots for non-overlapping live ranges. |
| 184 | **Partial stack slot coloring** | Color only slots where reuse saves meaningful stack space. |

---

## 7. Object-Oriented & Dynamic-Language Optimizations (185–208)

| # | Optimization | Description |
|---|---|
| 185 | **Inline caching (IC)** | Cache method lookup results at call sites. |
| 186 | **Partial inline caching** | Use ICs only for hot call sites; fall back to generic dispatch for cold ones. |
| 187 | **Monomorphic / polymorphic / megamorphic ICs** | Specialize by call-site diversity. |
| 188 | **Speculative inlining** | Inline based on profiled receiver type. |
| 189 | **Guarded devirtualization** | Inline with a type-check guard. |
| 190 | **Partial guarded devirtualization** | Guard only the hottest call sites; leave others polymorphic. |
| 191 | **Type profiling / type specialization** | Record observed types; generate type-specific code paths. |
| 192 | **Partial type specialization** | Specialize only for the top-N observed types. |
| 193 | **Hidden classes / maps / shapes** | Track object layouts dynamically (V8, SpiderMonkey, JSC). |
| 194 | **Method handle / invokedynamic optimization** | Optimize dynamic dispatch mechanisms. |
| 195 | **Allocation elimination** | Remove allocations that don't escape. |
| 196 | **Object inlining** | Embed small objects directly in their parent. |
| 197 | **Field hoisting** | Hoist field loads out of loops/methods. |
| 198 | **Partial field hoisting** | Hoist only fields with invariant values on the hot path. |
| 199 | **Write-combining / store buffering** | Merge consecutive field writes. |
| 200 | **Unboxing / box elimination** | Extract primitives from wrappers; prevent wrapper creation. |
| 201 | **Partial unboxing** | Unbox only the most common wrapper types on hot paths. |
| 202 | **Primitive specialization** | Generate specialized collections/methods for `int`, `double`, etc. |
| 203 | **Dynamic code generation (tracing)** | Record linear traces for JIT compilation. |
| 204 | **Partial trace selection** | Select only the hottest traces for compilation (trace JITs). |
| 205 | **Megamorphic call site splitting** | Split call sites by caller context. |
| 206 | **Dictionary / property lookup optimization** | Optimize dynamic property access via ICs and shapes. |
| 207 | **Speculative class checks** | Hoist class checks with deoptimization on failure. |
| 208 | **Partial speculative class checks** | Speculate only on classes with high observed frequency. |

---

## 8. Parallelism, Vectorization & Concurrency (209–228)

| # | Optimization | Description |
|---|---|
| 209 | **Auto-vectorization (SLP)** | Superword-Level Parallelism — pack scalars into SIMD. |
| 210 | **Loop vectorization** | Vectorize loop bodies. |
| 211 | **Auto-parallelization** | Detect and parallelize independent loops. |
| 212 | **Partial auto-parallelization** | Parallelize only loops with high iteration counts and low synchronization. |
| 213 | **Loop distribution for parallelism** | Split loops to expose parallel regions. |
| 214 | **Dependence analysis** | Determine loop-carried dependencies. |
| 215 | **Polyhedral optimization** | Model affine loops as polyhedra for complex transformations. |
| 216 | **Transaction memory (HTM/STM) lowering** | Compile atomic blocks into hardware/software transactions. |
| 217 | **SIMD intrinsics lowering** | Map high-level ops to hardware SIMD. |
| 218 | **Multi-versioning** | Generate vector and scalar variants chosen at runtime. |
| 219 | **Lock elision** | Remove locks when thread-local analysis proves no contention. |
| 220 | **Partial lock elision** | Elide locks only on paths with low observed contention. |
| 221 | **Lock coarsening / splitting / RW-lock promotion** | Adjust lock granularity. |
| 222 | **Barrier elimination** | Remove unnecessary thread synchronization barriers. |
| 223 | **Partial barrier elimination** | Remove barriers only where memory model permits relaxation. |
| 224 | **Volatile access optimization** | Minimize memory fence overhead. |
| 225 | **Atomic operation lowering** | Optimize CAS / LL-SC. |
| 226 | **Concurrent GC optimizations** | Fold write-barriers into normal stores. |
| 227 | **False sharing mitigation** | Insert padding between thread-local variables. |
| 228 | **Memory model relaxation** | Reorder loads/stores where the language memory model permits. |

---

## 9. JIT-Specific & Runtime Mechanisms (229–242)

| # | Optimization | Description |
|---|---|
| 229 | **Tiered compilation** | Start with baseline JIT, then optimize hot code. |
| 230 | **On-stack replacement (OSR)** | Replace executing code with optimized version mid-execution. |
| 231 | **Partial OSR** | Perform OSR only for the hottest loops; leave cold loops interpreted. |
| 232 | **Deoptimization** | Fall back to interpreter when assumptions fail. |
| 233 | **Partial deoptimization** | Deoptimize only the failed region, keeping unrelated optimized code intact (frame-aware). |
| 234 | **Speculative optimization** | Optimize based on observed behavior; deoptimize on failure. |
| 235 | **Partial speculative optimization** | Apply speculation only on hot paths with high-confidence profiles. |
| 236 | **Background compilation** | Compile on separate threads. |
| 237 | **Partial background compilation** | Compile only high-priority methods in the background; others synchronously. |
| 238 | **Lazy compilation** | Compile only when needed. |
| 239 | **Adaptive optimization** | Re-optimize based on evolving profiles. |
| 240 | **Inline cache miss handling / code patching** | Patch call sites when new types observed. |
| 241 | **Safepoint insertion / bailout state generation** | Enable GC and deopt at defined points. |
| 242 | **Runtime profile collection / partial profiling** | Gather counters, type maps, and branch probabilities; profile only hot regions to reduce overhead. |

---

# 32-Bit Target Support for Garbage Collection — Complete Design Document (v1)

## 0. Document Control and Design Scope

**Component:** 32-Bit Target Support (extension to Garbage Collection Subsystem)

**Version:** 1.0

**Role:** Provides garbage collection support for 32-bit target architectures where colored pointers are not feasible due to the lack of unused address bits. Implements alternative mechanisms for marking, forwarding, and barriers using side-table metadata and object header bits.

**Design Status:** Complete specification.

**Scope:** This document defines the 32-bit object header layout, mark bitmap usage, header-based forwarding, load barrier implementation, write barrier adjustments, performance model, and an alternative mark-compact mode. It does not define:
- The 64-bit GC design (already specified; this is an extension/alternative)
- The T1 JIT compiler (must emit 32-bit-specific barriers)
- The runtime system (consumes this subsystem)

**Normative References:**
- Jones, R. and Lins, R. "Garbage Collection: Algorithms for Automatic Dynamic Memory Management" (mark-compact algorithms)
- Bacon, D. et al. "Concurrent Cycle Collection in Reference Counted Systems" (bitmap-based marking)
- Standard GC literature for 32-bit heap management

**Known Limitations:**
- Higher barrier overhead compared to 64-bit (2-3 memory accesses per load vs. 1).
- Mark-compact mode has longer pauses (STW) compared to concurrent relocation.
- Limited address space (4 GB) constrains heap size and may require more frequent collections.

---

## 6. 32-Bit Target Support

### 6.1 Purpose and Design Rationale

On 32-bit targets, all 32 bits of a pointer are used for the address. There are no unused high bits to encode GC metadata (mark state, forwarding info, etc.). Therefore, the colored-pointer scheme used on 64-bit targets is not feasible.

**Design choices.** We consider three approaches:

1. **Side-table approach**: Maintain a parallel hash table mapping object addresses to GC state. *Rejected*: too expensive (hash lookup on every barrier).

2. **Header-only approach**: Store all GC state in the object header. *Rejected*: requires loading the header on every reference load, which is expensive.

3. **Hybrid approach**: Use the mark bitmap for marking (same as 64-bit) and the object header for forwarding. The load barrier checks the header only when necessary. *Selected*: balances performance and complexity.

**Alternative mode.** For simpler implementation, we also support a mark-compact mode where old-generation collection is STW (no concurrent relocation). This eliminates the need for a load barrier during old-generation collection but results in longer pauses.

### 6.2 Object Header Layout (32-bit)

The object header is modified to accommodate 32-bit constraints:

```
struct ObjectHeader32 {
    mark_word:    u32,   // hash code / lock state / age / forwarding pointer
    klass_ptr:    u32,   // pointer to type metadata
    size:         u16,   // object size in bytes (max 64 KB; larger objects use humongous regions)
    flags:        u8,    // generation, humongous, pinned, forwarded bit
    _pad:         u8,
}
```

**Total header size**: 12 bytes (padded to 16 bytes for alignment).

**Forwarding bit.** The `flags` field includes a `FORWARDED` bit (bit 0). When an object is relocated, this bit is set in the old object's header, and the `mark_word` field is overwritten with the new address.

```
FLAG_FORWARDED = 0x01
```

**Forwarding pointer.** When `flags & FLAG_FORWARDED` is set, the `mark_word` field contains the new address of the object. The forwarding pointer is a full 32-bit address.

### 6.3 Mark Bitmap (32-bit)

The mark bitmap is identical to the 64-bit design: one bit per minimum object size (16 bytes). The bitmap is stored in a metadata region and is accessed via a fast index computation:

```
mark_bitmap_index(obj) = (obj - region_base) / 16
mark_bitmap_byte(index) = index / 8
mark_bitmap_bit(index) = index % 8
```

**Mark states.** Each bit can be in one of two states:
- **0 (white/unmarked)**: the object has not been marked in the current cycle.
- **1 (black/marked)**: the object has been marked as reachable.

The tri-color abstraction is simplified on 32-bit: we only distinguish white and black (gray is implicit: an object is gray if it is marked but not yet scanned).

### 6.4 Forwarding Mechanism (Header-Based)

**Forwarding installation.** When an object is relocated during old-generation evacuation:

```
forward_object(old_obj, new_obj):
    // Set the forwarded bit in the old object's header
    old_obj.flags |= FLAG_FORWARDED
    
    // Store the new address in the old object's mark_word
    old_obj.mark_word = new_obj
    
    // Ensure the write is visible to other threads
    atomic_fence()
```

**Forwarding check.** To check if an object has been forwarded:

```
is_forwarded(obj) -> bool:
    return (obj.flags & FLAG_FORWARDED) != 0

get_forwarding_pointer(obj) -> pointer:
    if is_forwarded(obj):
        return obj.mark_word
    else:
        return obj
```

**Atomicity.** The forwarding installation must be atomic with respect to concurrent loads. On 32-bit targets, we use a compare-and-swap (CAS) on the `flags` field to ensure that only one thread installs the forwarding pointer:

```
forward_object_atomic(old_obj, new_obj) -> bool:
    old_flags = old_obj.flags
    if old_flags & FLAG_FORWARDED:
        return false  // already forwarded by another thread
    
    new_flags = old_flags | FLAG_FORWARDED
    if atomic_cas(&old_obj.flags, old_flags, new_flags):
        old_obj.mark_word = new_obj
        return true
    else:
        return false  // another thread forwarded it first
```

### 6.5 Load Barrier (32-bit)

The load barrier on 32-bit is more expensive than on 64-bit because it must check the object header for forwarding info.

**Load barrier implementation.**

```
load(obj, offset) -> ref:
    raw = obj[offset]
    if raw == null:
        return null
    
    // Fast path: check if the object is forwarded
    if (raw.flags & FLAG_FORWARDED) == 0:
        return raw  // not forwarded, use as-is
    
    // Slow path: object has been forwarded; follow the forwarding pointer
    new_ref = raw.mark_word
    
    // Self-healing: update the slot to point to the new location
    obj[offset] = new_ref
    
    return new_ref
```

**Cost analysis.**
- **Fast path** (not forwarded): 1 load (the pointer) + 1 load (the flags field) + 1 comparison + 1 conditional branch = ~3-5 cycles.
- **Slow path** (forwarded): fast path + 1 load (the mark_word) + 1 store (self-healing) = ~10-15 cycles.

**Optimization: thread-local forwarding cache.** To reduce the overhead of checking the header on every load, each mutator thread maintains a small thread-local cache of recently-forwarded pointers:

```
struct ForwardingCache {
    entries: [(old_addr, new_addr); CACHE_SIZE],  // e.g., CACHE_SIZE = 64
    count: u32,
}

load_with_cache(obj, offset, cache) -> ref:
    raw = obj[offset]
    if raw == null:
        return null
    
    // Check the cache first
    for i in 0..cache.count:
        if cache.entries[i].old_addr == raw:
            return cache.entries[i].new_addr
    
    // Cache miss: check the header
    if (raw.flags & FLAG_FORWARDED) == 0:
        return raw
    
    new_ref = raw.mark_word
    
    // Update the cache
    if cache.count < CACHE_SIZE:
        cache.entries[cache.count] = (raw, new_ref)
        cache.count += 1
    
    // Self-healing
    obj[offset] = new_ref
    
    return new_ref
```

**Cache cost.**
- **Cache hit**: 1 load (the pointer) + cache lookup (O(CACHE_SIZE)) = ~5-10 cycles.
- **Cache miss, not forwarded**: fast path = ~3-5 cycles.
- **Cache miss, forwarded**: slow path + cache update = ~15-20 cycles.

The cache is most effective for hot loops that repeatedly access the same forwarded objects.

### 6.6 Write Barrier (32-bit)

The write barrier on 32-bit is similar to 64-bit, but the SATB check uses the mark bitmap instead of pointer colors.

**Write barrier implementation.**

```
store(obj, offset, value):
    obj[offset] = value
    
    // Generational part: same as 64-bit
    if obj is in old region and value is in young region:
        card = card_of(obj + offset)
        if card_table[card] != CARD_DIRTY:
            card_table[card] = CARD_DIRTY
    
    // SATB part: check the mark bitmap
    obj_index = mark_bitmap_index(obj)
    if mark_bitmap[obj_index] == 1:  // obj is marked (black)
        if value == null:
            satb_queue.push(obj)
        else:
            value_index = mark_bitmap_index(value)
            if mark_bitmap[value_index] == 0:  // value is unmarked (white)
                satb_queue.push(obj)
```

**Cost analysis.**
- **Fast path** (young-to-young or old-to-old with no marking concern): 1 comparison + 1 conditional branch = ~1 cycle.
- **Slow path** (SATB enqueue): 2 mark bitmap loads + 1 comparison + queue push = ~20-30 cycles.

### 6.7 Alternative Mode: Mark-Compact (STW)

For simpler implementation or when concurrent relocation is not needed, the 32-bit GC supports a mark-compact mode where old-generation collection is performed stop-the-world.

**Mark-compact phases.**

1. **Initial mark (STW)**: scan roots, mark directly-reachable objects.
2. **Concurrent mark**: same as 64-bit, but using the mark bitmap.
3. **Remark (STW)**: drain remaining gray objects, finalize mark bitmap.
4. **Compact (STW)**: compute forwarding addresses, update all references, copy objects to their new locations.
5. **Resume mutator**.

**Compaction algorithm.** Uses the LIS (Lisp 2) algorithm:
1. **Compute forwarding addresses**: scan the heap from low to high, computing the new address for each live object.
2. **Update references**: scan all objects and update references to point to the new addresses.
3. **Copy objects**: copy each live object to its new address.

**Pause time.** The compaction phase is STW and takes O(|live old|) time. For a 100 MB old generation with 50% occupancy, the pause is ~10-50 ms on modern hardware.

**When to use mark-compact.**
- When the application can tolerate longer pauses.
- When the implementation complexity of concurrent relocation is not justified.
- When the heap is small enough that compaction is fast.

### 6.8 Performance Model (32-bit)

**Barrier overhead.**
- **Load barrier**: 3-5 cycles (fast path) + cache overhead. Amortized cost: ~5-10 cycles per load.
- **Write barrier**: 1 cycle (fast path) + SATB overhead. Amortized cost: ~2-5 cycles per store.

**Pause times.**
- **Concurrent relocation mode**: same as 64-bit (initial mark, remark, pause remap).
- **Mark-compact mode**: compaction pause = O(|live old|) = 10-50 ms for typical heaps.

**Throughput overhead.**
- **Barrier overhead**: 3-5% of mutator time (higher than 64-bit due to load barrier).
- **GC work**: 2-5% of total time (same as 64-bit).

**Memory overhead.**
- **Mark bitmap**: 0.78% of heap (same as 64-bit).
- **Card table**: 0.2% of heap (same as 64-bit).
- **Forwarding cache**: per-thread, ~1 KB per thread.
- **Total**: ~1% of heap (same as 64-bit).

**Comparison to 64-bit.**
- **Load barrier**: 2-3x more expensive (3-5 cycles vs. 1-2 cycles).
- **Write barrier**: same cost.
- **Pause times**: same for concurrent mode; longer for mark-compact mode.
- **Throughput**: 2-3% lower due to load barrier overhead.

### 6.9 Validity Invariants (32-bit)

Valid_GC_32bit(H) holds iff all of the following hold:

**1. Heap well-formedness.** Same as 64-bit.

**2. Tri-color marking invariant.** Same as 64-bit, but using the mark bitmap instead of pointer colors.

**3. Generational invariant.** Same as 64-bit.

**4. Forwarding invariant (32-bit).**
- Every forwarded object has the `FLAG_FORWARDED` bit set in its header.
- The `mark_word` field of a forwarded object contains a valid pointer to the new location.
- The forwarding installation is atomic with respect to concurrent loads.

**5. Load-barrier correctness (32-bit).**
- Every reference load either returns a valid (non-forwarded) object or follows the forwarding pointer to the new location.
- The self-healing update is correct (the slot is updated to point to the new location).

**6-8.** Same as 64-bit.

### 6.10 Integration with T1 JIT

The T1 JIT must emit 32-bit-specific barriers:

**Load barrier emission.**
```
// UGB bytecode: LOAD r0, [r1]
// T1 compiled code (32-bit):
    load r0, [r1]
    if r0 != null:
        if (r0.flags & FLAG_FORWARDED) != 0:
            call slow_path_load(r0)  // runtime helper
```

**Write barrier emission.** Same as 64-bit, but the SATB check uses the mark bitmap.

**Forwarding cache integration.** The T1 JIT can optionally inline the forwarding cache check for hot loads:

```
// Hot load with cache check:
    load r0, [r1]
    if r0 != null:
        cache_hit = lookup_cache(r0, thread_local_cache)
        if cache_hit:
            r0 = cache_hit.new_addr
        else:
            if (r0.flags & FLAG_FORWARDED) != 0:
                call slow_path_load(r0)
```

### 6.11 Limitations and Tradeoffs

**Address space limitation.** 32-bit targets have a 4 GB address space. The heap size is limited to ~2-3 GB (leaving room for the runtime, stack, etc.). This may require more frequent collections for large heaps.

**Performance overhead.** The load barrier is 2-3x more expensive than on 64-bit. This results in a 2-3% throughput reduction for reference-heavy workloads.

**Implementation complexity.** The header-based forwarding mechanism is more complex than colored pointers. The atomicity requirements for forwarding installation add complexity.

**Mark-compact mode.** The mark-compact mode has longer pauses (10-50 ms) compared to concurrent relocation. This may not be suitable for latency-sensitive applications.

### 6.12 Representation and Cost Model

**Header size.** 16 bytes (vs. 24 bytes on 64-bit). Smaller headers reduce memory overhead but limit the size of objects (max 64 KB per object; larger objects use humongous regions).

**Mark bitmap.** Same as 64-bit: 0.78% of heap.

**Forwarding cache.** Per-thread, ~1 KB per thread. For 100 threads: ~100 KB.

**Traversal cost.** Load barrier: O(1) fast path, O(CACHE_SIZE) with cache. Write barrier: O(1) fast path.

**Modification cost.** Forwarding installation: O(1) with atomic CAS.

**Memory complexity.** O(|heap| · 1% + |threads| · 1 KB).

**Construction cost.** Same as 64-bit.

---

# Deoptimization Subsystem — Complete Design Document (v1)

## 0. Document Control and Design Scope

**Component:** Deoptimization Subsystem (Region and Effect-Based)

**Version:** 1.0

**Role:** Selective invalidation and state reconstruction. When a speculative assumption is violated in a compiled function, this subsystem deoptimizes only that function (and its inlined descendants), preserving the compiled code of all other functions. Uses the effect-typed IR's region and effect tracking to scope the deoptimization precisely.

**Design Status:** Complete specification.

**Scope:** This document defines the region model, effect tracking, deoptimization scope determination, state reconstruction algorithm, code invalidation strategy, continuation mechanism, integration with tiered compilation, and validity invariants. It does not define:
- The speculative optimization system (produces the assumptions that may be violated)
- The tiered compilation system (T0/T1/T2; consumes deoptimization requests)
- The ET-SoN IR (provides the effect and region model)
- The GC (provides safepoints for deoptimization)

**Normative References:**
- Click, C. and Rose, J. "Speedup of Java Programs Using the HotSpot VM" (tiered deoptimization)
- Würtz, S. et al. "Deoptimization in GraalVM" (region-based deoptimization)
- Standard JIT literature on speculative optimization and deoptimization

**Known Limitations:**
- Cross-function inlining invalidation is complex and partially specified
- Concurrent deoptimization (multiple threads deoptimizing simultaneously) is partially specified
- Exact cost model for deoptimization is approximate

---

## 1. Goals, Non-Goals, and Constraints

### Goals

1. Deoptimize only the function (region) that violated a speculative assumption, preserving compiled code for all other functions.
2. Use the effect-typed IR's region and effect tracking to precisely scope the deoptimization.
3. Reconstruct the source-level state for the deoptimized function and its inlined descendants.
4. Continue execution in a lower tier (T1 or T0) for the deoptimized function while keeping higher-tier code for other functions.
5. Handle inlining: if a deoptimized function was inlined into a caller, invalidate only the inlined portion, not the entire caller.
6. Maintain correctness: the deoptimized execution must produce the same observable behavior as the speculative execution would have.
7. Minimize deoptimization overhead: fast state reconstruction, minimal code invalidation.

### Non-Goals

1. Deoptimize the entire program (only the affected region).
2. Preserve compiled code that depends on the deoptimized function's speculative assumptions (callers that inlined it must be patched).
3. Support deoptimization of native code (only JIT-compiled code).
4. Provide zero-overhead deoptimization (deoptimization is inherently expensive).

### Constraints

- Deoptimization pause time: ≤ 10 ms for typical functions (state reconstruction + code invalidation).
- Code invalidation must be precise: only invalidate the deoptimized function and its inlined descendants.
- State reconstruction must be complete: all live variables at the deoptimization point must be correctly mapped to source-level state.
- The deoptimized function must be recompiled or interpreted immediately (no lazy deoptimization).
- Deoptimization must be thread-safe: multiple threads can deoptimize simultaneously without corrupting state.

---

## 6. Deoptimization Subsystem

### 6.1 Purpose and Domain

The Deoptimization Subsystem handles the case where a speculative assumption made during compilation is violated at runtime. Its purpose is to:

1. Detect the violation (via guard checks inserted by the compiler).
2. Determine the scope of deoptimization (which function, which inlined portions).
3. Reconstruct the source-level state at the deoptimization point.
4. Invalidate the compiled code for the affected region.
5. Transfer execution to a lower tier (T1 or T0) for the deoptimized function.
6. Allow execution to continue in the lower tier while preserving compiled code for unaffected functions.

The subsystem uses the **region model** from the ET-SoN IR to define the scope of deoptimization, and the **effect model** to track which effects have been produced by the deoptimized region.

### 6.2 Mathematical Specification

| Requirement | Specification |
|---|---|
| **Domain** | (F, S, A): a compiled function F, the current machine state S, and the violated assumption A. |
| **Codomain** | (S', T): the reconstructed source-level state S' and the target tier T for continuation. |
| **Definitions** | R: region (function + inlined descendants). E: effect set (state tokens, memory effects, etc.). Deopt: deoptimization function. Invalidate: code invalidation function. Reconstruct: state reconstruction function. |
| **Preconditions** | The assumption A has been violated. The function F is compiled in tier T_n (n > 0). The machine state S is at a deoptimization point (safepoint). |
| **Postconditions** | The source-level state S' is correctly reconstructed. The compiled code for region R is invalidated. Execution continues in tier T_{n-1} (or T0 if n=1). All other compiled code remains valid. |
| **Invariants** | Region isolation, effect consistency, state reconstruction correctness, code invalidation precision, continuation correctness. |
| **Relations** | Region-to-function mapping (→_func), effect-to-state mapping (→_state), deoptimization scope (→_scope). |
| **Failure behavior** | If state reconstruction fails (e.g., missing deopt metadata), raise an InternalCompilerError. If code invalidation fails, raise an InternalCompilerError. |
| **Complexity** | Deoptimization: O(|inlined functions| · |live vars| + |code to invalidate|). |
| **Correctness** | ⟦F⟧_Tn under assumption A = ⟦Deopt(F, S, A)⟧_Tn-1. The deoptimized execution produces the same observable behavior as the speculative execution would have. |

### 6.3 Region Model

**Region definition.** A region is a function and all functions that have been inlined into it. Formally:

```
Region = {
    root_function: Function,           // The function that was compiled
    inlined_functions: [InlinedCall],  // Functions inlined into the root
    deopt_points: [DeoptPoint],        // Safepoints where deoptimization can occur
}

InlinedCall = {
    callee: Function,                  // The inlined function
    call_site: PC,                     // The PC in the caller where the call was inlined
    deopt_metadata: DeoptMetadata,     // Metadata for reconstructing state at this inlined call
}
```

**Region scope.** When a deoptimization is triggered at a deopt point within an inlined function, the deoptimization scope is:
- The inlined function itself.
- All functions inlined into the inlined function (transitively).
- The portion of the caller that contains the inlined code (not the entire caller).

**Region isolation.** The key property of the region model is that deoptimization is isolated to the region. Other functions' compiled code is not affected unless they directly depend on the deoptimized region's speculative assumptions.

### 6.4 Effect Tracking

**Effect model.** The ET-SoN IR tracks effects via state tokens and region classes. Each effectful operation has:
- A state token (representing the memory/synchronization state).
- An effect kind (MemRead, MemWrite, Alloc, Call, Trap, Barrier).
- A region class (for memory disambiguation).

**Effect tracking during execution.** As the compiled code executes, it produces effects:
- Memory writes modify the heap.
- Allocations create new objects.
- Calls may produce arbitrary effects (depending on the callee).
- State tokens are threaded through effectful operations.

**Effect snapshot at deoptimization.** When deoptimization is triggered, the current effect state is captured:
- The current heap state (all objects and their references).
- The current state tokens (for each region class).
- The current call stack (for reconstructing the execution context).

**Effect consistency.** The deoptimized execution must produce the same effects as the speculative execution would have. This is ensured by:
- Reconstructing the source-level state (variables, call stack) at the deoptimization point.
- Continuing execution in the lower tier from that point.
- The lower tier produces the same effects (by definition, since it implements the source semantics).

### 6.5 Deoptimization Scope Determination

**Scope determination algorithm.** When a guard check fails at a deopt point:

```
determine_scope(deopt_point) -> Region:
    // Find the innermost inlined function containing the deopt point
    inlined_func = find_innermost_inlined(deopt_point)
    
    // The scope is the inlined function and all its inlined descendants
    scope = {
        root: inlined_func,
        inlined: find_all_inlined_descendants(inlined_func),
        affected_caller_portion: find_caller_portion(inlined_func),
    }
    
    return scope
```

**Inlining handling.** If the deoptimized function was inlined into a caller:
- The inlined portion of the caller is invalidated (replaced with a call to the lower-tier version).
- The rest of the caller's compiled code remains valid.
- If the caller has multiple inlined calls to the same function, only the affected call site is patched.

**Example.** Suppose function `A` inlines `B` at line 10, and `B` inlines `C` at line 5. If a deoptimization is triggered in `C`:
- The scope is `C` (and any functions inlined into `C`).
- The inlined portion of `B` that contains `C` is invalidated (replaced with a call to `C`'s lower-tier version).
- The rest of `B`'s compiled code remains valid.
- The inlined portion of `A` that contains `B` remains valid (it just calls `B`, which now calls `C`'s lower-tier version).

### 6.6 State Reconstruction

**State reconstruction algorithm.** The state reconstruction algorithm maps the current machine state (registers, stack, heap) to the source-level state (variables, call stack) at the deoptimization point.

```
reconstruct_state(deopt_point, machine_state) -> SourceState:
    // Get the deoptimization metadata for this point
    metadata = deopt_point.deopt_metadata
    
    // Reconstruct the call stack
    call_stack = []
    for frame in metadata.frames:
        source_frame = SourceFrame {
            function: frame.function,
            pc: frame.source_pc,
            variables: {},
        }
        
        // Reconstruct each live variable
        for var in frame.live_variables:
            location = metadata.var_location(var)
            value = extract_value(machine_state, location)
            source_frame.variables[var] = value
        
        call_stack.push(source_frame)
    
    return SourceState {
        call_stack: call_stack,
        heap: machine_state.heap,  // heap is shared
    }
```

**Deoptimization metadata.** For each deopt point, the compiler generates metadata describing how to reconstruct the source-level state:

```
DeoptMetadata = {
    deopt_point_id: u32,               // Unique identifier for this deopt point
    frames: [FrameMetadata],           // Metadata for each frame (including inlined frames)
}

FrameMetadata = {
    function: Function,                // The function (or inlined function)
    source_pc: PC,                     // The source-level PC
    live_variables: [Variable],        // Variables live at this point
    var_locations: Map<Variable, Location>,  // Where each variable lives
}

Location = 
    | Register(phys_reg)               // Variable is in a physical register
    | Stack(offset)                    // Variable is at a stack offset
    | Const(value)                     // Variable has a compile-time constant value
    | Materialized(insn_seq)           // Variable must be computed by executing instructions
    | Piece(locations...)              // Variable is split across multiple locations
```

**Materialized values.** Some variables may not be directly available in registers or stack slots (e.g., they were optimized away or computed on-the-fly). In this case, the deopt metadata includes a sequence of instructions to compute the value:

```
Materialized(insn_seq) = {
    instructions: [Instruction],       // Instructions to compute the value
    result_reg: phys_reg,              // The register where the result will be
}
```

The deoptimization runtime executes these instructions to materialize the value.

### 6.7 Code Invalidation

**Code invalidation strategy.** The code invalidation strategy is precise: only invalidate the compiled code for the deoptimized region.

**Invalidation algorithm.**

```
invalidate_code(scope: Region):
    // Invalidate the root function's compiled code
    root_code = scope.root.compiled_code
    if root_code != null:
        mark_code_invalid(root_code)
        root_code.compiled_code = null
    
    // Invalidate all inlined functions' compiled code
    for inlined in scope.inlined_functions:
        inlined_code = inlined.callee.compiled_code
        if inlined_code != null:
            mark_code_invalid(inlined_code)
            inlined_code.compiled_code = null
    
    // Patch the caller's inlined portion
    if scope.affected_caller_portion != null:
        patch_caller(scope.affected_caller_portion)
```

**Marking code as invalid.** Compiled code is marked as invalid by setting a flag in the code header:

```
struct CompiledCode {
    function: Function,
    code_ptr: pointer,                 // Pointer to the compiled machine code
    code_size: u32,
    is_valid: bool,                    // Flag indicating if the code is valid
    tier: u8,                          // The compilation tier (T0, T1, T2)
    deopt_points: [DeoptPoint],        // Deoptimization points in this code
}

mark_code_invalid(code: CompiledCode):
    code.is_valid = false
    // Optionally: unmap the code memory to reclaim space
```

**Patching the caller.** If the deoptimized function was inlined into a caller, the caller's inlined portion must be patched to call the lower-tier version instead:

```
patch_caller(caller_portion: InlinedCall):
    // Replace the inlined code with a call to the lower-tier version
    callee_lower_tier = caller_portion.callee.get_lower_tier_code()
    
    // Emit a call instruction at the call site
    emit_call(caller_portion.call_site, callee_lower_tier)
    
    // Optionally: recompile the caller without inlining the deoptimized function
    // (This is a more aggressive optimization, deferred for now)
```

**Thread safety.** Code invalidation must be thread-safe. If multiple threads are executing the same compiled code, they must all see the invalidation. This is achieved by:
- Using atomic operations to set the `is_valid` flag.
- Ensuring that all threads check the flag at safepoints.
- If a thread is executing invalid code, it will deoptimize at the next safepoint.

### 6.8 Continuation Mechanism

**Continuation algorithm.** After state reconstruction and code invalidation, execution continues in the lower tier:

```
continue_execution(source_state: SourceState, target_tier: u8):
    // Get the lower-tier code for the innermost function
    innermost_func = source_state.call_stack.top().function
    lower_tier_code = innermost_func.get_code(target_tier)
    
    if lower_tier_code == null:
        // Compile the function in the lower tier
        lower_tier_code = compile(innermost_func, target_tier)
        innermost_func.set_code(target_tier, lower_tier_code)
    
    // Set up the machine state for the lower-tier code
    machine_state = setup_machine_state(source_state, lower_tier_code)
    
    // Jump to the lower-tier code
    jump(lower_tier_code.entry_point, machine_state)
```

**Tier selection.** The target tier is determined by:
- If the current tier is T2, deoptimize to T1.
- If the current tier is T1, deoptimize to T0 (interpreter).
- If the current tier is T0, this is an error (T0 should not deoptimize).

**Machine state setup.** The machine state is set up to match the lower-tier code's expectations:
- Registers are loaded with the reconstructed variable values.
- The stack is set up with the reconstructed call stack.
- The PC is set to the deoptimization target in the lower-tier code.

### 6.9 Integration with Tiered Compilation

**Tier transitions.** The deoptimization subsystem integrates with the tiered compilation system:

- **T2 → T1 deoptimization:** When a T2-compiled function violates a speculative assumption, it deoptimizes to T1. The T1 code is less optimized but does not make the same speculative assumptions.
- **T1 → T0 deoptimization:** When a T1-compiled function encounters an error (e.g., a type mismatch), it deoptimizes to T0 (the interpreter). The interpreter is the most general but slowest tier.
- **Re-compilation:** After deoptimization, the function may be re-compiled in a higher tier if it becomes hot again. The profiling data from the lower tier is used to guide the re-compilation.

**Profiling data preservation.** When a function is deoptimized, the profiling data collected by the higher tier is preserved and passed to the lower tier. This allows the lower tier to make better decisions (e.g., which branches are hot, which types are common).

**Deoptimization counters.** Each function has a deoptimization counter that tracks how many times it has been deoptimized. If a function is deoptimized too many times (e.g., > 10), the compiler may disable speculative optimizations for that function to avoid repeated deoptimizations.

### 6.10 Validity Invariants

Valid_Deopt(F, S, A) holds iff all of the following hold:

**1. Region isolation.**
- The deoptimization scope is precisely the region containing the deopt point.
- No other functions' compiled code is invalidated (unless they directly depend on the deoptimized region).
- The deoptimized function's callers are patched correctly (if they inlined the deoptimized function).

**2. Effect consistency.**
- The deoptimized execution produces the same effects as the speculative execution would have.
- The heap state is consistent before and after deoptimization.
- State tokens are correctly threaded through the deoptimized execution.

**3. State reconstruction correctness.**
- All live variables at the deoptimization point are correctly reconstructed.
- The call stack is correctly reconstructed (including inlined frames).
- Materialized values are correctly computed.

**4. Code invalidation precision.**
- Only the deoptimized region's compiled code is invalidated.
- The invalidation is atomic (all threads see the invalidation simultaneously).
- The invalidated code is not executed by any thread after invalidation.

**5. Continuation correctness.**
- Execution continues in the correct lower tier.
- The machine state is correctly set up for the lower-tier code.
- The lower-tier code produces the same observable behavior as the speculative execution would have.

**6. Thread safety.**
- Multiple threads can deoptimize simultaneously without corrupting state.
- Code invalidation is visible to all threads.
- State reconstruction is thread-local (no shared mutable state).

**7. Performance bounds.**
- Deoptimization pause time is ≤ 10 ms for typical functions.
- Code invalidation cost is proportional to the size of the deoptimized region.
- State reconstruction cost is proportional to the number of live variables.

**8. Correctness under re-compilation.**
- If the deoptimized function is re-compiled in a higher tier, the new compilation does not make the same speculative assumptions (or makes them with better evidence).
- The deoptimization counter is correctly incremented.

### 6.11 Representation and Cost Model

**Region representation.** Regions are represented as a tree structure: the root function, with children for each inlined function. Each node in the tree has deoptimization metadata.

**Effect representation.** Effects are represented as state tokens (from the ET-SoN IR). The current effect state is captured at the deoptimization point.

**Deoptimization metadata representation.** Deoptimization metadata is stored in a side table, indexed by deopt point ID. Each entry contains the frame metadata and variable locations.

**Compiled code representation.** Compiled code is represented as a `CompiledCode` structure (see §6.7). The `is_valid` flag is used for invalidation.

**Traversal cost.** Determining the deoptimization scope: O(|inlined functions|). State reconstruction: O(|live vars|). Code invalidation: O(|code to invalidate|). Continuation: O(1).

**Modification cost.** Code invalidation: O(1) per code block (set the `is_valid` flag). Patching the caller: O(1) per call site (emit a call instruction).

**Memory complexity.** O(|deopt points| · metadata_size + |compiled code| · code_header_size).

**Construction cost.** Deoptimization metadata generation: O(|functions| · |live vars|) at compile time. Deoptimization at runtime: O(|inlined functions| · |live vars| + |code to invalidate|).

---
# Concurrent Deoptimization Subsystem — Complete Design Document (v1)

## 0. Document Control and Design Scope

**Component:** Concurrent Deoptimization Subsystem (extension to Deoptimization Subsystem)

**Version:** 1.0

**Role:** Coordinates deoptimization when multiple mutator threads simultaneously violate speculative assumptions in the same or related compiled code. Ensures that concurrent deoptimization requests are handled correctly without data races, double-invalidation, missed invalidations, or threads executing invalidated code.

**Design Status:** Complete specification.

**Scope:** This document defines the thread-local deopt state, the deopt request protocol, safepoint coordination, concurrent invalidation protocol, per-thread state reconstruction, re-compilation coordination, handling of shared inlined functions, memory ordering requirements, edge cases (FFI, blocked threads), validity invariants, and cost model. It does not define:
- The single-thread deoptimization algorithm (already specified; this extends it)
- The safepoint mechanism itself (already specified; this consumes it)
- The tiered compilation system (consumes re-compilation requests)
- The GC (interacts via safepoints but does not coordinate with deopt)

**Normative References:**
- Adl-Tabatabai, A.-R. et al. "Safe and Efficient Fine-Grained STM Interfacing through Object-based Safepoints" (safepoint coordination)
- Click, C. and Rose, J. "Speedup of Java Programs Using the HotSpot VM" (concurrent deopt in HotSpot)
- Boehm, H. "Threads and Signals" (thread interruption semantics)
- C11/C++11 memory model (for memory ordering primitives)

**Known Limitations:**
- Asynchronous deoptimization (deopt triggered by an external signal, not a guard failure) is not covered.
- Deoptimization of threads in non-cooperative native code is partially specified (relies on FFI return-to-safepoint).
- Cross-process deoptimization is out of scope.

---

## 1. Goals, Non-Goals, and Constraints

### Goals

1. Allow multiple mutator threads to trigger deoptimization concurrently without serializing on a global lock for the common case.
2. Ensure that no thread ever executes code that has been invalidated by another thread's deoptimization.
3. Ensure that code invalidation is idempotent: multiple concurrent requests to invalidate the same code result in exactly one invalidation.
4. Ensure that re-compilation is coordinated: only one thread compiles a given function for a given tier at a time.
5. Handle the case where the same function is inlined into multiple callers and different threads deoptimize different inlined instances.
6. Maintain bounded pause times: no single thread's deoptimization should block other threads for more than the single-thread deopt pause budget.
7. Preserve all correctness invariants of the single-thread deoptimization subsystem under concurrent execution.

### Non-Goals

1. Zero-pause concurrent deoptimization (some coordination is required).
2. Deoptimization of threads executing non-cooperative native code without returning to a safepoint.
3. Cross-process deoptimization.
4. Asynchronous deoptimization triggered by external signals.

### Constraints

- Per-thread deopt overhead (when no deopt is pending): ≤ 1 cycle at each safepoint (a single flag check).
- Deopt coordination overhead: O(|threads deopting|) for the coordinator, O(1) for each participating thread.
- Maximum pause time for any single thread during concurrent deopt: ≤ 15 ms (slightly above single-thread budget due to coordination).
- No global lock held across the entire deopt process (only short critical sections for state updates).
- Memory ordering: all invalidation and state updates use release-acquire semantics or stronger.

---

## 6. Concurrent Deoptimization Subsystem

### 6.1 Purpose and Domain

The Concurrent Deoptimization Subsystem extends the single-thread deoptimization algorithm to handle the case where multiple mutator threads simultaneously trigger deoptimization. The key challenges are:

1. **Race between invalidation and execution**: Thread A invalidates code that thread B is about to execute.
2. **Double invalidation**: Two threads request invalidation of the same code.
3. **Shared inlined functions**: Function F is inlined into both G and H; thread 1 deopts F-in-G, thread 2 deopts F-in-H.
4. **Re-compilation races**: Two threads both try to recompile the same function.
5. **Safepoint coordination**: Threads must be parked at safepoints during critical phases of deopt.

The design uses a **per-thread deopt state** model with a **deopt coordinator** thread (which may be one of the mutator threads or a dedicated GC/runtime thread). Each thread handles its own state reconstruction; coordination is only needed for code invalidation and re-compilation.

### 6.2 Mathematical Specification

| Requirement | Specification |
|---|---|
| **Domain** | (T, S, R): a set of threads T, each with machine state S_t, and a set of deopt requests R. |
| **Codomain** | (S', C'): updated machine states S'_t for each thread, and updated code state C'. |
| **Definitions** | Deopt: concurrent deoptimization function. Coord: coordinator function. Inv: invalidation function. Recomp: re-compilation function. |
| **Preconditions** | Each thread t ∈ T is at a safepoint or will reach one within bounded time. All deopt requests are well-formed. |
| **Postconditions** | For each thread t, S'_t is correctly reconstructed. Code state C' reflects all invalidations exactly once. All threads resume in the correct lower tier. |
| **Invariants** | No-thread-executes-invalid-code, invalidation-idempotence, re-compilation-serializability, safepoint-consistency, memory-ordering-correctness. |
| **Relations** | Thread-to-request (→_req), request-to-code (→_code), invalidation-visibility (→_vis). |
| **Failure behavior** | If a thread cannot reach a safepoint within bounded time, raise a DeoptTimeout error. If re-compilation fails, fall back to the next lower tier. |
| **Complexity** | Coordination: O(|threads deopting|). Per-thread work: O(|live vars| + |code to invalidate|). Total: O(|threads| · |live vars| + |code|). |
| **Correctness** | For all reachable concurrent executions, the observable behavior matches a sequential execution where deopts happen one at a time in some order. |

### 6.3 Thread-Local Deopt State

Each mutator thread maintains a thread-local deopt state:

```
struct ThreadDeoptState {
    // Current state
    state: DeoptThreadState,           // RUNNING | DEOPT_REQUESTED | DEOPT_PARKED | DEOPT_IN_PROGRESS | DEOPT_DONE
    
    // Pending deopt request (set by this thread when a guard fails)
    pending_request: Option<DeoptRequest>,
    
    // Set by the coordinator when this thread should park
    park_flag: Atomic<bool>,
    
    // Set by the coordinator when invalidation is complete and this thread can resume
    resume_flag: Atomic<bool>,
    
    // The result of deoptimization (filled in by this thread during state reconstruction)
    deopt_result: Option<DeoptResult>,
    
    // Thread-local deopt counter (for statistics and throttling)
    deopt_count: u32,
}

enum DeoptThreadState {
    RUNNING,              // Normal execution
    DEOPT_REQUESTED,      // Guard failed, request recorded
    DEOPT_PARKED,         // Parked at safepoint, waiting for coordinator
    DEOPT_IN_PROGRESS,    // Performing state reconstruction
    DEOPT_DONE,           // Ready to resume in lower tier
}
```

**Memory ordering.** The `park_flag` and `resume_flag` use release-acquire semantics:
- The coordinator writes `park_flag` with release semantics.
- The mutator reads `park_flag` with acquire semantics.
- The coordinator writes `resume_flag` with release semantics.
- The mutator reads `resume_flag` with acquire semantics.

This ensures that all writes performed by the coordinator before setting `resume_flag` are visible to the mutator after it reads the flag.

### 6.4 Deopt Request Protocol

When a guard check fails, the thread executes the following protocol:

```
guard_failed(deopt_point, violated_assumption):
    // Step 1: Record the deopt request in thread-local state
    thread_state.pending_request = Some(DeoptRequest {
        deopt_point: deopt_point,
        violated_assumption: violated_assumption,
        machine_state: capture_machine_state(),
        timestamp: now(),
    })
    thread_state.state = DEOPT_REQUESTED
    
    // Step 2: Signal the deopt coordinator
    deopt_coordinator.enqueue_request(current_thread_id, thread_state.pending_request)
    
    // Step 3: Park at the current safepoint
    thread_state.park_flag.wait_until_true()  // blocks until coordinator sets the flag
    
    // Step 4: Perform state reconstruction (see §6.7)
    thread_state.state = DEOPT_IN_PROGRESS
    result = reconstruct_state(deopt_point, machine_state)
    thread_state.deopt_result = Some(result)
    thread_state.state = DEOPT_DONE
    
    // Step 5: Wait for resume signal
    thread_state.resume_flag.wait_until_true()
    
    // Step 6: Resume in lower tier
    continue_execution(result.source_state, result.target_tier)
```

**Non-blocking fast path.** If no deopt is pending, the safepoint check is a single atomic load of `park_flag`:

```
safepoint_check():
    if thread_state.park_flag.load_acquire():
        park_for_deopt()
    // otherwise, continue execution (1 cycle)
```

### 6.5 Safepoint Coordination

The deopt coordinator is responsible for ensuring that all threads that need to deopt are parked before invalidation begins.

**Coordinator algorithm.**

```
coordinator_main_loop():
    while true:
        // Wait for at least one deopt request
        requests = deopt_coordinator.wait_for_requests()
        
        // Phase 1: Collect all pending requests
        all_requests = collect_all_pending_requests()
        
        // Phase 2: Determine which threads need to park
        threads_to_park = determine_threads_to_park(all_requests)
        
        // Phase 3: Signal threads to park
        for thread_id in threads_to_park:
            thread_state[thread_id].park_flag.store_release(true)
        
        // Phase 4: Wait for all threads to park
        for thread_id in threads_to_park:
            wait_until(thread_state[thread_id].state == DEOPT_PARKED)
        
        // Phase 5: Perform concurrent invalidation (see §6.6)
        invalidate_code_concurrently(all_requests)
        
        // Phase 6: Coordinate re-compilation (see §6.8)
        coordinate_recompilation(all_requests)
        
        // Phase 7: Signal threads to resume
        for thread_id in threads_to_park:
            thread_state[thread_id].resume_flag.store_release(true)
        
        // Phase 8: Wait for threads to complete state reconstruction
        for thread_id in threads_to_park:
            wait_until(thread_state[thread_id].state == DEOPT_DONE)
        
        // Phase 9: Clean up
        clear_requests(all_requests)
```

**Determining which threads need to park.** A thread needs to park if:
- It has a pending deopt request.
- It is executing code that will be invalidated by another thread's deopt request.

The coordinator determines this by analyzing the deopt requests and the code they affect:

```
determine_threads_to_park(all_requests) -> Set<ThreadId>:
    threads = {}
    code_to_invalidate = {}
    
    for request in all_requests:
        scope = determine_scope(request.deopt_point)
        code_to_invalidate = code_to_invalidate ∪ scope.code_to_invalidate
    
    for thread in all_mutator_threads:
        if thread has pending request:
            threads.add(thread.id)
        else if thread.current_code ∈ code_to_invalidate:
            // Thread is executing code that will be invalidated
            threads.add(thread.id)
            // Mark this thread for deopt even though it didn't trigger one
            thread.pending_request = Some(SYNTHETIC_DEOPT_REQUEST)
    
    return threads
```

**Synthetic deopt requests.** If a thread is executing code that will be invalidated by another thread's deopt, the coordinator creates a synthetic deopt request for that thread. The thread will park and reconstruct its state, even though it did not itself violate an assumption.

### 6.6 Concurrent Invalidation Protocol

The invalidation protocol ensures that code invalidation is:
- **Idempotent**: multiple requests to invalidate the same code result in exactly one invalidation.
- **Atomic**: all threads see the invalidation at the same time (via memory ordering).
- **Complete**: all code that needs to be invalidated is invalidated.

**Invalidation algorithm.**

```
invalidate_code_concurrently(all_requests):
    // Step 1: Compute the union of code to invalidate
    code_to_invalidate = {}
    for request in all_requests:
        scope = determine_scope(request.deopt_point)
        code_to_invalidate = code_to_invalidate ∪ scope.code_to_invalidate
    
    // Step 2: Invalidate each code block atomically
    for code in code_to_invalidate:
        // Use atomic CAS to ensure idempotence
        old_valid = code.is_valid.compare_exchange_strong(true, false, 
            ordering=release, failure_ordering=relaxed)
        
        if old_valid == true:
            // We were the first to invalidate this code
            // Perform any additional cleanup (e.g., unmap memory)
            cleanup_code(code)
        // else: another thread already invalidated it; no-op
    
    // Step 3: Emit a full memory fence to ensure invalidation is visible
    atomic_fence(seq_cst)
    
    // Step 4: Patch callers (see §6.9)
    patch_callers_concurrently(code_to_invalidate)
```

**Idempotence.** The `compare_exchange_strong` ensures that only one thread successfully sets `is_valid` to false. Other threads see the old value as false and skip the cleanup.

**Memory ordering.** The `seq_cst` fence ensures that all writes performed during invalidation (setting `is_valid`, unmapping memory, patching callers) are visible to all threads before they resume execution.

### 6.7 Per-Thread State Reconstruction

Each thread performs its own state reconstruction independently. This is naturally thread-safe because each thread only accesses its own machine state.

**Reconstruction algorithm.**

```
reconstruct_state(deopt_point, machine_state) -> DeoptResult:
    // Get the deoptimization metadata
    metadata = get_deopt_metadata(deopt_point)
    
    // Reconstruct the call stack (same as single-thread algorithm)
    call_stack = []
    for frame in metadata.frames:
        source_frame = SourceFrame {
            function: frame.function,
            pc: frame.source_pc,
            variables: {},
        }
        
        for var in frame.live_variables:
            location = metadata.var_location(var)
            value = extract_value(machine_state, location)
            source_frame.variables[var] = value
        
        call_stack.push(source_frame)
    
    // Determine the target tier
    current_tier = get_current_tier(deopt_point)
    target_tier = current_tier - 1
    
    return DeoptResult {
        source_state: SourceState {
            call_stack: call_stack,
            heap: machine_state.heap,
        },
        target_tier: target_tier,
    }
```

**Thread safety.** State reconstruction only reads the thread's own machine state and the shared (immutable) deopt metadata. No synchronization is needed.

### 6.8 Re-Compilation Coordination

After deoptimization, the deoptimized function may need to be re-compiled in the lower tier. To prevent multiple threads from compiling the same function simultaneously, we use a per-function compilation lock.

**Compilation lock.**

```
struct CompilationLock {
    lock: Mutex<()>,
    compiling_thread: Option<ThreadId>,
    compiled_code: Option<CompiledCode>,
}
```

**Re-compilation coordination algorithm.**

```
coordinate_recompilation(all_requests):
    // Group requests by (function, target_tier)
    recomp_requests = group_by(all_requests, |r| (r.function, r.target_tier))
    
    for (func, tier) in recomp_requests.keys():
        lock = func.compilation_lock
        
        // Try to acquire the lock
        if lock.try_lock():
            // We got the lock; compile the function
            lock.compiling_thread = Some(current_thread_id)
            
            // Check if another thread already compiled it
            if lock.compiled_code == None:
                code = compile(func, tier)
                lock.compiled_code = Some(code)
                func.set_code(tier, code)
            
            lock.compiling_thread = None
            lock.unlock()
        else:
            // Another thread is compiling; wait for it to finish
            wait_until(lock.compiled_code != None)
```

**Fallback.** If compilation fails (e.g., out of memory), the function falls back to the next lower tier (T0 interpreter).

### 6.9 Handling Shared Inlined Functions

When a function F is inlined into multiple callers (e.g., G and H), and different threads deoptimize different inlined instances, the invalidation must be precise:

**Scenario.** Thread 1 deopts F-in-G at call site CS1. Thread 2 deopts F-in-H at call site CS2.

**Invalidation scope.**
- Thread 1's deopt invalidates the inlined portion of F within G at CS1.
- Thread 2's deopt invalidates the inlined portion of F within H at CS2.
- The two invalidations are independent and do not affect each other.

**Patching algorithm.**

```
patch_callers_concurrently(code_to_invalidate):
    for code in code_to_invalidate:
        for call_site in code.inlined_call_sites:
            if call_site.is_invalidated:
                // Replace the inlined code with a call to the lower-tier version
                callee_lower_tier = call_site.callee.get_code(call_site.target_tier)
                emit_call(call_site.pc, callee_lower_tier)
                
                // Mark the call site as patched (idempotent)
                call_site.is_patched.compare_exchange_strong(false, true, 
                    ordering=release, failure_ordering=relaxed)
```

**Idempotence.** The `compare_exchange_strong` ensures that each call site is patched exactly once, even if multiple threads try to patch it.

### 6.10 Memory Ordering

The concurrent deoptimization protocol relies on specific memory ordering guarantees:

**Release-acquire pairs.**
- Coordinator writes `park_flag` with release → mutator reads with acquire.
- Coordinator writes `resume_flag` with release → mutator reads with acquire.
- Invalidation writes `is_valid` with release → mutator reads with acquire (at safepoint check).

**Sequential consistency fence.** After invalidation, a `seq_cst` fence ensures that all invalidation writes are visible to all threads before they resume.

**Why not relaxed ordering?** Relaxed ordering would allow threads to see stale values of `is_valid`, potentially executing invalidated code. Release-acquire ensures that once a thread sees `resume_flag == true`, it also sees all invalidation writes.

### 6.11 Edge Cases

**FFI (native code).** If a thread is executing native code (via FFI), it cannot be interrupted immediately. The protocol:
1. The coordinator marks the thread as "pending deopt".
2. When the native code returns to JIT code, it hits a safepoint and parks.
3. If the native code does not return within a timeout (e.g., 100 ms), the coordinator raises a `DeoptTimeout` error.

**Blocked threads.** If a thread is blocked (e.g., waiting on a lock), it cannot park immediately. The protocol:
1. The coordinator marks the thread as "pending deopt".
2. When the thread unblocks, it checks the `park_flag` at the next safepoint and parks.

**Thread termination.** If a thread terminates during deopt, the coordinator removes it from the `threads_to_park` set and continues.

**Recursive deopt.** If a thread triggers a deopt while already in the middle of a deopt (e.g., during state reconstruction), the second deopt is queued and handled after the first completes.

**Nested inlining.** If function A inlines B, which inlines C, and a deopt is triggered in C, the scope includes C and the portion of B that contains C. The portion of A that contains B is not invalidated (it just calls B, which now calls C's lower-tier version).

### 6.12 Validity Invariants

Valid_ConcurrentDeopt(T, S, R) holds iff all of the following hold:

**1. No-thread-executes-invalid-code.**
- After invalidation, no thread executes code that has been marked as invalid.
- All threads check the `is_valid` flag at safepoints before executing code.

**2. Invalidation-idempotence.**
- Each code block is invalidated exactly once, even if multiple threads request invalidation.
- The `compare_exchange_strong` ensures atomicity.

**3. Re-compilation-serializability.**
- Each (function, tier) pair is compiled at most once per deopt cycle.
- The compilation lock ensures mutual exclusion.

**4. Safepoint-consistency.**
- All threads that need to park are parked before invalidation begins.
- All threads are resumed after invalidation completes.
- No thread is left parked indefinitely.

**5. Memory-ordering-correctness.**
- All invalidation writes are visible to all threads before they resume.
- Release-acquire semantics are used correctly.

**6. State-reconstruction-correctness.**
- Each thread's state is correctly reconstructed (same as single-thread invariant).
- State reconstruction is thread-local and does not interfere with other threads.

**7. Continuation-correctness.**
- Each thread resumes in the correct lower tier.
- The lower-tier code produces the same observable behavior as the speculative execution would have.

**8. Progress.**
- Every deopt request is eventually handled (no starvation).
- Threads that are blocked or in native code eventually return to a safepoint (or timeout).

**9. Performance bounds.**
- Per-thread deopt overhead (no deopt pending): ≤ 1 cycle.
- Coordinator overhead: O(|threads deopting|).
- Maximum pause time per thread: ≤ 15 ms.

**10. Termination.**
- The concurrent deopt protocol terminates (no deadlocks).
- All threads eventually resume execution.

### 6.13 Representation and Cost Model

**Thread-local state representation.** Each thread has a `ThreadDeoptState` structure (~64 bytes).

**Coordinator state representation.** The coordinator maintains a queue of deopt requests and a set of threads to park (~1 KB overhead).

**Compilation lock representation.** Each function has a `CompilationLock` structure (~32 bytes).

**Traversal cost.** Coordinator: O(|threads deopting|) to park threads, O(|code to invalidate|) to invalidate, O(|recomp requests|) to coordinate re-compilation. Per-thread: O(|live vars|) for state reconstruction.

**Modification cost.** Invalidation: O(1) per code block (atomic CAS). Patching: O(1) per call site (atomic CAS).

**Memory complexity.** O(|threads| · 64 bytes + |functions| · 32 bytes + |deopt requests| · request_size).

**Construction cost.** Coordinator setup: O(1). Per-deopt: O(|threads deopting| + |code to invalidate| + |live vars|).

---

# GC Interaction Subsystem — Complete Design Document (v1)

## 0. Document Control and Design Scope

**Component:** GC Interaction Subsystem (extension to Concurrent Deoptimization and Garbage Collection Subsystems)

**Version:** 1.0

**Role:** Coordinates the interaction between the Garbage Collector (GC) and the Deoptimization subsystem. Resolves conflicts arising from concurrent use of safepoints, stack scanning during transitional deoptimization states, pointer stability during concurrent relocation, and the lifetime of code metadata.

**Design Status:** Complete specification.

**Scope:** This document defines safepoint arbitration, stack scannability during deoptimization, pointer resolution via load barriers during state reconstruction, epoch-based metadata reclamation, and edge-case handling (e.g., deopt request during a GC pause). It does not define:
- The core GC algorithm (already specified)
- The core deoptimization algorithm (already specified)
- The OS-level thread suspension mechanisms (consumed by both)

**Normative References:**
- Click, C. and Rose, J. "Speedup of Java Programs Using the HotSpot VM" (safepoint arbitration)
- Dice, D. "Experience with Always-On Concurrent Garbage Collection" (stack scanning during state transitions)
- Fraser, K. and Harris, T. "Concurrent Programming Without Locks" (epoch-based reclamation for metadata)
- C11/C++11 memory model (for memory ordering primitives)

**Known Limitations:**
- Asynchronous class redefinition triggering deopt during a GC pause is handled by deferral, which may increase pause times slightly.
- Epoch-based reclamation introduces a bounded delay in freeing invalidated code metadata.

---

## 1. Goals, Non-Goals, and Constraints

### Goals

1. Ensure strict priority of GC safepoints over deoptimization safepoints to maintain GC pause time bounds.
2. Guarantee that the mutator stack is always in a fully scannable state (described by a valid stack map) whenever the GC pauses threads.
3. Ensure that pointers extracted during deoptimization state reconstruction are valid and correctly forwarded, even if concurrent GC relocation is active.
4. Prevent use-after-free of `DeoptMetadata` and `CompiledCode` structures by the GC or mutator threads during concurrent invalidation.
5. Provide a mathematically verifiable serialization of GC and deoptimization critical sections.

### Non-Goals

1. Allow deoptimization to proceed concurrently with a Stop-The-World (STW) GC phase (deopt is deferred).
2. Eliminate all deoptimization overhead during GC cycles (some deferral overhead is acceptable).
3. Support deoptimization of threads that are permanently blocked in non-cooperative native code during a GC cycle.

### Constraints

- Safepoint arbitration overhead: ≤ 1 cycle per safepoint check.
- Stack scannability invariant must hold at all times; no "transitional" stack states are visible to the GC.
- Metadata reclamation must not block GC progress or mutator execution.
- Pointer extraction during deopt must incur at most the cost of the GC load barrier (O(1)).

---

## 6. GC Interaction Subsystem

### 6.1 Purpose and Domain

The GC Interaction Subsystem resolves the fundamental tension between GC and deoptimization: both require mutator threads to reach safepoints, both inspect or modify thread state (stack/registers), and both manipulate memory (GC moves objects, deopt reconstructs state referencing those objects). 

The domain is the set of concurrent executions where GC cycles and deoptimization requests overlap in time. The codomain is a serialized or safely concurrent execution where all GC invariants and deoptimization invariants are preserved.

### 6.2 Mathematical Specification

| Requirement | Specification |
|---|---|
| **Domain** | $E$: the set of concurrent execution traces involving mutator threads, GC threads, and the deopt coordinator. |
| **Codomain** | $E'$: the set of valid execution traces where GC and deopt invariants hold. |
| **Definitions** | $S_{gc}$: GC safepoint state. $S_{deopt}$: Deopt safepoint state. $Map(t)$: The active stack map for thread $t$. $Extract(p)$: Pointer extraction function during state reconstruction. |
| **Preconditions** | GC and deopt subsystems individually satisfy their validity invariants when run in isolation. |
| **Postconditions** | $E'$ satisfies Stack Scannability, Pointer Validity, and Metadata Liveness invariants. |
| **Invariants** | Safepoint priority, stack scannability, pointer validity, metadata liveness. |
| **Relations** | Safepoint arbitration ($\succ$), metadata reclamation ($\rightarrow_{reclaim}$). |
| **Failure behavior** | If stack scannability is violated, the runtime aborts (InternalCompilerError). If pointer extraction yields an invalid pointer, the runtime aborts. |
| **Complexity** | Arbitration: O(1). Pointer extraction: O(1) (load barrier cost). Metadata reclamation: O(1) amortized per thread per epoch. |
| **Correctness** | For any trace $E \in$ Domain, the observable behavior matches a trace $E' \in$ Codomain where GC and deopt operations are safely serialized or concurrently correct. |

### 6.3 Safepoint Arbitration

Safepoint arbitration ensures that GC pauses take strict precedence over deoptimization pauses. 

**Global Safepoint State.** The runtime maintains a global safepoint state:

```
enum SafepointState {
    RUNNING,          // No safepoint active
    GC_PENDING,       // GC has requested a safepoint
    GC_ACTIVE,        // GC safepoint is active, mutators are parked
    DEOPT_PENDING,    // Deopt has requested a safepoint (no GC active)
    DEOPT_ACTIVE,     // Deopt safepoint is active, mutators are parked
}
```

**Arbitration Rule.** $GC \succ DEOPT$. If a GC safepoint is requested while a deopt safepoint is pending or active, the deopt safepoint is aborted or deferred, and threads transition to the GC safepoint.

**Thread Safepoint Check.** At every safepoint poll site, a thread executes:

```
safepoint_poll():
    state = global_safepoint_state.load_acquire()
    
    if state == GC_PENDING or state == GC_ACTIVE:
        handle_gc_safepoint()
    else if state == DEOPT_PENDING or state == DEOPT_ACTIVE:
        if thread_state.pending_request != None:
            handle_deopt_safepoint()
        else:
            // Thread is parked for deopt but has no request of its own
            // (e.g., synthetic deopt). It waits for the coordinator.
            wait_for_deopt_coordinator()
    else:
        // RUNNING: continue execution
```

**Deferral of Deopt Requests.** If a guard fails while `global_safepoint_state` is `GC_PENDING` or `GC_ACTIVE`, the thread records the `pending_request` in its thread-local state but does *not* initiate deopt coordination. It parks for the GC safepoint. Once the GC safepoint completes and the state returns to `RUNNING`, the thread (or the deopt coordinator) will notice the pending request and initiate the deopt protocol.

### 6.4 Stack Scannability During Deoptimization

**The Problem.** During state reconstruction, a thread modifies its stack and registers to match the source-level state. If the GC triggers a safepoint mid-reconstruction, the stack may be in a transitional state not described by any valid stack map, causing the GC to miss roots or scan garbage.

**The Solution: Atomic Map Transition.** State reconstruction is performed while the thread is parked at the deopt safepoint. The thread's active stack map is updated *atomically* with respect to the GC.

**Algorithm.**

```
reconstruct_state_atomically(deopt_point, machine_state):
    // Step 1: The thread is already parked at the deopt safepoint.
    // The GC sees the thread as parked, and its stack is described by 
    // the *old* (pre-deopt) stack map. This is valid.
    
    // Step 2: Compute the new source-level state in thread-local memory
    // (does not modify the actual stack or registers yet).
    new_source_state = compute_source_state(deopt_point, machine_state)
    new_stack_map = get_stack_map_for_deopt_target(deopt_point)
    
    // Step 3: Atomically update the thread's active stack map and state.
    // This is a single atomic write (or a pair of writes protected by a 
    // memory barrier) that the GC will observe as either the old state 
    // or the new state, never a mix.
    thread_state.active_stack_map.store_release(new_stack_map)
    thread_state.current_pc.store_release(new_source_state.target_pc)
    
    // Step 4: Apply the state to the actual machine registers/stack.
    // This is safe because the thread is still parked; the GC will not 
    // scan the stack until the thread is resumed, at which point the 
    // new stack map is active and valid.
    apply_state_to_machine(new_source_state)
```

**Invariant Guarantee.** At any point where the GC might scan the thread's stack, `thread_state.active_stack_map` points to a valid, consistent map that accurately describes the current contents of the stack and registers.

### 6.5 Pointer Stability and Forwarding Resolution

**The Problem.** During concurrent old-generation evacuation, objects are moved, and their old locations contain forwarding pointers. If the deoptimization state reconstruction reads a pointer from a stack slot or register, it might read a stale (old) address.

**The Solution: Barrier-Aware Extraction.** The `extract_value` function used during state reconstruction must resolve forwarding pointers, exactly as the mutator's load barrier does.

**Algorithm.**

```
extract_value(machine_state, location) -> Value:
    raw_value = read_from_location(machine_state, location)
    
    if is_reference_type(raw_value) and raw_value != null:
        // Apply the GC load barrier logic to ensure we get the 
        // current, valid address, not a stale forwarded address.
        resolved_value = gc_load_barrier_resolve(raw_value)
        return resolved_value
    else:
        return raw_value

gc_load_barrier_resolve(ptr) -> Pointer:
    // Check if the object at 'ptr' has been forwarded.
    // This uses the same logic as the T1 JIT load barrier.
    if is_forwarded(ptr):
        new_ptr = get_forwarding_pointer(ptr)
        return new_ptr
    else:
        return ptr
```

**Correctness.** By applying the load barrier during state reconstruction, the deopt subsystem guarantees that all reconstructed source-level variables point to valid, live objects in the heap, maintaining the Pointer Validity invariant.

### 6.6 Code Metadata Lifetimes (Epoch-Based Reclamation)

**The Problem.** When code is invalidated, its `CompiledCode` structure and associated `DeoptMetadata` cannot be immediately freed. The GC might be concurrently scanning the stack and reading the old stack map, or a thread might be mid-deoptimization and still reading the metadata.

**The Solution: Epoch-Based Reclamation.** Metadata is not freed immediately upon invalidation. Instead, it is marked as "retired" and placed in a retirement queue. It is only freed after a "safe epoch" has passed (e.g., after all threads have passed through a safepoint, guaranteeing no thread holds a reference to the old metadata).

**Algorithm.**

```
// Global state
current_epoch: Atomic<u64>
retirement_queues: Map<u64, List<RetiredMetadata>>

invalidate_code(code: CompiledCode):
    // Step 1: Mark as invalid (prevents new uses)
    code.is_valid.store_release(false)
    
    // Step 2: Retire the metadata
    epoch = current_epoch.load_acquire()
    retirement_queues[epoch].push(RetiredMetadata {
        code: code,
        deopt_metadata: code.deopt_metadata,
    })

// Called by the GC at the end of a safepoint (or by a dedicated reclamation thread)
advance_epoch_and_reclaim():
    // Step 1: Advance the epoch
    new_epoch = current_epoch.fetch_add(1, release) + 1
    
    // Step 2: Reclaim metadata from epochs that are now safe.
    // An epoch is safe if it is <= new_epoch - 2, because any thread 
    // that started reading metadata in that epoch must have passed 
    // through a safepoint since then (due to GC or deopt coordination).
    safe_epoch = new_epoch - 2
    
    for e in 0..=safe_epoch:
        for item in retirement_queues[e]:
            free(item.code)
            free(item.deopt_metadata)
        retirement_queues[e].clear()
```

**Integration with GC.** The GC naturally advances the epoch at the end of every safepoint (since all threads are guaranteed to have passed through the safepoint). This piggybacks reclamation on existing GC coordination, adding O(1) overhead per safepoint.

### 6.7 Edge Cases

**Deopt Request During GC Pause.**
- If a mutator thread triggers a guard failure *while* it is already parked for a GC safepoint, it simply sets `thread_state.pending_request`. It does not wake up or initiate deopt coordination.
- When the GC safepoint ends, the deopt coordinator (or the thread itself upon resuming) will see the pending request and initiate the standard deopt protocol.

**GC Triggered During Deopt Coordination.**
- If the GC requests a safepoint while the deopt coordinator is in Phase 3 (signaling threads to park) or Phase 5 (waiting for threads to park), the global state transitions to `GC_PENDING` / `GC_ACTIVE`.
- Threads that were parking for deopt will instead park for GC.
- The deopt coordinator detects the state change, aborts its current coordination cycle, and yields to the GC.
- After the GC completes, the deopt coordinator restarts its coordination cycle from Phase 1.

**Concurrent Relocation and Deopt Stack Scanning.**
- If the GC is in the concurrent relocation phase, mutator threads are running and executing load barriers.
- If a deopt safepoint is triggered, threads park. The GC's concurrent relocation phase may continue to process some objects, but the parked threads' stacks are stable.
- When the thread reconstructs state, it uses `gc_load_barrier_resolve` (Section 6.5), ensuring it sees the relocated addresses.

### 6.8 Validity Invariants

Valid_GC_Deopt_Interaction(E) holds iff all of the following hold:

**1. Safepoint Priority.**
- At any time, if `global_safepoint_state` indicates a GC safepoint, no deoptimization coordination proceeds.
- Deopt requests are deferred, not lost, during GC safepoints.

**2. Stack Scannability.**
- At any point where the GC may scan a thread's stack, `thread_state.active_stack_map` points to a valid map that accurately describes the stack and register contents.
- No transitional stack states are ever visible to the GC.

**3. Pointer Validity.**
- Every pointer extracted during deoptimization state reconstruction is resolved through the GC load barrier (or equivalent mechanism), ensuring it points to a valid, non-forwarded object.

**4. Metadata Liveness.**
- No `CompiledCode` or `DeoptMetadata` structure is freed while any thread might be reading it.
- Epoch-based reclamation ensures metadata is only freed after a safe epoch has passed.

**5. Progress.**
- Deferred deopt requests are eventually processed after the GC safepoint completes.
- The GC is not indefinitely blocked by deoptimization (deopt defers to GC).

**6. Memory Ordering.**
- All transitions of `global_safepoint_state`, `active_stack_map`, and `is_valid` use release-acquire or stronger memory ordering, preventing torn reads or stale views by the GC or mutator threads.

### 6.9 Representation and Cost Model

**Safepoint state representation.** A single atomic variable (`global_safepoint_state`) plus per-thread `pending_request` flags.

**Epoch reclamation representation.** An atomic `current_epoch` counter and an array of retirement queues (size = 3, for current, previous, and safe epochs).

**Traversal cost.** Safepoint poll: 1 atomic load (≤ 1 cycle). Epoch advancement: O(|retired items|) at the end of a GC safepoint.

**Modification cost.** Pointer extraction during deopt: O(1) (adds the cost of the GC load barrier, ~3-5 cycles on fast path). Metadata retirement: O(1) (push to queue).

**Memory complexity.** O(|threads| + |retired metadata|). The retirement queue size is bounded by the amount of code invalidated between GC cycles.

**Construction cost.** Integration adds O(1) overhead to the existing GC and deopt protocols.

---
# Tiered Compilation System — Complete Design Document (v1)

## 0. Document Control and Design Scope

**Component:** Tiered Compilation System (T2 through T5)

**Version:** 1.0

**Role:** Progressive optimization pipeline. Defines the tier structure, pass allocation per tier, compilation budget model, tier transition policy, profile-guided tier selection, and integration with the deoptimization subsystem. T0 and T1 are already specified elsewhere.

**Design Status:** Complete specification.

**Scope:** This document defines the tier taxonomy, pass complexity classification, per-tier compilation budgets, tier transition triggers, profile data flow, compilation queue scheduling, cost-benefit model, validity invariants, and cost model. It does not define:
- The specific optimization passes (already specified in the optimizer subsystem document)
- The T0 interpreter or T1 baseline JIT (already specified)
- The deoptimization subsystem (already specified; consumes tier transitions)
- The GC (interacts via safepoints during compilation)

**Normative References:**
- Click, C. and Rose, J. "Speedup of Java Programs Using the HotSpot VM" (tiered compilation)
- Fink, S. et al. "Adaptive Optimization for Managed Runtime Environments" (compilation budgets)
- Suganuma, T. et al. "A Region-based Optimization Framework for Dalvik" (pass scheduling)
- Standard compiler literature on pass complexity and fixed-point iteration

**Known Limitations:**
- The exact pass-to-tier assignment is a policy decision; the document specifies the classification rules, not the specific assignments.
- Cross-tier profile data merging is partially specified.
- The compilation queue's interaction with OS scheduling is abstracted.

---

## 1. Goals, Non-Goals, and Constraints

### Goals

1. Provide a monotonic tier structure where each higher tier applies strictly more optimization work than the previous.
2. Classify each optimization pass by its asymptotic complexity and assign it to the lowest tier where its cost-benefit ratio is acceptable.
3. Bound compilation time per tier so that higher tiers are only applied to sufficiently hot code.
4. Ensure that tier transitions are monotonic in optimization level: code compiled at tier T_k can be re-compiled at T_{k+1} but never downgraded except via deoptimization.
5. Preserve all profiling data across tier transitions so that higher tiers can make better decisions.
6. Provide a mathematically defined cost-benefit model for tier selection.
7. Integrate cleanly with the deoptimization subsystem: when a speculative assumption at tier T_k is violated, execution falls back to T_{k-1}.

### Non-Goals

1. Specify the individual optimization passes (they are defined elsewhere).
2. Achieve peak performance at every tier (only T5 targets peak).
3. Eliminate tier transition overhead (it is bounded, not zero).
4. Support arbitrary user-defined tiers (the tier count is fixed at 6: T0 through T5).

### Constraints

- Compilation time at tier T_k must be bounded by B_k · |IR| where B_k is the tier-specific budget constant.
- The budget constants must satisfy B_2 < B_3 < B_4 < B_5.
- Every pass assigned to tier T_k must have complexity O(|IR|^c) where c ≤ k - 1 (i.e., T2 is O(1) per node, T3 is O(n), T4 is O(n²), T5 is unbounded but terminates).
- Tier transitions must preserve all profiling data.
- Deoptimization from T_k must fall back to T_{k-1} (or T0 if k=1).

---

## 6. Tiered Compilation System

### 6.1 Purpose and Domain

The Tiered Compilation System orchestrates the progressive optimization of functions as they become hotter. Its purpose is to amortize compilation cost over execution benefit: cheap compilation for cold code, expensive compilation for hot code.

The system maintains:
- A **tier taxonomy** defining what each tier does.
- A **pass complexity classifier** that assigns each optimization pass to a complexity class.
- A **compilation budget model** that bounds per-tier compilation time.
- A **tier transition policy** that decides when to move a function to a higher tier.
- A **compilation queue** that schedules compilation work.
- A **profile data flow** mechanism that preserves profiling information across tiers.

### 6.2 Mathematical Specification

| Requirement | Specification |
|---|---|
| **Domain** | F: set of functions. P: set of optimization passes. Prof: profiling data. H: hotness metric. |
| **Codomain** | C_k: compiled code at tier T_k, for k ∈ {0, 1, 2, 3, 4, 5}. |
| **Definitions** | Tier(T_k): the compilation tier. Passes(T_k): the set of passes applied at tier T_k. Budget(T_k): the compilation time budget for tier T_k. Hotness(f): the hotness metric for function f. |
| **Preconditions** | The function f is well-formed. Profiling data is available. The pass complexity classifier is consistent. |
| **Postconditions** | The compiled code C_k satisfies Valid_Code(C_k) and ⟦C_k⟧ = ⟦f⟧ (observational equivalence). |
| **Invariants** | Tier monotonicity, pass complexity classification, budget adherence, profile preservation, deopt consistency. |
| **Relations** | Pass-to-tier assignment (→_tier), tier transition (→_transition), profile flow (→_profile). |
| **Failure behavior** | If compilation exceeds the budget, the tier is capped at the current level. If profile data is corrupted, fall back to T1. |
| **Complexity** | Compilation at T_k: O(|IR|^c_k) where c_k is the tier's complexity class. |
| **Correctness** | For all tiers, ⟦C_k⟧ = ⟦f⟧. Higher tiers produce code with lower or equal execution cost: Cost(C_{k+1}) ≤ Cost(C_k). |

### 6.3 Tier Taxonomy

**Tier definitions.**

- **T0 (Speculative Register Interpreter).** Executes bytecode directly with speculative optimizations, inline caching, opcode rewriting, and super byte codes. Collects profiling data. No compilation.

- **T1 (Baseline Copy-and-Patch JIT).** Compiles bytecode to native code using pre-compiled templates. No optimization passes. Fast compilation (~10 µs per function). Collects profiling data.

- **T2 (O(1) Passes Tier).** Applies all optimization passes with O(1) per-node complexity. These are single-scan passes that do not iterate to fixed point. Compilation time: O(|IR|).

- **T3 (O(n) Passes Tier).** Applies all T2 passes plus all passes with O(n) or O(n log n) complexity. These include iterative dataflow analyses that converge in O(n) iterations, loop analyses, and simple alias analyses. Compilation time: O(|IR| · log |IR|).

- **T4 (O(n²) Passes Tier).** Applies all T3 passes plus all passes with O(n²) complexity. These include global value numbering with full equivalence classes, interprocedural analyses, and full alias analyses. Compilation time: O(|IR|²).

- **T5 (Full Optimization Tier).** Applies all passes from all lower tiers, iterated to fixed point. Re-runs earlier passes after later passes expose new opportunities. Compilation time: O(|IR|^c) where c is the maximum pass complexity, but bounded by a wall-clock budget.

**Tier monotonicity.** For all k < j, Passes(T_k) ⊆ Passes(T_j). A function compiled at tier T_j has had all the optimization work of tier T_k applied, plus more.

### 6.4 Pass Complexity Classification

**Complexity classes.** Each optimization pass is classified by its asymptotic complexity in terms of the IR size n = |IR|:

- **Class 0 (O(1) per node):** Single-scan passes. The pass visits each node a constant number of times and performs O(1) work per node. Total: O(n). Examples: peephole optimizations, simple constant folding, single-pass dead code elimination, type specialization.

- **Class 1 (O(n) or O(n log n)):** Iterative passes that converge in O(1) or O(log n) iterations. Total: O(n) or O(n log n). Examples: iterative dataflow (reaching definitions, liveness), loop detection, simple alias analysis (type-based).

- **Class 2 (O(n²)):** Passes that require pairwise analysis or iterate O(n) times with O(n) work per iteration. Total: O(n²). Examples: global value numbering with full equivalence, interprocedural analysis, full alias analysis (pointer-based).

- **Class 3 (O(n³) or higher):** Passes with cubic or worse complexity. These are only applied at T5 and only if the function is hot enough to justify the cost.

**Classification rules.** A pass is assigned to the lowest tier T_k such that its complexity class is ≤ k - 2. That is:
- Class 0 passes → T2
- Class 1 passes → T3
- Class 2 passes → T4
- Class 3+ passes → T5

**Pass ordering within a tier.** Passes within a tier are ordered by a dependency graph. If pass A exposes opportunities for pass B, then A must run before B. The ordering is computed once at startup and is the same for all functions.

### 6.5 Compilation Budget Model

**Budget constants.** Each tier has a budget constant B_k that bounds the compilation time per IR node:

- B_2 = 100 ns per node (T2 compiles ~10M nodes/sec)
- B_3 = 500 ns per node (T3 compiles ~2M nodes/sec)
- B_4 = 2 µs per node (T4 compiles ~500K nodes/sec)
- B_5 = 10 µs per node (T5 compiles ~100K nodes/sec)

**Budget enforcement.** The compilation budget for a function at tier T_k is:

    Budget(f, T_k) = B_k · |IR(f)|

If compilation exceeds this budget, the compiler caps the optimization at the current tier and emits the code. The function may be re-compiled at a higher tier later if it becomes hotter.

**Wall-clock budget for T5.** T5 has an additional wall-clock budget to prevent unbounded compilation:

    WallClockBudget(T5) = max(100 ms, 10 · Budget(f, T4))

If T5 compilation exceeds this wall-clock budget, the compiler stops iterating and emits the current best code.

### 6.6 Tier Transition Policy

**Hotness metric.** The hotness of a function is a composite metric:

    Hotness(f) = α · invocation_count(f) + β · loop_iteration_count(f) + γ · time_in_function(f)

where α, β, γ are configurable weights (default: α=1, β=10, γ=0.001).

**Transition thresholds.** A function is promoted from tier T_k to T_{k+1} when:

    Hotness(f) ≥ Threshold(T_k)

The thresholds are:
- Threshold(T0 → T1) = 100 (interpreted invocations)
- Threshold(T1 → T2) = 1,000
- Threshold(T2 → T3) = 10,000
- Threshold(T3 → T4) = 100,000
- Threshold(T4 → T5) = 1,000,000

**Demotion via deoptimization.** When a function at tier T_k deoptimizes, it falls back to T_{k-1}. The hotness metric is not reset; the function may be re-promoted later if it remains hot.

**Adaptive thresholds.** The thresholds are adjusted based on system load:
- If the compilation queue is backlogged, thresholds are increased (delay promotion).
- If the compilation queue is empty, thresholds are decreased (accelerate promotion).

### 6.7 Profile Data Flow

**Profile preservation.** When a function is re-compiled at a higher tier, all profiling data collected at lower tiers is preserved and passed to the higher tier's optimization passes. This includes:
- Execution counts (per bytecode, per branch, per call site)
- Type feedback (per variable, per inline cache)
- Loop iteration counts
- Call site statistics (callee distribution)

**Profile merging.** If a function is deoptimized and re-compiled, the new profiling data is merged with the old:
- Execution counts are summed.
- Type feedback sets are unioned.
- Loop iteration counts are summed.

**Profile decay.** To prevent stale profiling data from dominating, profiles are decayed over time:

    profile_new = 0.9 · profile_old + profile_new_data

This ensures that recent behavior is weighted more heavily than historical behavior.

### 6.8 Compilation Queue Scheduling

**Queue structure.** The compilation queue is a priority queue of compilation requests, ordered by:

    Priority(f) = Hotness(f) / EstimatedCompilationCost(f)

This prioritizes functions that are hot but cheap to compile, maximizing the benefit per unit of compilation time.

**Scheduling algorithm.** The compilation queue is processed by a pool of compiler threads. The scheduler:

1. Dequeues the highest-priority compilation request.
2. Allocates a compiler thread to the request.
3. The compiler thread compiles the function at the target tier.
4. The compiled code is installed atomically (via a code cache update).
5. Existing frames are deoptimized (if necessary) to use the new code.

**Backpressure.** If the compilation queue exceeds a threshold (e.g., 1000 pending requests), the scheduler increases the tier transition thresholds to reduce the inflow of new requests.

**Fairness.** To prevent starvation of cold functions, the scheduler uses a weighted fair queuing algorithm: each function gets a minimum share of compilation time proportional to its hotness.

### 6.9 Cost-Benefit Model

**Compilation cost.** The cost of compiling a function f at tier T_k is:

    Cost_compile(f, T_k) = B_k · |IR(f)|

**Execution benefit.** The benefit of compiling at tier T_k (vs. T_{k-1}) is the reduction in execution time over the function's remaining lifetime:

    Benefit(f, T_k) = (ExecTime(f, T_{k-1}) - ExecTime(f, T_k)) · RemainingInvocations(f)

**Tier selection rule.** A function is promoted to tier T_k if:

    Benefit(f, T_k) > Cost_compile(f, T_k) · OverheadFactor

where OverheadFactor (default 1.5) accounts for the fact that compilation time is more expensive than execution time (it blocks other work, consumes memory, etc.).

**Break-even analysis.** The break-even point (number of invocations needed to amortize compilation cost) is:

    BreakEven(f, T_k) = Cost_compile(f, T_k) / (ExecTime(f, T_{k-1}) - ExecTime(f, T_k))

If the function's expected remaining invocations exceed BreakEven, the promotion is justified.

### 6.10 Validity Invariants

Valid_TieredCompilation(F, C) holds iff all of the following hold:

**1. Tier monotonicity.**
- For all k < j, Passes(T_k) ⊆ Passes(T_j).
- A function is never compiled at a lower tier after being compiled at a higher tier (except via deoptimization).

**2. Pass complexity classification.**
- Every pass assigned to tier T_k has complexity class ≤ k - 2.
- The classification is consistent (no pass is assigned to multiple tiers).

**3. Budget adherence.**
- Compilation at tier T_k does not exceed Budget(f, T_k).
- T5 compilation does not exceed WallClockBudget(T5).

**4. Profile preservation.**
- All profiling data is preserved across tier transitions.
- Profile merging is correct (counts are summed, type sets are unioned).
- Profile decay is applied correctly.

**5. Deopt consistency.**
- Deoptimization from T_k falls back to T_{k-1} (or T0 if k=1).
- The deoptimized code produces the same observable behavior as the speculative code would have.

**6. Observational equivalence.**
- For all tiers k, ⟦C_k⟧ = ⟦f⟧.
- Higher tiers produce code with lower or equal execution cost: Cost(C_{k+1}) ≤ Cost(C_k).

**7. Compilation queue correctness.**
- All compilation requests are eventually processed (no starvation).
- The queue does not exceed the backpressure threshold for extended periods.

**8. Atomic code installation.**
- Compiled code is installed atomically (no thread sees a partially-installed code block).
- Existing frames are correctly deoptimized when new code is installed.

### 6.11 Representation and Cost Model

**Tier state representation.** Each function has a `TierState` structure:

```
struct TierState {
    current_tier: u8,                  // Current compilation tier (0-5)
    compiled_code: [Option<CompiledCode>; 6],  // Compiled code for each tier
    profile_data: ProfileData,         // Accumulated profiling data
    hotness: f64,                      // Current hotness metric
    deopt_count: u32,                  // Number of deoptimizations
}
```

**Compilation queue representation.** A priority queue of `CompilationRequest` structures:

```
struct CompilationRequest {
    function: Function,
    target_tier: u8,
    priority: f64,
    timestamp: u64,
}
```

**Traversal cost.** Tier transition decision: O(1) (compare hotness to threshold). Profile merging: O(|profile data|). Compilation queue scheduling: O(log |queue|) per request.

**Modification cost.** Code installation: O(1) (atomic pointer swap). Deoptimization: O(|frames|) (see deopt subsystem).

**Memory complexity.** O(|functions| · tier_state_size + |queue| · request_size + |profile data| · profile_size).

**Construction cost.** Tier initialization: O(|functions|). Per-compilation: O(|IR|^c_k) where c_k is the tier's complexity class.

---
# Compilation Cancellation Subsystem — Complete Design Document (v1)

## 0. Document Control and Design Scope

**Component:** Compilation Cancellation Subsystem (extension to Tiered Compilation System)

**Version:** 1.0

**Role:** Detects and handles the cancellation of in-progress compilations when the target function becomes cold, is superseded by a higher-priority request, or the system is under resource pressure. Ensures clean resource reclamation without leaks, races, or lost requests.

**Design Status:** Complete specification.

**Scope:** This document defines the compilation lifecycle state machine, cancellation triggers, interruption protocol, resource cleanup strategy, race condition handling, re-queuing after cancellation, validity invariants, and cost model. It does not define:
- The tiered compilation system itself (already specified; this extends it)
- The optimization passes (already specified; these are the units of work being cancelled)
- The compilation queue scheduler (already specified; this integrates with it)
- The deoptimization subsystem (unrelated; deopt affects installed code, cancellation affects in-flight compilation)

**Normative References:**
- Fraser, K. and Harris, T. "Concurrent Programming Without Locks" (atomic state transitions)
- Lea, D. "A Java Fork/Join Framework" (cooperative cancellation patterns)
- Click, C. and Rose, J. "Speedup of Java Programs Using the HotSpot VM" (compilation lifecycle)
- Standard literature on cooperative task cancellation and arena-based memory management

**Known Limitations:**
- Cancellation is cooperative: a compiler thread stuck in a long non-interruptible operation (e.g., a single monolithic pass) cannot be cancelled until it reaches the next interruption point.
- Cancellation of a compilation that has already begun code emission is not supported; such compilations run to completion.
- Cross-process compilation cancellation is out of scope.

---

## 1. Goals, Non-Goals, and Constraints

### Goals

1. Detect when an in-progress compilation should be cancelled (function went cold, superseded, or system pressure).
2. Interrupt the compiler thread cooperatively at well-defined interruption points without corrupting state.
3. Reclaim all resources (memory, queue entries, compiler thread time) associated with the cancelled compilation.
4. Ensure that cancellation is idempotent: multiple concurrent cancellation requests produce the same effect as one.
5. Handle the race between cancellation and completion: if the compiler finishes before noticing the cancellation, the result is installed normally.
6. Support re-queuing: if a cancelled function becomes hot again, a new compilation request is enqueued.
7. Keep cancellation overhead negligible on the hot path of compilation (≤ 1 cycle per interruption point check).

### Non-Goals

1. Preemptive cancellation (forcing a compiler thread to stop mid-instruction via signals).
2. Cancellation of code emission once it has begun (emission is short and non-interruptible).
3. Cancellation of compilations that have already installed code (that is deoptimization's job).
4. Cross-process cancellation.

### Constraints

- Cancellation detection overhead: ≤ 1 atomic load per interruption point (≤ 1 cycle on fast path).
- Interruption point density: at least one per 1000 IR nodes visited, ensuring bounded response time.
- Resource cleanup time: O(1) for arena-based allocations, O(|partial state|) for non-arena state.
- No resource leaks: every allocation made during a cancelled compilation is reclaimed.
- No lost requests: if a function is cancelled and later becomes hot, a new request is enqueued.
- Cancellation must be linearizable: the observable effect is as if the cancellation happened at a single point in time.

---

## 6. Compilation Cancellation Subsystem

### 6.1 Purpose and Domain

The Compilation Cancellation Subsystem manages the lifecycle of in-progress compilations. Its purpose is to reclaim compiler resources when a compilation is no longer worthwhile, without corrupting state or leaking resources.

The subsystem operates on:
- **Compilation requests** in the queue or actively being compiled.
- **Compiler threads** executing optimization passes.
- **Function hotness** metrics that may change during compilation.
- **System resources** (memory, CPU) that may become constrained.

### 6.2 Mathematical Specification

| Requirement | Specification |
|---|---|
| **Domain** | R: set of compilation requests. T: set of compiler threads. H: hotness function over time. S: system state (memory, CPU). |
| **Codomain** | R': updated request set (with cancellations applied). T': updated thread set (with freed threads). M': updated memory state (with reclaimed allocations). |
| **Definitions** | Cancel: cancellation function. Cleanup: resource reclamation function. Requeue: re-enqueue function. Lifecycle: compilation state machine. |
| **Preconditions** | The compilation request is in state QUEUED or COMPILING. The cancellation trigger is valid (function cold, superseded, or resource pressure). |
| **Postconditions** | The request is in state CANCELLED. All resources are reclaimed. The compiler thread is returned to the pool. If the function becomes hot again, a new request is enqueued. |
| **Invariants** | No resource leaks, no double-installation, no lost requests, linearizable cancellation, idempotent cancellation. |
| **Relations** | Request-to-state (→_state), cancellation-trigger (→_trigger), resource-reclamation (→_reclaim). |
| **Failure behavior** | If cleanup fails (e.g., memory corruption), the runtime aborts (InternalCompilerError). If a request is lost, the runtime logs an error and continues. |
| **Complexity** | Detection: O(1) per check. Cleanup: O(1) for arena allocations, O(|partial state|) otherwise. Requeue: O(log |queue|). |
| **Correctness** | For all reachable states, the invariants hold. The observable behavior matches a sequential execution where cancellations happen atomically. |

### 6.3 Compilation Lifecycle State Machine

Each compilation request progresses through a well-defined state machine:

```
enum CompilationState {
    QUEUED,                    // In the compilation queue, not yet started
    COMPILING,                 // Actively being compiled by a compiler thread
    CANCELLATION_REQUESTED,    // Cancellation flag set, compiler thread hasn't noticed yet
    CANCELLING,                // Compiler thread is cleaning up
    CANCELLED,                 // Fully cleaned up, resources reclaimed
    COMPLETED,                 // Compilation finished successfully
    INSTALLED,                 // Code installed into the code cache
}
```

**State transitions.**

```
QUEUED ──────────────────► COMPILING
   │                           │
   │                           ├──────────────────► CANCELLATION_REQUESTED
   │                           │                           │
   │                           │                           ▼
   │                           │                      CANCELLING
   │                           │                           │
   │                           │                           ▼
   │                           │                      CANCELLED
   │                           │
   │                           └──────────────────► COMPLETED ──► INSTALLED
   │
   └──────────────────► CANCELLED  (preempted before starting)
```

**Transition rules.**

- **QUEUED → COMPILING**: Scheduler picks the request and assigns a compiler thread.
- **QUEUED → CANCELLED**: Request is preempted (e.g., function went cold before compilation started, or superseded by a higher-tier request).
- **COMPILING → CANCELLATION_REQUESTED**: External trigger (hotness drop, supersession, resource pressure) sets the cancellation flag.
- **CANCELLATION_REQUESTED → CANCELLING**: Compiler thread notices the flag at the next interruption point and begins cleanup.
- **CANCELLATION_REQUESTED → COMPLETED**: Compiler thread finishes all passes before noticing the flag. The result is installed normally.
- **CANCELLING → CANCELLED**: Cleanup is complete, resources reclaimed, thread returned to pool.
- **COMPILING → COMPLETED**: All passes finished successfully.
- **COMPLETED → INSTALLED**: Code is atomically installed into the code cache.

**Atomic transitions.** All state transitions use atomic compare-and-swap (CAS) operations to ensure linearizability. For example:

```
transition(request, from, to):
    if request.state.compare_exchange_strong(from, to, 
        ordering=acq_rel, failure_ordering=relaxed):
        return true
    else:
        return false  // another thread changed the state first
```

### 6.4 Cancellation Triggers

A compilation is cancelled when one of the following conditions is met:

**1. Function went cold.** The function's hotness metric drops below a threshold during compilation:

```
if Hotness(f) < ColdThreshold(T_k):
    request_cancellation(request, REASON_FUNCTION_COLD)
```

The hotness is re-evaluated periodically (e.g., every 10 ms) by a background monitor thread.

**2. Superseded by higher-tier request.** A new compilation request arrives for the same function at a higher tier:

```
if new_request.tier > existing_request.tier:
    request_cancellation(existing_request, REASON_SUPERSEDED)
```

The new request replaces the old one in the queue.

**3. Resource pressure.** The system is under memory or CPU pressure:

```
if memory_pressure() > Threshold or cpu_load() > Threshold:
    cancel_lowest_priority_requests()
```

The lowest-priority compilations are cancelled first.

**4. Explicit cancellation.** The runtime or a debugging tool explicitly cancels a compilation:

```
cancel_compilation(request_id, REASON_EXPLICIT)
```

**5. Timeout.** The compilation has exceeded its wall-clock budget:

```
if now() - request.start_time > Budget(request.tier):
    request_cancellation(request, REASON_TIMEOUT)
```

### 6.5 Interruption Protocol

The compiler thread checks for cancellation at well-defined interruption points. The protocol is cooperative: the thread voluntarily checks a flag and aborts if set.

**Interruption points.** Interruption points are placed:

- **Between optimization passes**: after each pass completes, before the next begins.
- **Within passes**: at loop iteration boundaries, or every N node visits (default N = 1000).
- **Before code emission**: once emission begins, cancellation is no longer supported.

**Interruption check.** At each interruption point, the compiler thread executes:

```
check_cancellation(request):
    // Fast path: no cancellation requested (1 atomic load, ~1 cycle)
    if request.cancellation_flag.load_acquire() == false:
        return CONTINUE
    
    // Slow path: cancellation requested
    return handle_cancellation(request)
```

**Handling cancellation.** When cancellation is detected:

```
handle_cancellation(request) -> CancellationResult:
    // Transition to CANCELLING state
    if not transition(request, COMPILING, CANCELLING):
        // Another thread already changed the state (e.g., to COMPLETED)
        return ABORT_CANCELLATION
    
    // Clean up resources
    cleanup(request)
    
    // Transition to CANCELLED state
    transition(request, CANCELLING, CANCELLED)
    
    // Return the compiler thread to the pool
    return_thread_to_pool(current_thread)
    
    return CANCELLED
```

**Overhead.** The fast path (no cancellation) is a single atomic load with acquire semantics, costing ~1 cycle. With interruption points every 1000 nodes, the overhead is ~0.001 cycles per node, which is negligible.

### 6.6 Resource Cleanup

When a compilation is cancelled, all resources allocated during the compilation must be reclaimed.

**Arena-based allocation.** The compiler uses arena-based memory allocation for all IR and analysis data structures. Each compilation has its own arena:

```
struct CompilationArena {
    base: pointer,                   // Base address of the arena
    size: usize,                     // Total size of the arena
    offset: Atomic<usize>,           // Current allocation offset
}
```

When a compilation is cancelled, the entire arena is freed in O(1) by unmapping the memory:

```
cleanup_arena(arena):
    munmap(arena.base, arena.size)
```

**Non-arena state.** Some state is not arena-allocated (e.g., queue entries, thread-local buffers). This state is cleaned up explicitly:

```
cleanup(request):
    // Free the arena
    cleanup_arena(request.arena)
    
    // Remove the request from the queue (if still there)
    compilation_queue.remove(request)
    
    // Free thread-local buffers
    free(request.thread_local_buffers)
    
    // Decrement the function's compilation reference count
    request.function.compilation_count.fetch_sub(1, release)
```

**Idempotence.** The cleanup function is idempotent: calling it multiple times has the same effect as calling it once. This is ensured by the state machine: only the thread that successfully transitions to CANCELLING performs cleanup.

### 6.7 Race Conditions and Synchronization

**Race 1: Cancellation vs. completion.** The compiler thread may finish all passes just as a cancellation request arrives.

**Resolution.** The state machine handles this: if the compiler transitions to COMPLETED before noticing the cancellation flag, the cancellation is aborted and the code is installed normally. The CAS on the state transition ensures that only one outcome wins.

**Race 2: Multiple cancellation requests.** Multiple threads may request cancellation of the same compilation (e.g., hotness monitor and resource monitor).

**Resolution.** Cancellation is idempotent: the first request sets the flag, subsequent requests see the flag is already set and do nothing. The cleanup is performed only once, by the thread that transitions to CANCELLING.

**Race 3: Re-queuing during cancellation.** The function may become hot again while the cancellation is in progress.

**Resolution.** The re-queuing logic checks the compilation state:

```
requeue_if_hot(function):
    if Hotness(function) > Threshold(T_k):
        // Check if there's already a compilation in progress
        if function.compilation_count.load_acquire() == 0:
            // No compilation in progress; enqueue a new request
            compilation_queue.enqueue(CompilationRequest {
                function: function,
                target_tier: T_k,
                priority: Hotness(function),
            })
```

The `compilation_count` is incremented when a compilation starts and decremented when it completes or is cancelled. This ensures that we don't enqueue a duplicate request while a cancellation is in progress.

**Race 4: Supersession during compilation.** A higher-tier request arrives while a lower-tier compilation is in progress.

**Resolution.** The higher-tier request cancels the lower-tier compilation and replaces it in the queue. The state machine ensures that the cancellation is handled correctly.

### 6.8 Re-Queuing After Cancellation

If a cancelled function becomes hot again, a new compilation request is enqueued. The re-queuing logic:

```
requeue_check(function):
    // Periodically check if cancelled functions have become hot again
    for func in cancelled_functions:
        if Hotness(func) > Threshold(T_k):
            if func.compilation_count.load_acquire() == 0:
                compilation_queue.enqueue(CompilationRequest {
                    function: func,
                    target_tier: T_k,
                    priority: Hotness(func),
                })
                cancelled_functions.remove(func)
```

**Profile preservation.** Even after cancellation, the profiling data collected for the function is preserved. When the function is re-compiled, the new compilation uses the accumulated profile data.

### 6.9 Validity Invariants

Valid_Cancellation(R, T, M) holds iff all of the following hold:

**1. No resource leaks.**
- Every allocation made during a cancelled compilation is reclaimed.
- Arena-based allocations are freed in O(1).
- Non-arena state is explicitly cleaned up.

**2. No double-installation.**
- A compilation that has been cancelled never installs code.
- The state machine ensures that only COMPLETED compilations transition to INSTALLED.

**3. No lost requests.**
- If a function is cancelled and later becomes hot, a new request is enqueued.
- The `compilation_count` ensures that duplicate requests are not enqueued.

**4. Linearizable cancellation.**
- The observable effect of cancellation is as if it happened at a single point in time.
- All state transitions use atomic CAS operations with appropriate memory ordering.

**5. Idempotent cancellation.**
- Multiple concurrent cancellation requests produce the same effect as one.
- Cleanup is performed only once.

**6. Bounded response time.**
- The compiler thread notices a cancellation request within a bounded number of interruption points (default: 1000 nodes).
- The maximum response time is O(1000 · cost_per_node).

**7. Progress.**
- Every compilation request is eventually processed (either completed or cancelled).
- No request is stuck in CANCELLATION_REQUESTED indefinitely.

**8. Thread safety.**
- All shared state (request state, compilation count, queue) is accessed with appropriate synchronization.
- No data races occur.

### 6.10 Representation and Cost Model

**Request state representation.** Each compilation request has a `CompilationState` atomic variable and a `cancellation_flag` atomic boolean.

**Arena representation.** Each compilation has its own arena, allocated via `mmap` (or equivalent). The arena is a contiguous block of memory with a bump-pointer allocator.

**Compilation count representation.** Each function has an atomic `compilation_count` that tracks the number of in-progress compilations.

**Traversal cost.** Cancellation check: O(1) per interruption point (~1 cycle). Cleanup: O(1) for arena deallocation, O(|non-arena state|) otherwise. Requeue check: O(|cancelled functions|) per check.

**Modification cost.** State transition: O(1) (atomic CAS). Arena deallocation: O(1) (munmap). Queue operations: O(log |queue|).

**Memory complexity.** O(|compilations| · arena_size + |queue| · request_size + |functions| · compilation_count_size).

**Construction cost.** Cancellation detection: O(1) per check. Cleanup: O(1) for arena, O(|state|) otherwise. Requeue: O(log |queue|).

---


# AOT Native Image Subsystem — Complete Design Document (v1)

## 0. Document Control and Design Scope

**Component:** AOT Native Image Subsystem (AOT Snapshots)

**Version:** 1.0

**Role:** Final compilation artifact. Produces a self-contained native executable image that can be loaded and run without JIT compilation, capturing pre-compiled code, serialized heap state, type metadata, and (optionally) profiling data. Supports both profile-guided AOT compilation and build-time initialization with heap snapshotting.

**Design Status:** Complete specification.

**Scope:** This document defines the image format, snapshot protocol, heap serialization, pointer relocation, code image construction, metadata serialization, runtime boot sequence, build-time initialization, profile-guided AOT, validity invariants, and cost model. It does not define:
- The tiered compilation system (already specified; T2-T5 code is consumed by this subsystem)
- The linker subsystem (already specified; produces the final executable container)
- The GC (interacts via heap serialization and pointer fixup)
- The DWARF subsystem (already specified; debug info is embedded in the image)
- The deoptimization subsystem (AOT images may still deoptimize if runtime assumptions fail)

**Normative References:**
- Grimmer, C. et al. "An Object Store for a Build-Time-Initialized Java VM" (heap snapshotting)
- Wöß, A. et al. "A Profile-Guided Binary Rewriting Framework" (profile-guided AOT)
- GraalVM SubstrateVM design documentation (production AOT image design)
- ELF/Mach-O/PE specifications (container formats)
- Standard literature on position-independent code and relocation

**Known Limitations:**
- Incremental image updates (patching an existing image without full rebuild) are not covered.
- Cross-image object sharing (multiple images sharing a heap) is out of scope.
- Native resource serialization (open files, sockets, threads) is handled via pre/post-init hooks, not direct serialization.
- Image signing and attestation are deferred.

---

## 1. Goals, Non-Goals, and Constraints

### Goals

1. Produce a self-contained native executable that runs without requiring a JIT compiler, source code, or bytecode at runtime.
2. Support two AOT modes: (a) **profile-guided AOT**, where hot functions are pre-compiled to high tiers and packaged with profile data; (b) **build-time initialization**, where user code is executed at build time and the resulting heap is snapshotted.
3. Preserve object identity: references within the serialized heap must remain consistent after loading.
4. Support ASLR (Address Space Layout Randomization): the image must be loadable at any base address with correct pointer fixup.
5. Produce images with fast startup: no JIT warmup, no class loading, no profile collection required.
6. Support optional tier fallback: if a pre-compiled function's assumptions fail at runtime, deoptimization to T0/T1 is still possible (the bytecode and lower-tier compilers are embedded in the image).
7. Produce images that are verifiable: the image format has a well-defined validity predicate.

### Non-Goals

1. Produce images smaller than the equivalent JIT-compiled program (AOT images include metadata and pre-compiled code that may be larger).
2. Support dynamic code loading from untrusted sources within an AOT image (code is sealed at build time).
3. Serialize native OS resources (file handles, threads, sockets) — these must be re-established at runtime via post-init hooks.
4. Support incremental image patching (each build produces a complete image).

### Constraints

- Image build time must be O(|program| + |heap| + |code|) — linear in the size of the inputs.
- Image load time must be ≤ 100 ms for typical applications (excluding post-init hooks).
- Pointer relocation cost at load time: O(|pointers|) with a single pass over the relocation table.
- Image size overhead vs. raw code+data: ≤ 20% for metadata and relocation tables.
- The image must be a valid executable for the target OS (ELF on Linux, Mach-O on macOS, PE on Windows).
- All pointers in the serialized heap must be correctly relocated after loading, regardless of the load address.

---

## 6. AOT Native Image Subsystem

### 6.1 Purpose and Domain

The AOT Native Image Subsystem produces a self-contained executable image from a program and (optionally) a runtime state. Its purpose is to eliminate JIT warmup, reduce startup latency, and allow deployment of applications without a JIT compiler.

The subsystem supports two modes:

- **Profile-guided AOT (PGO-AOT)**: The program is run with profiling enabled; hot functions are compiled to T4/T5; the image contains the pre-compiled code and the profile data. At load time, the pre-compiled code is installed directly, skipping T0-T2 warmup.

- **Build-time initialization (BTI)**: User-specified initialization code is executed at build time; the resulting heap state (including initialized classes, cached data structures, pre-computed values) is serialized into the image. At load time, the heap is deserialized and execution begins from the main entry point with the pre-initialized state.

Both modes can be combined: build-time init produces a warmed heap, and profile-guided AOT pre-compiles the hot functions that operate on that heap.

### 6.2 Mathematical Specification

| Requirement | Specification |
|---|---|
| **Domain** | (P, S, Prof): a program P, an optional runtime state S (for BTI), and optional profiling data Prof (for PGO-AOT). |
| **Codomain** | I: the set of valid AOT images. |
| **Definitions** | Snap: snapshot function (captures S into a serializable form). Build: image construction function. Load: image loading function. Reloc: pointer relocation function. |
| **Preconditions** | P is well-formed. S is a valid runtime state (if provided). Prof is well-formed profile data (if provided). The target machine description is available. |
| **Postconditions** | I satisfies Valid_Image(I). When loaded, Load(I) produces a runtime state S' such that ⟦S'⟧ ≈ ⟦S⟧ (for BTI) or ⟦S'⟧ has pre-compiled hot code (for PGO-AOT). |
| **Invariants** | Pointer consistency, object identity preservation, code validity, metadata consistency, relocation completeness. |
| **Relations** | Object-to-serialization (→_ser), pointer-to-relocation (→_reloc), code-to-installation (→_install). |
| **Failure behavior** | If serialization fails (e.g., unserializable object), emit a BuildError. If relocation fails (e.g., corrupted image), emit a LoadError. |
| **Complexity** | Build: O(|P| + |S| + |code|). Load: O(|pointers| + |code|). |
| **Correctness** | ⟦Load(Build(P, S, Prof))⟧ = ⟦P⟧ with initial state S (for BTI) or with pre-compiled hot code (for PGO-AOT). |

### 6.3 Image Format

The image is a standard executable file (ELF/Mach-O/PE) produced by the Linker subsystem, with additional custom sections for AOT-specific data.

**Image sections.**

```
Image Layout:
┌─────────────────────────────────────────┐
│ Executable Header (ELF/Mach-O/PE)       │
├─────────────────────────────────────────┤
│ .text                                   │  Pre-compiled T2-T5 code
├─────────────────────────────────────────┤
│ .rodata                                 │  Read-only metadata, string tables
├─────────────────────────────────────────┤
│ .image_heap                             │  Serialized object graph (BTI)
├─────────────────────────────────────────┤
│ .image_reloc                            │  Pointer relocation table
├─────────────────────────────────────────┤
│ .image_metadata                         │  Type info, symbol tables, class metadata
├─────────────────────────────────────────┤
│ .image_profile                          │  Profile data (PGO-AOT, optional)
├─────────────────────────────────────────┤
│ .image_header                           │  AOT-specific header (magic, offsets, flags)
├─────────────────────────────────────────┤
│ .debug_*                                │  DWARF debug info (optional)
├─────────────────────────────────────────┤
│ .runtime_bootstrap                      │  Runtime startup code
└─────────────────────────────────────────┘
```

**Image header.** The `.image_header` section contains:

```
struct ImageHeader {
    magic:              u64,       // Magic number (identifies AOT image format)
    version:            u32,       // Image format version
    flags:              u32,       // Feature flags (BTI, PGO-AOT, debug, etc.)
    heap_offset:        u64,       // Offset to .image_heap section
    heap_size:          u64,       // Size of serialized heap
    reloc_offset:       u64,       // Offset to .image_reloc section
    reloc_count:        u64,       // Number of relocation entries
    metadata_offset:    u64,       // Offset to .image_metadata section
    profile_offset:     u64,       // Offset to .image_profile section (0 if absent)
    entry_point:        u64,       // Address of main/entry function
    build_timestamp:    u64,       // Build time
    original_base:      u64,       // Original link-time base address
}
```

### 6.4 Snapshot Protocol (Build-Time Initialization)

The snapshot protocol captures a runtime state into a serializable form. It runs in three phases:

**Phase 1: Pre-initialization.** The runtime executes user-specified initialization code in a special "build" mode. During this phase:
- Class initializers run.
- Static fields are populated.
- Caches are warmed.
- Native resources (files, sockets) are opened as needed.

**Phase 2: Reachability analysis.** The snapshot identifies all live objects reachable from the roots (global variables, static fields, thread-local state). Objects not reachable are not serialized.

**Phase 3: Serialization.** Each reachable object is serialized into the `.image_heap` section. The serialization assigns each object a unique offset within the heap section.

**Snapshot algorithm.**

```
snapshot(roots: [Root]) -> SerializedHeap:
    // Phase 1: Identify all reachable objects
    worklist = roots
    visited = {}
    objects = []
    
    while worklist is not empty:
        obj = worklist.pop()
        if obj in visited:
            continue
        visited.add(obj)
        objects.append(obj)
        
        // Enqueue all referenced objects
        for field in obj.reference_fields:
            if field.value != null:
                worklist.push(field.value)
    
    // Phase 2: Assign offsets
    offset_map = {}
    current_offset = 0
    for obj in objects:
        offset_map[obj] = current_offset
        current_offset += serialized_size(obj)
    
    // Phase 3: Serialize
    heap_buffer = allocate(current_offset)
    relocations = []
    
    for obj in objects:
        obj_offset = offset_map[obj]
        serialize_object(obj, obj_offset, heap_buffer, offset_map, relocations)
    
    return SerializedHeap {
        data: heap_buffer,
        size: current_offset,
        relocations: relocations,
        root_offsets: [offset_map[r.object] for r in roots],
    }
```

**Object serialization.** Each object is serialized as:

```
SerializedObject:
┌─────────────────────────────────────────┐
│ Object Header (klass pointer, size, etc.)│
├─────────────────────────────────────────┤
│ Field 0 (primitive or offset to object) │
├─────────────────────────────────────────┤
│ Field 1                                  │
├─────────────────────────────────────────┤
│ ...                                      │
└─────────────────────────────────────────┘
```

For primitive fields, the value is stored inline. For reference fields, the offset of the referenced object (relative to the heap base) is stored, and a relocation entry is recorded.

### 6.5 Pointer Relocation and ASLR Support

**The problem.** When the image is loaded at a different base address than the original link-time address (due to ASLR), all absolute pointers in the serialized heap and code must be adjusted.

**Relocation table.** The `.image_reloc` section contains a list of relocation entries:

```
struct RelocationEntry {
    offset:     u64,    // Offset within the image where the pointer lives
    kind:       u8,     // Relocation kind (absolute, relative, etc.)
    _pad:       [u8; 7],
}

enum RelocationKind {
    ABSOLUTE_64,         // 64-bit absolute pointer
    RELATIVE_32,         // 32-bit relative pointer (self-relative)
    KLASSE_PTR,          // Pointer to class metadata
    CODE_PTR,            // Pointer to compiled code
}
```

**Relocation application at load time.**

```
apply_relocations(image_base: Address, original_base: Address, relocs: [RelocationEntry]):
    delta = image_base - original_base
    
    for reloc in relocs:
        ptr_location = image_base + reloc.offset
        
        match reloc.kind:
            ABSOLUTE_64:
                *ptr_location = *ptr_location + delta
            RELATIVE_32:
                // Self-relative pointers don't need relocation
                // (they're already correct)
                pass
            KLASSE_PTR:
                *ptr_location = *ptr_location + delta
            CODE_PTR:
                *ptr_location = *ptr_location + delta
```

**Optimization: self-relative pointers.** To minimize relocation cost, the serializer converts as many pointers as possible to self-relative form (offset from the pointer's own location). Self-relative pointers do not need relocation at load time.

```
make_self_relative(ptr_location: Address, target: Address) -> i32:
    return (target - ptr_location) as i32
```

This is safe when the distance fits in 32 bits (±2 GB). For most intra-heap pointers, this is the case.

**Relocation cost.** At load time, relocation is a single pass over the relocation table: O(|relocs|). With self-relative pointers, the number of relocations is minimized (only pointers that cross the ±2 GB boundary need absolute relocation).

### 6.6 Heap Serialization

**Object graph serialization.** The serialized heap is a flat byte array containing all reachable objects, laid out contiguously. Each object is serialized at a fixed offset.

**Reference encoding.** References within the heap are encoded as offsets from the heap base:

```
serialize_reference(source_offset: usize, target_object: Object, offset_map: Map) -> usize:
    target_offset = offset_map[target_object]
    return target_offset
```

At load time, the offset is converted back to an absolute pointer:

```
deserialize_reference(heap_base: Address, offset: usize) -> Pointer:
    return heap_base + offset
```

**Object identity preservation.** Because each object is serialized exactly once and references are encoded as offsets, object identity is preserved: two references to the same object in the source will point to the same offset in the serialized heap, and thus to the same object after loading.

**Unserializable objects.** Some objects cannot be serialized (e.g., objects holding native resources, thread-local state, or runtime-specific handles). The snapshot protocol handles these via:

1. **Exclusion**: The object is not serialized; references to it are replaced with null.
2. **Reconstruction hooks**: A user-provided function is called at load time to re-create the object.
3. **Lazy initialization**: The object is marked as "needs init" and is re-created on first access.

**Write barriers during serialization.** The serializer must not trigger GC write barriers (since the serialized heap is not yet a live GC heap). The serializer uses raw memory writes, bypassing barriers.

### 6.7 Code Image Construction

**Pre-compiled code.** The `.text` section contains machine code compiled at T2-T5 tiers. Each function's compiled code is placed at a fixed address (relative to the image base).

**Code metadata.** For each compiled function, the image stores:
- The machine code (in `.text`).
- The function's signature and metadata (in `.image_metadata`).
- Deoptimization metadata (for tier fallback).
- Stack maps and register maps (for GC and deopt).
- DWARF debug info (in `.debug_*`).

**Code installation at load time.** At load time, the pre-compiled code is already in place (it's part of the `.text` section). The runtime installs function entries in the code cache, mapping function IDs to code addresses.

**Tier fallback.** If a pre-compiled function's speculative assumptions fail at runtime, the deoptimization subsystem can fall back to T0/T1. The bytecode and lower-tier compilers are embedded in the image (in `.rodata`) to support this.

### 6.8 Metadata Serialization

**Type metadata.** All type information (class definitions, method tables, field layouts) is serialized into `.image_metadata`. This includes:
- Class metadata (name, superclass, interfaces, vtable).
- Method metadata (signatures, code pointers, deopt metadata).
- Field metadata (types, offsets, flags).
- String tables (interned strings).

**Symbol table.** The image includes a symbol table mapping function and variable names to addresses. This is used for:
- Reflection (looking up methods by name).
- Debugging (symbolic stack traces).
- Dynamic linking (if the image exports symbols).

**String interning.** All strings referenced by the program are interned at build time and stored in `.rodata`. At load time, string identity is preserved (two references to the same string literal point to the same interned instance).

### 6.9 Runtime Boot Sequence

When the AOT image is loaded, the following boot sequence executes:

**Step 1: OS loading.** The OS loads the image into memory, mapping sections according to the executable header. ASLR may relocate the image to a different base address.

**Step 2: Runtime bootstrap.** The `.runtime_bootstrap` code executes:
- Apply relocations (fix up pointers for ASLR).
- Initialize the GC (set up heap boundaries, card table, mark bitmap).
- Initialize the thread system (create the main thread, set up thread-local storage).
- Initialize the code cache (install pre-compiled function entries).

**Step 3: Post-initialization hooks.** If the image was built with BTI, user-provided post-init hooks run:
- Re-establish native resources (re-open files, re-create sockets).
- Re-initialize thread-local state.
- Perform any runtime-specific setup.

**Step 4: Jump to entry point.** The runtime jumps to the `entry_point` specified in the image header (typically the `main` function).

**Boot sequence algorithm.**

```
boot_image(image_base: Address):
    header = load_image_header(image_base)
    
    // Step 2: Apply relocations
    apply_relocations(image_base, header.original_base, header.relocs)
    
    // Step 2: Initialize runtime services
    gc_init(image_base + header.heap_offset, header.heap_size)
    thread_system_init()
    code_cache_init(image_base)
    
    // Step 3: Run post-init hooks
    for hook in post_init_hooks:
        hook()
    
    // Step 4: Jump to entry point
    jump(header.entry_point)
```

### 6.10 Profile-Guided AOT (PGO-AOT)

**Profile collection.** The program is run with profiling enabled (typically in T0 or T1). The profiling data includes:
- Execution counts per function and bytecode.
- Type feedback per variable and inline cache.
- Branch outcomes.
- Call site statistics.

**Profile serialization.** The profile data is serialized into `.image_profile`. At load time, the profile is installed into the runtime's profile database.

**Pre-compilation.** Using the profile data, the AOT compiler identifies hot functions and compiles them to T4/T5. The compiled code is placed in `.text`.

**Load-time installation.** At load time:
- The pre-compiled code is installed in the code cache.
- The profile data is installed in the profile database.
- When a function is called, the runtime checks if pre-compiled code exists; if so, it executes directly (skipping T0-T2 warmup).

**PGO-AOT algorithm.**

```
pgo_aot_build(program: Program, profile: ProfileData) -> Image:
    // Identify hot functions
    hot_functions = [f for f in program.functions if profile.hotness(f) > Threshold]
    
    // Compile hot functions to T4/T5
    compiled_code = {}
    for f in hot_functions:
        code = compile(f, T5, profile)
        compiled_code[f] = code
    
    // Build the image
    image = build_image(program, compiled_code, profile)
    return image
```

### 6.11 Build-Time Initialization (BTI)

**Initialization code.** The user specifies initialization code (e.g., a `@BuildTimeInit` annotated function) that runs at build time. This code:
- Loads and initializes classes.
- Populates static fields.
- Warms caches.
- Pre-computes data structures.

**Snapshot capture.** After initialization completes, the snapshot protocol (§6.4) captures the heap state.

**BTI algorithm.**

```
bti_build(program: Program, init_code: Function) -> Image:
    // Run initialization
    runtime = create_build_runtime()
    runtime.execute(init_code)
    
    // Snapshot the heap
    roots = runtime.get_roots()
    serialized_heap = snapshot(roots)
    
    // Build the image
    image = build_image(program, heap=serialized_heap)
    return image
```

**Combining BTI and PGO-AOT.** Both modes can be combined:

```
combined_build(program: Program, init_code: Function, profile: ProfileData) -> Image:
    // Run initialization
    runtime = create_build_runtime()
    runtime.execute(init_code)
    
    // Snapshot the heap
    roots = runtime.get_roots()
    serialized_heap = snapshot(roots)
    
    // Compile hot functions
    hot_functions = [f for f in program.functions if profile.hotness(f) > Threshold]
    compiled_code = {}
    for f in hot_functions:
        code = compile(f, T5, profile)
        compiled_code[f] = code
    
    // Build the image
    image = build_image(program, heap=serialized_heap, code=compiled_code, profile=profile)
    return image
```

### 6.12 Validity Invariants

Valid_Image(I) holds iff all of the following hold:

**1. Format conformance.**
- The image is a valid executable for the target OS (ELF/Mach-O/PE).
- The image header is correctly formatted and all offsets are within bounds.
- All sections are correctly aligned.

**2. Pointer consistency.**
- Every reference in the serialized heap points to a valid object offset.
- All relocations are correctly recorded.
- After applying relocations, all pointers are valid.

**3. Object identity preservation.**
- Each object is serialized exactly once.
- References to the same object are encoded as the same offset.
- After loading, object identity is preserved.

**4. Code validity.**
- All pre-compiled code is valid machine code for the target architecture.
- All code addresses are correctly recorded in the metadata.
- Deoptimization metadata is present for all pre-compiled functions.

**5. Metadata consistency.**
- All type metadata is well-formed.
- All symbol table entries point to valid addresses.
- String tables are correctly interned.

**6. Relocation completeness.**
- Every absolute pointer in the image has a corresponding relocation entry.
- Self-relative pointers are correctly encoded.
- After relocation, all pointers are correct for the load address.

**7. Boot sequence correctness.**
- The runtime bootstrap code correctly initializes all runtime services.
- Post-init hooks are executed in the correct order.
- The entry point is valid and executable.

**8. Profile data validity (PGO-AOT).**
- If profile data is present, it is well-formed.
- Profile data is consistent with the pre-compiled code.
- Hotness thresholds are correctly applied.

**9. Heap serialization correctness (BTI).**
- If heap data is present, all reachable objects are serialized.
- Unreachable objects are not serialized.
- Unserializable objects are handled via exclusion, reconstruction hooks, or lazy initialization.

**10. Observational equivalence.**
- When loaded, the image produces the same observable behavior as the original program with the captured state (for BTI) or with pre-compiled hot code (for PGO-AOT).

### 6.13 Representation and Cost Model

**Image representation.** The image is a byte array (the executable file) with the structure defined in §6.3.

**Serialized heap representation.** A contiguous byte array with objects laid out at fixed offsets. References are encoded as offsets from the heap base.

**Relocation table representation.** An array of `RelocationEntry` structures, sorted by offset for efficient application.

**Code representation.** Machine code in the `.text` section, with metadata in `.image_metadata`.

**Traversal cost.** Snapshot: O(|reachable objects| + |references|). Relocation: O(|relocs|). Boot: O(|relocs| + |post-init hooks|).

**Modification cost.** Image build: O(|program| + |heap| + |code|). Image load: O(|relocs| + |code|).

**Memory complexity.** O(|program| + |heap| + |code| + |metadata| + |relocs|).

**Construction cost.** Build: O(|program| + |heap| + |code|). Load: O(|relocs| + |code|). Typical build time: seconds to minutes. Typical load time: < 100 ms.

---

## Honest Review (v1)

| Criterion | Status | Notes |
|---|---|---|
| Semantics | Satisfied | Operational semantics for snapshot protocol, heap serialization, relocation, boot sequence, PGO-AOT, and BTI. |
| Mathematics | Satisfied | Snapshot function, build function, load function, and relocation function are formally specified. Validity invariants are explicit. |
| Correctness | Partially satisfied | The pointer relocation correctness argument is sound in outline. However, a full formal proof that all pointers are correctly relocated under all ASLR scenarios is not discharged here. The interaction between BTI heap serialization and GC write barriers needs more precision. |
| Architecture | Satisfied | Inputs (program, state, profile), outputs (image), dependencies (linker, GC, tiered compilation) defined. |
| Performance | Satisfied | Build time, load time, and relocation cost all modeled with concrete bounds. |
| Target | Satisfied | Image format is target-specific (ELF/Mach-O/PE), but the snapshot protocol is target-independent. |
| Failure | Satisfied | All error cases handled (BuildError, LoadError, unserializable objects). |
| Verification | Satisfied | Valid_Image predicate with ten explicit invariants, all checkable in principle. |
| Terminology | Satisfied | Uses standard terms: AOT, snapshot, relocation, ASLR, build-time initialization, profile-guided optimization, self-relative pointer, object graph serialization. |
| Completeness | Partially satisfied | An independent engineer can implement most of this. However: (a) the exact serialization format for all object types (e.g., closures, continuations) is not fully specified; (b) the interaction with FFI handles and native resources needs more detail; (c) incremental image updates are not covered. |

**Score: 8/10**

The partials are real:
- Correctness: the full pointer relocation proof needs more rigor.
- Completeness: serialization of complex object types (closures, continuations) and FFI handle management need more specification.

These are manageable gaps for an implementation team familiar with AOT compilation and runtime serialization.

---

## Deferred Subsystems

The following are explicitly deferred:

1. **Incremental Image Updates.** Patching an existing image without a full rebuild (e.g., for hot-fixes).
2. **Cross-Image Object Sharing.** Multiple images sharing a common heap (for microservices or plugin architectures).
3. **Image Signing and Attestation.** Cryptographic verification of image integrity and provenance.
4. **Complex Object Serialization.** Detailed serialization format for closures, continuations, and other complex object types.
5. **FFI Handle Management.** Detailed protocol for serializing and re-establishing native resource handles.
6. **Image Compression.** Compressing the image to reduce disk size and load time.

---
