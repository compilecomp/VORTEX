

# 1. Foundational architecture

## Proposed compilation architecture

Register-based VM

Bytecode, runtime state, execution semantics

1\. VSDG

Value-centric dependency representation

2\. Sea of Nodes

Graph-based optimization and control/data dependencies

3\. Low-Level IR

Explicit control flow, operations, and machine-independent lowering

4\. Machine IR

Target-specific instructions, registers, constraints, and scheduling

Native machine code

Executable code, runtime integration, deoptimization support

This is our initial structural proposal, not a claim that this is the optimal sequence.

# 2. The four IR levels

We need to establish exactly what each representation is responsible for.

## 2.1 VSDG

Value State Dependence Graph.

Its purpose is to represent computation through value dependencies and state dependencies.

The critical distinction is that data dependencies and effects must not be conflated.

We need to define:

- Value nodes.
- State nodes.
- Memory dependencies.
- Side-effect dependencies.
- Control-dependent computation.
- Function entry and exit.
- Exceptional control flow.
- Cyclic dependencies and loop representation.
- Undefined and poison-like values, if the language semantics require them.

A major design question: whether the VSDG is strictly acyclic, uses explicit loop constructs, or permits certain cyclic structures.

We cannot leave that ambiguous.

## 2.2 Sea of Nodes

This level should expose explicit graph relationships between operations.

Potential responsibilities:

- Global value numbering.
- Common subexpression elimination.
- Constant propagation.
- Dead-code elimination.
- Escape analysis.
- Load/store optimization.
- Control-dependent simplification.
- Loop optimization.
- Speculative specialization.

But we must establish which transformations actually benefit from this representation. Merely having a graph does not make every graph optimization profitable.

## 2.3 Low-Level IR

This is where abstract computation becomes much more explicit.

We need to define:

- Control-flow blocks.
- Explicit terminators.
- SSA or non-SSA representation.
- Phi nodes or block arguments.
- Explicit memory operations.
- Calling conventions at the compiler level.
- Deoptimization points.
- Safepoints.
- Exception edges.
- Representation-specific arithmetic.

This level should make the eventual machine implementation predictable without prematurely committing to individual machine instructions.

## 2.4 Machine IR

This is the target-specific representation.

Responsibilities include:

- Physical register constraints.
- Virtual register allocation.
- Register classes.
- Instruction selection.
- Register allocation.
- Spill and reload placement.
- Instruction scheduling.
- ABI compliance.
- Machine-level control flow.
- Code layout.
- Encoding constraints.

We will need to decide the ordering of instruction selection, register allocation, and scheduling based on the target architecture rather than adopting a conventional compiler pipeline blindly.

# 3. Important architectural concern: VSDG versus Sea of Nodes

I want to investigate this before we write optimization rules.

These two representations have substantial conceptual overlap.

If both represent:

- Value dependencies.
- State dependencies.
- Memory effects.
- Control dependencies.
- Computation ordering.

Then having both may introduce unnecessary conversion costs and duplicate optimization infrastructure.

There are two possible designs:

Design A: Distinct optimization domains

- VSDG handles value/state dependence and high-level transformations.
- Sea of Nodes handles global graph optimization, control dependencies, and scheduling preparation.
- Explicit conversion rules govern the boundary.

Design B: Unified graph infrastructure

- Both representations share underlying graph infrastructure.
- Different graph disciplines and invariants apply at different compilation stages.
- Conversion is minimized.

Neither is automatically superior. We need to compare construction costs, optimization opportunities, graph traversal complexity, memory consumption, and information preservation.

I am not selecting one yet.

# 4. Decisions we must establish before T0

There are several architectural variables that fundamentally affect the lowest tiers.

| Variable       | What we need to establish                                            |
| -------------- | -------------------------------------------------------------------- |
| VM values      | Tagged values, NaN boxing, unboxed values, or another representation |
| Register model | Fixed virtual registers, dynamically allocated registers, or both    |
| Type system    | Static, dynamic, gradual, or mixed                                   |
| Memory model   | Heap layout, object model, aliasing rules                            |
| GC             | Reference counting, tracing GC, moving GC, or another collector      |
| Exceptions     | Explicit exceptional edges, runtime unwinding, or both               |
| Calls          | Direct, indirect, dynamic dispatch                                   |
| Targets        | x86-64, AArch64, or additional architectures                         |

These aren't minor implementation details. For example, the value representation directly affects arithmetic lowering, register pressure, calling conventions, specialization, and generated code size.

# 5. Proposed tier structure

I would initially investigate this arrangement:

| Tier | Purpose                             |
| ---- | ----------------------------------- |
| T0   | Interpreter                         |
| T1   | Extremely fast baseline compilation |
| T2   | Optimizing compilation              |
| T3   | Aggressive optimizing compilation   |
| T4   | Optional trace-based specialization |

Important: tracing should not necessarily be a higher tier. It can be an independent compilation path that produces specialized code and interacts with the normal tier system.

We will investigate that separately.

## 6. Our first actual design task

I propose beginning with T0 and T1, specifically:

1. VM execution semantics.
2. Bytecode/register representation.
3. Runtime value representation.
4. Baseline compilation requirements.
5. Compilation latency constraints.
6. Generated-code representation.
7. Runtime integration.
8. Tier-up mechanisms.
9. Deoptimization and recovery requirements.

We should establish the runtime contract before optimizing the compiler. Otherwise, we'll end up optimizing against assumptions that the runtime cannot satisfy.

First architectural decision: the VM's value model.

This is where I suggest we begin. Everything from arithmetic specialization to register allocation eventually runs into it.

Then Design A is our architectural direction.

Good. We're establishing the actual VM contract now. I've separated your selections from the decisions that remain open.

There are several important consequences here, particularly from combining NaN boxing, dynamically allocated VM registers, and a concurrent generational collector.
1. Architecture decision ledger

Selected
 = architectural decision supplied by you.

Unresolved
 = requires further specification.
Component       Current decision
VM values       Selected  NaN boxing
Register model  Selected  Dynamically allocated registers
Language type system    Selected  None; engine uses UGB
Memory model    Unresolved
Garbage collector       Selected  Incremental, concurrent, generational
Exceptions      Unresolved
Calls   Unresolved
Targets Clarification needed  x86-64, AArch64, R-V5

2. Consequences of the selected decisions
2.1 VM values: NaN boxing
Selected
We need to define the representation much more precisely.

NaN boxing uses the available bit patterns in IEEE-754 binary64 values to encode non-floating-point values.

The architecture must establish:
1. Which bit patterns represent ordinary floating-point numbers.
2. Which NaN patterns are reserved for tagged values.
3. How integers are represented.
4. How heap references are represented.
5. How booleans, null, undefined, and other immediate values are represented.
6. How invalid or reserved patterns are handled.
7. Whether pointers are compressed, offset-based, or directly encoded.
8. How canonicalization interacts with floating-point operations.
Particularly important: GC interaction.

The collector must reliably distinguish heap references from non-reference values.

A false positive can cause unnecessary retention. A false negative can cause memory corruption.

We therefore need an exact reference-identification invariant, not merely a convenient bit mask.

We also need to establish how concurrent collection interacts with references being modified by generated code.
2.2 Register model: dynamically allocated registers
Selected
There are two different register systems here, and we must not confuse them.
- VM registers: Logical registers addressed by UGB instructions.
- Machine registers: Physical CPU registers used by generated native code.
Your dynamically allocated register model concerns the former.

The existing UGB terminology provides a useful starting point: methods have register_count and arg_count, and virtual registers are dynamically addressed within method frames.

For the new architecture, we need to define:
- Register numbering.
- Register lifetime.
- Register initialization.
- Register reuse.
- Register frame layout.
- Argument placement.
- Return-value placement.
- Register bounds behavior.
- Register storage during deoptimization.
- Register visibility to the garbage collector.
One critical distinction:

Dynamically allocated VM registers do not imply dynamically allocated native registers.

The optimizing compiler can map VM register values into physical registers, stack slots, constants, or eliminated values.

Those mappings need not preserve VM register numbering.
2.3 Type system: none
Selected
This is an execution engine, not a language compiler with a source-language type system.

UGB provides the execution semantics.

However, we must distinguish three things:
- Source-language type information.
- Runtime value representation and tags.
- Compiler-inferred facts about values.
The first is not required by your architecture.

The second is unavoidable with NaN boxing.

The third can still be extremely useful.

For example, the optimizer might prove that a particular value is always an integer, even though the engine has no static type system.

That is a runtime/compiler fact, not a language-level type declaration.

We must preserve that distinction throughout all four IR levels.
2.4 Garbage collector
Selected
Incremental, concurrent, generational GC.

This is one of the most consequential decisions in the entire architecture.

The JIT cannot treat garbage collection as an external service.

Generated code participates in the collector's correctness protocol.

We will need to specify:
- Root identification.
- Stack maps.
- Register maps.
- Safepoint placement.
- Object allocation.
- Write barriers.
- Read barriers, if required.
- Remembered sets.
- Generational promotion.
- Concurrent marking interaction.
- Object relocation, if supported.
- Atomicity requirements.
- Mutator/collector synchronization.
There is also a particularly important question concerning generated code:

Which GC operations are permitted to execute without leaving native code, and which require runtime helpers?

That decision affects baseline compilation, optimization, register preservation, and safepoint frequency.

We should not decide it prematurely.
3. Still unresolved
These deserve independent design work.
Area    Decisions needed
Memory model    Heap organization, object layout, arrays, fields, aliasing, memory ordering
Exceptions      Exceptional CFG edges, runtime unwinding, handler lookup, deoptimization interaction
Calls   Direct calls, indirect calls, dynamic dispatch, calling conventions, inline caches

I recommend we treat exceptions and calls as separate architectural subsystems rather than burying them inside ordinary control-flow rules.
4. Target architectures
You specified:
- x86-64
- AArch64
- R-V5
I need to clarify the last one: do you mean RISC-V 64-bit (RV64)?

This matters because machine instruction selection, register allocation, ABI handling, atomic operations, and instruction scheduling must be designed around actual ISA specifications.

We should not invent an architecture called R-V5 and hope the assembler cooperates.

5. What I want to establish next
Before writing T0 optimization rules, I want to finish the VM execution contract.

My proposed order:
1. NaN-boxed value encoding.
2. UGB register semantics.
3. VM frame layout.
4. Memory and object model.
5. GC/JIT interaction.
6. Calls and exceptions.
7. Target-specific execution constraints.
Only then should we begin defining the actual T0 rules.

The next major specification should be NaN boxing. It is a foundational representation decision that affects the GC, all four IRs, register allocation, and native execution. We should define its complete bit-level encoding and invariants before building optimizations on top of it.

We'll treat VSDG and Sea of Nodes as two genuinely distinct optimization domains, not two interchangeable graph formats.

That means each must justify its existence through transformations, invariants, and capabilities that the other does not provide as effectively.

Our initial architectural contract:

1. VSDG: Owns value/state dependence and transformations that benefit from that representation.
2. Sea of Nodes: Owns graph-based optimization with explicit control and data dependencies.
3. Conversion boundary: Has formally specified input requirements, output guarantees, information-preservation rules, and computational costs.
4. No duplicate optimization: A transformation must have an explicitly assigned owner unless there is a demonstrated reason to perform it in both domains.
5. No gratuitous conversion: Conversion must provide sufficient optimization value to justify its cost.

One particularly important rule: neither representation gets to silently weaken semantic guarantees established by the previous representation.

We should eventually specify the conversion as a formal semantic-preservation obligation, not merely a graph translation.

I also want to investigate whether some optimizations should be deliberately delayed until Sea of Nodes rather than performed in VSDG. That decision needs evidence, not intuition.

Design A is selected. We can now develop its two domains independently, beginning with the exact mathematical and operational definition of VSDG.

Confirmed: RISC-V 64-bit (RV64) is our third target.

And your NaN-boxing priority is clear:

- Minimize runtime overhead.
- Minimize instruction count on hot paths.
- Avoid unnecessary decoding and encoding.
- Architecture-specific implementations are acceptable.
- Portability does not require identical internal representations.
- Correctness must survive hostile execution conditions, not merely ordinary test cases.

That last distinction matters. CPU corruption experience is useful, but we should still distinguish hardware faults, compiler miscompilation, memory corruption, and representation invariant violations. They require different defenses.

## 1. NaN boxing: design objective

I propose we establish this as the optimization objective:

Minimize the total execution cost of representing, accessing, testing, and manipulating VM values, subject to exact representation correctness and GC compatibility.

We should measure at least:

1. Tag extraction cost.
2. Type-check cost.
3. Integer extraction cost.
4. Floating-point extraction cost.
5. Pointer extraction cost.
6. Value construction cost.
7. Branches introduced by decoding.
8. Register pressure.
9. Memory traffic.
10. Interaction with GC barriers.

A representation that saves one instruction during construction but adds two instructions to every type check is probably a bad trade. The hot-path frequency matters.

# 2.1.1 Important distinction: VM NaN boxing versus hardware NaN boxing

These are not the same thing.

RISC-V's floating-point ISA already specifies NaN boxing for narrower floating-point values held in wider floating-point registers. For example, a valid 32-bit value in a 64-bit floating-point register has all upper 32 bits set to one.&#x20;

[image](https://www.google.com/s2/favicons?domain=https://docs.riscv.org\&sz=32)

RISC-V Documentation+1



That is useful hardware behavior, but it does not automatically give us an efficient tagged VM value representation.

We must design our VM encoding independently.

# 2.1.2 Proposed representation requirements

I want to investigate a 64-bit tagged representation first.

| Property                       | Requirement                                    |
| ------------------------------ | ---------------------------------------------- |
| Value width                    | 64 bits                                        |
| Ordinary floating-point values | Direct binary64 representation where permitted |
| Integer values                 | Immediate representation                       |
| Booleans                       | Immediate representation                       |
| Null/undefined                 | Immediate representation                       |
| Heap references                | Direct or encoded references                   |
| Tag extraction                 | Minimum practical instruction cost             |
| Construction                   | Minimum practical instruction cost             |
| Invalid encodings              | Precisely defined behavior                     |
| GC identification              | Exact reference classification                 |

One important trade-off: reserving NaN bit patterns for tags means we potentially sacrifice some IEEE-754 NaN payload representations.

We need to decide whether preserving arbitrary NaN payloads is a semantic requirement.

# 2.1.3 Architecture-specific representations

I agree that maintaining different encoding versions is acceptable.

I would actually recommend that we avoid forcing one universal physical encoding across all targets.

Target-specific value ABI

x86-64

Encoding selected for x86-64 addressing constraints, integer instructions, and register allocation.

AArch64

Encoding selected with its address-space configuration, pointer features, and instruction capabilities in mind.

RV64

Encoding selected around the actual supported ISA extensions, address-space configuration, and integer/floating-point execution facilities.

All three must implement the same abstract VM value semantics.

The physical representation can differ.

# 2.1.4 Corruption resistance

I want three distinct operating policies:

- Production: No unnecessary validation on proven-safe hot paths.
- Debug: Aggressive representation invariant checks.
- Diagnostic: Detect malformed tags, invalid references, and violated runtime contracts.

One warning: we cannot assume that every malformed 64-bit value is recoverable. In particular, a corrupted heap reference must never be allowed to become an unchecked memory access.

We should also distinguish hardware error detection from software representation validation.

# 2.1.5 First decision: NaN semantics

This is the first decision that materially changes the available encoding space.

A. How should NaN values behave?

Canonicalize NaNs

All NaN results use a canonical representation. Maximizes freedom for tagging.

Preserve NaN payloads

Preserve payload information wherever possible. Restricts available tag space.

Preserve payloads selectively

Preserve them where practical, with explicitly defined canonicalization cases.

B. Integer representation

Immediate tagged integers

Heap-boxed integers

Immediate integers with overflow fallback

Commit these decisions

I would investigate canonicalized NaNs and immediate integers first, but I am not treating either as selected.

We need the bit-level mathematics before declaring this representation optimal.

# NV-1: NaN-Boxed VM Value Representation



Architectural specification in development

Bit allocation not yet frozen



Selected decisions

| Property               | Decision                                  |
| ---------------------- | ----------------------------------------- |
| VM word width          | 64 bits                                   |
| Floating-point format  | IEEE-754 binary64                         |
| NaN semantics          | Partial payload preservation              |
| Integer representation | Immediate                                 |
| VM architecture        | Register-based                            |
| GC                     | Incremental, concurrent, generational     |
| Targets                | x86-64, AArch64, RV64                     |
| Encoding portability   | Architecture-specific encodings permitted |

Our objective is not to make NaN boxing elegant. It is to make the common operations cheap.

There is a critical distinction in the design: we need to optimize the representation of values, not just the cost of extracting a tag. The encoding must also make allocation, GC scanning, register spilling, arithmetic, and deoptimization efficient.

# 1. Mathematical foundation

IEEE-754 binary64 has this layout:

64-bit binary64 representation

1 bit

Sign

11 bits

Exponent

52 bits

Fraction

Bit 63Bit 0

The exponent is 11 bits, and the fraction is 52 bits.

A binary64 NaN has:

\\[ E=2047=2^{11}-1 \\]

and

\\[ F\neq 0 \\]

Therefore, the set of NaN bit patterns has cardinality:

\\[ N\_{\mathrm{NaN}}=2(2^{52}-1) \\]

\\[ \boxed{N\_{\mathrm{NaN}}=2^{53}-2} \\]

This is the theoretical space from which we can reserve encodings.

However, not all these patterns are equally useful.

We need to consider:

- Quiet versus signaling NaNs.
- Positive versus negative NaNs.
- Hardware canonicalization.
- Floating-point operations that produce NaNs.
- NaN payload propagation.
- Interoperability with native floating-point code.

Important: We cannot simply assume that every NaN payload survives an arithmetic operation. IEEE-754 does not promise universal payload propagation across all operations and hardware implementations.

That is why partial preservation is a reasonable direction.

# 2. Bit allocation proposal

Here is the first concrete encoding candidate.

We reserve three of the eight possible negative quiet-NaN prefix patterns.

The 16-bit prefix determines the value category. The remaining 48 bits carry its payload.

## Proposed 64-bit encoding

16 bits

Category tag

48 bits

Payload

Bits 63–48 | Bits 47–0

| Prefix            | Meaning                     |
| ----------------- | --------------------------- |
| `0xFFF8`          | Immediate integer           |
| `0xFFF9`          | Heap reference              |
| `0xFFFA`          | Special immediate           |
| `0xFFFB`–`0xFFFF` | Floating-point NaN patterns |
| Other patterns    | Floating-point values       |

This has an interesting property: we don't need a separate tag field inside the payload.

The entire 48-bit payload remains available to each category.

## 2.1 Integer encoding

The integer payload is signed two's-complement, 48-bit.

Its range is:

\\[ -2^{47}\leq n\leq 2^{47}-1 \\]

Therefore:

\\[ \boxed{-140737488355328\leq n\leq140737488355327} \\]

This is a 48-bit signed immediate integer.

The payload is extracted and sign-extended to 64 bits.

Important limitation: This does not represent every possible signed 64-bit integer.

If UGB requires exact signed 64-bit integer semantics, we need an overflow representation. That could be a boxed extended integer, a multiword representation, or another explicitly defined mechanism.

There is no way to injectively encode every 64-bit integer, every binary64 value, and additional reference categories into the same 64-bit word. The cardinality constraints prohibit it.

We must not quietly change integer semantics to accommodate the representation.

## 2.2 Heap reference encoding

The proposed reference payload is 48 bits.

I would investigate heap-relative offsets rather than directly embedding native pointers.

\\[ address=heap\\\_base+offset \\]

With a byte-addressed 48-bit offset:

\\[ 0\leq offset<2^{48} \\]

That gives a theoretical heap address window of 256 TiB per heap base.

Advantages:

- No dependence on native pointer canonicality.
- No dependence on a particular virtual-address width.
- Easier relocation of the heap base.
- Stable representation across different machine architectures.
- Potentially simpler GC reference handling.

However, this introduces address reconstruction and heap-base management.

We need to measure that cost against direct pointer encoding.

We also need to define whether offset zero is reserved, whether multiple heaps are supported, and whether references can escape the heap.

## 2.3 Special immediates

The `0xFFFA` category has a 48-bit payload.

It can encode:

| Payload | Meaning                |
| ------- | ---------------------- |
| 0       | Null                   |
| 1       | Undefined              |
| 2       | False                  |
| 3       | True                   |
| 4       | Uninitialized          |
| 5       | Internal invalid value |

Additional special values can be assigned later.

This avoids consuming separate 16-bit category prefixes for every singleton value.

The uninitialized and invalid states must remain distinct from user-visible values.

# 3. What NaN payload preservation actually means

Our candidate reserves:

- All three selected negative quiet-NaN prefix groups for VM tags.
- The remaining five negative quiet-NaN groups for floating-point NaNs.

Consequently, it preserves:

- All positive NaN bit patterns.
- All negative signaling-NaN bit patterns.
- Five eighths of negative quiet-NaN bit patterns.

Partial preservation

This is a precise definition of partial preservation, rather than an undefined promise to preserve NaNs whenever convenient.

There is one necessary rule:

A floating-point NaN whose bit pattern collides with a reserved VM tag must be transformed before being stored as a VM value.

The default candidate is canonicalization to:

\\[ \texttt{0x7FF8000000000000} \\]

That is a positive quiet NaN outside our reserved region.

We should preserve non-colliding payloads during bitwise transport, loads, stores, and value copies.

Arithmetic is a separate issue.

RISC-V's standard floating-point arithmetic produces canonical NaNs rather than guaranteeing payload propagation. Its floating-point transfers preserve NaN payload bits.&#x20;

[image](https://www.google.com/s2/favicons?domain=https://docs.riscv.org\&sz=32)

RISC-V Documentation+1



That means our VM cannot assume arithmetic NaN propagation is identical across x86-64, AArch64, and RV64.

We need an explicit VM-level contract for this.

# 4. Revised prefix selection

I would actually revise the initial prefix assignment before freezing it.

The reason is that we should leave common hardware-generated NaN patterns available to floating-point values where possible.

A particularly useful pattern to leave unreserved is:

\\[ \texttt{0xFFF8000000000000} \\]

This is a commonly encountered negative quiet-NaN indefinite value on x86 implementations.

Our revised proposal reserves the upper half of negative quiet-NaN prefix space instead.

Revised encoding

| Prefix            | Category                      |
| ----------------- | ----------------------------- |
| `0xFFFC`          | Integer                       |
| `0xFFFD`          | Heap reference                |
| `0xFFFE`          | Special immediate             |
| `0xFFFF`          | Invalid/reserved              |
| `0xFFF8`–`0xFFFB` | Preserved floating-point NaNs |

This gives us four contiguous tag categories and leaves the lower four negative quiet-NaN prefix groups available.

The exact hardware NaN patterns must still be tested on every supported target.

## 4.1 Why four contiguous prefixes?

The four prefixes share their upper 14 bits.

This permits a unified tag-region test:

\\[ (w\mathbin{\&}\texttt{0xFFFC000000000000}) = \texttt{0xFFFC000000000000} \\]

The category is then:

\\[ tag=(w\gg48)\mathbin{\&}3 \\]

And the payload:

\\[ payload=w\mathbin{\&}(2^{48}-1) \\]

This gives us:

- One common tag-region predicate.
- Two bits for category selection.
- A full 48-bit payload.
- One reserved invalid category.

This is an attractive encoding structure, but it is not yet a proven instruction-count optimum. Constant materialization, register availability, and target-specific instruction selection still matter.

# 5. Floating-point operation requirements

We need to separate four operations.

| Operation                 | Required behavior                              |
| ------------------------- | ---------------------------------------------- |
| Bitwise copy              | Preserve exact permitted NaN bits              |
| Load/store                | Preserve exact permitted NaN bits              |
| Floating-point arithmetic | Apply the VM's defined NaN result policy       |
| Conversion to VM value    | Reject or canonicalize reserved tag collisions |

## 5.1 Collision handling

Suppose a native floating-point operation produces a bit pattern belonging to the reserved VM region.

We cannot interpret that result as an integer or pointer.

The conversion boundary must normalize it.

The proposed normalization is:

\\[ NaN\_{\mathrm{collision}} \rightarrow \texttt{0x7FF8000000000000} \\]

This is safe with respect to our chosen tag region.

However, there is an optimization opportunity worth investigating: avoiding unnecessary normalization for operations whose possible outputs are already proven not to collide.

That would require operation-specific reasoning and target-specific guarantees.

We should not put an unconditional normalization sequence after every floating-point operation without first examining those guarantees.

## 5.2 NaN semantics across architectures

There are two possible contracts:

A. Target-native propagation

NaN payload propagation follows the underlying hardware behavior, subject to reserved-pattern normalization.

B. VM-defined propagation

The VM defines a consistent result policy independent of hardware.

I lean toward B if cross-platform reproducibility matters, but we haven't established that requirement.

This is an architectural decision, not a minor implementation detail.

# 6. Register representation and native execution

This is where I want to avoid a potentially expensive mistake.

A 64-bit NaN-boxed VM value does not necessarily need to occupy one general-purpose native register throughout execution.

The compiler should be permitted to use different physical representations internally.

For example:

| VM value              | Possible native representation        |
| --------------------- | ------------------------------------- |
| Tagged integer        | General-purpose register              |
| Heap reference        | General-purpose register              |
| Boolean               | Immediate or general-purpose register |
| Null                  | Constant                              |
| Binary64 number       | Floating-point register               |
| Compile-time constant | Immediate or constant materialization |

This is especially important for floating-point-heavy workloads.

If every floating-point operation requires:

1. Extracting a VM word.
2. Moving it to an FPU register.
3. Performing arithmetic.
4. Moving it back.
5. Checking its tag region.

Then we may have created a representation whose overhead dominates the actual computation.

Instead, we should investigate keeping unboxed floating-point values in FPU registers across multiple operations.

The VM's canonical storage representation remains 64-bit NaN-boxed, but the compiler can use temporary unboxed representations.

This introduces a requirement for precise representation tracking in the IR and register allocator.

# 7. GC compatibility

Our concurrent generational collector imposes additional requirements.

The reference category is:

\\[ tag=01_2 \\]

under the proposed category assignment.

A heap reference must be recognizable without examining arbitrary object contents.

However, this does not mean the collector should conservatively scan every machine register and interpret every matching 64-bit pattern as a pointer.

We should use precise root maps.

The runtime must know:

- Which machine registers contain references.
- Which stack slots contain references.
- Which slots contain tagged VM values.
- Which slots contain unboxed floating-point values.
- Which values are temporarily unavailable because they were optimized away.
- Which references need relocation after collection.

This is particularly important when the compiler keeps values unboxed.

GC root classification must be based on compiler/runtime metadata, not assumptions about whatever bits happen to occupy a register.

# 8. Corruption and validation requirements

Since you've dealt with CPU corruption before, I want a clear distinction here.

NaN boxing provides category encoding. It does not provide hardware fault tolerance.

A bit flip can turn:

- An integer into another integer.
- A reference into another reference.
- A valid value into an invalid value.
- A floating-point value into a tagged value.

The invalid category catches some malformed values, not all corruption.

I propose three validation levels:

| Level      | Behavior                                                          |
| ---------- | ----------------------------------------------------------------- |
| Production | Maintain proven invariants; avoid redundant hot-path checks       |
| Debug      | Validate category, reference range, and representation invariants |
| Diagnostic | Add stronger consistency checks and corruption reporting          |

If hardware fault detection is a requirement, we must separately investigate ECC, memory protection, redundancy, and fault containment.

Adding arbitrary checks to every arithmetic operation is not a substitute.

# 9. Preliminary NV-1 rule list

These are proposed rules, not finalized requirements.

| ID     | Rule                                                                     | Status     |
| ------ | ------------------------------------------------------------------------ | ---------- |
| NV-001 | VM values occupy 64-bit words                                            | Selected   |
| NV-002 | Immediate integers use signed 48-bit payloads                            | Proposed   |
| NV-003 | Heap references use 48-bit payloads                                      | Proposed   |
| NV-004 | Special values share one category                                        | Proposed   |
| NV-005 | One category is reserved for invalid values                              | Proposed   |
| NV-006 | Reserved NaN patterns cannot be stored as ordinary floating-point values | Required   |
| NV-007 | Non-colliding NaN payloads are preserved during bitwise transport        | Proposed   |
| NV-008 | Native arithmetic NaN behavior is explicitly specified                   | Unresolved |
| NV-009 | Native registers may hold unboxed VM values                              | Proposed   |
| NV-010 | GC uses precise root maps                                                | Required   |
| NV-011 | Target-specific encodings are permitted                                  | Selected   |
| NV-012 | Production checks must be justified by their safety requirements         | Proposed   |

# 10. Remaining mathematical and architectural questions

We cannot finish NV-1 without resolving these.

10.1 Integer overflow representation

Signed 48-bit integers only

48-bit immediates, extended integers boxed on overflow

Exact 64-bit integer semantics required

10.2 Heap references

Heap-relative 48-bit offsets

Direct 48-bit pointer encoding

Compare both mathematically and architecturally first

10.3 Arithmetic NaN behavior

Target-native propagation with collision normalization

Canonical NaNs for arithmetic results

Investigate performance and semantic costs first

10.4 Native register representation

Hybrid tagged and unboxed native representations

Always retain NaN-boxed representation

Evaluate both before selecting

Submit architectural decisions

One final point: I don't want to call this representation optimal yet. We have a promising encoding with a useful tag structure, but we have not compared its complete cost against alternative NaN-boxing layouts, split representations, or target-specific encodings.

That comparison is part of NV-1, not optional future polish.


# NV-1 continued: Full mathematical and architectural analysis



Decisions incorporated

Representation not frozen



Your decisions give us the following direction:

| Property                       | Decision                        |
| ------------------------------ | ------------------------------- |
| Immediate integer              | Signed 48-bit                   |
| Integer overflow               | Heap-boxed                      |
| Heap references                | 48-bit heap-relative offsets    |
| Arithmetic NaNs                | Canonical                       |
| Transported NaNs               | Preserve non-colliding payloads |
| Native register representation | Under investigation             |

There is an important optimization opportunity here: the VM storage representation and the compiler's internal representation should not be required to match.

We will investigate that in detail.

# 1. Final candidate encoding layout

Let's establish the candidate precisely before discussing its costs.

## 1.1 Binary64 structure

A binary64 word is:

\\[ W = S\cdot 2^{63}+E\cdot 2^{52}+F \\]

Where:

- \\(S\\) is the sign bit.
- \\(E\\) is the 11-bit exponent.
- \\(F\\) is the 52-bit fraction.

A negative quiet NaN has:

\\[ S=1,\quad E=2047,\quad F\_{51}=1 \\]

Its top 16 bits occupy one of eight possible prefixes:

\\[ \texttt{0xFFF8}\ldots\texttt{0xFFFF} \\]

Our candidate reserves the upper four.

## 1.2 Proposed allocation

## NV-1 candidate: 64-bit word

16 bits

Tag

48 bits

Payload

63–48 | 47–0

| Prefix            | Category              |
| ----------------- | --------------------- |
| `0xFFFC`          | Immediate integer     |
| `0xFFFD`          | Heap reference        |
| `0xFFFE`          | Special immediate     |
| `0xFFFF`          | Invalid/reserved      |
| `0xFFF8`–`0xFFFB` | Floating-point NaNs   |
| Other patterns    | Floating-point values |

The three active categories are distinguished by their low two prefix bits:

\\[ tag=(W\gg48)\mathbin{\&}3 \\]

The common reserved-region test is:

\\[ (W\mathbin{\&}M)=P \\]

Where:

\\[ M=\texttt{0xFFFC000000000000} \\]

\\[ P=\texttt{0xFFFC000000000000} \\]

This is a convenient structural property: all four categories share the same upper 14 bits.

The invalid category is deliberate. We should not assign all possible patterns to valid values merely because they are available.

NaN-space accounting:

There are eight negative quiet-NaN prefix groups. Four are reserved, leaving four available.

Thus, we preserve half of the negative quiet-NaN prefix space, plus all positive NaN patterns and all negative signaling-NaN patterns.

This is a trade-off, not a claim that we preserve every IEEE-754 payload.

## 1.3 Exact category equations

For the integer category:

\\[ W=\texttt{0xFFFC}\cdot2^{48}+p \\]

For the reference category:

\\[ W=\texttt{0xFFFD}\cdot2^{48}+p \\]

For special values:

\\[ W=\texttt{0xFFFE}\cdot2^{48}+p \\]

Where:

\\[ 0\leq p<2^{48} \\]

The invalid category is:

\\[ W\in[\texttt{0xFFFF000000000000}, \texttt{0xFFFFFFFFFFFFFFFF}] \\]

Any occurrence of this category in a value that is supposed to be valid must be rejected by the relevant runtime boundary.

# 2. Immediate integers with boxed overflow

Selected architecture

This is a good candidate for workloads where small integers are common.

We have:

\\[ -2^{47}\leq n\leq2^{47}-1 \\]

The immediate range is:

\\[ [-140737488355328,\ 140737488355327] \\]

The integer payload is a signed two's-complement 48-bit number.

Decoding requires sign extension:

\\[ n=\operatorname{sext}\_{48}(p) \\]

Encoding requires:

\\[ p=n\bmod2^{48} \\]

provided \\(n\\) is within the immediate range.

## 2.1 Overflow policy

When the result falls outside the immediate range, the result becomes a heap-allocated integer object.

This gives us two representations:

| Representation | Contents                                  |
| -------------- | ----------------------------------------- |
| Immediate      | Signed 48-bit integer                     |
| Boxed          | Integer value outside the immediate range |

The boxed integer is referenced through the ordinary heap-reference category.

This means we do not need another NaN prefix.

The object header identifies the integer object representation.

For example, an integer operation can have three conceptual stages:

1. Test whether the operands are immediate integers.
2. Perform the operation using native integer arithmetic.
3. If the result does not fit, invoke the appropriate boxed-integer operation.

The slow path must preserve exact integer semantics.

## 2.2 Addition and subtraction

For two immediate operands:

\\[ a,b\in[-2^{47},2^{47}-1] \\]

Their sum satisfies:

\\[ -2^{48}\leq a+b\leq2^{48}-2 \\]

Their difference satisfies:

\\[ -(2^{48}-1)\leq a-b\leq2^{48}-1 \\]

Both fit in signed 64-bit arithmetic.

Therefore, 64-bit addition and subtraction are sufficient to compute the candidate result without signed 64-bit overflow.

The result must then be checked against the 48-bit range.

The important observation is that we do not need a general 64-bit overflow check for immediate addition or subtraction.

We need a 48-bit representability check.

That is a smaller problem.

## 2.3 Multiplication

Multiplication is more interesting.

The largest positive product is:

\\[ (2^{47}-1)^2 \\]

This approaches \\(2^{94}\\).

A 64-bit result alone is insufficient to determine whether every multiplication result fits.

We need to examine the complete product or use an equivalent mathematically correct overflow test.

The supported architectures have relevant high-half multiplication facilities:

- x86-64: high-half multiplication through integer multiply instructions.
- AArch64: `SMULH` alongside low-half multiplication.
- RV64: `MULH` alongside `MUL`.

The exact lowering will depend on operand constraints and the surrounding computation.

We should compare:

1. Full-width product followed by a 48-bit range check.
2. A high-half-based representability test.
3. A specialized check that avoids materializing unnecessary result bits.

We cannot declare one optimal without target-specific measurements and instruction-level analysis.

## 2.4 Boxed integer semantics remain unresolved

There is one important distinction we must not silently decide.

Does the engine require:

- Exact signed 64-bit integer semantics, with values outside 64 bits handled according to UGB rules?
- Arbitrary-precision integers?

The representation can support either, but the object layout and arithmetic implementation differ considerably.

For arbitrary precision, boxed integer objects need variable-length magnitude storage and sign information.

For fixed-width integers, the box can be much simpler.

The immediate representation does not determine the integer semantic domain. UGB does.

# 3. Heap-relative reference encoding

Selected architecture

The reference payload represents an offset from a heap base.

\\[ A=B+O \\]

Where:

- \\(A\\) is the reconstructed native address.
- \\(B\\) is the heap base.
- \\(O\\) is the unsigned 48-bit byte offset.

The payload range is:

\\[ 0\leq O\leq2^{48}-1 \\]

This provides a theoretical address window of:

\\[ 2^{48}\text{ bytes}=256\text{ TiB} \\]

Important: this is the representable offset window, not a claim that we should reserve or physically commit 256 TiB of memory.

## 3.1 Zero-offset reservation

I recommend reserving offset zero.

Thus:

\\[ O=0\Rightarrow\text{invalid reference} \\]

And valid heap references satisfy:

\\[ 1\leq O\leq2^{48}-1 \\]

This gives us an additional corruption-detection invariant.

It does not make arbitrary bit corruption safe, but it prevents one particularly dangerous class of invalid reference from becoming a valid heap-base reference.

## 3.2 Byte offsets versus scaled offsets

We should compare two alternatives.

| Property              | Byte offset         | Scaled offset                 |
| --------------------- | ------------------- | ----------------------------- |
| Payload               | Byte address offset | Offset divided by alignment   |
| Reconstruction        | Base + offset       | Base + shifted offset         |
| Address window        | 256 TiB             | Larger                        |
| Interior references   | Easier              | More restrictive              |
| Arithmetic            | Simpler             | Requires scaling              |
| Alignment assumptions | Minimal             | Requires guaranteed alignment |

For a scale factor \\(2^k\\):

\\[ A=B+(O\ll k) \\]

The theoretical window becomes:

\\[ 2^{48+k}\text{ bytes} \\]

However, scaled offsets only make sense if every encoded reference points to an appropriately aligned object start.

They complicate interior references and address calculations.

My current preference is byte offsets. A 256 TiB window is already substantial, and the simpler reconstruction semantics are valuable.

This remains a proposal pending target-specific address-space analysis.

## 3.3 Heap-base requirements

The heap base must satisfy:

1. The heap's representable address range cannot overflow the 48-bit offset.
2. All valid object addresses must be representable.
3. Heap-base changes must preserve or correctly update references.
4. Every reconstructed address must be valid under the target's virtual-address rules.
5. Invalid offsets must never be dereferenced.
6. Concurrent collection must not expose stale references to mutator code.

The last requirement is especially important.

A moving collector that relocates an object changes its offset.

Therefore, references to that object must be updated, or a forwarding/indirection mechanism must preserve correctness.

Relative references do not magically eliminate relocation work.

## 3.4 Target-specific consequences

- x86-64: Canonical virtual-address restrictions and LA48/LA57 configurations matter.
- AArch64: Virtual-address size, TBI, MTE, and pointer authentication configurations matter.
- RV64: The configured virtual-address scheme and translation mode matter.

The offset encoding avoids embedding target-native pointer bit patterns in VM values, but it does not eliminate native address reconstruction requirements.

# 4. Arithmetic NaNs: canonicalization policy

Selected

Your choice is particularly useful for cross-platform consistency.

We establish the following contract:

A floating-point arithmetic operation that produces NaN produces the VM's canonical binary64 NaN.

The canonical value is:

\\[ \boxed{\texttt{0x7FF8000000000000}} \\]

This is a positive quiet NaN.

It is outside our reserved tag region.

The RISC-V floating-point specification already uses this canonical binary64 representation for ordinary NaN-producing arithmetic. It also specifies exceptions for certain operations, including sign-injection instructions.&#x20;

[image](https://www.google.com/s2/favicons?domain=https://github.com\&sz=32)

GitHub+1



IEEE-754 defines NaN payloads and propagation recommendations, but does not require every implementation to propagate payloads identically.&#x20;

[image](https://www.google.com/s2/favicons?domain=https://rtrb.org\&sz=32)

RTRB



## 4.1 Operation classification

We need to distinguish numerical operations from bit-preserving operations.

| Operation             | Proposed behavior                           |
| --------------------- | ------------------------------------------- |
| Addition              | Canonicalize NaN result                     |
| Subtraction           | Canonicalize NaN result                     |
| Multiplication        | Canonicalize NaN result                     |
| Division              | Canonicalize NaN result                     |
| Square root           | Canonicalize NaN result                     |
| Fused multiply-add    | Canonicalize NaN result                     |
| Bitwise copy          | Preserve permitted payload                  |
| Load/store            | Preserve permitted payload                  |
| Bit reinterpretation  | Preserve permitted payload unless collision |
| Sign manipulation     | Preserve payload subject to collision rules |
| Arithmetic conversion | Canonicalize NaN result                     |

We should define minimum/maximum separately because their NaN behavior can differ from ordinary arithmetic.

Likewise, comparisons need their own semantics.

## 4.2 Canonicalization cost

On RISC-V, ordinary arithmetic NaN results are already canonical under the standard floating-point behavior. That is a useful advantage.

On x86-64 and AArch64, we cannot assume every arithmetic operation produces the same canonical bit pattern.

A hardware arithmetic operation can propagate an input NaN payload.

Therefore, we need a target-independent lowering strategy.

There are two approaches:

Approach A: Immediate normalization

After an arithmetic operation, determine whether its result is NaN and replace it with the canonical value.

Approach B: Deferred normalization

Keep the result in an internal floating-point representation and normalize when it crosses an observable VM-value boundary.

Approach B can avoid repeated conversions.

For example:

\\[ x=a+b \\]

\\[ y=x\times c \\]

If \\(x\\) is never observed independently, the compiler may be able to defer normalization until \\(y\\) becomes observable.

However, this requires a carefully specified floating-point semantic model.

If NaN payload inspection is possible between those operations, normalization cannot be deferred across that observation.

This is exactly where our VSDG and Sea of Nodes distinction becomes useful:

- VSDG can preserve value/state dependencies and represent the arithmetic results.
- Sea of Nodes can reason about observability, dependencies, and opportunities to delay representation normalization.
- LLIR must eventually enforce the required observable semantics.

## 4.3 Floating-point exception flags

We must not confuse NaN canonicalization with floating-point exception behavior.

Canonicalizing the result does not mean suppressing:

- Invalid operation.
- Divide by zero.
- Overflow.
- Underflow.
- Inexact.

These flags must follow the VM's specified floating-point environment.

In particular, the compiler must not eliminate an operation that changes observable floating-point exception state.

NV-1 therefore does not authorize arbitrary floating-point reassociation or exception suppression.

# 5. Native register representation investigation

Not selected

This is the most interesting part of the current design.

I do not recommend forcing all values to remain NaN-boxed in native registers.

That would defeat one of the primary advantages of a multi-tier optimizing JIT.

Instead, we should investigate four possible execution representations.

## 5.1 Candidate A: Always-tagged

Every VM value remains a complete 64-bit NaN-boxed word.

Representation

GPR

64-bit tagged value

One representation for integers, references, floating-point values, and special values.

Advantages:

- Simple register representation.
- Simple spill representation.
- Straightforward safepoint handling.
- Easy value copying.
- Straightforward deoptimization.

Disadvantages:

- Floating-point operations require unboxing.
- Integer operations require decoding.
- Type checks may appear frequently.
- Tagged values can consume additional instructions in arithmetic-heavy code.

This is attractive for baseline compilation.

It is not necessarily attractive for optimizing compilation.

## 5.2 Candidate B: Fully unboxed

Values are represented in native registers according to their inferred representation.

| Value     | Native representation               |
| --------- | ----------------------------------- |
| Integer   | Native integer register             |
| Float     | Native floating-point register      |
| Reference | Native pointer or offset            |
| Boolean   | Integer register or condition value |
| Constant  | Immediate or constant               |

Advantages:

- Less tagging overhead.
- Natural arithmetic.
- Potentially fewer conversions.
- Better opportunities for register allocation.

Disadvantages:

- Representation tracking becomes more complicated.
- Safepoint handling becomes more complicated.
- Deoptimization requires reconstruction.
- Mixed-type operations require conversions.
- Moving GC requires special handling for native pointers.

Fully unboxed execution can be very efficient, but it requires stronger compiler/runtime coordination.

## 5.3 Candidate C: Hybrid tagged and unboxed

This is my preferred research candidate.

The compiler chooses representations based on the operations and lifetime of individual values.

For example:

Example: floating-point computation

NaN-boxed VM input

Unbox once

Native FPR: binary64

Multiple native operations

Native FPR: binary64 result

Normalize and box when observable

NaN-boxed VM output

The key is that the compiler should not repeatedly convert between tagged and unboxed representations when the value remains within one computation region.

For integers, the same idea applies.

An integer known to fit in the immediate range can remain in a native GPR without carrying the 16-bit NaN tag during every operation.

An integer that overflows the immediate range can transition into the boxed representation.

For references, we should initially keep the offset representation in native GPRs rather than eagerly converting every reference into a raw native pointer.

This avoids creating unnecessary GC relocation obligations.

## 5.4 Candidate D: Adaptive representation selection

This goes further than simply using a hybrid representation.

The compiler could select the representation according to estimated execution cost.

For a value \\(v\\), define:

\\[ C(v,r)= C\_{\mathrm{conversion}} +C\_{\mathrm{operation}} +C\_{\mathrm{register}} +C\_{\mathrm{spill}} +C\_{\mathrm{GC}} +C\_{\mathrm{deopt}} \\]

Where \\(r\\) is a candidate representation.

The compiler selects:

\\[ r^\*=\arg\min_r C(v,r) \\]

subject to correctness constraints.

This is a conceptual cost model, not yet an implementable cost equation. We need target-specific coefficients, frequency estimates, and dependency modeling.

For example:

- A value used in many floating-point operations may benefit from FPR storage.
- A value frequently passed through generic VM helpers may benefit from tagged storage.
- A heap reference crossing many safepoints may benefit from remaining as an offset.
- A reference used repeatedly for memory accesses may benefit from temporary native-pointer decoding.

The important thing is that representation decisions should account for the entire live range, not just the next instruction.

## 5.5 Preliminary comparison

| Property              | Always tagged      | Fully unboxed      | Hybrid             | Adaptive              |
| --------------------- | ------------------ | ------------------ | ------------------ | --------------------- |
| Baseline simplicity   | Excellent          | Poor               | Good               | Poor                  |
| Arithmetic efficiency | Variable           | Excellent          | Excellent          | Potentially excellent |
| GC integration        | Simplest           | Complex            | Manageable         | Complex               |
| Deoptimization        | Simplest           | Complex            | Manageable         | Complex               |
| Compilation cost      | Low                | High               | Medium             | Potentially high      |
| Register pressure     | Workload-dependent | Workload-dependent | Workload-dependent | Workload-dependent    |

There is no universal winner.

My current recommendation is to investigate Hybrid as the default optimizing-tier strategy, with Always-tagged as the baseline strategy.

Adaptive selection can be evaluated later, after we have actual profiling data and a representation-aware register allocator.

# 6. GC interaction with all four representations

This needs its own explicit contract.

Your collector is incremental, concurrent, and generational.

That means the JIT must coordinate with the collector rather than simply provide a list of pointers when collection begins.

## 6.1 Root categories

We need at least these root-map categories:

| Category          | Contents                   | GC treatment            |
| ----------------- | -------------------------- | ----------------------- |
| Tagged value      | NaN-boxed VM value         | Inspect tag             |
| Encoded reference | Heap-relative offset       | Decode and trace        |
| Native pointer    | Reconstructed heap address | Trace and relocate      |
| Unboxed float     | Binary64 value             | Never treat as pointer  |
| Unboxed integer   | Native integer             | Never treat as pointer  |
| Constant          | Compile-time value         | Usually no mutable root |

A precise stack map must describe which physical registers and stack locations hold which categories at each safepoint.

A general-purpose register containing an integer must never be scanned as though it were a heap pointer.

## 6.2 Moving collector interaction

Suppose:

\\[ R=B+O \\]

The collector moves an object from offset \\(O_1\\) to \\(O_2\\).

The encoded reference must be updated:

\\[ O_1\rightarrow O_2 \\]

If the heap base remains fixed, the base-relative representation remains valid after relocation, provided all references are updated correctly.

If the heap base changes, the heap's relocation policy must preserve or reconstruct the relationship.

This is one reason I prefer keeping encoded offsets in native registers across safepoints.

Raw native pointers require relocation-aware register maps, pinning, or another mechanism.

## 6.3 Write barriers

A concurrent generational collector generally needs to track mutations that could violate its tracing invariants.

Depending on the collector's marking algorithm, this may require:

- A generational remembered-set barrier.
- An incremental marking barrier.
- A SATB barrier, if snapshot-at-the-beginning marking is used.
- An incremental-update barrier, if that is the chosen invariant.

We have not selected the collector's exact marking algorithm, so we must not prescribe a single barrier implementation yet.

The JIT's memory-store lowering must be designed to accommodate the selected protocol.

## 6.4 Safepoint constraints

A generated method must have a defined way to reach a collector-safe state.

That means:

1. All live references are discoverable.
2. All relocatable references can be updated.
3. No untracked temporary pointer remains live across a relocating collection.
4. The collector can distinguish encoded references from ordinary integers.
5. Deoptimization can reconstruct the interpreter-visible VM state.

We will eventually need precise safepoint maps, not merely conservative register scanning.

# 7. Cost model: what actually determines the winner?

We need to avoid optimizing one instruction while making the whole execution path slower.

For a representation \\(r\\), define its total expected cost over an execution interval:

\\[ C(r)= N_cC_c+ N_oC_o+ N_sC_s+ N_gC_g+ N_dC_d \\]

Where:

| Variable  | Meaning                                         |
| --------- | ----------------------------------------------- |
| \\(N_c\\) | Number of representation conversions            |
| \\(C_c\\) | Average conversion cost                         |
| \\(N_o\\) | Number of operations                            |
| \\(C_o\\) | Average operation cost                          |
| \\(N_s\\) | Number of spills/reloads                        |
| \\(C_s\\) | Average spill/reload cost                       |
| \\(N_g\\) | Number of GC-related interactions               |
| \\(C_g\\) | Average GC interaction cost                     |
| \\(N_d\\) | Number of deoptimization/materialization events |
| \\(C_d\\) | Average associated cost                         |

This is a starting model. It does not yet account for overlapping execution, memory-level parallelism, cache misses, branch prediction, or dependency chains.

Those require a more complete microarchitectural model.

## 7.1 A particularly important consideration: conversion amortization

Suppose a floating-point value undergoes \\(n\\) operations.

With repeated boxing and unboxing:

\\[ C\_{\mathrm{repeated}}=n(C\_{\mathrm{unbox}}+C\_{\mathrm{operation}}+C\_{\mathrm{box}}) \\]

With one unboxing and one final boxing:

\\[ C\_{\mathrm{amortized}}= C\_{\mathrm{unbox}}+ nC\_{\mathrm{operation}}+ C\_{\mathrm{box}} \\]

The difference is:

\\[ \Delta C=(n-1)(C\_{\mathrm{unbox}}+C\_{\mathrm{box}}) \\]

This is a real structural advantage of unboxed live ranges.

It does not prove that hybrid execution wins every workload. Register spills, safepoints, and conversion boundaries can change the result.

But it gives us a mathematically justified optimization target.

# 8. NV-1 mandatory invariants

These are the invariants I would require before accepting the representation.

| ID      | Invariant                                                                           |
| ------- | ----------------------------------------------------------------------------------- |
| INV-001 | Every valid VM value has exactly one valid storage representation.                  |
| INV-002 | No valid value uses the reserved invalid category.                                  |
| INV-003 | Immediate integers decode to their exact signed 48-bit value.                       |
| INV-004 | Integer overflow never silently truncates.                                          |
| INV-005 | Every heap reference identifies a valid object or is rejected before dereferencing. |
| INV-006 | Offset zero is never a valid heap reference.                                        |
| INV-007 | Arithmetic NaN results use the canonical VM NaN.                                    |
| INV-008 | Permitted non-colliding NaN payloads survive bitwise transport.                     |
| INV-009 | Reserved NaN patterns cannot be mistaken for ordinary floating-point values.        |
| INV-010 | Unboxed values are never accidentally interpreted as tagged values.                 |
| INV-011 | GC root maps accurately describe live references.                                   |
| INV-012 | Relocatable references remain correct across collection.                            |
| INV-013 | Deoptimization reconstructs the exact interpreter-visible state.                    |
| INV-014 | Target-specific encodings preserve identical abstract VM semantics.                 |
| INV-015 | Invalid representations cannot become unchecked native memory accesses.             |

These are correctness requirements, not optional optimizations.

# 9. Performance rules under consideration

Here is the first batch of actual NV-1 optimization rules.

| Rule   | Description                                                          | Status    |
| ------ | -------------------------------------------------------------------- | --------- |
| NV-P01 | Immediate integers avoid heap allocation within range                | Candidate |
| NV-P02 | Integer arithmetic remains unboxed during eligible live ranges       | Candidate |
| NV-P03 | Floating-point arithmetic remains in native FPRs where profitable    | Candidate |
| NV-P04 | Avoid repeated boxing/unboxing within a computation region           | Candidate |
| NV-P05 | Keep references encoded across safepoints where practical            | Candidate |
| NV-P06 | Decode heap offsets only when needed for addressing                  | Candidate |
| NV-P07 | Defer NaN normalization when semantic observability permits          | Candidate |
| NV-P08 | Avoid scanning non-reference native registers during GC              | Required  |
| NV-P09 | Avoid redundant tag extraction when representation is already proven | Candidate |
| NV-P10 | Use target-specific lowering for encoding and decoding               | Candidate |

A candidate becomes mandatory only after we establish its applicable conditions and prove its correctness.

# 10. Outstanding questions

We have made substantial progress, but NV-1 is not finished.

These are the remaining issues I consider architecturally significant.

## 10.1 Integer semantics

We need to establish whether boxed integers represent:

- Exact signed 64-bit integers.
- Arbitrary-precision integers.
- Another UGB-defined integer domain.

This changes object layouts, arithmetic helpers, overflow behavior, and optimization opportunities.

## 10.2 Heap object alignment

We need to establish:

- Minimum object alignment.
- Maximum object size.
- Whether interior references are permitted.
- Whether references can cross heap regions.
- Whether multiple heaps or isolates are supported.

This determines whether byte offsets remain the best representation.

## 10.3 Native register allocation

We need to investigate:

- GPR/FPR register pressure.
- Tagged versus unboxed live ranges.
- Representation transitions.
- Spill-slot representation.
- Safepoint constraints.
- Deoptimization reconstruction.
- Register allocation interactions across calls.

I particularly want to avoid a representation-selection algorithm that saves conversion instructions but causes enough extra spills to lose overall.

## 10.4 Floating-point semantics

We still need exact rules for:

- Signaling NaN inputs.
- NaN payload inspection.
- Minimum/maximum operations.
- Comparisons.
- Floating-point conversions.
- Rounding modes.
- Exception flags.
- Fused operations.
- Bitwise reinterpretation.

Canonical arithmetic NaNs alone do not define the entire floating-point contract.

# 11. NV-1 current status

## NaN-boxed VM representation

Architecture

Established direction

64-bit encoding

Candidate

Integer representation

Decision made

Heap references

Decision made

NaN semantics

Policy selected

Native representation

Under investigation

GC integration

Partially specified

Formal invariants

Draft

Not approved for implementation as a frozen standard.

## My architectural recommendation

I would retain the current four-prefix encoding as our leading candidate, use signed 48-bit immediate integers with boxed overflow, and use byte-based heap-relative offsets.

For the optimizing compiler, I would investigate hybrid representation as the default design, without yet making it mandatory.

The most important unresolved optimization is native representation selection. That decision will affect VSDG, Sea of Nodes, LLIR, Machine IR, register allocation, and GC integration.

Next, we should settle the integer semantic domain and heap object layout. Those two decisions determine whether our proposed 48-bit immediate and reference encodings are actually sufficient, rather than merely attractive on paper.

Candidate D is interesting. It changes how we should approach the rest of the architecture.

I would actually make adaptive representation selection a first-class compiler subsystem, rather than an isolated optimization pass.

And I would avoid deciding the integer domain and heap layout prematurely. Those are foundational decisions, but they should follow from the engine's semantic requirements.

Let's separate the problems.

# 1. Adaptive representation selection (ARS)

Selected research direction

The compiler should choose the representation of each value according to its expected total execution cost.

Not according to a fixed rule such as "integers go in GPRs" or "references always stay tagged."

## 1.1 Candidate representations

| Representation | Description                                  |
| -------------- | -------------------------------------------- |
| `TAGGED64`     | Complete NaN-boxed VM value                  |
| `I64`          | Native signed integer                        |
| `F64`          | Native binary64                              |
| `REF_OFFSET`   | Encoded heap-relative reference              |
| `NATIVE_REF`   | Reconstructed native pointer                 |
| `CONSTANT`     | Compile-time known value                     |
| `SPECIALIZED`  | Representation with proven value constraints |

`SPECIALIZED` needs further definition. It should describe a proven subset of a representation, not become an excuse for creating arbitrary types.

## 1.2 Selection objective

For a value \\(v\\), candidate representation \\(r\\), and execution region \\(G\\):

\\[ r^\*=\arg\min\_{r\in R(v)} C(v,r,G) \\]

Where:

\\[ C = C\_{\mathrm{conversion}} +C\_{\mathrm{operations}} +C\_{\mathrm{register}} +C\_{\mathrm{memory}} +C\_{\mathrm{GC}} +C\_{\mathrm{deopt}} \\]

The important addition is \\(G\\).

We should not select a representation independently for every instruction. That would produce excessive conversions.

Instead, we investigate selecting representations for connected computation regions and live ranges.

### Example

Suppose a value participates in 100 floating-point operations.

- Tagged storage might require repeated unboxing.
- F64 storage might require one unboxing and one final boxing.
- F64 storage might also increase FPR pressure.

ARS must account for all three.

## 1.3 Hard constraints

The optimizer must never select a representation that violates:

1. Value semantics.
2. Overflow semantics.
3. NaN semantics.
4. GC visibility.
5. Safepoint requirements.
6. Calling convention requirements.
7. Deoptimization reconstructability.

Correctness constraints are not cost penalties. They are feasibility conditions.

# 2. Where ARS belongs in the four IR levels

This is important.

I propose that representation selection have different responsibilities at different levels.

VSDG

Semantic analysis

- Determine value properties.
- Track dependencies.
- Establish representation-independent facts.

Sea of Nodes

Optimization planning

- Analyze computation regions.
- Estimate representation transitions.
- Evaluate unboxing opportunities.
- Consider GC and observability boundaries.

Low-Level IR

Representation commitment

- Select concrete value representations.
- Insert necessary conversions.
- Establish explicit safepoint and spill requirements.

Machine IR

Physical realization

- Map representations to register classes.
- Allocate physical registers.
- Resolve spills.
- Materialize conversions and constraints.

I would not let Machine IR freely change the semantic representation decisions. It can influence their profitability, but the compiler must maintain a clear contract between representation selection and machine lowering.

# 3. Integer semantic domain

We should not assume that the language uses arbitrary-precision integers just because boxed overflow is available.

There are several possible semantic domains.

| Domain                        | Immediate      | Boxed                               |
| ----------------------------- | -------------- | ----------------------------------- |
| Fixed signed 64-bit           | Fast           | Overflow handling only if specified |
| Arbitrary precision           | Fast           | Extended magnitude                  |
| Fixed signed 128-bit          | Fast           | Larger values                       |
| UGB-defined numeric semantics | Depends on UGB | Depends on UGB                      |

The key question is whether the integer object is:

- A different storage representation of the same integer domain.
- A different numeric category with different semantics.

Those are very different designs.

For example, if integers are arbitrary precision, then:

\\[ 2^{100}+2^{100} \\]

must produce the exact mathematical result.

If integers are fixed-width, the same expression might overflow, wrap, trap, or produce another UGB-defined result.

We cannot select an object layout until that is established.

# 4. Heap object layout

I suggest we treat heap layout as a separate specification, `HEAP-1`.

It should define:

1. Object header.
2. Object size representation.
3. Object alignment.
4. Object type identification.
5. Field layout.
6. Array layout.
7. Variable-length payloads.
8. Reference encoding.
9. Forwarding and relocation.
10. GC metadata.
11. Allocation fast paths.
12. Concurrent mutation requirements.

One particularly important question: should GC metadata live inside objects, outside objects, or be split between both?

That affects object density, traversal performance, allocator complexity, and cache behavior.

I would investigate side metadata seriously rather than automatically putting everything into object headers.

# 5. What I want to establish before selecting layouts

There are three decisions I would tackle in this order:

First: integer semantics.

This determines what boxed integers must represent.

Second: object model.

This determines what an object actually is and what references point to.

Third: heap organization.

This determines how objects are allocated, located, traversed, moved, and collected.

The order matters. Otherwise we risk designing a beautiful heap that cannot efficiently represent the engine's actual objects.

## 6. Two foundational questions

I want to avoid imposing assumptions about UGB.

Integer semantics: Does UGB define the integer domain, or is UGB intentionally generic and expected to support multiple numeric semantics?

Object model: Are we building a general-purpose object engine with objects, arrays, strings, closures, and other heap entities, or is the heap primarily an implementation detail for a narrower execution model?

These two answers will determine the next specifications.

And one architectural rule I would establish now:

ARS must be allowed to choose representations adaptively, but it must never be allowed to invent semantic guarantees that UGB does not provide.


That establishes three important architectural constraints.

1. UGB owns integer semantics. The JIT does not get to choose whether integers are signed, unsigned, fixed-width, arbitrary-precision, wrapping, trapping, or anything else. Those rules come from UGB. The JIT must preserve them exactly.
2. The engine has a general-purpose heap. Objects, arrays, strings, closures, and other UGB-defined heap entities are first-class. We cannot design a heap that only happens to work for the first two object types we implement.
3. Specialization is automatic, but not semantic invention. The engine may discover and exploit properties of existing UGB entities. It may not invent new language-level entities, operations, or guarantees.

That last distinction is especially important. We are designing an optimizer that specializes the implementation of existing semantics, not a language extension disguised as an optimization.

## The specialization boundary

We should make this a foundational rule:

A specialization may narrow the implementation strategy, but it must not expand or alter the semantic contract.

For example:

- An object with a stable observed layout may receive a specialized access path.
- An array with proven element representation may receive specialized storage or access operations, provided UGB-visible behavior remains identical.
- A closure with a known target may receive a direct-call path, while preserving the required closure semantics.
- A string may receive specialized comparison or indexing paths if they preserve UGB's string semantics.
- An object may be scalar-replaced or eliminated if its identity, mutation, aliasing, and observability requirements permit it.

But the optimizer cannot assume an object is immutable merely because it has not observed a mutation. It cannot assume a call is non-throwing because it usually does not throw. It cannot silently replace a general object with a restricted object type.

Those are proof obligations, not optimistic guesses.

## Consequence for Adaptive Representation Selection

ARS now has two distinct dimensions:

- Value representation selection: tagged, unboxed integer, floating-point, heap offset, native reference, and so on.
- Entity implementation specialization: layout, field access, array storage, string representation, closure environment, allocation strategy, and other implementation choices.

They interact, but they are not the same subsystem. We should avoid turning ARS into a gigantic bucket containing every optimization anyone can think of.

A specialized object layout may influence the best representation of its fields. Conversely, a field's selected representation may constrain the layout. Their decisions therefore need a shared cost and correctness model, with explicit ownership of each decision.

This also means automatic object specialization must be a core architectural capability, not an optional optimization pass bolted onto the highest tier.

I would give it its own subsystem, provisionally called the Automatic Object Specialization System (AOSS).

## 1. Three levels of object specialization

We need to keep these separate.

| Level            | What changes                                       | What cannot change                            |
| ---------------- | -------------------------------------------------- | --------------------------------------------- |
| Structural       | Field layout, offsets, storage organization        | Object identity and observable field behavior |
| Representational | How fields and references are stored               | UGB value semantics                           |
| Operational      | Access paths, allocation, calls, mutation handling | UGB operation semantics                       |

For example, suppose UGB permits an object to have dynamically added properties.

The engine might discover that a particular group of objects consistently has the same property layout. It can then specialize their storage and generate direct field accesses.

However, if an operation adds a property that violates the assumed layout, the engine must handle that correctly. It might transition the object to another layout, use a generic path, or invalidate dependent compiled code.

It cannot simply pretend the property was never added. That would be a rather aggressive interpretation of optimization.

## 2. Specialization needs explicit correctness rules

I propose these initial architectural requirements:

- SPEC-001: Semantic preservation. Every specialization must preserve the complete observable behavior defined by UGB.
- SPEC-002: No semantic invention. Specialization cannot introduce new guest-visible types, operations, guarantees, or restrictions.
- SPEC-003: General fallback. Every speculative specialization must have a correct path for cases outside its assumptions.
- SPEC-004: Guard validity. Runtime assumptions must be guarded, proven invariant, or otherwise maintained.
- SPEC-005: Mutation correctness. Specialization must account for mutations, aliases, and changes to object structure.
- SPEC-006: Identity preservation. Object identity and aliasing behavior must remain correct.
- SPEC-007: GC correctness. Specialized layouts must remain compatible with allocation, tracing, relocation, and barriers.
- SPEC-008: Deoptimization correctness. The engine must reconstruct the exact required UGB-visible state when leaving specialized execution.
- SPEC-009: Profitability. A specialization must justify its compilation, code-size, memory, and execution costs.

These are proposed requirements, not a completed specification.

## 3. Implications for the rest of the architecture

UGB semantic contract

Defines integer domains, object behavior, operations, identity, and observable semantics.

Automatic specialization analysis

Discovers properties, evaluates whether specialization is legal, and determines which assumptions need protection.

Specialized implementation planning

Coordinates object layouts, value representations, access paths, allocation strategies, and generated code.

Tier-specific realization

Baseline and optimizing tiers implement the same specialization semantics at different costs and sophistication levels.

One important consequence: specialization cannot belong exclusively to the optimizing JIT. The baseline tier may use simpler specialization mechanisms, while higher tiers exploit richer analysis. The semantic contract remains identical.

## 4. Two things we must not prematurely freeze

NV-1 integer representation: UGB defines the integer domains, so our 48-bit immediate proposal must be checked against every domain UGB actually supports. We might need multiple immediate encodings, tagged boxed forms, or different representations selected by ARS. The 48-bit range is not sacred.

HEAP-1 object layout: We should not choose a universal object header or fixed layout before examining every UGB-defined heap entity. Objects, arrays, strings, closures, and other entities may have fundamentally different storage requirements. A common allocation and GC framework does not imply identical physical layouts.

## 5. The next design target

I suggest we define the specialization model before the physical heap layout.

Specifically, we need to establish:

1. What properties can the engine discover automatically?
2. Which properties are stable, mutable, or invalidatable?
3. What makes a specialization legal?
4. How are specialized and general representations related?
5. How are transitions between representations handled?
6. How are specialization decisions shared across compilation tiers?
7. How does the system decide whether specialization is actually profitable?

This is the foundation. Once it is defined, we can design HEAP-1 around the requirements rather than inventing an object layout and discovering six chapters later that it cannot support the optimizer.

The governing principle: UGB defines what an entity is. The engine determines how efficiently that entity can be implemented.


Then we start with the Automatic Object Specialization System (AOSS). Before designing headers, layouts, or machine instructions, we need to define exactly what this system is allowed to specialize, how it proves a specialization is valid, and how it recovers when its assumptions stop holding.

I'm treating AOSS as a core architectural subsystem, not merely a collection of optimization passes.

# AOSS-1: Automatic Object Specialization Model



Draft specification

&#x20;Version 0.1 · Not frozen



## 1. Fundamental model

Every heap entity has two distinct descriptions:

- Semantic identity: What the entity means under UGB.
- Physical implementation: How the engine represents, stores, accesses, and manipulates it.

AOSS operates on the second description.

For example, two objects may have identical UGB-visible behavior while using different physical layouts. One might use a general-purpose layout, while another uses a specialized layout with fixed field offsets.

The specialization is valid only if the difference is invisible to UGB execution.

We should formalize this distinction:

\\[ \operatorname{Semantics}(E\_{\text{general}}) = \operatorname{Semantics}(E\_{\text{specialized}}) \\]

This equality covers all observable behavior, not merely the results of ordinary field reads.

It includes mutation, identity, aliasing, exceptions, ordering, allocation behavior where observable, and interactions with the collector.

## 2. What AOSS may specialize

We need a classification of specialization opportunities.

| Domain               | Potential specialization                                          |
| -------------------- | ----------------------------------------------------------------- |
| Object structure     | Field layout, offsets, property lookup                            |
| Field representation | Tagged, unboxed, compressed or indirect storage                   |
| Arrays               | Element representation, indexing, bounds-check paths              |
| Strings              | Storage representation, length and access operations              |
| Closures             | Environment layout, captured-value representation                 |
| Calls                | Known targets, direct-call paths, specialized calling conventions |
| Allocation           | Object size, allocation path, initialization                      |
| Access               | Direct access, guarded access, cached metadata                    |
| Lifetime             | Escape analysis, scalar replacement, allocation elimination       |

These are candidate capabilities, not blanket permissions.

For example, specialized string indexing is only valid once we know whether UGB defines strings as UTF-8, UTF-16, byte sequences, or something else. We must not quietly select a convenient string model.

Likewise, specialized arrays cannot assume homogeneous elements unless UGB semantics and runtime evidence justify it.

## 3. Facts versus assumptions

This is one of the most important parts of the design.

AOSS must distinguish three categories of information.

A. Proven facts

Strongest

Properties established by semantics, static proof, or invariants maintained by the runtime.

Example: an object's allocation size is fixed after allocation, if UGB and the object model guarantee that.

B. Guarded assumptions

Conditional

Properties observed or inferred that require runtime validation.

Example: a particular call site currently sees only one closure target.

C. Heuristics

Weakest

Predictions used to estimate profitability, not establish correctness.

Example: a particular object shape is likely to remain common.

A heuristic must never become a correctness assumption.

A profile saying that 99.9% of objects have a certain shape does not prove the remaining 0.1% cannot exist.

AOSS must preserve this separation throughout every IR.

## 4. Specialization lifecycle

A specialization should have an explicit lifecycle.

1. Discovery

   Collect structural and behavioral information about UGB entities. Identify recurring properties and candidate specialization opportunities.
2. Legality analysis

   Determine whether the candidate preserves UGB semantics. Reject candidates whose assumptions cannot be established or protected.
3. Profitability analysis

   Estimate execution savings against compilation cost, code size, metadata, memory overhead, and invalidation risk.
4. Construction

   Create the specialized implementation, required metadata, guards, and dependency records.
5. Execution

   Execute through the specialized path while maintaining its required invariants.
6. Invalidation or transition

   If an assumption ceases to hold, invalidate dependent compiled code or transition the affected entity safely.
7. Retirement

   Reclaim obsolete specialization metadata and code when no longer needed, respecting concurrent execution and GC requirements.

The lifecycle must be concurrency-safe. Our collector is concurrent and incremental, so specialization metadata cannot be casually mutated or reclaimed while generated code or collector threads still reference it.

## 5. Specialization dependencies

This deserves its own subsystem.

Suppose compiled code assumes:

- Object shape `S` is unchanged.
- Field `x` occupies offset `O`.
- Field `x` has representation `R`.
- The call target remains `T`.

The compiled code depends on those properties.

We need a dependency model that records exactly which assumptions each compiled artifact relies upon.

A useful abstraction is:

\\[ D(C)=\\{d_1,d_2,\ldots,d_n\\} \\]

Where \\(D(C)\\) is the set of dependencies for compiled artifact \\(C\\).

If dependency \\(d_i\\) becomes invalid, the engine must identify affected code.

Crucially, we should not automatically invalidate every specialization whenever one property changes. Dependencies need sufficiently fine granularity to avoid unnecessary recompilation.

That creates a design question: should dependencies be attached to individual object layouts, layout versions, properties, call sites, or some combination?

We should investigate that before selecting a mechanism.

## 6. Initial correctness requirements

| ID       | Requirement                                                         |
| -------- | ------------------------------------------------------------------- |
| AOSS-001 | Specialization preserves UGB-visible semantics.                     |
| AOSS-002 | Specialization cannot introduce guest-visible semantic entities.    |
| AOSS-003 | Every specialization has a valid correctness basis.                 |
| AOSS-004 | Heuristic observations cannot establish correctness.                |
| AOSS-005 | Every guarded assumption has a defined failure path.                |
| AOSS-006 | Mutations preserve object identity and aliasing semantics.          |
| AOSS-007 | Dependencies are tracked for compiled artifacts.                    |
| AOSS-008 | Invalidation cannot leave executing code using invalid assumptions. |
| AOSS-009 | GC can correctly trace and relocate specialized entities.           |
| AOSS-010 | Deoptimization reconstructs the required guest-visible state.       |
| AOSS-011 | Specialization costs are included in profitability decisions.       |
| AOSS-012 | General-purpose execution remains semantically complete.            |

None of these requirements specifies the physical implementation yet.

# 7. The next critical design decision: specialization granularity

There are several possible levels at which AOSS can specialize.

A. Per-object specialization

Each individual object can receive its own specialized implementation.

- Very fine-grained.
- Potentially large metadata and code-management costs.
- Useful when individual objects have unusual characteristics.

B. Per-layout specialization

Objects sharing a structural layout share a specialization.

- Potentially amortizes compilation and metadata.
- Natural fit for repeated object shapes.
- Requires careful handling of layout transitions.

C. Per-allocation-site specialization

Objects created at a particular allocation site can share assumptions.

- Can exploit construction patterns.
- Does not guarantee all resulting objects remain equivalent.
- Requires handling subsequent mutation and escaping.

D. Combined specialization

The engine uses multiple granularities, choosing according to the property being specialized.

- More expressive.
- More complicated dependency and invalidation model.
- Requires a principled way to prevent redundant or conflicting specializations.

My initial research direction is D, but that is not a conclusion. We need to establish whether the additional complexity actually pays for itself.

The important distinction is that specialization granularity need not be identical for every optimization. A layout can be shared across many objects, while a call site can have its own target specialization and an individual object can have a unique representation.

Next, we should investigate specialization granularity rigorously: the possible units of specialization, metadata overhead, code sharing, invalidation complexity, compilation cost, and the conditions under which each granularity wins. Only after that should we define the physical specialization metadata.

No heap header design yet. We have enough ways to make that decision badly without rushing.
---

## HEAP-1: Object Layout and Header Specification

### 1. The Universal 64-bit Header

Every heap entity (objects, arrays, closures, strings) begins with an identical 8-byte header. To accommodate a moving, concurrent GC without expanding the header, we partition the 64 bits into a 62-bit Layout Identity and a 2-bit GC State field.

#### 1.1 Bit Allocation

| Bits | Field | Description |
|------|-------|-------------|
| 62–63 | GC_State | 00: Valid LayoutID (Normal execution); 01: Being marked (Concurrent marking phase); 10: Reserved for future GC states; 11: Forwarding pointer (Evacuation phase) |
| 0–61 | Payload | If GC_State == 00: 62-bit hash of the canonical layout. If GC_State == 11: 62-bit heap-relative offset to the new object location. |

#### 1.2 Invariants

**HEAP-001**: The LayoutID hash function is constrained to never produce a value where bits 62–63 are 11. (e.g., The hash is computed over 62 bits, and the top two bits are explicitly masked to 00 upon storage).

**HEAP-002**: During normal mutator execution, the header is always read as a valid LayoutID. The mutator never observes GC_State != 00.

**HEAP-003**: The GC may temporarily overwrite the header with a forwarding pointer (GC_State == 11) only during a stop-the-world evacuation phase or via a thread-local handshake, ensuring no mutator reads a torn or invalid state.

### 2. Uniform Payload Layout: Objects vs. Arrays

Because the header is universally 8 bytes, the payload for all entities begins at exactly offset 8. This creates beautiful uniformity for the JIT and the GC.

#### 2.1 Regular Objects

| Offset | Size | Content |
|--------|------|---------|
| 0 | 8 bytes | Header (LayoutID, GC_State == 00) |
| 8 | 8 bytes | Field 0 (NV-1 64-bit NaN-boxed value or unboxed representation) |
| 16 | 8 bytes | Field 1 |
| ... | ... | ... |

#### 2.2 Arrays

Arrays share the exact same header. The "length" of the array is simply treated as the first field of the array's payload.

| Offset | Size | Content |
|--------|------|---------|
| 0 | 8 bytes | Header (LayoutID identifies "Array of Type T", GC_State == 00) |
| 8 | 8 bytes | Array Length (Unsigned 64-bit integer, or 48-bit immediate if bounded) |
| 16 | 8 bytes | Element 0 |
| 24 | 8 bytes | Element 1 |
| ... | ... | ... |

**Why this wins:**
- The JIT can emit the exact same base-offset addressing mode (`[base + 8]`) to read either an object's first field or an array's length.
- The GC's object-size calculation becomes trivial: `Size = Header(8) + Payload_Size`. For arrays, the LayoutID can encode the element type and a fixed overhead, and the length at offset 8 provides the dynamic multiplier.

### 3. Synergy with NV-1: 48-bit Heap-Relative Offsets

Recall from NV-1 that heap references are encoded as 48-bit offsets from a `heap_base`:

```
Native Address = heap_base + offset
```

We can now introduce a critical optimization: All heap entities are strictly 8-byte aligned.

Because the offset is always a multiple of 8, the lower 3 bits of the offset are always zero.

While the NV-1 specification allows a full 48-bit byte offset (256 TiB), the JIT and GC can internally treat the stored 48-bit value as a 45-bit scaled offset (multiplied by 8), or simply rely on the hardware's natural alignment.

More importantly, this alignment guarantees that the 64-bit header at `heap_base + offset` is always naturally aligned for a single 64-bit load (`mov rax, [rbx + offset]`), which is the fundamental O(1) guard check for AOSS.

### 4. GC Metadata: The Side-Table Approach

Since the 64-bit header is fully committed to LayoutID and forwarding, we cannot steal bits for concurrent marking or remembered sets without violating the mutator's O(1) fast path.

Therefore, we formally adopt Side Metadata for GC state:

- **Mark Bitmap**: A dense bit array indexed by the 48-bit heap offset (shifted right by 3, due to 8-byte alignment). This requires only 2^45 bits ≈ 4 TB of virtual address space for the bitmap, which can be sparsely committed. The GC sets the bit corresponding to an object's offset during concurrent marking.
- **Card Table / Remembered Set**: A byte-per-region table for generational write barriers, also indexed by the heap offset.
- **Mutator Cost**: A write barrier sets a bit/byte in the side table. It does not modify the object header. This keeps the mutator fast and the header pristine.

### 5. AOSS Integration: The Invalidation Handshake

Let's trace a full lifecycle to prove this architecture holds together:

1. **Allocation**: Mutator allocates an object. It computes the 62-bit LayoutID, sets GC_State = 00, and writes it to offset 0.
2. **Execution**: JIT-compiled code executes. It guards a fast path with: `cmp [obj + 0], expected_LayoutID`. This is a single, aligned 64-bit load and compare.
3. **Mutation**: Mutator adds a field, changing the shape. It computes New_LayoutID, and writes it to `[obj + 0]`. Simultaneously, it logs `(obj_offset, Old_LayoutID, New_LayoutID)` into the thread-local ShapeTransitionBuffer.
4. **GC Phase**: GC reaches a safepoint. It scans the ShapeTransitionBuffer. For each Old_LayoutID, it queries the JIT's DependencyMap.
5. **Invalidation**: JIT marks dependent CodeBlobs as DEOPT_PENDING.
6. **Resumption**: Mutators resume. Any thread hitting the old compiled code will either deopt at the entry guard or at the next safepoint, falling back to generic code that can handle the new shape.

No locks were held by the mutator during invalidation. No header bloat was introduced.

### 6. Finalized HEAP-1 Layouts

#### 6.1 Array Layout

| Offset | Size | Content |
|--------|------|---------|
| 0 | 8 bytes | Header (62-bit LayoutID, 2-bit GC_State) |
| 8 | 8 bytes | Length & Flags. Bits 0–47: 48-bit unsigned length. Bits 48–63: Reserved/Element Type Hint. |
| 16 | 8 bytes | Element 0 (NV-1 value) |
| 24 | 8 bytes | Element 1 |
| ... | ... | ... |

**Note**: The 48-bit length supports arrays up to 281 trillion elements. The first element is perfectly 8-byte aligned at offset 16.

#### 6.2 String Layout (Compressed)

| Offset | Size | Content |
|--------|------|---------|
| 0 | 8 bytes | Header (62-bit LayoutID, 2-bit GC_State) |
| 8 | 8 bytes | Length & Encoding. Bits 0–47: 48-bit character count. Bits 48–63: Encoding flag (e.g., 0x0000 = Latin-1, 0x0001 = UTF-16). |
| 16 | 1 or 2 bytes | Character Data (1 byte per char for Latin-1, 2 bytes for UTF-16). |
| ... | ... | ... |

**Note**: The character data starts at offset 16. For Latin-1, the string is highly compact. For UTF-16, it is standard. The LayoutID will reflect the specific string encoding to allow the JIT to emit specialized access paths.

---

## NV-1: VM Value Representation

### Core Value Model

- **64-bit NaN-boxed values** — every VM value fits in a single 64-bit word
- **48-bit heap-relative offsets** for references (not raw pointers)
- **Native Address = heap_base + offset**
- 16-bit tag space in the upper bits
- Lower 48 bits carry the payload (integer, float bits, or heap offset)

### Representation Types (used in LLIR/MIR)

| Representation | Symbol | Physical Meaning | Register Class |
|---|---|---|---|
| Tagged | TAGGED | Full 64-bit NV-1 boxed value | GPR |
| Integer | I64 | Native 64-bit signed integer | GPR |
| Float | F64 | Native binary64 floating-point | FPR |
| Ref Offset | REF_OFF | 48-bit heap-relative offset (lower 16 bits zero or sign-extended) | GPR |
| Ref Raw | REF_RAW | Reconstructed native pointer (heap_base + offset) | GPR |
| Constant | CONST | Compile-time known value (may be immediate or materialized) | Any |
| Condition | COND | Boolean condition (may be a flag or a 0/1 integer) | GPR or flags |

### Conversion Instructions (LLIR)

| Instruction | Input | Output | Semantics |
|---|---|---|---|
| BOX_INT(i) | I64 | TAGGED | Encode integer as NV-1 tagged immediate (or allocate boxed if overflow) |
| UNBOX_INT(v) | TAGGED | I64 | Extract and sign-extend 48-bit integer payload |
| TAG_TO_REF(v) | TAGGED | REF_OFF | Extract 48-bit heap offset from tagged reference |
| REF_OFF_TO_RAW(off) | REF_OFF | REF_RAW | Compute heap_base + offset |
| RAW_TO_REF_OFF(ptr) | REF_RAW | REF_OFF | Compute ptr - heap_base |
| TAG_TO_F64(v) | TAGGED | F64 | Extract binary64 value (may require NaN normalization) |
| F64_TO_TAG(f) | F64 | TAGGED | Encode float as NV-1 (canonicalize if NaN collides with tag space) |
| REF_OFF_TO_TAG(off) | REF_OFF | TAGGED | Encode offset as tagged reference |
| TAG_TO_REF_RAW(v) | TAGGED | REF_RAW | Compound: extract offset, then reconstruct pointer |

---

## T0/T1: Baseline Compilation and Bytecode Representation

### 1. Bytecode Representation

Since the VM is register-based, the bytecode must explicitly reference virtual registers.

#### 1.1 Method Frame and Register Allocation

Every compiled method (or function) has a header defining its register usage:
- `arg_count`: Number of arguments passed to the method.
- `register_count`: Total number of virtual registers used by the method (including arguments).

The interpreter allocates a frame of size `register_count * 8 bytes` on the VM stack. All virtual registers are initially stored as 64-bit NV-1 tagged values.

#### 1.2 Instruction Encoding

**Decision: Variable-length / Hybrid encoding (Candidate C)**

- **Standard Instructions (1-byte registers)**: The opcode is followed by 1-byte register indices. This covers 95% of operations in typical methods, keeping the bytecode stream extremely compact and L1 I-cache friendly.
- **Extended Instructions (Prefix + 2-byte registers)**: A specific prefix opcode (e.g., 0xFF) indicates that the following instruction uses 2-byte register indices.

**Invariant T0-001**: The interpreter dynamically expands the VM frame if an extended instruction references a register index ≥ 256.

### 2. T0: The Interpreter

#### 2.1 Dispatch Mechanism: Computed Goto + Superinstructions

- **Computed Goto**: The interpreter uses an array of labels (or a jump table) to dispatch directly to the handler for the next opcode, eliminating the branch prediction penalty of a switch statement.
- **Superinstructions**: The bytecode stream can contain fused opcodes (e.g., LOAD_FIELD_AND_BRANCH, ADD_CONST_AND_STORE). The T0 profiler identifies frequent adjacent opcodes and dynamically rewrites the bytecode stream in memory to use superinstructions, reducing dispatch overhead by up to 30%.

#### 2.2 Polymorphic Inline Caches (PICs) in T0

T0 is not a dumb interpreter; it caches type feedback.

- At every property access or method call, the bytecode instruction is paired with an Inline Cache (IC) slot in the method's metadata.
- The IC slot holds an array of the last N (e.g., N=4) `(LayoutID, Offset/Target)` pairs.
- **Execution**: T0 checks the object's LayoutID against the IC. If it hits, T0 directly loads the field or jumps to the target without performing a full property lookup. If it misses, T0 performs the generic lookup, updates the IC, and proceeds.
- **Benefit**: This makes T0 exceptionally fast for monomorphic and low-polymorphism code, significantly delaying the need to tier-up to T1/T2.

#### 2.3 Profiling for Tier-Up

T0 must collect profiling data to decide when to trigger T1 or T2.
- **Back-edge counters**: Loops increment a counter. When it hits a threshold, the method is queued for T1 compilation.
- **Call-site counters**: Function calls increment a counter. When it hits a threshold, the method is queued for T1.
- **Type/Shape feedback**: T0 records the LayoutID of objects accessed at specific bytecode instructions (Inline Caches). This data is passed to the optimizing tiers.

### 3. T1: Stencil-Based Baseline Compiler (The Copy & Patch Architecture)

This is a radical departure from traditional baseline compilers (like C1 or Maglev). T1 does not build an IR, does not run an instruction selector, and does not run a graph-coloring register allocator.

T1's sole objective is minimum compilation latency. It achieves this by consuming pre-compiled machine code templates (Stencils).

#### 3.1 The Stencil Concept

At VM startup (or in a background thread), the engine pre-assembles thousands of Stencils. A stencil is a fully assembled, executable chunk of machine code for a specific bytecode operation, but with "holes" (relocations) left for runtime patching.

There are two primary variants of stencils for register operations:
- **Register-Register Stencil**: Assumes the VM virtual registers map to specific physical registers (e.g., v0 -> r8, v1 -> r9).
- **Register-Stack Stencil**: Assumes one or both operands are spilled to the VM stack frame.

#### 3.2 The Fixed-Map Physical Convention

To make stencils work without a register allocator, T1 enforces a strict physical mapping for the first K virtual registers (e.g., K=8 or 16, depending on the target ISA's callee-saved registers).

```
VM_Reg_0 → Phys_Reg_R8
VM_Reg_1 → Phys_Reg_R9
...
VM_Reg_K → Stack_Slot_0
```

#### 3.3 The Copy and Patch Algorithm

When T1 compiles a method, it executes the following O(N) algorithm:

1. **Allocate Code Buffer**: Reserve a contiguous block of executable memory.
2. **Iterate Bytecode**: For each bytecode instruction:
   - Determine if the operands are in the "Fast Physical Map" (registers < K) or spilled to the stack.
   - Select the corresponding Stencil (e.g., ADD_R_R or ADD_R_STACK).
   - **Copy**: memcpy the stencil's machine code bytes into the code buffer.
   - **Patch**: Fill in the "holes" in the stencil with the actual stack offsets or branch targets.
3. **Patch Branches**: Calculate the relative offsets for jumps and patch the branch instructions.
4. **Relocate**: Apply final relocations to make the code executable.

**Why this wins mathematically:**
- Traditional T1 compilation requires parsing bytecode, building LLIR, running a linear-scan allocator, selecting instructions, and assembling. That is O(N log N) with high constant factors.
- Stencil T1 is a simple memcpy and integer addition (for patching). It is effectively O(N) with a near-zero constant factor. Compilation latency drops from milliseconds to microseconds.

### 4. Advanced Deoptimization: Regions and Guard Fusion

Traditional deoptimization invalidates an entire CodeBlob (the whole compiled method) when a single assumption fails. This causes massive "deoptimization storms" and forces the recompilation of cold, unrelated code.

We will implement Region-Based Deoptimization with Guard Fusion.

#### 4.1 Defining Regions

A CodeBlob is not a monolithic block of code; it is partitioned into Regions.
- A Region is typically a natural loop, a straight-line block cluster, or a function body.
- Each Region has a single Entry Point and an Exit Point.

**Invariant DEOPT-001**: Control flow can only enter a Region at its Entry Point. (Internal branches loop back to the entry or stay within the region).

#### 4.2 Selective Invalidation

The AOSS DependencyMap is updated. Instead of mapping `LayoutID -> List<CodeBlob>`, it maps:

```
LayoutID → List<RegionDescriptor>
```

When the GC detects a shape transition and notifies the JIT:
1. The JIT identifies the specific RegionDescriptors that depend on the old LayoutID.
2. The JIT patches only the Entry Point of those specific regions. It overwrites the first few bytes of the region's entry with a jump to a Deoptimization Stub.
3. The rest of the CodeBlob (other regions, other loops) continues to execute normally.

#### 4.3 Guard Fusion (Minimizing Runtime Overhead)

Specialization requires guard checks (e.g., `cmp [obj], LayoutID`). If a method has 10 specialized loops, checking the guard at the entry of every loop adds significant runtime overhead.

Guard Fusion is an optimization applied during T2/T3 compilation (and selectively in T1 if profitable) to minimize guard checks.

- **Intra-Region Fusion**: If a loop checks LayoutID_A at the entry, and then checks LayoutID_A again inside the loop body, the inner check is eliminated. The entry guard dominates the inner check.
- **Inter-Region Fusion**: If Region A (a loop) and Region B (the subsequent straight-line code) both require LayoutID_A, and control flow strictly passes from A to B, the JIT fuses them into a single Region_AB. The guard is placed only at the entry of Region_AB.

**Invariant DEOPT-002**: A fused region is invalidated as a single unit. If the assumption fails, the entire fused region deopts.

#### 4.4 The Deoptimization Stub

When a patched region entry is hit, execution jumps to a Deopt Stub.
1. The stub captures the current physical register state and VM stack pointer.
2. It looks up the Safepoint Metadata for the exact PC where the deopt occurred.
3. It reconstructs the NV-1 tagged values for all live VM virtual registers.
4. It transitions the thread's execution state back to T0 (the interpreter) or directly to a newly compiled, un-specialized T1 stencil path, resuming exactly at the bytecode instruction that failed the assumption.

### 5. Summary of the Execution Contract

| Component | Mechanism | Primary Advantage |
|-----------|-----------|-------------------|
| T0 Interpreter | Computed Goto, Superinstructions, PICs | Extremely fast cold paths; delays tier-up via type caching. |
| T1 Compiler | Stencil Copy & Patch, Fixed Physical Map | Microsecond compilation latency; O(N) complexity. |
| Deoptimization | Region-based invalidation, Guard Fusion | Zero deopt storms; minimal runtime guard overhead. |

---

## T2/T3: Optimization Pipeline and IR Domain Mapping

### The Complete Optimization Pass Taxonomy

#### 1. Expression & Local Optimizations (Basic Block Level)

- **Constant Folding**: Evaluating constant expressions at compile time.
- **Constant Propagation**: Substituting variables with known constant values.
- **Copy Propagation**: Replacing uses of a variable with the variable it was directly copied from.
- **Common Subexpression Elimination (CSE)**: Replacing repeated identical computations with a single saved result.
- **Local Value Numbering (LVN)**: Assigning unique "value numbers" to expressions within a single basic block to find redundancies.
- **Dead Code Elimination (DCE)**: Removing instructions whose results are never used.
- **Dead Store Elimination (DSE)**: Removing memory writes that are overwritten before being read.
- **Algebraic Simplification**: Applying algebraic identities (e.g., `x * 1` → `x`).
- **Strength Reduction**: Replacing expensive operations with cheaper ones (e.g., `x * 2` → `x << 1`).
- **Tree Height Reduction**: Rearranging the evaluation order of an expression tree to minimize critical path length and maximize Instruction-Level Parallelism (ILP).
- **DAG Compression / DAG Combining**: Collapsing redundant nodes in a Directed Acyclic Graph representation of instructions.
- **Peephole Optimization**: Scanning a small "window" of generated instructions and replacing them with a shorter or faster equivalent sequence.

#### 2. Global & Control Flow Optimizations

- **Global Value Numbering (GVN)**: Extending LVN across basic block boundaries using dominance frontiers.
- **Sparse Conditional Constant Propagation (SCCP)**: Simultaneously propagating constants and eliminating dead code by evaluating branches with known constant conditions.
- **Partial Redundancy Elimination (PRE)**: Eliminating computations that are redundant on *some* (but not all) execution paths by inserting the computation on the missing paths.
- **Lazy Code Motion (LCM)**: An optimal form of PRE that delays computations as late as possible and hoists them as early as necessary to minimize register pressure.
- **Code Hoisting**: Moving computations as early as possible to ensure they are computed only once before a branching point.
- **Code Sinking**: Moving computations as late as possible down the CFG to avoid executing them on paths where their result is not needed.
- **Jump Threading**: Duplicating a block of code to turn an indirect or multi-hop branch into a direct, predictable branch.
- **Tail Duplication**: Duplicating the successors of a block to expose more jump threading or inlining opportunities.
- **Unreachable Code Elimination**: Removing basic blocks that have no incoming edges in the CFG.
- **Dead Argument Elimination (DAE)**: Removing function parameters that are never used by the callee.
- **Control Flow Simplification**: Merging trivial blocks, removing empty blocks, and simplifying branch conditions.

#### 3. Loop Optimizations (The Largest Category)

- **Loop Invariant Code Motion (LICM) / Loop Hoisting**: Moving computations that yield the same result on every iteration outside the loop body.
- **Loop Unrolling**: Duplicating the loop body multiple times to reduce branch overhead and expose ILP.
- **Unroll and Jam**: Unrolling an outer loop and "jamming" (fusing) the inner loops together to improve register reuse and cache locality.
- **Loop Fusion / Loop Jamming**: Combining two adjacent loops that iterate over the same range into a single loop.
- **Loop Fission / Loop Distribution**: Splitting a single loop into multiple loops to isolate independent statements, improving cache locality or enabling vectorization.
- **Loop Interchange**: Swapping the order of nested loops to ensure the innermost loop accesses memory in a contiguous, cache-friendly stride.
- **Loop Tiling / Loop Blocking**: Partitioning loop iterations into smaller "tiles" or "blocks" that fit entirely within the CPU cache.
- **Loop Unswitching**: Moving a loop-invariant conditional branch outside the loop, duplicating the loop body for the `true` and `false` cases.
- **Loop Versioning**: Creating multiple runtime versions of a loop, guarded by checks (e.g., aliasing checks), to allow aggressive optimization on the "safe" version.
- **Loop Peeling**: Executing the first few (or last few) iterations of a loop outside the loop body to simplify the conditions inside the main loop.
- **Loop Rerolling / Loop Collapsing**: The inverse of unrolling; detecting unrolled loop patterns and compressing them back into a compact loop to reduce I-cache pressure.
- **Loop Inversion**: Converting a `while` loop into a `do-while` loop with a preceding conditional branch to reduce the number of branches executed.
- **Induction Variable Elimination**: Removing redundant loop counters by expressing them as simple linear functions of a primary induction variable.
- **Loop Strength Reduction**: Replacing expensive operations inside a loop (like multiplication) with cheaper ones (like addition) based on the loop induction variable.
- **Loop Normalization / Canonicalization**: Transforming loops to have a lower bound of 0 and a step of 1 to simplify subsequent analyses.

#### 4. Vectorization & Parallelization

- **Loop Vectorization (Auto-vectorization)**: Transforming scalar loop operations into SIMD (Single Instruction, Multiple Data) instructions.
- **Superword Level Parallelism (SLP) Vectorization**: Packing adjacent, independent scalar operations within a *single basic block* into SIMD instructions, even outside of loops.
- **Predicated / Masked Vectorization**: Using hardware mask registers to safely vectorize loops with conditional branches or irregular memory accesses.
- **Reduction Tree Optimization**: Reassociating commutative/associative operations (like summing an array) into a balanced tree to maximize SIMD utilization.
- **Loop Parallelization / Auto-parallelization**: Inserting threading constructs (like OpenMP) into loops proven to have no cross-iteration dependencies.
- **Thread-Level Speculation (TLS)**: Speculatively executing loop iterations in parallel, with hardware/software rollback mechanisms if a dependency violation is detected.

#### 5. Interprocedural & Whole-Program Optimizations (IPO/WPO)

- **Function Inlining / Inline Expansion**: Replacing a function call with the actual body of the called function.
- **Partial Inlining**: Inlining only the "hot" (frequently executed) paths of a function, leaving the "cold" paths as a separate function call.
- **Function Cloning / Clone Specialization**: Creating a specialized copy of a function optimized for a specific set of constant arguments or type contexts.
- **Outline Extraction / Function Outlining**: The inverse of inlining; extracting cold, rarely executed code (like error handling) into a separate function to improve I-cache density of the hot path.
- **Dead Function Elimination (DFE)**: Removing entire functions that are never called anywhere in the program.
- **Identical Code Folding (ICF) / COMDAT Folding**: Merging separate functions that compile to the exact same machine code into a single shared instance.
- **Interprocedural Constant Propagation**: Propagating constant arguments from a caller into the callee's body.
- **Argument Promotion**: Replacing a pointer/reference argument with its individual scalar fields if the function only accesses those fields.
- **Return Value Optimization (RVO) / Named Return Value Optimization (NRVO)**: Eliminating the temporary object created to hold a function's return value by constructing it directly in the caller's stack frame.
- **Link-Time Optimization (LTO) / Whole-Program Optimization (WPO)**: Deferring optimization until the linking stage to allow IPO across separately compiled translation units.
- **Profile-Guided Optimization (PGO) / Feedback-Directed Optimization (FDO)**: Using runtime profiling data to guide inlining, branching, and layout decisions.

#### 6. Type, Alias, and Memory Optimizations

- **Class Hierarchy Analysis (CHA)**: Building a graph of class inheritances to enable devirtualization.
- **Rapid Type Analysis (RTA)**: A more precise, flow-insensitive type analysis that only considers classes that are actually instantiated.
- **Variable Type Analysis (VTA)**: Tracking the flow of object types through variables to enable precise devirtualization.
- **Devirtualization / Speculative Devirtualization**: Resolving dynamic dispatch (virtual methods, interfaces) into direct, static function calls.
- **Points-to Analysis / Alias Analysis (e.g., Andersen's, Steensgaard's)**: Determining whether two pointers can possibly refer to the same memory location.
- **Shape Analysis**: Analyzing the topological structure of heap-allocated data structures (e.g., proving a pointer points to an acyclic linked list).
- **Escape Analysis**: Determining if an object's reference "escapes" the current method or thread.
- **Scalar Replacement of Aggregates (SROA)**: Breaking down arrays or structs into individual scalar variables (registers) when they do not escape.
- **Array Bounds Check Elimination (ABCE)**: Proving that array accesses are within valid bounds, allowing the compiler to remove runtime safety checks.
- **Null Check Elimination**: Proving a pointer/reference cannot be null at a specific point, removing the runtime null check.
- **Write Barrier Elision**: Removing garbage collector write barriers when escape analysis proves an object is newly allocated and cannot be seen by other threads.
- **Read Barrier Elision**: Removing GC read barriers when thread-local or provably safe.
- **Lock Elision / Lock Coarsening**: Removing or merging synchronization primitives when escape analysis proves an object is thread-local.
- **Store Merging / Write Combining**: Combining multiple adjacent, narrow memory writes (e.g., four 1-byte stores) into a single wider memory write.
- **Load-Store Forwarding / Store-Load Forwarding**: Replacing a memory load with a value from a preceding store to the same address.

### IR Domain Mapping

#### 1. VSDG Domain: Semantic & Dependence Analysis

**Mandate**: Understand what is computed and why, independent of explicit control flow. Focus on value/state dependencies, side-effect ordering, and high-level semantic facts.

**Assigned Passes:**
- **Type & Alias Analysis**: Class Hierarchy Analysis (CHA), Rapid Type Analysis (RTA), Variable Type Analysis (VTA), Points-to/Alias Analysis, Shape Analysis.
- **High-Level Specialization**: Speculative Devirtualization, Function Cloning / Clone Specialization, Partial Evaluation foundations.
- **Lifetime & Escape**: Escape Analysis (determining if an object escapes the method/thread).
- **Local Semantic Simplification**: Constant Folding, Algebraic Simplification, Strength Reduction (local), Tree Height Reduction (for ILP exposure).
- **Functional Transformations**: Lambda Lifting, Closure Conversion, Strictness Analysis (if UGB semantics require it).

**Why here?** VSDG excels at proving that "Operation A's result is independent of State B's mutation" without needing to traverse explicit basic blocks or dominance frontiers.

#### 2. Sea of Nodes Domain: Global Graph & Control-Flow Optimization

**Mandate**: Optimize how the computation is structured across explicit control flow. Focus on redundancy elimination, loop transformations, and guard management.

**Assigned Passes:**
- **Global Redundancy Elimination**: Global Value Numbering (GVN), Partial Redundancy Elimination (PRE), Lazy Code Motion (LCM), Common Subexpression Elimination (CSE).
- **Loop Optimizations**: Loop Invariant Code Motion (LICM), Loop Unrolling, Loop Fusion/Fission, Loop Tiling, Loop Strength Reduction, Induction Variable Elimination, Loop Unswitching.
- **Control Flow Manipulation**: Jump Threading, Tail Duplication, Control Flow Simplification, Unreachable Code Elimination.
- **Speculative Execution Management**: Guard Insertion, Guard Fusion (treating guard checks as values to be GVN'd or PRE'd), Speculative Load Hoisting.
- **Interprocedural**: Function Inlining, Partial Inlining, Dead Argument Elimination (DAE).

**Why here?** PRE, LICM, and Guard Fusion fundamentally require dominance frontiers and explicit control dependence graphs. Sea of Nodes provides the region/phi-node infrastructure necessary to safely move code across basic block boundaries.

#### 3. Low-Level IR (LLIR) Domain: Representation Commitment

**Mandate**: Commit to explicit, machine-independent representations. Introduce architecture-neutral low-level details and prepare for physical lowering.

**Assigned Passes:**
- **Aggregates**: Scalar Replacement of Aggregates (SROA) (Note: Escape Analysis proves it's safe in VSDG; SROA physically breaks the struct/array into SSA scalars here).
- **Safety Elision**: Array Bounds Check Elimination (ABCE), Null Check Elimination, Lock Elision, Write/Read Barrier Elision (based on VSDG escape proofs).
- **Local Cleanup**: Dead Code Elimination (DCE), Dead Store Elimination (DSE), Copy Propagation, Local Value Numbering (LVN).
- **Runtime Integration**: Explicit Deoptimization Point (Safepoint) insertion, Exception Edge materialization.

**Why here?** LLIR is the last stage before machine specifics. It must resolve high-level abstractions (like "this object doesn't escape") into concrete SSA variables and explicit memory operations.

#### 4. Machine IR (MIR) Domain: Physical Realization

**Mandate**: Map LLIR to target-specific hardware constraints.

**Assigned Passes:**
- **Resource Allocation**: Register Allocation (Linear Scan for T1/T2, Graph Coloring/Iterated Coalescing for T3), Live Range Splitting, Stack Slot Coloring/Sharing.
- **Instruction Level**: Instruction Selection (DAG/Tree Pattern Matching), Instruction Scheduling (List Scheduling), If-Conversion / Predication, Peephole Optimization, Branch Shortening.
- **Target Specifics**: Delay Slot Filling (if applicable), Frame Pointer Omission.

### The VSDG → Sea of Nodes Conversion Boundary

This is the most critical handshake in the optimizing pipeline. If done poorly, we lose semantic guarantees or pay massive reconstruction costs.

#### 1. Input Requirements (From VSDG)

The VSDG must provide the Sea of Nodes constructor with:
- **Value Nodes**: Pure computations with explicit data dependencies. Each value node is tagged with its proven type/shape facts (e.g., `Proven: LayoutID == 0x4A`).
- **State/Memory Nodes**: Explicit tokens representing side-effect ordering (e.g., `Memory(State_in) -> Load -> Memory(State_out)`).
- **Guarded Assumptions**: Explicit Assume nodes (e.g., `Assume(LayoutID == 0x4A)`), which the Sea of Nodes will later lower into actual control-flow guards.
- **Region Boundaries**: High-level loop and exception region markers, without explicit branch instructions.

#### 2. Output Guarantees (To Sea of Nodes)

The resulting Sea of Nodes graph must guarantee:
- **Strict SSA Form**: All values are defined exactly once. Phi nodes (φ) or Block Arguments are inserted at dominance frontiers for values that cross high-level region boundaries.
- **Explicit Control Edges**: Every node is control-dependent on a specific region or basic block entry.
- **Preserved Dependence**: All data and memory dependencies from the VSDG are strictly preserved. No reordering of side-effects is permitted.
- **Guard Normalization**: All Assume nodes are converted into explicit Guard nodes that yield a boolean value and a control-flow edge to a deoptimization stub.

#### 3. Information Lost / Transformed

- **Lost**: The VSDG's implicit, global view of state. Sea of Nodes fragments this into explicit memory state tokens passed between basic blocks.
- **Transformed**: Cyclic dependencies in VSDG (if used for loops) are unrolled or converted into explicit back-edges with Phi nodes in Sea of Nodes.

### Synergy with AOSS and Region-Based Deoptimization

- **Guard Fusion is PRE/GVN**: In Sea of Nodes, a `Guard(LayoutID == X)` is treated as a value-producing node. Global Value Numbering (GVN) will automatically identify redundant guards in the same region. Partial Redundancy Elimination (PRE) will hoist a guard out of a loop (Loop Unswitching) or fuse guards across region boundaries, perfectly matching your "Guard Fusion" requirement.
- **SROA enables Unboxed Live Ranges**: VSDG's Escape Analysis proves an object doesn't escape. LLIR's SROA pass breaks it into scalar NV-1 values. MIR's Register Allocation keeps those scalars in physical registers, achieving the "Hybrid tagged and unboxed" execution model we defined in NV-1.
- **Devirtualization + Inline Caching**: T0's Polymorphic Inline Cache (PIC) provides the profiling data. T2's Sea of Nodes uses Speculative Devirtualization to replace the indirect call with a direct call, guarded by a `Guard(LayoutID == X)`. If the GC invalidates this, the Region-Based Deopt patches only that specific call site's region.

---

## VSDG-1: Node Taxonomy and Dependence Rules

### 1. Fundamental Node Categories

The VSDG represents computation as a directed graph where nodes are classified into five mutually exclusive categories:

| Category | Symbol | Purpose | Produces |
|----------|--------|---------|----------|
| Value | V | Pure computations, data transformations | A 64-bit NV-1 value |
| State | S | Side-effect ordering, memory operations | A state token (abstract) |
| Control | C | Branching, merging, region boundaries | Control-flow tokens |
| Guard | G | Speculative assumptions, runtime checks | Boolean + control edge to deopt |
| Region | R | Loop headers, exception scopes, function boundaries | Region entry/exit tokens |

### 2. Edge Types (Dependencies)

Edges in the VSDG represent dependencies. There are four types:

#### 2.1 Data Dependency (→d)

- **From**: Value node Vi
- **To**: Value node Vj or State node Sk
- **Semantics**: Vj requires the result of Vi to execute.
- **Example**: ADD(V1, V2) has data edges from V1 and V2.

#### 2.2 State Dependency (→s)

- **From**: State node Si
- **To**: State node Sj or Value node Vk (for loads/stores)
- **Semantics**: Sj must execute after Si. Represents side-effect ordering.
- **Example**: STORE(addr, val) has a state edge from the previous memory state and produces a new memory state.

#### 2.3 Control Dependency (→c)

- **From**: Control node Ci or Region node Rj
- **To**: Any node Nk
- **Semantics**: Nk is control-dependent on Ci or Rj. It only executes if control reaches that point.
- **Example**: A BRANCH(cond, true_target, false_target) node has control edges to both targets.

#### 2.4 Guard Dependency (→g)

- **From**: Guard node Gi
- **To**: Any node Nj that relies on the assumption
- **Semantics**: Nj is only valid if Gi's assumption holds. If the assumption fails, execution transfers to a deoptimization stub.
- **Example**: A specialized field load guarded by `Assume(LayoutID == X)` has a guard edge from the Assume node.

### 3. Specific Node Types

#### 3.1 Value Nodes (V)

| Node Type | Inputs | Outputs | Semantics |
|-----------|--------|---------|-----------|
| CONST(val) | None | V | Produces a compile-time constant NV-1 value |
| ADD(a, b) | Va, Vb | V | NV-1 tagged addition with overflow handling |
| SUB(a, b) | Va, Vb | V | NV-1 tagged subtraction |
| MUL(a, b) | Va, Vb | V | NV-1 tagged multiplication |
| DIV(a, b) | Va, Vb | V | NV-1 tagged division (may trap on zero) |
| LOAD(addr) | Vaddr, Smem | V, Smem' | Loads a 64-bit NV-1 value from memory |
| STORE(addr, val) | Vaddr, Vval, Smem | Smem' | Stores a 64-bit NV-1 value to memory |
| CALL(target, args...) | Vtarget, Vargs..., Sstate | Vresult, Sstate' | Invokes a function (may be direct or indirect) |
| PHI(v1, v2, ...) | V1, V2, ... | V | Merges values from different control paths (Sea of Nodes will materialize this) |
| EXTRACT_TAG(v) | V | V | Extracts the 16-bit tag from an NV-1 value |
| EXTRACT_PAYLOAD(v) | V | V | Extracts the 48-bit payload from an NV-1 value |
| BOX_INT(i) | Vi | V | Converts a native integer to NV-1 tagged integer |
| UNBOX_INT(v) | V | V | Converts an NV-1 tagged integer to native integer |

#### 3.2 State Nodes (S)

| Node Type | Inputs | Outputs | Semantics |
|-----------|--------|---------|-----------|
| MEMORY(state_in) | Sin | Sout | Represents the abstract memory state |
| FRAME(state_in) | Sin | Sout | Represents the VM stack frame state |
| EXCEPTION(state_in) | Sin | Sout | Represents exceptional control flow state |
| WORLD(state_in) | Sin | Sout | Represents all external side effects (I/O, system calls) |

#### 3.3 Control Nodes (C)

| Node Type | Inputs | Outputs | Semantics |
|-----------|--------|---------|-----------|
| ENTRY | None | C | Function entry point |
| EXIT(result) | Vresult, Sstate | None | Function exit, returns result |
| BRANCH(cond, T, F) | Vcond, Cin | CT, CF | Conditional branch |
| JUMP(target) | Cin | Ctarget | Unconditional jump |
| MERGE(c1, c2, ...) | C1, C2, ... | Cout | Merges control flow from multiple predecessors |
| LOOP_HEADER(body, exit) | Cbody, Cexit | Centry | Marks the entry of a loop |
| LOOP_BACK(body) | Cbody | Cheader | Back-edge of a loop |

#### 3.4 Guard Nodes (G)

| Node Type | Inputs | Outputs | Semantics |
|-----------|--------|---------|-----------|
| ASSUME_LAYOUT(obj, expected_id) | Vobj, Vexpected | G | Asserts object's LayoutID matches expected |
| ASSUME_TYPE(v, expected_tag) | Vv, Vexpected | G | Asserts NV-1 tag matches expected |
| ASSUME_NONNULL(v) | Vv | G | Asserts value is not null |
| ASSUME_BOUNDS(idx, len) | Vidx, Vlen | G | Asserts array index is within bounds |
| ASSUME_ALIAS(a, b, alias) | Va, Vb, Valias | G | Asserts whether two pointers alias |

Each guard node has an implicit control edge to a deoptimization stub if the assumption fails.

#### 3.5 Region Nodes (R)

| Node Type | Inputs | Outputs | Semantics |
|-----------|--------|---------|-----------|
| FUNCTION_REGION(entry, exit) | Centry, Cexit | R | Marks a function boundary |
| LOOP_REGION(header, body, exit) | Cheader, Cbody, Cexit | R | Marks a loop boundary (for Region-Based Deopt) |
| EXCEPTION_REGION(try, catch) | Ctry, Ccatch | R | Marks an exception handling scope |

### 4. VSDG Invariants

#### 4.1 Structural Invariants

- **INV-VSDG-001**: Every value node has exactly one definition (SSA-like property, though VSDG is not strictly SSA yet).
- **INV-VSDG-002**: State nodes form a total order within each state chain (memory, frame, exception, world).
- **INV-VSDG-003**: Control nodes form a directed acyclic graph (DAG) except for explicit loop back-edges.
- **INV-VSDG-004**: Guard nodes dominate all nodes that depend on them via guard edges.
- **INV-VSDG-005**: Region nodes properly nest (no crossing region boundaries).

#### 4.2 Semantic Invariants

- **INV-VSDG-006**: All memory operations (LOAD, STORE) have explicit state dependencies on the MEMORY state chain.
- **INV-VSDG-007**: All function calls have explicit state dependencies on the WORLD state chain (for I/O effects).
- **INV-VSDG-008**: All speculative operations have explicit guard dependencies.
- **INV-VSDG-009**: No node may execute if its control dependency is not satisfied.
- **INV-VSDG-010**: Guard failure transfers control to a deoptimization stub that reconstructs the exact interpreter state.

### 5. Loop Representation

Loops in VSDG are represented using explicit LOOP_HEADER and LOOP_BACK nodes, forming a cycle in the control graph.

**Example: Simple loop**
```
LOOP_HEADER(L1)
  body:
    ADD(V1, V2) -> V3
    STORE(addr, V3)
  LOOP_BACK(L1)
EXIT
```

The cycle is: LOOP_HEADER -> body -> LOOP_BACK -> LOOP_HEADER.

This explicit cycle representation allows the optimizer to:
- Identify loop-invariant computations (nodes outside the cycle but dominating the loop).
- Perform loop unrolling by duplicating the cycle.
- Apply loop strength reduction by analyzing induction variables within the cycle.

### 6. Exception Representation

Exceptions are modeled using EXCEPTION_REGION nodes and the EXCEPTION state chain.

**Example: Try-catch**
```
EXCEPTION_REGION(try_block, catch_block)
  try_block:
    CALL(target, args) -> result
    // If CALL throws, control transfers to catch_block
  catch_block:
    HANDLE_EXCEPTION()
```

The CALL node has an implicit exceptional edge to the catch_block if the function throws. The VSDG does not need explicit exception edges for every operation; the EXCEPTION_REGION scope captures all exceptional control flow within it.

### 7. Conversion Boundary: VSDG → Sea of Nodes

The conversion from VSDG to Sea of Nodes performs the following transformations:

#### 7.1 Input (VSDG)
- Value nodes with data dependencies
- State nodes with state dependencies
- Control nodes with control dependencies (including cycles for loops)
- Guard nodes with guard dependencies
- Region nodes marking boundaries

#### 7.2 Transformation Steps
1. **Linearize Control Flow**: Convert the VSDG's control graph into an explicit Control Flow Graph (CFG) with basic blocks.
2. **Insert Phi Nodes**: At dominance frontiers, insert PHI nodes to merge values from different control paths. This converts the VSDG into strict SSA form.
3. **Materialize Guards**: Convert ASSUME_* guard nodes into explicit Guard nodes with boolean outputs and control edges to deoptimization stubs.
4. **Flatten State Chains**: Convert the abstract state chains into explicit memory tokens passed between basic blocks.
5. **Preserve Dependencies**: All data, state, control, and guard dependencies from the VSDG are strictly preserved in the Sea of Nodes graph.

#### 7.3 Output (Sea of Nodes)
- Strict SSA form with explicit phi nodes
- Explicit basic blocks with control-flow edges
- Explicit guard nodes with deoptimization targets
- Explicit memory state tokens
- All VSDG invariants preserved

#### 7.4 Information Lost
- The VSDG's implicit, global view of state is fragmented into explicit memory tokens.
- Cyclic control dependencies are unrolled into explicit back-edges with phi nodes.
- Region boundaries are preserved but may be fused or split by Sea of Nodes optimizations (e.g., Guard Fusion).

### 8. Example: VSDG for a Simple Function

**Source code (conceptual):**
```
function add_fields(obj, x, y):
    field_a = obj.field_a
    field_b = obj.field_b
    return field_a + field_b + x + y
```

**VSDG representation:**
```
ENTRY
LOAD(obj, MEMORY_0) -> V_obj, MEMORY_1
ASSUME_LAYOUT(V_obj, expected_id) -> G1
LOAD_FIELD(V_obj, offset_a, MEMORY_1) -> V_a, MEMORY_2  [guarded by G1]
LOAD_FIELD(V_obj, offset_b, MEMORY_2) -> V_b, MEMORY_3  [guarded by G1]
ADD(V_a, V_b) -> V_ab
ADD(V_ab, x) -> V_abx
ADD(V_abx, y) -> V_result
EXIT(V_result, MEMORY_3)
```

**Dependencies:**
- LOAD_FIELD nodes are data-dependent on V_obj
- LOAD_FIELD nodes are state-dependent on MEMORY_1, MEMORY_2
- LOAD_FIELD nodes are guard-dependent on G1
- ADD nodes are data-dependent on their operands
- EXIT is state-dependent on MEMORY_3

This VSDG is then converted to Sea of Nodes, where the ASSUME_LAYOUT becomes an explicit guard with a deoptimization edge, and the LOAD_FIELD operations can be optimized (e.g., CSE if obj is proven not to mutate between loads).

---

## Sea of Nodes-1: Graph Structure and Optimization Passes

Following the strict Design A boundary, the Sea of Nodes (SoN) tier receives the semantically analyzed, dependence-rich graph from the VSDG and transforms it into an explicitly structured, control-flow-aware, Strict Static Single Assignment (SSA) graph.

Its mandate is structural optimization: eliminating redundancy, moving code across control-flow boundaries, and fusing guards, without altering the fundamental semantic guarantees established by the VSDG.

### 1. Core Graph Structure

The SoN graph is a directed graph G = (N, E), partitioned into Basic Blocks (B).

#### 1.1 Basic Blocks

A basic block Bi is a sequence of nodes with:
- A single Entry Point (a BlockHeader node).
- A single Exit Point (a Branch, Jump, or Return node).
- No internal control-flow transfers.

#### 1.2 Node Types in SoN

Nodes in SoN are categorized by the type of edge they produce/consume:
- **Data Nodes (Nd)**: Pure computations (e.g., Add, Load, Call). Produce data values.
- **Control Nodes (Nc)**: BlockHeader, If, Jump, Return. Produce control tokens.
- **State/Memory Nodes (Ns)**: MemoryPhi, MemoryState. Produce abstract memory state tokens.
- **Guard Nodes (Ng)**: GuardLayout, GuardType. Produce a boolean control edge (continuation) and a deoptimization edge (failure).

#### 1.3 Edge Types

- **Control Edges (→c)**: Connect Nc to Nc or Nc to the first node in a block. Define execution order.
- **Data Edges (→d)**: Connect Nd to Nd. Define value dependencies.
- **State Edges (→s)**: Connect Ns to Ns or Nd (for loads/stores). Define side-effect ordering.
- **Phi Edges (→φ)**: Connect the last node of a predecessor block to a Phi node in the current block.

### 2. Strict SSA and Phi Nodes

To enable global optimization, all values must be in Strict SSA form.

#### 2.1 Data Phi Nodes

When a value v is defined in multiple predecessor blocks and used in a successor block Bmerge, a Data Phi Node is inserted at the entry of Bmerge:

```
v_merge = φ(v_pred1, v_pred2, …, v_predk)
```

**Invariant SON-001**: Every data value is defined exactly once. Phi nodes are the only exception, and they must reside at the dominance frontier of the defining blocks.

#### 2.2 Memory Phi Nodes (Memory SSA)

Unlike VSDG's abstract state chains, SoN makes memory state explicit to enable precise alias analysis and load/store optimization.

When memory is mutated in divergent control paths, a Memory Phi Node merges the state:

```
mem_merge = φ_mem(mem_pred1, mem_pred2, …, mem_predk)
```

**Invariant SON-002**: Every Load and Store node takes an explicit MemoryState input and produces a new MemoryState output. This creates a precise, versioned history of memory, allowing the optimizer to prove that a Load is independent of a distant Store.

### 3. Core Optimization Passes

These passes operate on the SoN graph. They are ordered to maximize profitability and minimize redundant traversals.

#### 3.1 Global Value Numbering (GVN) & Common Subexpression Elimination (CSE)

**Objective**: Identify and eliminate redundant computations across the entire method.

**Mechanism:**
1. Traverse the graph in dominance order.
2. Assign a "Value Number" to each node based on its opcode and the Value Numbers of its inputs (data and memory state).
3. If a node N2 has the same opcode and identical input Value Numbers as an already-visited node N1, N2 is redundant.
4. **Action**: Replace all uses of N2 with N1, and delete N2 (Dead Code Elimination).

**Crucial for Guards**: `Guard(LayoutID == X)` is treated as a value-producing node. If two guards check the same condition and are on the same control path, GVN eliminates the second one. This is the foundational mechanism for Guard Fusion.

#### 3.2 Partial Redundancy Elimination (PRE) & Lazy Code Motion (LCM)

**Objective**: Eliminate computations that are redundant on some but not all execution paths, by inserting them on the missing paths to minimize register pressure.

**Mechanism:**
1. Identify an expression E computed in block B2, which is also computed in a predecessor B1.
2. If E is loop-invariant or dominates a merge point, PRE hoists E to the dominance frontier.
3. **LCM Refinement**: Instead of hoisting E as early as possible (which increases register pressure), LCM delays the insertion of E as late as possible while still guaranteeing it is computed before any use.

**Application to Guards**: PRE is the primary engine for Inter-Region Guard Fusion. If Region A and Region B both require `Guard(LayoutID == X)`, and control flows strictly A → B, PRE will hoist the guard to the entry of Region A, fusing the checks and eliminating the redundant check in Region B.

#### 3.3 Loop Optimizations

Loops are identified via back-edges in the control graph. The following passes operate on loop regions:

- **Loop Invariant Code Motion (LICM)**: Identifies nodes inside a loop whose inputs are defined outside the loop (or are loop-invariant Phis) and which have no side effects (no MemoryState mutation). Hoists them to the loop pre-header.
- **Loop Unswitching**: If an If node inside a loop depends only on loop-invariant values, duplicate the loop body. One copy assumes the true branch, the other assumes the false branch. This exposes massive optimization opportunities (like vectorization or aggressive LICM) inside the duplicated bodies.
- **Induction Variable Analysis & Strength Reduction**: Identifies variables that increment by a constant each iteration. Replaces expensive operations (e.g., `base + (i * stride)`) with a simple addition (`ptr = ptr + stride`) in the loop body.

#### 3.4 Control Flow Simplification

- **Jump Threading**: If a block ends in a conditional branch, and the condition is a known constant (via SCCP) or a Phi node that resolves to a constant along a specific path, duplicate the block's successors and thread the jump directly to the resolved target.
- **Tail Duplication**: Duplicates the single successor of multiple blocks to expose more Jump Threading or Inlining opportunities.
- **Unreachable Code Elimination**: Removes blocks with no incoming control edges after simplification.

### 4. Synergy with AOSS and Region-Based Deoptimization

The SoN tier is where the AOSS specialization model becomes physically manifest in the graph.

- **Guard Insertion**: The VSDG provides ASSUME_LAYOUT nodes. SoN lowers these into explicit `GuardLayout(obj, expected_id, deopt_target)` nodes.
- **Guard Fusion (via GVN/PRE)**: As described above, redundant guards are mathematically eliminated. A single guard at the entry of a fused region protects all subsequent specialized operations (e.g., direct field loads, devirtualized calls) within that region.
- **Speculative Devirtualization**: An indirect Call node is replaced by a direct Call to a specific target, preceded by a `GuardType(receiver, expected_id)`. If the guard fails, it deopts. GVN ensures this guard is not duplicated if the receiver's type was already guarded earlier in the block.
- **Trace Compilation Integration**: If optional tracing is enabled, a "trace" is simply a SoN graph extracted from the interpreter's hot path. It is inherently a single-entry, single-exit graph with heavy guard nodes at every type-sensitive operation. The exact same SoN passes (GVN, PRE, LICM) optimize the trace.

### 5. Invariants and Conversion Boundary (SoN → LLIR)

Before the graph is passed to Low-Level IR (LLIR), it must satisfy strict invariants. The SoN tier is not allowed to weaken VSDG's semantic guarantees.

| Invariant | Description |
|-----------|-------------|
| SON-INV-01 | Strict SSA: Every data and memory value is defined exactly once. |
| SON-INV-02 | Dominance: Every use of a value is dominated by its definition. |
| SON-INV-03 | Side-Effect Preservation: The topological order of all MemoryState and WorldState mutations is identical to the VSDG's state chain order. No reordering of visible side effects is permitted. |
| SON-INV-04 | Guard Completeness: Every speculative operation (devirtualized call, unchecked array load, unboxed field access) is strictly control-dependent on a valid Guard node that dominates it. |
| SON-INV-05 | Deopt Reachability: Every Guard node has a valid, computable path to a deoptimization stub that can reconstruct the VSDG/interpreter state. |

**What is Lost/Transformed in this Boundary?**
- **Lost**: The VSDG's global, implicit view of state. It is now fragmented into explicit MemoryState tokens and Phi nodes.
- **Transformed**: High-level VSDG Region nodes are mapped to explicit SoN control-flow structures (loop pre-headers, back-edges, exception handlers).

### 6. Example: SoN Transformation of a Guarded Load

**VSDG Input:**
```
LOAD_FIELD(obj, offset_a, MEMORY_1) [guarded by ASSUME_LAYOUT(obj, 0x4A)]
```

**SoN Output (after conversion and GVN/PRE):**
```
Block_PreHeader:
    mem_0 = ...
    guard_1 = GuardLayout(obj, 0x4A, deopt_stub_1)
    Jump Block_Loop

Block_Loop:
    mem_1 = Phi(mem_0, mem_3)
    // GVN proves obj's layout is still 0x4A here, no new guard needed.
    // LICM hoisted this load out of the loop if offset_a is invariant.
    val_a = LoadField(obj, offset_a, mem_1) [Control: guard_1]
    ...
```

---

## LLIR-1: Low-Level IR and Representation Commitment

The Low-Level IR is the critical transition boundary where abstract optimization meets physical reality. It receives the optimized, strict-SSA Sea of Nodes graph and transforms it into an explicitly scheduled, representation-committed, machine-independent instruction stream ready for target-specific lowering.

This is where the Adaptive Representation Selection (ARS) model from NV-1 becomes physically manifest, where safepoints are materialized with precise metadata, and where the "hybrid tagged and unboxed" execution model is actually constructed.

### 1. Core LLIR Structure

LLIR abandons the graph-based representation of SoN in favor of an explicit linear instruction sequence organized into basic blocks.

#### 1.1 Basic Blocks

Each basic block Bi contains:
- **Block Header**: Block ID, predecessor list, successor list, exception handler scope.
- **Instruction Sequence**: A linear list of instructions I1, I2, …, In.
- **Terminator**: The final instruction, which is always one of:
  - `BRANCH(cond, true_target, false_target)`
  - `JUMP(target)`
  - `RETURN(value)`
  - `DEOPT(safepoint_id, vm_state)`
  - `THROW(exception_value)`

#### 1.2 Instruction Format

Every instruction Ik has the form:

```
Ik: opcode(op1, op2, …, opn) → (r1, r2, …, rm) [metadata]
```

Where:
- opi are operands (registers, constants, or memory locations).
- rj are result registers (virtual registers in LLIR, physical in MIR).
- Metadata includes: representation types, safepoint info, side-effect flags, guard dependencies.

### 2. Operand and Representation Model

This is the heart of LLIR. Every operand and result has an explicit representation type chosen by ARS.

#### 2.1 Representation Types

| Representation | Symbol | Physical Meaning | Register Class |
|---|---|---|---|
| Tagged | TAGGED | Full 64-bit NV-1 boxed value | GPR |
| Integer | I64 | Native 64-bit signed integer | GPR |
| Float | F64 | Native binary64 floating-point | FPR |
| Ref Offset | REF_OFF | 48-bit heap-relative offset (lower 16 bits zero or sign-extended) | GPR |
| Ref Raw | REF_RAW | Reconstructed native pointer (heap_base + offset) | GPR |
| Constant | CONST | Compile-time known value (may be immediate or materialized) | Any |
| Condition | COND | Boolean condition (may be a flag or a 0/1 integer) | GPR or flags |

#### 2.2 Representation Commitment Rules

- **INV-LLIR-001**: Every virtual register has exactly one representation type for its entire live range within a single basic block. (Cross-block, a register may change representation if explicitly converted.)
- **INV-LLIR-002**: Every instruction's operands must match the instruction's expected representation types. Mismatches require explicit conversion instructions.
- **INV-LLIR-003**: REF_RAW values may only exist within a non-safepoint region (a sequence of instructions with no calls, back-edges, or explicit yield points). Crossing a safepoint requires converting back to REF_OFF.

#### 2.3 Conversion Instructions

When a value must change representation, explicit conversion instructions are inserted:

| Instruction | Input | Output | Semantics |
|---|---|---|---|
| BOX_INT(i) | I64 | TAGGED | Encode integer as NV-1 tagged immediate (or allocate boxed if overflow) |
| UNBOX_INT(v) | TAGGED | I64 | Extract and sign-extend 48-bit integer payload |
| TAG_TO_REF(v) | TAGGED | REF_OFF | Extract 48-bit heap offset from tagged reference |
| REF_OFF_TO_RAW(off) | REF_OFF | REF_RAW | Compute heap_base + offset |
| RAW_TO_REF_OFF(ptr) | REF_RAW | REF_OFF | Compute ptr - heap_base |
| TAG_TO_F64(v) | TAGGED | F64 | Extract binary64 value (may require NaN normalization) |
| F64_TO_TAG(f) | F64 | TAGGED | Encode float as NV-1 (canonicalize if NaN collides with tag space) |
| REF_OFF_TO_TAG(off) | REF_OFF | TAGGED | Encode offset as tagged reference |
| TAG_TO_REF_RAW(v) | TAGGED | REF_RAW | Compound: extract offset, then reconstruct pointer |

**INV-LLIR-004**: Conversion instructions are not optional. If an instruction expects I64 but receives TAGGED, the compiler must insert UNBOX_INT before the use.

### 3. Lowering Rules: SoN → LLIR

The conversion from SoN to LLIR performs the following transformations:

#### 3.1 Phi Node Resolution

SoN's strict SSA phi nodes are converted to parallel copy instructions at block boundaries:

**SoN:**
```
Block_Merge:
    v = Phi(v1, v2, v3)
```

**LLIR:**
```
Block_Pred1:
    ...
    COPY_PARALLEL(v <- v1)
    JUMP Block_Merge

Block_Pred2:
    ...
    COPY_PARALLEL(v <- v2)
    JUMP Block_Merge

Block_Merge:
    // v is now defined by the parallel copy from the predecessor
    ...
```

**INV-LLIR-005**: COPY_PARALLEL instructions are semantically atomic (all copies happen simultaneously). MIR's register allocator will resolve these into actual move sequences, handling overlaps correctly.

#### 3.2 High-Level Operation Lowering

SoN's abstract operations are lowered to explicit pointer arithmetic and memory operations:

**SoN:**
```
val = LoadField(obj, offset_a, mem)
```

**LLIR:**
```
obj_off = TAG_TO_REF(obj)           // Extract 48-bit offset
obj_ptr = REF_OFF_TO_RAW(obj_off)   // Reconstruct native pointer
addr = ADD(obj_ptr, offset_a)       // Compute field address
val_tagged = LOAD(addr)             // Load 64-bit NV-1 value
```

**INV-LLIR-006**: All memory operations (LOAD, STORE) operate on REF_RAW pointers. The compiler must ensure the pointer is reconstructed before the memory access and converted back to REF_OFF before any safepoint.

#### 3.3 Guard Lowering

SoN's Guard nodes become explicit branch instructions with deoptimization targets:

**SoN:**
```
guard = GuardLayout(obj, 0x4A, deopt_stub_1)
```

**LLIR:**
```
obj_off = TAG_TO_REF(obj)
obj_ptr = REF_OFF_TO_RAW(obj_off)
layout_id = LOAD(obj_ptr)           // Load 64-bit header
expected = CONST(0x4A)
cmp = CMP(layout_id, expected)
BRANCH_ON_FALSE(cmp, deopt_stub_1)  // Deopt if mismatch
// Fall-through: obj is proven to have LayoutID 0x4A
```

**INV-LLIR-007**: Every deoptimization target is a separate basic block ending in a DEOPT instruction with a complete VM state reconstruction map.

### 4. Scalar Replacement of Aggregates (SROA)

Based on VSDG's escape analysis, LLIR breaks down aggregate objects into individual scalar values.

#### 4.1 SROA Transformation

**SoN (object does not escape):**
```
obj = ALLOC(layout, size)
STORE_FIELD(obj, offset_0, val_0, mem)
STORE_FIELD(obj, offset_1, val_1, mem)
result = LOAD_FIELD(obj, offset_0, mem)
```

**LLIR (after SROA):**
```
// No allocation, no stores, no loads
// val_0 and val_1 are kept in virtual registers
result = val_0  // Direct forwarding
```

**INV-LLIR-008**: SROA is only permitted if VSDG has proven the object does not escape the current method or thread. If the object escapes, it must remain a heap-allocated aggregate.

#### 4.2 Partial SROA

If only some fields of an object are accessed, and the object does not escape, only those fields are scalar-replaced. The object is still allocated, but the accessed fields are kept in registers:

**LLIR:**
```
obj = ALLOC(layout, size)
STORE_FIELD(obj, offset_1, val_1, mem)  // Field 1 stored to heap
// Field 0 kept in register, not stored
result = val_0  // Direct use of register
```

### 5. Safety Elision

Based on VSDG's proofs, LLIR removes runtime safety checks:

#### 5.1 Array Bounds Check Elimination (ABCE)

**SoN:**
```
len = LOAD_LENGTH(arr)
idx = ...
GUARD(idx < len, deopt_bounds)
val = LOAD_ELEMENT(arr, idx)
```

**LLIR (if VSDG proves idx < len):**
```
// Guard removed
val = LOAD_ELEMENT(arr, idx)
```

**INV-LLIR-009**: ABCE is only permitted if VSDG has mathematically proven the bounds condition. Heuristic predictions are not sufficient.

#### 5.2 Null Check Elimination

Similar to ABCE, null checks are removed if VSDG proves the value cannot be null.

#### 5.3 Write Barrier Elision

If VSDG's escape analysis proves an object is newly allocated and has not escaped the current thread, write barriers are elided:

**SoN:**
```
STORE_FIELD(obj, offset, val, mem)
WRITE_BARRIER(obj, val)  // Required for generational GC
```

**LLIR (if obj is proven thread-local and newly allocated):**
```
STORE_FIELD(obj, offset, val, mem)
// Write barrier elided
```

**INV-LLIR-010**: Write barrier elision requires proof that the object is in the young generation and has not been seen by older-generation objects.

### 6. Safepoint and Deoptimization Materialization

This is where the JIT-GC contract is physically realized.

#### 6.1 Safepoint Points

A safepoint is any point where the GC may need to inspect or relocate live references. Safepoints occur at:
- Function calls (the callee may trigger GC).
- Loop back-edges (to allow GC to make progress during long-running loops).
- Explicit yield points (for cooperative multitasking or GC requests).

#### 6.2 Safepoint Metadata

At each safepoint, LLIR attaches a SafepointRecord containing:
- **Safepoint ID**: Unique identifier for this PC location.
- **Live Register Map**: For each physical register (after MIR allocation), its representation type and its mapping to VM state:
  ```
  GPR_R8: TAGGED -> VM_Reg_3
  FPR_F0: F64 -> (unboxed float, not a VM register)
  GPR_R9: REF_OFF -> VM_Reg_5
  ```
- **Stack Map**: For each stack slot, its representation type and VM mapping.
- **Deopt Target**: The bytecode PC and method ID to resume if deoptimization occurs.

**INV-LLIR-011**: Every live REF_OFF or REF_RAW value at a safepoint must be explicitly recorded in the safepoint metadata. The GC must be able to locate and relocate all live references.

**INV-LLIR-012**: REF_RAW values may not be live across a safepoint. If a raw pointer is needed after a safepoint, it must be reconstructed from the REF_OFF after the safepoint.

#### 6.3 Deoptimization Stubs

Each guard's failure path is a separate basic block ending in a DEOPT instruction:

**LLIR:**
```
Block_Deopt_1:
    // Capture current physical state
    DEOPT(safepoint_id = 42, vm_state = {
        VM_Reg_0: GPR_R8 (TAGGED),
        VM_Reg_1: Stack_Slot_0 (TAGGED),
        VM_Reg_2: FPR_F0 (F64),
        ...
    })
```

The DEOPT instruction is a pseudo-instruction that MIR will lower to a call to a runtime deoptimization handler. The handler uses the vm_state map to reconstruct the interpreter's stack frame and virtual registers, then resumes execution in T0 at the appropriate bytecode PC.

**INV-LLIR-013**: Every DEOPT instruction must provide a complete, consistent mapping from physical state to VM state. No live VM value may be omitted.

### 7. Calling Convention Commitment

LLIR commits to a machine-independent calling convention:

#### 7.1 Argument Passing
- First N arguments (e.g., N=4 on x86-64, N=8 on AArch64) are passed in registers.
- Remaining arguments are passed on the stack.
- Argument representation: Each argument has an explicit representation type. The caller and callee must agree on the representation.

#### 7.2 Return Values
- Single return value is returned in a designated register (e.g., RAX on x86-64, X0 on AArch64).
- Multiple return values (rare) are returned in a struct on the stack or in multiple registers.

#### 7.3 Callee-Saved Registers
- LLIR assumes a standard ABI where certain registers are callee-saved.
- The register allocator (in MIR) will respect these constraints.

**INV-LLIR-014**: All function calls must explicitly specify the representation types of arguments and return values. Mismatches between caller and callee are a compilation error.

### 8. Invariants and Conversion Boundary (LLIR → MIR)

Before LLIR is passed to MIR, it must satisfy strict invariants:

| Invariant | Description |
|-----------|-------------|
| LLIR-INV-01 | Explicit Representations: Every operand and result has a concrete representation type. No abstract "value" types remain. |
| LLIR-INV-02 | No Phi Nodes: All phi nodes have been converted to parallel copies at block boundaries. |
| LLIR-INV-03 | Explicit Conversions: All representation changes are explicit conversion instructions. No implicit coercions. |
| LLIR-INV-04 | Safepoint Metadata: Every safepoint has a complete, consistent safepoint record. |
| LLIR-INV-05 | Deopt Completeness: Every deopt point has a complete VM state reconstruction map. |
| LLIR-INV-06 | Memory Safety: All memory operations use REF_RAW pointers, and raw pointers are not live across safepoints. |
| LLIR-INV-07 | Calling Convention: All calls specify explicit argument and return representations. |

**What is Lost/Transformed in this Boundary?**
- **Lost**: The SoN graph structure. LLIR is a linear instruction sequence, not a graph.
- **Transformed**: Abstract memory state tokens are gone. Memory operations are now explicit loads and stores with no explicit state threading (MIR will handle dependency tracking for scheduling).
- **Preserved**: All semantic guarantees from VSDG and SoN. All guard dependencies, all side-effect orderings, all speculative assumptions.

### 9. Example: LLIR for a Guarded Field Access

**SoN Input:**
```
guard = GuardLayout(obj, 0x4A, deopt_1)
val = LoadField(obj, offset_8, mem) [guarded by guard]
```

**LLIR Output:**
```
Block_Entry:
    obj_tagged = VM_Reg_0 (TAGGED)
    obj_off = TAG_TO_REF(obj_tagged)
    obj_ptr = REF_OFF_TO_RAW(obj_off)
    layout_id = LOAD(obj_ptr)              // Load 64-bit header
    expected = CONST(0x4A)
    cmp = CMP(layout_id, expected)
    BRANCH_ON_FALSE(cmp, Block_Deopt_1)    // Guard check
    // Fall-through: obj is proven to have LayoutID 0x4A
    field_addr = ADD(obj_ptr, 8)           // Compute field address
    val_tagged = LOAD(field_addr)          // Load 64-bit NV-1 value
    VM_Reg_1 = val_tagged                  // Store result
    JUMP Block_Exit

Block_Deopt_1:
    DEOPT(safepoint_id = 7, vm_state = {
        VM_Reg_0: GPR_R8 (TAGGED),
        ...
    })

Block_Exit:
    ...
```

**Safepoint Metadata (at the LOAD instruction, if it's a safepoint):**
```
SafepointRecord {
    id: 7,
    live_registers: [
        {phys: GPR_R8, repr: TAGGED, vm: VM_Reg_0},
        {phys: GPR_R9, repr: REF_RAW, vm: None},  // Temporary, not a VM reg
        ...
    ],
    stack_map: [...],
    deopt_target: {method: "foo", bytecode_pc: 0x1A}
}
```

---

## MIR-1: Machine IR and Physical Realization

The Machine IR is the final compilation tier, where LLIR's abstract, representation-committed instructions are transformed into concrete, executable machine code for the target architecture (x86-64, AArch64, or RV64).

This is where virtual registers become physical registers, where LLIR's pseudo-instructions become real CPU instructions, and where the final bytes of native code are emitted.

### 1. Core MIR Structure

MIR abandons LLIR's linear instruction sequence in favor of a target-specific instruction graph with explicit physical constraints.

#### 1.1 MIR Instruction Format

Every MIR instruction Ik has the form:

```
Ik: target_opcode(op1, op2, …, opn) → (r1, r2, …, rm) [constraints]
```

Where:
- opi are operands (physical registers, stack slots, or immediate constants).
- rj are result physical registers.
- Constraints include: register class restrictions, latency information, side effects, and scheduling dependencies.

#### 1.2 Physical Register Model

Each target architecture defines its physical register file:

**x86-64:**
- GPRs: RAX, RBX, RCX, RDX, RSI, RDI, RBP, RSP, R8–R15 (16 total)
- FPRs: XMM0–XMM15 (16 total, 128-bit each)
- Callee-saved: RBX, RBP, R12–R15 (6 GPRs)
- Caller-saved: RAX, RCX, RDX, RSI, RDI, R8–R11 (10 GPRs)

**AArch64:**
- GPRs: X0–X30 (31 total, plus SP)
- FPRs: Q0–Q31 (32 total, 128-bit each)
- Callee-saved: X19–X28, X29 (FP), X30 (LR) (11 GPRs)
- Caller-saved: X0–X18 (19 GPRs)

**RV64:**
- GPRs: X0–X31 (32 total, X0 is hardwired zero)
- FPRs: F0–F31 (32 total, 64-bit each)
- Callee-saved: X8–X9, X18–X27 (12 GPRs)
- Caller-saved: X1–X7, X10–X17, X28–X31 (20 GPRs)

#### 1.3 Register Classes

Each operand and result is assigned a register class based on its representation type:

| Representation | x86-64 Register Class | AArch64 Register Class | RV64 Register Class |
|---|---|---|---|
| TAGGED | GPR64 | GPR64 | GPR64 |
| I64 | GPR64 | GPR64 | GPR64 |
| F64 | FPR64 (XMM) | FPR64 (Q) | FPR64 (F) |
| REF_OFF | GPR64 | GPR64 | GPR64 |
| REF_RAW | GPR64 | GPR64 | GPR64 |
| COND | FLAGS or GPR8 | GPR32 (WZR) | GPR32 |

**INV-MIR-001**: Every operand and result must be assigned to a valid register class for the target architecture.

### 2. Instruction Selection

This phase maps LLIR's abstract instructions to target-specific machine instructions.

#### 2.1 Selection Strategy

We use DAG-based instruction selection (similar to LLVM's SelectionDAG or V8's TurboFan):
1. Convert LLIR's linear instruction sequence into a Selection DAG (Directed Acyclic Graph).
2. Match patterns in the DAG against target-specific instruction templates.
3. Emit the selected machine instructions, respecting operand constraints and register classes.

#### 2.2 Example: x86-64 Instruction Selection

**LLIR:**
```
obj_ptr = REF_OFF_TO_RAW(obj_off)
field_addr = ADD(obj_ptr, 8)
val_tagged = LOAD(field_addr)
```

**x86-64 MIR (after selection):**
```asm
; obj_off is in GPR_R9
lea r10, [r9 + 8]        ; r10 = obj_ptr + 8 (field address)
mov rax, [r10]           ; rax = *field_addr (load 64-bit value)
```

**AArch64 MIR:**
```asm
; obj_off is in X9
add x10, x9, #8          ; x10 = obj_off + 8
ldr x0, [x10]            ; x0 = *x10 (load 64-bit value)
```

**RV64 MIR:**
```asm
; obj_off is in a3 (x13)
addi a4, a3, 8           ; a4 = a3 + 8
ld a0, 0(a4)             ; a0 = *a4 (load 64-bit value)
```

#### 2.3 Complex Pattern Matching

Some LLIR instructions require multiple machine instructions:

**LLIR:**
```
result = BOX_INT(i64_val)
```

**x86-64 MIR (if i64_val fits in 48 bits):**
```asm
mov rax, rdi             ; rax = i64_val
mov rdx, 0xFFFC000000000000  ; rdx = tag prefix
or rax, rdx              ; rax = tagged value
```

**x86-64 MIR (if overflow handling is needed):**
```asm
mov rax, rdi
mov rcx, 0xFFFC000000000000
test rax, 0xFFFF000000000000  ; Check if upper 16 bits are non-zero
jnz .slow_path           ; Overflow: allocate boxed integer
or rax, rcx              ; Fast path: tag it
jmp .done
.slow_path:
    call allocate_boxed_int
.done:
```

**INV-MIR-002**: Instruction selection must preserve all semantic guarantees from LLIR. No reordering of side effects, no elimination of guards, no incorrect NaN handling.

### 3. Register Allocation

This is the most complex phase in MIR. We must assign physical registers to all virtual registers (from LLIR) while respecting:
- Register class constraints (GPR vs FPR)
- Callee-saved vs caller-saved conventions
- Register pressure (limited number of physical registers)
- Spill code insertion (when registers are exhausted)

#### 3.1 Allocation Strategy

We use Linear Scan Register Allocation for T1 and T2 (fast, predictable), and Graph Coloring with Iterated Coalescing for T3 (higher quality, slower).

**Linear Scan Algorithm (T1/T2):**
1. **Compute Live Intervals**: For each virtual register, determine its live range (from definition to last use).
2. **Sort Intervals**: Sort by start position.
3. **Allocate Registers**: Iterate through intervals, assigning physical registers from the appropriate register class.
4. **Spill on Pressure**: If no physical register is available, spill the interval with the furthest endpoint to a stack slot.
5. **Insert Spill Code**: Insert MOV instructions to spill/reload values.

**Graph Coloring Algorithm (T3):**
1. **Build Interference Graph**: Nodes are virtual registers; edges connect registers with overlapping live ranges.
2. **Simplify**: Remove low-degree nodes (easy to color).
3. **Coalesce**: Merge nodes connected by copy instructions (if they don't create excessive interference).
4. **Freeze**: If coalescing creates too much interference, freeze the node (give up on coalescing).
5. **Spill**: If a node cannot be colored, spill it to a stack slot.
6. **Select Colors**: Assign physical registers (colors) to unspilled nodes.
7. **Iterate**: Repeat until all nodes are colored or spilled.

#### 3.2 Spill Slot Management

When a virtual register is spilled, it is assigned a stack slot:
- **Slot Size**: 8 bytes (matching NV-1's 64-bit values).
- **Slot Alignment**: 8-byte aligned.
- **Slot Location**: In the method's stack frame, below the frame pointer.

**INV-MIR-003**: Every spilled value must be reloaded before use. The register allocator must insert explicit MOV instructions for spill/reload.

**INV-MIR-004**: Spill slots must be recorded in the safepoint metadata (from LLIR) so the GC can scan them for references.

#### 3.3 Example: Register Allocation

**LLIR (before allocation):**
```
v1 = TAG_TO_REF(obj_tagged)
v2 = REF_OFF_TO_RAW(v1)
v3 = LOAD(v2)
v4 = ADD(v3, 1)
```

**MIR (after Linear Scan, x86-64):**
```asm
; v1 -> R9, v2 -> R10, v3 -> RAX, v4 -> RAX (reused)
mov r9, rdi              ; v1 = obj_tagged (assume obj_tagged is in RDI)
lea r10, [r9 + 0]        ; v2 = v1 + heap_base (simplified)
mov rax, [r10]           ; v3 = *v2
add rax, 1               ; v4 = v3 + 1
```

**MIR (after spilling, if R10 is unavailable):**
```asm
mov r9, rdi
mov [rsp - 8], r9        ; Spill v1 to stack
lea rax, [r9 + 0]        ; v2 = v1 + heap_base
mov [rsp - 16], rax      ; Spill v2 to stack
mov rax, [rax]           ; v3 = *v2
add rax, 1               ; v4 = v3 + 1
```

### 4. Instruction Scheduling

After register allocation, instructions are reordered to maximize Instruction-Level Parallelism (ILP) and minimize pipeline stalls.

#### 4.1 Scheduling Strategy

We use List Scheduling:
1. **Build Dependency Graph**: Nodes are instructions; edges represent data dependencies, control dependencies, and memory dependencies.
2. **Compute Priorities**: Assign priorities based on latency, critical path length, and register pressure.
3. **Schedule Instructions**: Iteratively select the highest-priority ready instruction and emit it.
4. **Respect Constraints**: Do not reorder instructions across safepoints, guards, or side-effect boundaries.

#### 4.2 Example: Instruction Scheduling

**Before Scheduling:**
```asm
mov rax, [r9]            ; Load A (latency: 4 cycles)
mov rbx, [r10]           ; Load B (latency: 4 cycles)
add rcx, rax, rbx        ; Add A + B (depends on loads)
mov rdx, [r11]           ; Load C (latency: 4 cycles)
mul rdx, rcx             ; Multiply (depends on add and load C)
```

**After Scheduling (loads moved earlier to hide latency):**
```asm
mov rax, [r9]            ; Load A
mov rbx, [r10]           ; Load B
mov rdx, [r11]           ; Load C (moved up, independent)
add rcx, rax, rbx        ; Add A + B (waits for loads)
mul rdx, rcx             ; Multiply (waits for add and load C)
```

**INV-MIR-005**: Instruction scheduling must preserve all semantic guarantees. No reordering of memory operations across potential aliases, no reordering across guards or safepoints.

### 5. Final Code Emission

The final phase converts MIR instructions into executable machine code bytes.

#### 5.1 Emission Process

1. **Encode Instructions**: Convert each MIR instruction into its binary encoding (opcode, operands, immediates).
2. **Resolve Relocations**: Patch branch targets, external symbol addresses, and constant pool references.
3. **Align Code**: Insert NOPs to align basic blocks and loop headers for optimal instruction fetch.
4. **Mark Executable**: Set memory protection to executable (via mprotect or equivalent).

#### 5.2 Relocation Types

| Relocation Type | Description | Example |
|---|---|---|
| R_BRANCH | Relative branch offset | JMP target_label |
| R_ABS64 | Absolute 64-bit address | MOV rax, [external_symbol] |
| R_RIP_REL | RIP-relative addressing (x86-64) | MOV rax, [rip + offset] |
| R_PAGE | Page-relative addressing (AArch64) | ADR x0, label |
| R_CONST | Constant pool reference | LDR x0, =constant_value |

#### 5.3 Code Layout

The final CodeBlob layout:
```
[ Code Header ]
  - Method ID
  - Code size
  - Safepoint records
  - Deoptimization maps
  - GC metadata

[ Executable Code ]
  - Machine instructions

[ Constant Pool ]
  - Literal constants (e.g., 64-bit floats, large integers)

[ Exception Table ]
  - Exception handler ranges and targets
```

**INV-MIR-006**: All relocations must be resolved before the code is marked executable. Unresolved relocations are a fatal compilation error.

### 6. Integration with AOSS and Region-Based Deoptimization

MIR must respect the AOSS specialization model and region-based deoptimization:

#### 6.1 Guard Code Generation

Guards from LLIR become explicit comparison and branch instructions:

**LLIR:**
```
cmp = CMP(layout_id, expected)
BRANCH_ON_FALSE(cmp, Block_Deopt_1)
```

**x86-64 MIR:**
```asm
cmp rax, 0x4A            ; Compare layout_id with expected
jne .deopt_stub_1        ; Jump to deopt if not equal
```

#### 6.2 Deoptimization Stub Emission

Each DEOPT instruction from LLIR becomes a small code stub:

**x86-64 MIR:**
```asm
.deopt_stub_1:
    ; Save all live registers to a known location
    mov [rsp - 8], rax
    mov [rsp - 16], rbx
    ...
    ; Call runtime deoptimization handler
    lea rdi, [rsp - 128]   ; Pointer to saved state
    mov rsi, safepoint_id  ; Safepoint ID
    call deopt_handler
```

The deopt_handler uses the safepoint metadata (from LLIR) to reconstruct the interpreter state and resume execution in T0.

#### 6.3 Region Boundaries

MIR must preserve region boundaries for selective invalidation:
- Each region has a unique Region ID.
- The region's entry point is marked in the code header.
- When AOSS invalidates a region, the JIT patches the first few bytes of the region's entry with a jump to the deopt stub.

**INV-MIR-007**: Region boundaries must be preserved in the final code. The JIT must be able to patch a region's entry point without affecting other regions.

### 7. Invariants and Final Contract

Before the code is emitted, MIR must satisfy strict invariants:

| Invariant | Description |
|-----------|-------------|
| MIR-INV-01 | Physical Registers: All virtual registers have been assigned to physical registers or spilled to stack slots. |
| MIR-INV-02 | Register Classes: All operands and results are in valid register classes for the target architecture. |
| MIR-INV-03 | Calling Convention: All function calls respect the target ABI (callee-saved registers, argument passing, return values). |
| MIR-INV-04 | Safepoint Metadata: All safepoints have complete, accurate metadata (register maps, stack maps, deopt targets). |
| MIR-INV-05 | Relocations: All relocations are resolved. No unresolved symbols or branch targets. |
| MIR-INV-06 | Region Boundaries: All region boundaries are preserved and marked in the code header. |
| MIR-INV-07 | Semantic Preservation: All semantic guarantees from LLIR (and transitively from VSDG and SoN) are preserved. |

### 8. Example: Complete MIR Pipeline

**LLIR Input:**
```
Block_Entry:
    obj_tagged = VM_Reg_0 (TAGGED)
    obj_off = TAG_TO_REF(obj_tagged)
    obj_ptr = REF_OFF_TO_RAW(obj_off)
    layout_id = LOAD(obj_ptr)
    expected = CONST(0x4A)
    cmp = CMP(layout_id, expected)
    BRANCH_ON_FALSE(cmp, Block_Deopt_1)
    field_addr = ADD(obj_ptr, 8)
    val_tagged = LOAD(field_addr)
    VM_Reg_1 = val_tagged
    JUMP Block_Exit

Block_Deopt_1:
    DEOPT(safepoint_id = 7, vm_state = {...})

Block_Exit:
    ...
```

**x86-64 MIR Output (after instruction selection, register allocation, and scheduling):**
```asm
Block_Entry:
    ; obj_tagged is in RDI (VM_Reg_0)
    lea r9, [rdi + 0]              ; obj_off = obj_tagged (simplified)
    lea r10, [r9 + heap_base]      ; obj_ptr = obj_off + heap_base
    mov rax, [r10]                 ; layout_id = *obj_ptr
    cmp rax, 0x4A                  ; Compare with expected
    jne .deopt_stub_7              ; Deopt if mismatch
    lea r11, [r10 + 8]             ; field_addr = obj_ptr + 8
    mov rax, [r11]                 ; val_tagged = *field_addr
    mov rsi, rax                   ; VM_Reg_1 = val_tagged (in RSI)
    jmp .exit

.deopt_stub_7:
    ; Save live registers
    mov [rsp - 8], rdi
    mov [rsp - 16], rsi
    ...
    lea rdi, [rsp - 128]
    mov rsi, 7                     ; Safepoint ID
    call deopt_handler

.exit:
    ...
```

**Code Header:**
```
CodeHeader {
    method_id: "foo",
    code_size: 128,
    regions: [
        {id: 1, entry_offset: 0, deopt_stub_offset: 64}
    ],
    safepoints: [
        {id: 7, pc_offset: 48, register_map: [...], stack_map: [...]}
    ]
}
```

---

## FFI-1: Polyglot Trampoline and Universal Engine Integration

### 1. The Universal Engine Concept

This is not a single-language VM. It is a universal execution engine that can host multiple language runtimes (JavaScript, Python, Ruby, Lua, custom DSLs, etc.) simultaneously, with near-zero overhead when crossing language boundaries.

The key insight: FFI is not a special case. An FFI call is just another function call in the IR. The trampoline is just another kind of function prologue/epilogue. The same VSDG→SoN→LLIR→MIR pipeline handles everything.

### 2. FFI Trampoline Architecture

#### 2.1 Trampoline Generation (Copy-and-Patch at Compile Time)

Just as T1 uses stencils for baseline compilation, the FFI system uses FFI Stencils for marshaling code.

**FFI Stencil**: A pre-assembled machine code template for converting between NV-1 and an external ABI, with holes for runtime patching.

**Example: JavaScript → C FFI**

JavaScript call: `js_func(c_func, arg1, arg2)`

**Generated Trampoline (x86-64):**
```asm
; Marshal arg1 (NV-1 tagged) → C int
mov rax, [rsp + 16]        ; Load NV-1 arg1
test rax, 0xFFFC000000000000  ; Check if tagged integer
jne .slow_path_arg1        ; Not an integer: slow path
mov rdi, rax               ; rdi = C int (first arg)
and rdi, 0x0000FFFFFFFFFFFF   ; Extract 48-bit payload
jmp .marshal_arg2
.slow_path_arg1:
    call js_to_c_int_slow      ; Runtime helper for boxed/float/etc.
    mov rdi, rax

.marshal_arg2:
; Marshal arg2 (NV-1 tagged) → C pointer
mov rax, [rsp + 24]        ; Load NV-1 arg2
test rax, 0xFFFC000000000000
jne .slow_path_arg2
mov rsi, rax
and rsi, 0x0000FFFFFFFFFFFF
add rsi, heap_base         ; Reconstruct native pointer
jmp .call_c
.slow_path_arg2:
    call js_to_c_ptr_slow
    mov rsi, rax

.call_c:
; Call C function
call [c_func_ptr]          ; Indirect call to C function
mov rdx, rax               ; Save C return value

; Marshal C int return → NV-1 tagged
mov rax, rdx
mov rcx, 0xFFFC000000000000
or rax, rcx                ; Tag as integer
ret
```

#### 2.2 Trampoline Stencil Types

| Stencil Type | Purpose | Example |
|---|---|---|
| TAGGED_TO_C_INT | NV-1 integer → C int | JavaScript 42 → C 42 |
| TAGGED_TO_C_PTR | NV-1 reference → C void* | JavaScript object → C pointer |
| TAGGED_TO_C_DOUBLE | NV-1 float → C double | JavaScript 3.14 → C 3.14 |
| C_INT_TO_TAGGED | C int → NV-1 integer | C 42 → JavaScript 42 |
| C_PTR_TO_TAGGED | C void* → NV-1 reference | C pointer → JavaScript object |
| C_DOUBLE_TO_TAGGED | C double → NV-1 float | C 3.14 → JavaScript 3.14 |
| PYTHON_OBJ_TO_TAGGED | Python object → NV-1 | Python int → JavaScript number |
| TAGGED_TO_PYTHON_OBJ | NV-1 → Python object | JavaScript number → Python int |

#### 2.3 Trampoline Generation Algorithm

At compile time, the FFI system:
1. **Analyzes the call site**: Determines source language types and target language types.
2. **Selects stencils**: Chooses the appropriate marshaling stencils for each argument and return value.
3. **Copies and patches**: Assembles the trampoline by copying stencils and patching holes (e.g., heap base, function pointer, slow-path targets).
4. **Inserts GC barriers**: If the foreign function may allocate or trigger GC, insert write barriers and safepoints.
5. **Emits the trampoline**: The trampoline is a small, standalone code blob that can be called like any other function.

**INV-FFI-001**: Every FFI trampoline must be generated at compile time. No runtime code generation for trampolines.

**INV-FFI-002**: Every FFI trampoline must respect the target ABI (calling convention, register usage, stack alignment).

**INV-FFI-003**: Every FFI trampoline must integrate with the safepoint system. If the foreign function may block or allocate, the trampoline must reach a safepoint before the call.

### 3. GC Integration with Foreign Objects

#### 3.1 Foreign Object Pinning

When passing a reference to a foreign function, the object may need to be pinned (prevented from moving) if the foreign code retains the pointer:

**LLIR:**
```
obj_tagged = VM_Reg_0
obj_off = TAG_TO_REF(obj_tagged)
obj_ptr = REF_OFF_TO_RAW(obj_off)
PIN(obj_ptr)               ; Prevent GC from moving this object
CALL_FOREIGN(c_func, obj_ptr)
UNPIN(obj_ptr)             ; Allow GC to move again
```

**MIR (x86-64):**
```asm
; Pin the object by adding it to a thread-local pin table
lea rdi, [obj_ptr]
call gc_pin_object

; Call foreign function
call [c_func_ptr]

; Unpin the object
lea rdi, [obj_ptr]
call gc_unpin_object
```

**INV-FFI-004**: Pinned objects must be tracked in a thread-local pin table. The GC must scan this table and avoid relocating pinned objects.

#### 3.2 Write Barriers for Foreign Mutations

If a foreign function mutates a heap object (e.g., writes to a field), it must invoke a write barrier:

**C code:**
```c
void set_field(void* obj, void* value) {
    ((uint64_t*)obj)[1] = (uint64_t)value;  // Write to field
    gc_write_barrier(obj, value);           // Notify GC
}
```

The `gc_write_barrier` function updates the remembered set and marks the object as modified for the concurrent collector.

**INV-FFI-005**: Foreign code that mutates heap objects must invoke write barriers. Failure to do so is a correctness violation.

### 4. Exception Translation

Different languages have different exception models. The FFI trampoline must translate exceptions:

**JavaScript → C:**
- JavaScript exceptions are NV-1 tagged values.
- C uses return codes or errno.
- The trampoline catches JavaScript exceptions and converts them to C error codes.

**C → JavaScript:**
- C uses return codes or setjmp/longjmp.
- JavaScript uses try-catch with NV-1 exception objects.
- The trampoline catches C errors and throws JavaScript exceptions.

**Example: C → JavaScript Exception Translation**
```asm
; Call JavaScript function
call js_func
test rax, rax              ; Check if exception occurred
jnz .handle_exception
ret                        ; Normal return

.handle_exception:
    ; rax contains the exception object (NV-1 tagged)
    ; Convert to C error code
    mov rdi, rax
    call js_exception_to_c_error
    mov rax, -1                ; Return error code
    ret
```

**INV-FFI-006**: Exception translation must preserve the semantic contract of both languages. A JavaScript exception must not silently become a C success code.

**INV-FFI-007**: Cross-language inlining (Universal Engine 6.1) is permitted only when the SemanticCompatibilityMatrix check (FFI-1 section 5) proves exception visibility, ordering, value semantics, resource discipline, GC visibility, and continuation compatibility for the specific call site. An unproven inline is a specification violation even when mechanically cheap.

---

### 5. The SemanticCompatibilityMatrix (Corrected Addition)

The Universal Engine section (6.1/6.2) and the trampoline architecture establish that mechanical boundary overhead can approach zero. What the 1.0 text lacked is the formal precondition checklist for when cross-language inlining is legal at all (finding FFI-1, resolved). "Near-zero overhead if semantic adapters are eliminable" requires a proof object that says the adapters ARE eliminable. That proof is the SemanticCompatibilityMatrix, and AOSS must consult it before any cross-language inlining is attempted. If the matrix cannot prove semantic equivalence for a boundary, inlining is aborted and the trampoline remains, regardless of how cheap the mechanical marshaling would be.

**Definition.** For an ordered pair of language contexts (caller L1, callee L2), the matrix is the set of compatibility facts the boundary must establish:

| Dimension | Fact required for inlining | Failure action |
|---|---|---|
| Exception visibility | Every exception L2 can raise is either caught inside the callee, or L1 has a handler prepared to catch its translated form; the exception-class mapping is total for the callee's raise set | Abort inline; keep the trampoline's exception adapter |
| Global-state ordering | L2's writes to state observable by L1 (globals, shared objects, FFI-visible memory) respect L1's evaluation-order expectations; no reordering hazard across the inlined body | Abort inline |
| Value semantics | The argument and return value domains used at the call site have identical concrete semantics in both languages (integer width/wrap policy, float NaN policy per NV-1 section 4, string encoding, reference identity) | Narrow to the proven domain via specialization, or abort |
| Resource discipline | Capabilities/authorities the callee requires are a subset of what the caller passes; no ambient authority is acquired inside (UGB-INV-01) | Abort inline |
| GC visibility | Allocation and safepoint behavior of the inlined body remains compatible with the caller's safepoint map; no foreign thread-state transition remains | Rewrite safepoints during inlining, or abort |
| Continuation | Control never resumes L2 after a throws path that L1 does not model (no finally/ensure halves escaping the inlined region) | Abort inline |

**Mechanics.** The matrix is computed per call site from the callee's summary (raise set, effect summary, value-domain profile) and the caller's context (handlers in scope, evaluation-order constraints). The ForeignCall node (REVISED section 6) carries source_semantics, target_semantics, and exception_adapter fields precisely so this computation has the data it needs. The AOSS check is a pure function: check_matrix(caller_ctx, callee_summary) returns Ok(inline_plan) or Err(reason). The reason is recorded in the compilation log so a rejected inline is diagnosable.

**Rule.** A cross-language inline without a proven matrix entry is a specification violation, even if it appears to work for the tested cases.

---

## T2-T5: Optimizing Tier Specification

### 1. Tier Overview

| Tier | Purpose | Optimization Level | Compilation Strategy |
|------|---------|-------------------|---------------------|
| T0 | Interpreter | None | Bytecode interpretation |
| T1 | Baseline Compiler | None | Stencil copy-and-patch |
| T2 | Optimizing Compiler | Moderate | VSDG→SoN→LLIR→MIR (first pass) |
| T3 | Aggressive Optimizer | High | VSDG→SoN→LLIR→MIR (more passes, speculative) |
| T4 | Trace Compiler | Very High | Extract hot traces, compile with same pipeline |
| T5 | Whole-Program Optimizer | Maximum | Cross-module optimization, PGO |

### 2. T2: Optimizing Compiler

#### 2.1 Purpose
T2 is the first optimizing tier. It takes methods that have been identified as hot (via T0/T1 profiling) and compiles them with moderate optimization.

#### 2.2 Compilation Pipeline
```
Bytecode → VSDG → SoN → LLIR → MIR → Native Code
```

#### 2.3 Optimization Passes (Moderate)
- **VSDG**: Escape analysis, basic constant folding, type inference.
- **SoN**: GVN, CSE, LICM, basic guard insertion.
- **LLIR**: SROA (for non-escaping objects), bounds check elimination (if provable).
- **MIR**: Linear scan register allocation, basic instruction scheduling.

#### 2.4 Compilation Latency Target
T2 should compile at ~100 KB/s (10x slower than T1, but 10x faster than T3).

#### 2.5 Tier-Up Trigger
A method tiers up from T1 to T2 when:
- Back-edge counter exceeds threshold (e.g., 10,000 iterations).
- Call-site counter exceeds threshold (e.g., 1,000 calls).
- T1 profiling reveals stable types/shapes (making optimization profitable).

### 3. T3: Aggressive Optimizer

#### 3.1 Purpose
T3 is the aggressive optimization tier. It takes very hot methods and applies speculative optimization, devirtualization, and advanced transformations.

#### 3.2 Compilation Pipeline
```
Bytecode → VSDG → SoN → LLIR → MIR → Native Code
```
(Same pipeline as T2, but with more passes and higher optimization thresholds.)

#### 3.3 Optimization Passes (Aggressive)
- **VSDG**: Advanced escape analysis, shape analysis, alias analysis.
- **SoN**: PRE, loop unswitching, speculative devirtualization, guard fusion, inlining.
- **LLIR**: Advanced SROA, write barrier elision, lock elision.
- **MIR**: Graph coloring register allocation, advanced instruction scheduling, if-conversion.

#### 3.4 Speculative Optimization
T3 makes speculative assumptions based on profiling data:
- "This call site always targets ClassA.method" → Devirtualize.
- "This object always has LayoutID 0x4A" → Specialize field access.
- "This array is always dense" → Eliminate bounds checks.

If the assumption fails, the code deoptimizes (via region-based deopt) and falls back to T2 or T1.

#### 3.5 Compilation Latency Target
T3 should compile at ~10 KB/s (10x slower than T2, but produces much faster code).

#### 3.6 Tier-Up Trigger
A method tiers up from T2 to T3 when:
- Execution time in T2 exceeds threshold (e.g., 100ms).
- T2 profiling reveals highly stable types/shapes (making aggressive optimization profitable).
- The method is on the critical path (e.g., startup, user input handling).

### 4. T4: Trace Compiler

#### 4.1 Purpose
T4 is the trace-based specialization tier. It identifies hot linear execution paths (traces) across function boundaries and compiles them with extreme optimization.

#### 4.2 Trace Extraction
The interpreter (T0) or baseline compiler (T1) records execution traces:
1. Start recording at a hot loop header or function entry.
2. Record every bytecode instruction executed.
3. Stop recording at a loop back-edge, function return, or trace length limit.

**Example Trace:**
```
Loop header (iteration 1)
  Load field A
  Add constant
  Store field B
  Call function X
  Load field C
  Return
Loop header (iteration 2)
  ...
```

#### 4.3 Trace Compilation
The trace is converted to VSDG, optimized with the same SoN→LLIR→MIR pipeline, and emitted as native code.

**Key difference**: A trace is a single-entry, single-exit graph with no internal branches (except for loop back-edges). This allows extremely aggressive optimization:
- No control-flow merging → no phi nodes → simpler GVN.
- Linear execution → easy LICM and strength reduction.
- Heavy guard insertion → every type-sensitive operation is guarded.

#### 4.4 Guard Density
Traces have many guards (one at every type-sensitive operation). If any guard fails, the trace deoptimizes.

**Example:**
```
guard1 = GuardLayout(obj, 0x4A)
val = LoadField(obj, offset_8)  // Specialized load
guard2 = GuardType(val, TAGGED_INT)
result = Add(val, 1)            // Specialized add
guard3 = GuardLayout(obj, 0x4A)  // Redundant, will be fused by SoN
StoreField(obj, offset_16, result)
```

SoN's GVN and PRE passes will fuse redundant guards, reducing runtime overhead.

#### 4.5 Compilation Latency Target
T4 should compile at ~1 KB/s (slowest tier, but produces the fastest code for hot traces).

#### 4.6 Tier-Up Trigger
A trace is compiled by T4 when:
- The trace is executed frequently (e.g., 10,000 times).
- The trace has stable types/shapes (making specialization profitable).

### 5. T5: Whole-Program Optimizer with Supercompilation

#### 5.1 T5's Mandate
T5 is not just another optimization pass. It is a program transformation engine that can:
- Apply all 100+ deterministic optimization passes from the taxonomy
- Perform supercompilation (driving the program with abstract inputs)
- Perform partial evaluation (specializing with respect to known inputs)
- Generate residual programs that are faster than any hand-written code
- Eliminate interpretive overhead entirely for hot paths

#### 5.2 T5 vs T2-T4

| Aspect | T2-T4 | T5 |
|--------|-------|-----|
| Scope | Individual methods or traces | Entire program or large subsets |
| Optimization Strategy | Conventional passes (GVN, PRE, LICM, etc.) | All passes + supercompilation + partial evaluation |
| Speculation | Speculative devirtualization, type specialization | Full program specialization based on profiling |
| Compilation Time | Milliseconds to seconds | Minutes to hours (offline) |
| Code Quality | Good to excellent | Theoretical maximum |
| Use Case | JIT compilation | Production builds, offline optimization |

#### 5.3 Supercompilation in T5

**What is Supercompilation?**

Supercompilation (pioneered by Valentin Turchin) is a semantics-driven transformation that:
1. Drives the program with abstract inputs (symbolic values)
2. Records the resulting execution trace (a "configuration")
3. Generalizes the configuration to handle concrete inputs
4. Folds the generalized configuration back into the program
5. Eliminates all interpretive overhead (no runtime dispatch, no dynamic checks)

**Example:**
```python
def power(x, n):
    if n == 0:
        return 1
    else:
        return x * power(x, n - 1)
```

After supercompilation with n=3:
```python
def power_x_3(x):
    return x * x * x  # All recursion eliminated, no runtime checks
```

**Supercompilation in Our Architecture:**

T5 uses supercompilation to transform the VSDG/SoN graph:

**Input VSDG:**
```
CALL(power, x, n)
  if n == 0:
    return 1
  else:
    return x * CALL(power, x, n - 1)
```

**Supercompilation Process:**
1. Drive the program with abstract inputs: x = ?, n = 3
2. Record the execution:
   - n == 3 → false → x * CALL(power, x, 2)
   - n == 2 → false → x * x * CALL(power, x, 1)
   - n == 1 → false → x * x * x * CALL(power, x, 0)
   - n == 0 → true → return 1
3. Generalize: Replace 3 with n, but keep the structure
4. Fold: Generate x * x * x for n = 3

**Output VSDG:**
```
; Specialized for n = 3
result = MUL(x, x)
result = MUL(result, x)
RETURN(result)
```

**Supercompilation Benefits:**
- Eliminates recursion: Recursive calls are unrolled and simplified
- Eliminates dispatch: Dynamic dispatch is replaced with direct calls
- Eliminates interpretive overhead: No runtime type checks, no dynamic binding
- Produces optimal code: The residual program is as fast as hand-written code

#### 5.4 Partial Evaluation in T5

**What is Partial Evaluation?**

Partial evaluation specializes a program with respect to some known inputs (static inputs), generating a residual program that is faster for the remaining unknown inputs (dynamic inputs).

**Example:**
```python
def interpret(program, input):
    for instruction in program:
        if instruction.op == "ADD":
            input = input + instruction.arg
        elif instruction.op == "MUL":
            input = input * instruction.arg
    return input
```

After partial evaluation with a known program:
```python
def specialized_program(input):
    input = input + 5      # From program[0]
    input = input * 3      # From program[1]
    input = input + 2      # From program[2]
    return input
```

The interpreter loop is eliminated. The residual program is a direct computation.

**Partial Evaluation in Our Architecture:**

T5 uses partial evaluation to specialize the engine itself:
- **Input**: The VSDG representation of a method, with some inputs known (e.g., from profiling)
- **Process**:
  1. Identify static inputs (e.g., constants, stable types, known call targets)
  2. Drive the VSDG with these static inputs
  3. Generate a residual VSDG that only depends on dynamic inputs
  4. Optimize the residual VSDG with all other passes

**Example:**
```
; Original VSDG
obj = LOAD(obj_ref)
if obj.layout == 0x4A:
    val = LOAD_FIELD(obj, offset_8)
else:
    val = GENERIC_LOAD(obj, "field")
result = ADD(val, 1)
```

After partial evaluation (knowing obj.layout == 0x4A from profiling):
```
; Residual VSDG
obj = LOAD(obj_ref)
val = LOAD_FIELD(obj, offset_8)  ; Direct field access, no check
result = ADD(val, 1)
```

The layout check is eliminated. The residual program is specialized for the known layout.

#### 5.5 T5's Complete Optimization Pipeline

T5 applies every optimization pass in a carefully orchestrated sequence:

**Phase 1: Semantic Analysis (VSDG)**
- All type analyses: CHA, RTA, VTA
- All alias analyses: Andersen's, Steensgaard's
- Escape analysis (comprehensive)
- Shape analysis (proving data structure invariants)
- Strictness analysis (if applicable)
- Partial evaluation (specialize with respect to known inputs)
- Supercompilation (drive with abstract inputs, generalize, fold)

**Phase 2: Global Optimization (SoN)**
- All redundancy elimination: GVN, CSE, PRE, LCM
- All loop optimizations: LICM, unrolling, unswitching, fusion, fission, tiling, interchange, strength reduction, induction variable elimination
- All control flow optimizations: Jump threading, tail duplication, unreachable code elimination
- All interprocedural optimizations: Inlining (aggressive), partial inlining, function cloning, dead argument elimination
- Guard insertion and fusion (comprehensive)
- Speculative devirtualization (aggressive)

**Phase 3: Representation Commitment (LLIR)**
- All safety elision: ABCE, null check elimination, write/read barrier elision, lock elision
- All aggregate optimizations: SROA (comprehensive), scalar replacement
- All local optimizations: DCE, DSE, copy propagation, LVN, algebraic simplification, strength reduction
- Representation selection (ARS, comprehensive)

**Phase 4: Physical Realization (MIR)**
- All register allocation: Graph coloring with iterated coalescing, live range splitting, stack slot coloring
- All instruction selection: DAG-based pattern matching, target-specific optimizations
- All instruction scheduling: List scheduling, delay slot filling (if applicable)
- All peephole optimizations: Target-specific instruction combining
- All backend optimizations: If-conversion, branch shortening, frame pointer omission

**Phase 5: Whole-Program Optimization**
- Cross-module inlining: Inline across module boundaries
- Dead function elimination: Remove unused functions
- Identical code folding: Merge identical functions
- Profile-guided optimization: Use profiling data to guide all decisions
- Link-time optimization: Optimize the entire program as a unit

#### 5.6 T5's Compilation Strategy

T5 is not just "run all passes once." It uses an iterative, fixed-point strategy:

**Iterative Optimization:**
1. Apply all VSDG passes
2. Convert to SoN
3. Apply all SoN passes
4. If the graph changed, go back to step 1 (VSDG can benefit from SoN optimizations)
5. Convert to LLIR
6. Apply all LLIR passes
7. Convert to MIR
8. Apply all MIR passes
9. If any pass made a significant change, go back to step 3 (SoN can benefit from LLIR/MIR optimizations)
10. Emit final code

**Fixed-Point Detection:**

T5 continues iterating until:
- No pass makes any change (true fixed point), OR
- A maximum iteration count is reached (e.g., 10 iterations), OR
- The improvement falls below a threshold (e.g., < 1% code size reduction)

**Pass Ordering:**

The order of passes matters. T5 uses a heuristically determined order based on:
- Which passes enable other passes (e.g., inlining enables more inlining)
- Which passes are most profitable (e.g., supercompilation early, peephole late)
- Which passes interact well (e.g., GVN before PRE)

#### 5.7 T5's Compilation Latency and Use Cases

**Compilation Latency:**

T5 is offline (not JIT). It is used for:
- Production builds: Optimize the entire program before deployment (minutes to hours)
- Idle-time optimization: Optimize during idle periods (e.g., overnight)
- Profile-guided rebuilds: Rebuild with profiling data after warm-up

**Code Quality:**

T5 produces the theoretical maximum code quality:
- All interpretive overhead eliminated (via supercompilation)
- All dynamic checks eliminated (via partial evaluation)
- All redundancy eliminated (via GVN, PRE, etc.)
- All loops optimized (via LICM, unrolling, etc.)
- All functions inlined (where profitable)
- All registers optimally allocated (via graph coloring)

**Code Size:**

T5 may produce larger code due to:
- Aggressive inlining (duplicating function bodies)
- Loop unrolling (duplicating loop bodies)
- Specialization (multiple versions of functions for different types)

However, T5 can also reduce code size via:
- Dead code elimination
- Identical code folding
- Outlining cold paths

#### 5.8 T5 Integration with the Rest of the Architecture

**T5 and AOSS:**
T5 uses AOSS's specialization metadata to guide optimization:
- If AOSS has identified a stable shape, T5 specializes for that shape
- If AOSS has identified a stable call target, T5 devirtualizes the call
- If AOSS has identified a stable type, T5 eliminates type checks

**T5 and FFI:**
T5 can inline FFI trampolines if both languages are compiled by the same engine:
- The trampoline is eliminated
- The two languages are merged into a single, optimized function
- Zero overhead for cross-language calls

**T5 and Region-Based Deopt:**
T5 respects region boundaries:
- Each region can be independently optimized
- If a region's assumptions are invalidated, only that region deopts
- T5 can fuse regions to reduce guard overhead (via PRE/GVN)

#### 5.9 Example: T5 Supercompilation of a Recursive Function

**Original VSDG:**
```
function factorial(n):
    if n == 0:
        return 1
    else:
        return n * factorial(n - 1)
```

**T5 Supercompilation with n = 5:**
1. Drive with abstract input n = 5
2. Record execution:
   - n == 5 → false → 5 * factorial(4)
   - n == 4 → false → 5 * 4 * factorial(3)
   - n == 3 → false → 5 * 4 * 3 * factorial(2)
   - n == 2 → false → 5 * 4 * 3 * 2 * factorial(1)
   - n == 1 → false → 5 * 4 * 3 * 2 * 1 * factorial(0)
   - n == 0 → true → return 1
3. Generalize and fold:
   - Result: 5 * 4 * 3 * 2 * 1 = 120

**Residual VSDG:**
```
function factorial_5():
    return 120  ; All recursion eliminated, computed at compile time
```

**After MIR:**
```asm
mov rax, 120
ret
```

The entire recursive function is replaced with a single constant return. This is the power of supercompilation.

### 6. Universal Engine: Cross-Language Optimization

The key insight: T2-T5 can optimize across language boundaries.

#### 6.1 Cross-Language Inlining

If a JavaScript function calls a Python function, and both are compiled by T3, the optimizer can inline the Python function into the JavaScript function:

**Before inlining:**
```
JavaScript:
    call python_func(arg)

Python:
    return arg + 1
```

**After inlining (in T3's SoN graph):**
```
JavaScript:
    ; Inline Python function body
    result = Add(arg, 1)
```

The FFI trampoline is eliminated. The two languages are now a single, optimized function.

#### 6.2 Cross-Language Specialization

If a JavaScript function always passes an integer to a Python function, T3 can specialize the Python function for integer arguments:

**Before specialization:**
```
Python:
    def add(x, y):
        return x + y  ; Generic add (handles int, float, string, etc.)
```

**After specialization (in T3):**
```
Python_int_int:
    ; Specialized for integer arguments
    result = AddInt(x, y)  ; Fast integer add
```

The JavaScript caller is updated to call Python_int_int instead of the generic add.

#### 6.3 Near-Zero-Overhead FFI (Corrected)

If both languages are compiled by the same engine, cross-language calls can be inlined with the trampoline eliminated, but only when the SemanticCompatibilityMatrix (FFI-1 section 5) proves the boundary's semantic adapters eliminable for that call site. The previous unqualified claim of "zero overhead" is retracted; the defensible contract is: mechanical overhead approaches zero when, and only when, the matrix proof succeeds. The trampoline remains when:
- The foreign function is not compiled by this engine (e.g., a native C library).
- The foreign function is too large or complex to inline.
- The SemanticCompatibilityMatrix check fails (exception models, evaluation-order hazards, value-domain mismatches, resource discipline, GC visibility, or continuation compatibility).

### 7. Summary: The Complete Tier Structure

| Tier | Purpose | Pipeline | Optimization | Latency | Use Case |
|------|---------|----------|-------------|---------|----------|
| T0 | Interpreter | None | None | Fastest | Cold code, startup, deopt fallback |
| T1 | Baseline Compiler | Stencil | None | Very fast | Warm-up, moderate-use code |
| T2 | Optimizing Compiler | VSDG→SoN→LLIR→MIR | Moderate | Fast | Hot code, first optimization pass |
| T3 | Aggressive Optimizer | VSDG→SoN→LLIR→MIR | High | Slow | Very hot code, speculative optimization |
| T4 | Trace Compiler | VSDG→SoN→LLIR→MIR | Very high | Very slow | Hottest traces, cross-function optimization |
| T5 | Whole-Program Optimizer | VSDG→SoN→LLIR→MIR | Maximum | Offline | Production builds, PGO |

FFI Trampoline: Generated at compile time using copy-and-patch. Near-zero overhead when both languages are compiled by the same engine.

---

## CRITICAL ASSESSMENT

### 🟢 Genuine Strengths (The Good Ideas)

#### 1. Stencil-Based T1 (Copy-and-Patch)
This is the strongest idea in the entire architecture. Apple's research (Chevalier-Boisvert et al.) has shown this approach can achieve sub-millisecond compilation latency while producing code within 2-3x of optimized JITs. This is real and valuable.

#### 2. Region-Based Deoptimization with Guard Fusion
Traditional method-level deopt causes "deopt storms" (e.g., V8's historic problems with `arguments` object). Partitioning code into regions and fusing guards via PRE/GVN is a legitimate improvement over current systems.

#### 3. GC-Driven Invalidation
Offloading shape-transition invalidation to the GC via a thread-local buffer is clever. It keeps the mutator fast and batches invalidations naturally.

#### 4. NaN Boxing + 48-bit Offsets
Space-efficient. 64-bit values with compressed heap references is proven (JavaScriptCore uses similar schemes).

### 🔴 Critical Flaws (Where the Design Breaks Down)

#### 1. VSDG → Sea of Nodes Conversion is a Fundamental Mistake
This is the single biggest architectural error. The design acknowledges the overlap in Section 3 but then proposes "Design A: Distinct Optimization Domains" as if naming the problem solves it.

**Reality:** Both IRs track value dependencies, state dependencies, memory effects, and control dependencies. Converting between them:
- Duplicates optimization infrastructure (you need GVN-like analysis in VSDG AND SoN)
- Loses information at the boundary (the file admits "implicit global view of state is fragmented")
- Adds O(N) conversion cost for no optimization benefit

**What real compilers do:** Graal, SpiderMonkey Ion, and V8 TurboFan all use a single graph IR. LuaJIT skips graphs entirely and goes straight to SSA. No production compiler maintains two overlapping graph representations.

**Impact:** T2/T3 compilation latency will be 2-3x slower than competing JITs due to redundant graph construction and conversion overhead.

#### 2. 64-bit Header with Only LayoutID is Unrealistic
The design commits the entire 64-bit header to LayoutID (62 bits) + GC state (2 bits). This leaves zero room for:
- Lock bits (for synchronized objects)
- Identity hash code caching
- Mark bits for non-side-table GC strategies
- Forwarding pointers during concurrent evacuation (the design uses 2 bits for this, but 62 bits for a forwarding offset is insufficient for large heaps)

**Real-world comparison:** V8 uses 32-64 bit headers with tag, size, map pointer, and GC bits. Java's HotSpot uses 96-128 bit headers. Your design is too aggressive and will require constant header expansion or complex workarounds.

#### 3. T5 Supercompilation is Academically Interesting but Practically Useless
The file claims T5 can "eliminate all interpretive overhead" and produce "theoretical maximum" code via supercompilation.

**Reality:** Supercompilation (Turchin's algorithm) only works well for:
- Pure functional programs
- Programs with simple recursion
- Programs without side effects, exceptions, or GC

For real-world code (with loops, exceptions, dynamic dispatch, I/O), supercompilation:
- Doesn't terminate or takes exponential time
- Produces code larger than the original due to aggressive unrolling
- Cannot handle the VSDG/SoN graph's complexity (memory state, guards, deopt points)

**Impact:** T5 will either hang on real programs or produce marginally better code than T3 at 1000x the compilation cost. It's not a "universal optimizer" — it's a research curiosity that doesn't scale.

#### 4. Stencil T1 is Too Rigid
The "fixed physical map" (first K virtual registers → physical registers) breaks down for:
- Methods with >K registers (requires spilling, which stencils don't handle well)
- Complex control flow (stencils are linear, not graph-based)
- Exception handling (requires dynamic stack unwinding info)

**Real-world comparison:** V8's Maglev and HotSpot's C1 use linear-scan register allocation, which handles arbitrary register counts and control flow. Stencils are great for simple code but degrade quickly.

#### 5. "Polyglot Universal Engine" with "Zero-Overhead FFI" is Misleading
The design claims cross-language calls can be inlined to zero overhead if both languages are compiled by the same engine.

**Reality:** Different languages have incompatible semantics:
- Exception models: JavaScript uses try-catch with objects, C uses return codes, Python uses exceptions with tracebacks
- Memory models: Python has reference counting, JavaScript has tracing GC, Rust has ownership
- Calling conventions: Different ABIs, different stack layouts

Inlining a Python function into JavaScript requires translating exception handling, GC barriers, and type systems. This is not "zero overhead" — it's a massive engineering effort that produces code slower than a well-optimized FFI trampoline.

### 📊 Realistic Performance Comparison

| Metric | This Design | V8/SpiderMonkey | LuaJIT |
|--------|-------------|-----------------|--------|
| T0 Interpreter | Fast (computed goto) | Fast (Ignition) | Very fast (trace-based) |
| T1 Compilation Latency | **Excellent** (stencils) | Good (Maglev) | N/A |
| T2/T3 Code Quality | **Mediocre** (due to VSDG→SoN overhead) | Excellent | Excellent |
| T2/T3 Compilation Latency | **Slow** (dual-graph conversion) | Fast | Very fast |
| Deopt Overhead | **Good** (region-based) | Poor (method-level) | Poor (trace-level) |
| Memory Footprint | **High** (AOSS metadata, region maps) | Moderate | Low |
| Startup Time | **Slow** (complex initialization) | Fast | Very fast |
| Peak Performance | **70-80% of V8** | 100% | 90-95% of V8 |

### 🎯 What Would Actually Happen

If you built this:
1. **Months 1-6:** T0 interpreter works well. T1 stencils are fast but hit limitations with complex code.
2. **Months 6-12:** VSDG→SoN conversion becomes a bottleneck. Compilation latency is 2-3x slower than expected.
3. **Months 12-18:** AOSS metadata overhead causes memory pressure. Region-based deopt works but guard fusion is hard to implement correctly.

### ✅ Recommendations

If you want to build a competitive JIT:
1. Drop VSDG. Use a single graph IR (Sea of Nodes or similar). The VSDG→SoN conversion is a net loss.
2. Expand the object header to 96 or 128 bits. The space savings aren't worth the complexity.
3. Keep T1 stencils but add a fallback to linear-scan for complex methods.

### 🏁 Final Verdict

**Innovation Score:** 8/10 (stencils, region-based deopt, GC-driven invalidation are genuinely novel)

**Practical Viability:** 4/10 (VSDG→SoN conversion, 64-bit header, T5 supercompilation are showstoppers)

---

## REVISED ARCHITECTURE (Post-Critique Corrections)

### What Survives

These remain strong ideas:
- Register-based VM
- NaN-boxed value model
- Stencil/copy-and-patch T1
- Computed-goto interpreter with superinstructions
- Polymorphic inline caches
- Region-based deoptimization
- Guard fusion
- Adaptive representation selection
- AOSS as a real subsystem
- Polyglot FFI trampolines generated by copy-and-patch
- T2–T5 sharing the same compiler infrastructure
- T5 having supercompilation available, but bounded

### What Must Change

These earlier choices are not production-safe as stated:
- Separate VSDG → Sea of Nodes conversion as mandatory pipeline
- 64-bit object header containing only LayoutID/GC bits as universal default
- Raw layout hash as sole correctness identity
- GC-only delayed invalidation for all specialization dependencies
- Unbounded T5 supercompilation / "theoretical maximum" claim
- "Zero-overhead FFI" without qualifying semantic adapters
- Fixed physical-register-map T1 without fallback

### 1. VSDG Should Not Be a Separate Mandatory Graph IR

The critique is right that a mandatory VSDG → SoN conversion is likely a net loss.

The original Design A had a valid motivation: separate semantic dependence analysis from control-flow optimization. But the implementation should not require two full overlapping graph representations.

**Revised decision:**

Use a single canonical optimizing graph IR, with VSDG as a semantic/dependence view or overlay, not a separate lowering stage.

So instead of:
```
Bytecode → VSDG → Sea of Nodes → LLIR → MIR
```

we use:
```
Bytecode / Trace / Foreign frontend
  → Unified semantic SSA graph
    with VSDG-style dependence overlays
  → optimized graph scheduling / restructuring
  → LLIR
  → MIR
```

The "four IR levels" can still exist conceptually, but VSDG is no longer a duplicated physical graph.

**Revised IR roles:**

| Level | Revised role |
|-------|-------------|
| VSDG | Dependence/fact overlay on the unified graph: value, state, memory, effects, aliasing, exceptions, specialization assumptions |
| Sea of Nodes / unified graph | Canonical optimizing SSA graph with control, data, memory, guards, regions |
| LLIR | Explicit representations, calls, safepoints, deopt maps, lowered memory operations |
| MIR | Target instructions, register constraints, allocation, scheduling, emission |

This preserves the useful part of VSDG without paying for a second graph.

**New rule:**

VSDG owns analyses, not a separate mandatory representation.

That means:
- no full VSDG graph construction followed by full SoN reconstruction;
- no duplicate GVN-like infrastructure;
- no lossy "global state view fragmented at conversion boundary";
- no O(N) graph translation just to change terminology.

The compiler can expose a VSDG view for analysis/debugging, but the physical node storage is shared.

**Overlay mutation API (required definition).** The overlay's operational vagueness is resolved here (finding VSDG-OVERLAY, resolved). The contract between the base graph and the overlay is:

1. **Single-writer transactional mutation.** All base-graph mutations occur inside an explicit transaction: Graph::edit(|tx| ...). Outside a transaction the graph and every attached overlay are immutable. There is no side-channel mutation path.
2. **Overlay reads are illegal inside transactions.** During Graph::edit, overlay queries return Err(OverlayLocked) and assert in debug builds. A pass that needs overlay facts must read them before entering the transaction or re-read after commit. No pass can act on overlay state while that state is stale mid-mutation.
3. **Event log, not callbacks, during mutation.** The transaction records primitive events (NodeInserted, NodeReplaced, EdgeDeleted, EdgeRetargeted, GuardAdded, RegionSplit) with their operands. It does not invoke overlay logic reentrantly.
4. **Commit-time synchronization.** On tx.commit(), the graph applies events to registered overlays in event order. Each overlay implements on_event(&Event) and reports is_stale(). An overlay that cannot maintain an invariant incrementally marks itself stale and is rebuilt from the base graph on next read. The overlay is a cache; the base graph is the truth.
5. **Read-only overlay by default during SoN passes.** Optimization passes (GVN, PRE, LICM, guard fusion) run against the base graph and treat overlays as read-only side facts. A pass that invalidates an overlay fact (e.g., GVN redirects an edge) performs the mutation inside a transaction; commit refreshes the overlay. A pass may also explicitly detach an overlay before a long restructuring phase and re-attach afterwards.
6. **Versioning.** Both graph and overlay carry monotonic revision counters. overlay.revision() == graph.revision() implies the overlay reflects the current graph. Any consumer verifies freshness in O(1) before trusting overlay facts.

Under this contract, GVN operates on the base graph; overlay facts it consumes are re-validated by revision after its transactions commit. An edge deletion in the base graph is delivered to the overlay as an EdgeDeleted event at commit, so the overlay is never silently out of date across a transaction boundary.

### 2. Layout Hashes Cannot Be Raw Correctness Identities

A hash of a field layout is useful, but a raw hash is not a proof of layout identity.

A collision must never permit:
```
object has Layout B
guard expects Layout A
hash(A) == hash(B)
specialized code loads wrong offset
```

That would be a memory-safety bug.

**Revised decision:**

Use hashes for content addressing and lookup, but the runtime must intern layouts into canonical descriptors.

```
layout_hash = Hash(canonical_layout_description)

LayoutRegistry:
    hash → list of canonical LayoutDescriptor*
```

The actual stored identity should be one of:
- canonical LayoutDescriptor*,
- canonical compressed layout handle,
- canonical 64-bit LayoutID allocated after collision resolution.

The hash may be cached, but it is not the sole identity.

**Correct guard model:**

Fast guard:
```
cmp [obj + layout_offset], expected_layout_handle
jne deopt
```

Where expected_layout_handle uniquely identifies a canonical layout descriptor.

Hash comparison may be used as a fast reject in some paths, but not as the only correctness check.

**Revised rule:**

Hashes accelerate identity. They do not establish identity.

### 3. The 64-bit Header is Too Aggressive as the Universal Default

A 64-bit header can work in a constrained engine if almost everything is in side tables. But for a general-purpose polyglot engine with:
- moving GC,
- concurrent marking,
- locks,
- identity hashes,
- foreign interop,
- arrays,
- strings,
- closures,
- dynamic object shapes,
- deoptimization metadata,

a universal 64-bit header becomes a constant fight.

Also, overwriting the header with forwarding information complicates layout guards. If compiled code expects to read a LayoutID from the header, the GC cannot arbitrarily replace that word while mutators are running unless there is a read barrier or a strict phase protocol.

**Revised default header:**

Use a 128-bit default object header.

```
Object:
+0   LayoutID / LayoutDescriptor handle     64 bits
+8   Auxiliary metadata word                64 bits
+16  Payload begins
```

```
Array:
+0   LayoutID / LayoutDescriptor handle     64 bits
+8   Auxiliary metadata word                64 bits
+16  Length + encoding/flags                64 bits
+24  Elements begin
```

```
String (compressed):
+0   LayoutID / LayoutDescriptor handle     64 bits
+8   Auxiliary metadata word                64 bits
+16  Length + encoding/flags                64 bits
+24  Character data begins
```

**Auxiliary word:**

The auxiliary word can contain or reference:
- GC epoch/state,
- size class,
- allocation kind,
- forwarding side-table index,
- lock state,
- identity-hash state,
- remembered-set/card hints,
- foreign lifetime flags.

Large or rare metadata lives in side tables.

For example:
```
aux bits:
    has_identity_hash?
    has_lock_sidecar?
    forwarding_mode?
    size_class
    gc_epoch
    remembered_set_hint
```

**Optional compact mode:**

A 64-bit header can remain as an optional compact object mode for restricted objects:
```
Compact object:
    no lock sidecar
    no identity hash unless externalized
    no foreign pinning
    fixed layout
    side-metadata GC only
```

But it should not be the universal object model.

**Revised rule:**

Default correctness and interoperability first. Compact headers are a specialization, not the foundation.

### 4. GC-Driven Invalidation Must Become GC-Assisted Invalidation

This is a subtle but important correctness issue.

The earlier design said:
- mutator changes object layout;
- mutator logs transition;
- GC later invalidates dependent code.

That is not always safe.

If compiled code depends only on a per-object guard, then changing the object's layout is okay because the next guard check fails.

But if compiled code depends on a global fact such as:
```
Layout L is stable
all objects of kind K have field x at offset O
call target T cannot change
array representation R is globally invariant
```

then waiting until the next GC to invalidate code can be incorrect.

**Revised dependency classes:**

| Dependency type | Example | Invalidation timing |
|---|---|---|
| Local guarded fact | obj.layout == L at this access | Guard failure is sufficient |
| Code-local invariant | this region assumes obj.layout remains L after entry | Region must guard or deopt on mutation |
| Global dependency | all Layout L objects have field x at offset O | Must invalidate before violating publication |

**Revised invalidation protocol:**

The mutator cannot always defer correctness to GC.

Instead:
```
Shape transition / semantic mutation
  → update object metadata atomically
  → log transition
  → if dependency requires immediate invalidation:
      trigger region invalidation or safepoint handshake
  → GC later batches cleanup, retirement, and metadata reclamation
```

The GC can still help with:
- batching,
- deduplication,
- code retirement,
- stale metadata reclamation,
- dependency table compaction,
- finding unreachable code blobs.

But the GC is not the sole correctness mechanism.

**Revised rule:**

GC may batch invalidation work, but semantic publication must not expose invalidated assumptions to unchecked compiled code.

This preserves the original performance idea without making it unsound.

### 5. T1 Stencils Need a Real Fallback Path

Stencil T1 remains one of the best parts of the design.

But the fixed physical map cannot be the whole story.

A pure fixed-map stencil compiler will struggle with:
- high register pressure,
- complex control flow,
- exception regions,
- OSR entry,
- debug instrumentation,
- FFI calls,
- large methods,
- uncommon branches,
- deopt reconstruction.

**Revised T1 structure:**

Use three baseline paths:
```
T1a: pure copy-and-patch stencils
T1b: stencils + simple location allocation / patchable spill slots
T1c: fallback baseline compiler with linear-scan
```

**T1a:**

For simple hot methods:
```
bytecode → stencil copy → patch immediates/offsets/branches → code
```

**T1b:**

For larger methods:
```
bytecode
  → stencil selection with operand-location descriptors
  → patch spill/reload variants
  → emit code
```

**T1c:**

For complex methods:
```
bytecode
  → lightweight CFG
  → simple LLIR
  → linear-scan
  → machine code
```

This avoids the "stencils degrade badly after K registers" failure mode.

**Revised rule:**

T1 is stencil-first, not stencil-only.

That keeps the latency win without making complex methods pathological.

### 6. Polyglot FFI is Not Automatically Zero-Overhead

The critique is right that "zero-overhead FFI" is misleading unless carefully qualified.

There are two different costs:
- mechanical call overhead
- semantic adaptation overhead

The engine can often eliminate the first. It cannot magically eliminate the second.

**Mechanical overhead** includes:
- register moves,
- ABI adaptation,
- stack rearrangement,
- boxing/unboxing,
- trampoline call/return,
- handle setup.

Copy-and-patch trampolines are excellent for reducing this.

**Semantic overhead** includes:
- exception translation,
- object lifetime translation,
- foreign memory ownership,
- dynamic type conversion,
- traceback preservation,
- mutation barriers,
- thread-state transitions,
- safepoint coordination,
- reference-counting ↔ tracing-GC bridging,
- capability/security checks.

These are real semantics. They cannot be optimized away unless proven unnecessary.

**Revised FFI model:**

The universal engine should represent foreign calls as explicit semantic nodes:
```
ForeignCall {
    target_abi,
    source_semantics,
    target_semantics,
    argument_adapters,
    return_adapter,
    exception_adapter,
    lifetime_policy,
    gc_safepoint_policy,
    memory_effect_summary
}
```

If both sides are lowered into the same semantic graph and adapters are proven redundant, the call may inline to near-zero overhead. Otherwise, the trampoline remains.

**Correct claim:**

Not: "Polyglot FFI is zero overhead."

But: "Polyglot calls use the same compiler infrastructure, and mechanical boundary overhead can approach zero when semantic adapters are eliminable."

That is the defensible version.

### 7. T5 Supercompilation Must Be Bounded and Selective

The critique is right that unbounded supercompilation as a general "ultimate optimizer" is not realistic.

Supercompilation is powerful, but it has severe practical issues:
- nontermination,
- explosive specialization,
- huge residual code,
- difficulty with effects,
- difficulty with exceptions,
- difficulty with foreign calls,
- poor cost predictability.

So T5 should not be "run supercompilation on everything until maximum optimization."

**Revised T5:**

T5 is an offline or idle-time portfolio optimizer with access to every deterministic pass, including:
- whole-program analysis,
- aggressive inlining,
- partial evaluation,
- bounded supercompilation,
- deforestation/fusion,
- specialization,
- global code layout,
- identical code folding,
- cross-language optimization,
- profile-guided restructuring.

But every aggressive transformation has a budget.

**Supercompilation constraints:**

T5 supercompilation should require:
- fuel limit
- residual code size limit
- specialization depth limit
- termination/generalization checks
- effect boundary restrictions
- deopt/exception compatibility
- profitability check
- fallback to ordinary optimization

**Good targets:**
- pure numeric kernels,
- parser/interpreter loops with known program structure,
- small recursive functions,
- DSL interpreters,
- format serializers/deserializers,
- known immutable data transformations,
- monomorphic call chains,
- stable FFI adapter chains.

**Bad targets:**
- arbitrary effectful application code,
- highly dynamic megamorphic call sites,
- foreign calls with unknown behavior,
- exception-heavy code,
- large loops with unpredictable exits,
- code whose residual size explodes.

**Revised T5 rule:**

Supercompilation is a tool in T5, not the definition of T5.

And we should explicitly retract: "theoretical maximum code quality." That is not a serious production claim.

### 8. Revised Tier Structure

The tiers should be understood as different budgets and frontends over the same engine, not separate compilers.

```
T0: Interpreter
T1: Stencil baseline
T2: Fast optimizing method compiler
T3: Aggressive optimizing method/region compiler
T4: Trace frontend using same optimizer
T5: Offline / idle whole-program portfolio optimizer
```

**T0:**
- computed goto;
- superinstructions;
- PICs;
- lightweight type/layout feedback;
- bytecode verification and profiling;
- deopt fallback target.

**T1:**
- stencil-first;
- copy-and-patch bytecode to machine code;
- optional simple location allocation;
- fallback linear-scan baseline for complex methods;
- region metadata emitted from the start.

**T2:**
- unified graph IR;
- cheap GVN/CSE;
- simple escape analysis;
- simple ARS;
- guard insertion/fusion;
- limited inlining;
- linear-scan allocation.

Goal: good code quickly.

**T3:**
- same graph IR;
- stronger alias/escape analysis;
- speculative devirtualization;
- AOSS specialization;
- partial escape analysis;
- loop optimizations;
- stronger ARS;
- graph-coloring or advanced split/live-range allocation when profitable.

Goal: hot code fast.

**T4:**
- trace frontend;
- records hot cross-function paths;
- lowers into same graph IR;
- optimized as a specialized region;
- exits guarded;
- not necessarily "above" T3.

Goal: specialize hot paths that method compilation handles poorly.

**T5:**
- whole-program or large-subprogram optimizer;
- all deterministic passes available;
- bounded partial evaluation;
- bounded supercompilation;
- cross-language semantic inlining where legal;
- global code layout;
- profile-guided specialization;
- offline/idle-time only.

Goal: maximum practical improvement under explicit budgets.

### 9. Revised Architecture Summary

The corrected architecture is:

```
┌─────────────────────────────┐
│ UGB / language frontends     │
│ bytecode / traces / FFI      │
└──────────────┬──────────────┘
               │
               T0 interpreter
               │
               T1 stencil baseline
               │
               ▼
┌────────────────────────────────────┐
│ Unified optimizing graph IR         │
│                                    │
│ - SSA values                       │
│ - control/effect/memory edges       │
│ - VSDG-style dependence overlays    │
│ - AOSS facts/guards/dependencies    │
│ - region/deopt metadata             │
└────────────────┬───────────────────┘
                 │
                 LLIR
                 │
                 MIR
                 │
                 native code
```

VSDG survives as a semantic discipline, not a duplicated graph stage.

T5 survives as a budgeted portfolio optimizer, not an unbounded academic transformation engine.

The object model survives, but with a more realistic default header.

FFI survives, but with explicit semantic adapters.

### 10. Updated Decision Ledger

| Subsystem | Previous direction | Revised direction |
|---|---|---|
| VSDG | Separate IR before SoN | Dependence/fact overlay on unified graph |
| SoN | Separate graph target | Canonical optimizing graph |
| Layout identity | Raw layout hash | Interned canonical layout handle, hash-assisted |
| Object header | 64-bit universal header | 128-bit default, optional compact mode |
| GC invalidation | GC-driven delayed invalidation | GC-assisted; immediate publication safety where required |
| T1 | Fixed stencil map only | Stencil-first with T1b/T1c fallback |
| FFI | "Zero-overhead" polyglot | Near-zero mechanical overhead when semantic adapters eliminable |
| T5 | Ultimate supercompiler | Offline portfolio optimizer with bounded supercompilation |
| Tier compiler sharing | Partly specified | Explicitly one shared compiler core |
| Region deopt | Keep | Keep, but tie to precise dependency publication rules |

### Bottom Line

The critique is correct that the earlier design, if implemented literally, would probably lose to mature production JITs.

But the architecture can be made much more competitive by making these changes:
1. Drop mandatory VSDG → SoN conversion.
2. Use one canonical graph IR with VSDG-style overlays.
3. Use canonical layout descriptors, not raw hashes, for correctness.
4. Use 128-bit default headers or a fully specified side-metadata protocol.
5. Make invalidation publication-safe, not merely GC-delayed.
6. Keep stencil T1, but add fallback baseline compilation.
7. Treat polyglot FFI as semantic adaptation, not just ABI marshaling.
8. Make T5 powerful but bounded.

---

## OSR-1: On-Stack Replacement Protocol Specification
**Version 1.0 · Core Execution Contract**

### 1. Fundamental Model & Triggers
OSR allows the engine to transfer execution from a lower tier (T0/T1) to a higher tier (T2/T3/T4) *while a method is actively executing*, without waiting for the method to return.

#### 1.1 OSR Trigger Points
- **T0 (Interpreter)**: Back-edge counters (for loops) and call-site counters. When a threshold is crossed, the interpreter invokes the OSR trampoline.
- **T1 (Stencil Baseline)**: T1 stencils contain a relative jump from each loop back-edge to the CodeBlob's single shared OSR Trampoline Stub (see OSR-1 section 6). The stub, not the stencil, owns the counter, which lives in the Metadata Pool. If tripped, the stub invokes trigger_osr.
- **T2/T3**: Generally do not trigger OSR to higher tiers; they rely on standard method-entry tier-ups or trace recording (T4).

#### 1.2 The OSR Target
The target of an OSR transition is a specific **Bytecode PC** (usually a loop header or a hot call site). The compiler must generate a specialized entry point that accepts the interpreter's state and transitions it into the optimized graph's state.

---

### 2. The OSR Entry Node (Unified Graph IR)
In the Unified Graph IR, OSR is not a magical runtime hack; it is a first-class control-flow construct.

#### 2.1 The `OSR_ENTRY` Block
For every bytecode PC that is a valid OSR target, the compiler generates a special basic block: `OSR_ENTRY_<pc>`.
- **Inputs**: A single pointer to the current VM frame (`vm_frame_ptr`).
- **Outputs**: The initial values for all variables that are **live** at the target bytecode PC.
- **Control Flow**: It acts as a predecessor to the target block (e.g., the loop header).

#### 2.2 Integration with Loop Phis
When OSR-ing into a loop, the loop header contains Phi nodes (φ) merging values from the pre-header and the loop back-edge. 
The `OSR_ENTRY` block acts as a "virtual pre-header." It extracts the current values from the VM frame and feeds them directly into the loop's Phi nodes, initializing the loop's state for the optimized code.

---

### 3. State Mapping & ARS Integration (The Hard Part)
This is where OSR interacts with Adaptive Representation Selection (ARS). T0 stores everything as 64-bit `TAGGED` (NaN-boxed) values. T2/T3 may expect unboxed `I64`, `F64`, or `REF_OFF` values.

#### 3.1 Extraction and Conversion
The `OSR_ENTRY` block must emit instructions to load the VM frame slots and convert them to the representations expected by the optimized graph.
```text
; Example: T3 expects an unboxed float (F64) for variable v1
v1_tagged = LOAD_VM_FRAME(vm_frame_ptr, offset_0)
v1_f64 = TAG_TO_F64(v1_tagged)  ; Extract binary64
```

#### 3.2 Representation Mismatch Guards
What if T3 assumed `v1` is always a float, but at the exact moment of OSR, the VM frame holds an integer? 
We cannot silently corrupt the execution. The `OSR_ENTRY` block must emit a **Guard**:
```text
v1_tagged = LOAD_VM_FRAME(vm_frame_ptr, offset_0)
v1_is_float = CHECK_TAG(v1_tagged, FLOAT_TAG)
GUARD(v1_is_float, deopt_osr_mismatch) ; Deopt if assumption fails
v1_f64 = TAG_TO_F64(v1_tagged)
```
If the guard fails, execution deoptimizes back to T0. T0 will resume interpretation. (The engine must track "OSR deopt failures" to prevent infinite recompilation loops).

---

### 4. Guard Handling & Control Flow at OSR Entry
When we OSR into the middle of a method, we bypass the method's normal entry point. This means we bypass the **Region Entry Guards** (e.g., `obj.layout == X`).

#### 4.1 The Dominator Guard Injection
We cannot execute specialized operations (like direct field loads) without verifying their assumptions. 
During the VSDG/SoN lowering phase, the compiler queries the graph: *"What guards dominate the OSR target block?"*
The compiler then **injects those exact guards** into the `OSR_ENTRY` block.

```text
OSR_ENTRY_loop_header:
  ; 1. Extract state & check representations (ARS)
  obj_tagged = LOAD_VM_FRAME(...)
  GUARD_TAG(obj_tagged, REF_TAG, deopt_mismatch)
  obj_off = TAG_TO_REF(obj_tagged)
  
  ; 2. Re-execute dominating region guards
  obj_ptr = REF_OFF_TO_RAW(obj_off)
  layout_id = LOAD(obj_ptr)
  GUARD(layout_id == 0x4A, deopt_layout_changed) ; Re-check layout!
  
  ; 3. Jump to the actual loop header
  JUMP loop_header
```
This ensures that even though we entered via the side door, all semantic assumptions remain mathematically proven.

---

### 5. LLIR & MIR Lowering
#### 5.1 LLIR Lowering
In LLIR, the `OSR_ENTRY` block is lowered into explicit memory loads from the VM frame, explicit conversion instructions (`UNBOX_*`, `TAG_TO_*`), and explicit guard branches. 
- **Invariant OSR-LLIR-01**: Every live variable at the OSR PC must have a corresponding `LOAD_VM_FRAME` and conversion sequence in the `OSR_ENTRY` block.
- **Invariant OSR-LLIR-02**: The `OSR_ENTRY` block must end with a jump to the target block or its immediate dominator.

#### 5.2 MIR Lowering & The OSR Stub
In MIR, the `OSR_ENTRY` block becomes a physical **OSR Stub** in the code cache.
- The Register Allocator treats the `OSR_ENTRY` block as a root. It allocates physical registers for the unboxed values.
- The stub ends with a jump to the optimized loop's physical address.
- **Safepoint Metadata**: The OSR stub itself is a safepoint. If a GC happens exactly while the OSR stub is executing (rare, but possible), the GC must know how to scan the VM frame.

---

### 6. T1 Stencil OSR Mechanism
T1 uses Copy-and-Patch stencils, so it doesn't have a graph to insert `OSR_ENTRY` blocks into. How does T1 do OSR?

#### 6.1 The T1 OSR Trampoline (Corrected: Stencil Purity)

The 1.0 mechanism embedded a 32-bit counter inside the T1 stencil code stream. That causes a stencil-pool explosion and is corrected here (finding T1-OSR, resolved).

A counter in the code stream requires pre-compiled "stencil-with-counter" variants for every register mapping and spill state that a back-edge can occur in. The Stencil Pool grows combinatorially with the fixed physical map width, destroying the memory footprint and startup advantages of copy-and-patch.

The corrected mechanism keeps stencils pure and moves the counter outside them:

1. **Pure stencils**: a stencil contains no counters and no counter increment code. Its back-edge terminator is a relative jump to the CodeBlob's OSR Trampoline Stub. Stencil selection remains a pure function of the bytecode operand locations.
2. **Shared OSR Trampoline Stub**: each T1 CodeBlob carries exactly one OSR Trampoline Stub at its end, emitted once per blob during T1 assembly, not per stencil instance. The stub saves caller-saved physical registers, loads the back-edge counter from the method's Metadata Pool entry, decrements it, and, on trip, calls trigger_osr(method_id, bytecode_pc, vm_frame_ptr) with the metadata-supplied PC. On non-trip, it restores registers and jumps back to the loop head through a patchable back-link.
3. **Counter in the Metadata Pool**: the counter is a writable 32-bit slot in the Metadata Pool, addressed by the stub. It is thread-local (PROF-1 section 3.1) and needs no atomicity.
4. **Post-OSR patching**: after T2 emits the OSR Stub, the runtime patches the T1 back-edge jump to target the T2 OSR Stub directly. The T1 trampoline and its metadata counter become cold and are reclaimed by the Code Cache sweeper (CODE-1 section 7).

Pool accounting: stencils per bytecode opcode remain O(operand-location variants). The OSR mechanism adds one stub per CodeBlob and one metadata slot per loop, both O(methods), not O(variants).

### 7. Deoptimization from OSR Code
If the optimized (OSR-compiled) code hits a failing guard *inside* the loop, it must deoptimize back to T0.

#### 7.1 State Reconstruction
The deoptimization mechanism is identical to standard deopt, but with one caveat: the physical registers now hold **unboxed** values (e.g., `F64`), while T0 expects **tagged** values (`TAGGED`).
- The Safepoint Map (generated in LLIR) explicitly records the representation of every physical register at the deopt PC.
- The Deopt Stub reads the map, sees that `FPR_F0` holds an `F64`, and emits the boxing logic (`F64_TO_TAG`) before writing the value back to the T0 VM frame.

---

### 8. OSR Invariants
To guarantee correctness, the following invariants must hold:

| ID | Invariant |
|---|---|
| **OSR-INV-01** | **State Completeness**: The `OSR_ENTRY` block must extract and convert *every* variable that is live at the target bytecode PC. No live VM value may be left uninitialized. |
| **OSR-INV-02** | **Guard Dominance**: The `OSR_ENTRY` block must re-evaluate all guards that dominate the target PC in the optimized graph. Specialized operations must never execute without their assumptions being verified post-OSR. |
| **OSR-INV-03** | **Representation Safety**: If ARS selects an unboxed representation for an OSR live variable, a type-check guard must be emitted in the `OSR_ENTRY` block. Mismatches must deoptimize, never corrupt. |
| **OSR-INV-04** | **Deopt Reversibility**: The safepoint map for any PC reachable from an `OSR_ENTRY` block must accurately map physical unboxed registers back to tagged VM frame slots. |
| **OSR-INV-05** | **No Infinite OSR Loops**: If an OSR transition immediately deoptimizes due to a representation/guard mismatch at the entry block, the runtime must mark the method/PC as "OSR-unstable" and prevent further OSR attempts for that specific profile. |
| **OSR-INV-06** | **Stencil Purity**: T1 stencils must not contain profiling counters, OSR state, or per-method writable data. All counters live in the Metadata Pool and are accessed only through shared stubs. A stencil variant created solely to host a counter is a specification violation. |

---

### Summary of the OSR Flow
1. **T0 Interpreter** hits a back-edge counter.
2. **T0** calls `trigger_osr`.
3. **T2 Compiler** builds the Unified Graph, identifies the loop, and generates an `OSR_ENTRY` block.
4. **ARS** decides loop variables should be unboxed. `OSR_ENTRY` emits unboxing logic and type guards.
5. **Graph Optimizer** identifies dominating layout guards and injects them into `OSR_ENTRY`.
6. **MIR** emits the physical OSR Stub and patches the T0/T1 back-edge.
7. **Execution** seamlessly jumps into the optimized loop, fully unboxed and guarded.
8. If a guard fails inside the loop, **Deopt** boxes the values back to `TAGGED` and resumes T0.
---

## PROF-1: Continuous Profiling & PGO Infrastructure
**Version 1.0 · Core Execution Contract**

### 1. Fundamental Design Philosophy
The profiling infrastructure must satisfy three competing constraints:
1. **Near-Zero Mutator Overhead**: Profiling must not degrade the performance of T2/T3 hot code. Atomic counter increments on every loop iteration cause severe cache-line bouncing and pipeline stalls.
2. **High-Fidelity Feedback**: The compiler needs accurate type, shape, and control-flow data to drive AOSS specialization, ARS (Adaptive Representation Selection), and guard insertion.
3. **Decoupled Collection and Analysis**: Mutator threads collect raw data asynchronously. Compiler threads analyze aggregated data. They never block each other.

### 2. Profile Data Taxonomy
We categorize profile data into five distinct domains. Each domain requires different collection strategies.

| Domain | Data Collected | Primary Consumer |
| :--- | :--- | :--- |
| **Control Flow** | Back-edge counts, call-site counts, branch outcomes (taken/not-taken). | Tier-up triggers, T5 code layout, T5 loop unrolling. |
| **Type & Shape (AOSS)** | Receiver types, LayoutIDs, property access shapes, call targets. | AOSS specialization, ARS unboxing, Guard insertion. |
| **Allocation** | Allocation sites, object sizes, escape hints (thread-local vs. global). | T2/T3 Escape Analysis, Write Barrier elision, T5 allocation hoisting. |
| **Deoptimization** | Deopt reasons, frequencies, penalty counters per region/guard. | Preventing deopt storms, adjusting speculation confidence. |
| **Memory/Vector** | Array access strides, alignment, element homogeneity. | T3 Auto-vectorization, SLP, Array unboxing. |

### 3. Collection Mechanisms (Minimizing Overhead)
We do not use a single mechanism for all tiers. The collection strategy evolves as code gets hotter.

#### 3.1 T0/T1: Instrumentation (Thread-Local Counters)
T0 (Interpreter) and T1 (Stencil Baseline) use explicit instrumentation.
- **Thread-Local Buffers (TLB)**: Each thread maintains a local array of counters. Mutators increment their local TLB. This avoids atomic operations and cache-line contention entirely.
- **Flushing**: At safepoints (or via a periodic timer interrupt), the TLB is flushed and merged into the global `MethodProfile`.
- **T1 Stencil Integration**: T1 stencils for back-edges include a `INCREMENT_TLB(offset)` instruction. Because it's a thread-local memory write, it is extremely cheap (1-2 cycles).

#### 3.2 T2/T3: Inline Caches, Sampled Back-Edge Counters & PMU

Once code reaches T2/T3, full instrumentation of every edge is gone. The 1.0 text stated that edge counters are stopped entirely and that control-flow feedback comes from Inline Caches plus hardware sampling. That is insufficient, and it is corrected here (finding PROF-1, resolved).

T4 (the trace compiler) requires an accurate signal for which loops are hot enough to justify trace recording. PMU/PEBS sampling gives statistical PC histograms whose skew is too large to reliably identify short, tight loops: a 3-instruction loop and a 30-instruction loop with equal iteration counts produce very different sample densities, and low-frequency sampling routinely misses tight loops entirely. Inline cache misses say nothing about loop iteration counts. Both signals are retained for type feedback and code layout, but neither is a back-edge counter.

The corrected contract: T2/T3 retain a lightweight, sampled back-edge counter per loop:

- **Emission**: the optimizing backend emits, on each loop back-edge, a decrement of a dedicated register preloaded from the loop's profile slot and a predicted-not-taken branch to the sample handler every N iterations (sampling mask, e.g. N=64). The steady-state cost is one decrement and one not-taken branch per iteration, which is 1-2 cycles and does not use atomics (PROF-INV-01 preserved: the counter is thread-local, so no LOCK prefix is involved).
- **Sample handler**: reloads the register from the profile slot for the next window, folds the window count into the thread-local profile buffer, and, when the accumulated count crosses the T4 trigger threshold, enqueues a trace-recording request for that loop header.
- **Register pressure**: the counter occupies one dedicated scratch register. If register pressure in a loop is extreme, the register allocator may spill exactly this counter across the loop; spilling a value touched once per N iterations is acceptable, and the fallback is to sample via the loop's existing safepoint instead.
- **What is NOT retained**: per-edge instrumentation, branch-outcome counters, and call-site counting in T2/T3 code. Those remain IC- and PMU-derived.

Tier-up consumers of the sampled counters:
- T4 trace recording requests (primary consumer).
- T2 to T3 promotion corroboration when execution-time sampling is ambiguous.

### 4. Profile Storage & Architecture
The profile data is structured hierarchically to support both online JIT and offline T5 PGO.

#### 4.1 Method-Level Profile
Attached to the `MethodDescriptor`. Contains:
- **Counters**: Aggregated back-edge and call-site counts.
- **IC History**: A ring buffer of the last $N$ observed LayoutIDs/Types for each call/access site.
- **Deopt Log**: A map of `RegionID -> DeoptReason -> Count`.

#### 4.2 Global/Module-Level Profile
Aggregated data for whole-program analysis (used by T5).
- **Call Graph**: Edges weighted by call-site counts.
- **Class Hierarchy Analysis (CHA) Weights**: Which subclasses are actually instantiated and called.
- **Serialized Format**: Can be dumped to disk at shutdown for offline T5 PGO.

#### 4.3 The IC Stub Array (Side-Table)
Instead of embedding IC state directly in the executable code (which bloats the code cache and complicates patching), we use a side-table.
- The compiled code contains a `LOAD_IC_STUB(index)` instruction.
- The IC Stub Array holds the current state (Monomorphic, Polymorphic, Megamorphic) and the associated fast-path code pointers.
- **Benefit**: IC state can be updated, shared across different compiled versions of the same method, and garbage-collected independently of the code cache.

### 5. Integration with Optimization Tiers

#### 5.1 Tier-Up Triggers
Tier-ups are driven by the global `MethodProfile`.
- **T0 $\rightarrow$ T1**: Triggered when a T0 TLB counter exceeds `THRESHOLD_T1` (e.g., 1,000).
- **T1 $\rightarrow$ T2**: Triggered when the T1 TLB counter exceeds `THRESHOLD_T2` (e.g., 10,000) AND the IC history shows stable types (monomorphic or low-polymorphism).
- **T2 $\rightarrow$ T3**: Triggered when execution time in T2 exceeds a threshold, OR when T2 deoptimizes due to a megamorphic site, prompting T3 to compile a highly specialized version with aggressive guards.

#### 5.2 Driving AOSS and ARS
- **AOSS**: The compiler queries the IC History for an object access. If 99% of observations are `LayoutID_A`, AOSS emits a `GuardLayout(LayoutID_A)` and specializes the field offset.
- **ARS**: The compiler queries the type profile for a variable. If the profile proves it is 100% `F64` (unboxed float), ARS commits to the `F64` representation in LLIR, eliminating NV-1 boxing/unboxing.

### 6. The Deoptimization Feedback Loop (Crucial)
If T3 speculates on a profile and deoptimizes, we must update the profile to prevent an infinite "deopt storm" (compile $\rightarrow$ deopt $\rightarrow$ compile $\rightarrow$ deopt).

#### 6.1 Deopt Penalties
When a region deopts, the runtime executes a `RECORD_DEOPT(region_id, reason)` stub before jumping to T0.
- The `MethodProfile` increments the deopt counter for that specific `RegionID` and `Reason`.
- **Confidence Decay**: The profile applies a penalty to the specific type/shape assumption that failed.
- **Recompilation Guard**: When the compiler is invoked again, it checks the deopt counter. If it exceeds `MAX_DEOPTS`, the compiler **bans** that specific speculation and forces a generic, un-specialized path (or a safer, heavily guarded path).

### 7. T5 Offline PGO Integration
T5 operates offline or during idle time. It consumes the serialized Global Profile to perform transformations that are too expensive for online JIT.

#### 7.1 Profile-Guided Code Layout
T5 uses the control-flow profile to sort basic blocks.
- **Hot/Cold Splitting**: Blocks with high execution frequency are placed sequentially in the "Hot" section (improving I-cache locality and branch prediction). Blocks with low frequency (error handling, rare deopts) are moved to the "Cold" section.

#### 7.2 Profile-Guided Inlining
T5 uses the weighted call graph to decide inlining.
- Instead of just inlining "small" functions, T5 inlines functions that are called frequently *from hot paths*, even if they are moderately large.
- **Cloning**: If a function is called with different type profiles from different sites, T5 clones the function, specializing each clone for its specific call-site profile.

#### 7.3 Profile-Guided Loop Unrolling
T5 uses back-edge counts and loop-body size to calculate the optimal unrolling factor, balancing I-cache pressure against loop overhead.

### 8. PROF-1 Invariants
To guarantee correctness and performance, the following invariants must hold:

| ID | Invariant |
| :--- | :--- |
| **PROF-INV-01** | **No Atomic Contention on Hot Paths**: T2/T3 compiled code must never execute atomic instructions (e.g., `LOCK XADD`) for profiling. Profiling is done via IC misses or statistical sampling. |
| **PROF-INV-02** | **Thread-Local Safety**: TLB flushes must only occur at safepoints or via thread-local handshake, ensuring no data races during the merge. |
| **PROF-INV-03** | **Deopt Prevention**: If a specific speculation deoptimizes more than $K$ times, the compiler must permanently disable that speculation for the method. |
| **PROF-INV-04** | **IC Consistency**: The IC Stub Array must be updated atomically with respect to the mutator. A mutator must never see a partially updated IC state. |
| **PROF-INV-05** | **Profile Completeness**: The T0/T1 profile must capture all live virtual registers' types at safepoints to ensure T2/T3 ARS has complete information. |
| **PROF-INV-06** | **T5 Serialization Safety**: The global profile can be serialized for T5 without stopping the world, using a copy-on-write or double-buffering mechanism. |
| **PROF-INV-07** | **Back-Edge Sampling Presence**: Every T2/T3-compiled loop must carry the sampled back-edge counter (section 3.2). A T2/T3 code path with uncounted loop back-edges is incorrect, because T4 trace recording cannot discover hot loops from PMU sampling or IC misses alone. |
| **PROF-INV-08** | **Sampling Budget**: The sampled counter may cost at most one decrement and one not-taken branch per iteration. It must not introduce atomics, locks, or store traffic to shared memory on the steady-state path. |
---

## CODE-1: Code Cache & Executable Memory Management
**Version 1.0 · Core Execution Contract**

### 1. Fundamental Design Philosophy
Executable memory is the most constrained and expensive resource in a modern JIT. Operating systems strictly enforce W^X (Write XOR Execute) security policies, and executable pages cannot be easily shared or swapped. 
The architecture must satisfy four competing constraints:
1. **Strict W^X Compliance**: Memory must never be simultaneously writable and executable.
2. **Metadata Separation**: Executable pages must contain *only* instructions and read-only constants. Metadata (safepoint maps, dependencies) must live in standard read-write memory.
3. **Fragmentation Resistance**: The code cache must not fragment into unusable slivers over long-running sessions.
4. **Concurrent Safety**: Compiler threads must allocate and patch code while mutator threads execute it, without data races or ICache coherency failures.

---

### 2. Memory Topology: The Four Heaps
We do not use a single monolithic "Code Cache". We segregate memory by lifetime, mutability, and access patterns.

| Heap Name | Protection | Contents | Allocation Strategy |
| :--- | :--- | :--- | :--- |
| **Stencil Pool** | Read-Only, Execute (RX) | Pre-compiled T1 Stencils, shared across all threads. | Allocated once at startup. Never freed. |
| **Code Pool** | RX (with RW alias) | T1b/T1c/T2/T3/T4 emitted machine code. | Bump-pointer with TLABs. Evictable. |
| **Stub Pool** | RX (with RW alias) | Deopt stubs, OSR stubs, FFI trampolines, PIC miss stubs. | Bump-pointer. Evictable. |
| **Metadata Pool** | Read-Write (RW) | CodeBlob headers, Safepoint maps, Region descriptors, AOSS dependencies. | Standard RW allocator. Tied to Code Pool lifecycle. |

**Why separate Metadata?**
If a 10KB T3 method has 2KB of safepoint/dependency metadata, embedding it in the Code Pool wastes 20% of your scarce executable memory. By placing the `CodeBlobHeader` and `SafepointRecords` in the Metadata Pool, the Code Pool remains dense with pure instructions, maximizing I-Cache efficiency.

---

### 3. Virtual Memory & W^X Protocol
To satisfy OS security models (macOS `MAP_JIT`, Linux `SELinux/PaX`, Windows `CFG`), we use a **Dual-Mapping Protocol**.

#### 3.1 Dual-Mapping Mechanism
When the engine requests a new page of executable memory from the OS (via `mmap` or `VirtualAlloc`), it maps the *same underlying physical memory* twice into the process's virtual address space:
1. **RW View**: Used *only* by compiler threads and the patcher to write instructions and patch branches.
2. **RX View**: Used *only* by mutator threads to execute the code.

This eliminates the need for costly `mprotect` system calls (which trigger TLB shootdowns and stall all cores) when transitioning between writing and executing.

#### 3.2 Apple Silicon (AArch64) Special Case
On macOS/iOS, dual-mapping is restricted. We must use the `MAP_JIT` flag and the thread-local write toggle:
```c
pthread_jit_write_protect_np(0); // Enable RW for this thread
// ... write instructions ...
pthread_jit_write_protect_np(1); // Re-enable RX
sys_icache_invalidate(start, length); // Crucial: sync ICache
```

#### 3.3 ICache Coherency Invariant
On x86-64, the instruction cache is coherent with the data cache. On AArch64 and RV64, it is not.
**CODE-INV-01**: After any compiler thread finishes writing to the RW view of the Code/Stub Pool, it *must* execute an architecture-specific ICache flush (e.g., `DC CVAU` / `IC IVAU` on AArch64) before publishing the code pointer to mutator threads.

---

### 4. CodeBlob Physical Anatomy
A compiled method (CodeBlob) is split across the Code Pool and Metadata Pool.

#### 4.1 In the Metadata Pool (RW)
```text
CodeBlobHeader {
    method_id: u64,
    tier: u8 (T1, T2, T3, T4),
    code_size: u32,
    code_ptr_rx: *u8,       // Pointer to the RX view in the Code Pool
    region_count: u16,
    regions: [RegionDescriptor], // Entry offsets, deopt targets
    safepoint_count: u16,
    safepoints: [SafepointRecord], // PC offsets, register maps, stack maps
    dependencies: [AOSS_Dependency], // LayoutIDs, Call targets this code relies on
    state: enum (Active, DeoptPending, Zombie)
}
```

#### 4.2 In the Code Pool (RX)
```text
[ Executable Instructions ]
    - Prologue / Stack allocation
    - Region 1 (Loop A)
    - Region 2 (Straight-line)
    - Epilogue / Return
[ Read-Only Constant Pool ]
    - 64-bit float literals, large integer immediates, method pointers
```

---

### 5. Allocation & Fragmentation Management
The Code Pool is notorious for fragmenting as methods are compiled, deoptimized, and recompiled.

#### 5.1 Compiler Thread-Local Allocation Buffers (TLABs)
To prevent lock contention when multiple compiler threads (T2/T3) emit code simultaneously:
1. The Code Pool maintains a global free-list of large memory chunks (e.g., 256KB).
2. When a compiler thread needs space, it claims a chunk (TLAB) via an atomic pointer bump.
3. The thread allocates its CodeBlob locally within its TLAB without locks.
4. If the TLAB is exhausted, it requests a new one.

#### 5.2 Size-Segregated Arenas
To prevent a massive T3 method from fragmenting the space needed for tiny FFI stubs, the Code Pool is divided into size classes:
- **Arena S (Small)**: Stubs, trampolines, T1a stencils (< 1KB).
- **Arena M (Medium)**: T1c/T2 methods (1KB - 16KB).
- **Arena L (Large)**: T3/T4 highly optimized methods (> 16KB).
Allocation only occurs within the appropriate arena.

---

### 6. Region-Based Deopt & Patching Protocol
How do we patch a Region's entry point to jump to a Deopt Stub without violating W^X or causing race conditions?

#### 6.1 The Patchable NOP Sled
When T2/T3 emits a Region, the first 4 to 16 bytes of the Region's entry point are reserved as a **Patchable NOP Sled** (e.g., `NOP; NOP; NOP; NOP` on x86, or `BTI` / `NOP` equivalents on AArch64).

#### 6.2 Safepoint-Synchronized Patching
When the AOSS DependencyMap detects a global invalidation (e.g., a LayoutID transition):
1. The JIT marks the `CodeBlobHeader` as `DeoptPending`.
2. The JIT waits for the next **Global Safepoint** (where all mutator threads are paused).
3. At the safepoint, the JIT uses the **RW View** of the Code Pool to overwrite the NOP sled with a `JMP deopt_stub`.
4. The JIT flushes the ICache for that specific region.
5. Mutator threads resume. Any thread that was about to enter the region now hits the `JMP` and deoptimizes safely.

**CODE-INV-02**: Code patching *never* occurs while mutator threads are actively executing the patched region. Patching is strictly serialized with global safepoints.

---

### 7. Lifecycle & Reclamation (Sweeping)
When a method deoptimizes, its CodeBlob cannot be immediately freed because threads might still have return addresses pointing into it (the "Zombie" problem).

#### 7.1 State Machine
1. **Active**: Executable, referenced by AOSS dependencies.
2. **DeoptPending**: Dependencies invalidated. Entry points patched to deopt stubs.
3. **Zombie**: All mutator threads have passed through a safepoint since the patch. No thread can possibly be executing the old code.
4. **Reclaimable**: Ready to be freed.

#### 7.2 The Sweeper
During a GC safepoint, the GC and JIT coordinate:
1. The JIT scans all thread stacks. If a return address falls within a `DeoptPending` CodeBlob, the stack is rewritten to point to the T0 interpreter frame (or the code is kept alive).
2. If no stack references a `DeoptPending` CodeBlob, it transitions to `Zombie`.
3. The Sweeper reclaims the memory in the Code Pool and Metadata Pool.
4. To prevent Code Pool fragmentation, the Sweeper can optionally perform **Code Compaction** (moving live CodeBlobs to defragment the RX heap), updating all internal pointers and stack maps atomically during the safepoint.

---

### 8. CODE-1 Invariants

| ID | Invariant |
| :--- | :--- |
| **CODE-INV-01** | **W^X Strictness**: No page in the Code/Stub Pool is ever simultaneously writable and executable. Dual-mapping or OS-specific JIT toggles are mandatory. |
| **CODE-INV-02** | **Metadata Separation**: Safepoint maps, dependency records, and CodeBlob headers reside in the RW Metadata Pool, not the RX Code Pool. |
| **CODE-INV-03** | **ICache Coherency**: Any write to the Code/Stub Pool via the RW view must be followed by an architecture-specific ICache invalidation before the RX view is exposed to mutators. |
| **CODE-INV-04** | **Safepoint Patching**: Instruction patching for Region Deopt only occurs during a global safepoint. Mutators never observe a partially patched instruction stream. |
| **CODE-INV-05** | **Zombie Safety**: A CodeBlob's executable memory is never reclaimed until a full safepoint has occurred *after* its invalidation, guaranteeing no thread holds a stale return address. |
| **CODE-INV-06** | **Lock-Free Allocation**: Compiler threads allocate Code Pool memory via TLABs without acquiring global mutexes. |
---

## CONC-1: Concurrency & Lock-Free Compiler Structures
**Version 1.1 · Adds the dependency add-vs-invalidate handshake**

### 1. Fundamental Design Philosophy
In a multi-tier JIT with a concurrent GC, compiler threads (T2/T3/T4) and mutator threads must operate concurrently without blocking each other. Global mutexes on the compilation or execution hot paths are strictly forbidden. We achieve this through **lock-free data structures**, **Read-Copy-Update (RCU)** patterns, and **Epoch-Based Reclamation (EBR)** tied to the GC's safepoint protocol.

### 2. Core Lock-Free Data Structures

#### 2.1 AOSS DependencyMap (RCU-Based)
The DependencyMap tracks which compiled Regions depend on which `CanonicalLayoutHandle`s. 
- **Read Path (Compilers/Invalidators)**: Reads are completely lock-free. We use an RCU (Read-Copy-Update) linked-list or hash-map per layout. Compilers traverse the list without acquiring locks.
- **Write Path (Mutators/Shape Transitions)**: When a new dependency is added, the compiler allocates a new node, copies the existing list, and atomically swaps the head pointer using `memory_order_release`.
- **Reclamation**: Old list nodes are not freed immediately. They are placed in a thread-local retirement queue. The concurrent GC's epoch-based reclamation (triggered at safepoints) safely frees these nodes once all compiler threads have passed the current epoch.

#### 2.1.1 The Add-vs-Invalidate Handshake (Corrected Addition)

The 1.0 RCU description leaves one race unspecified (finding CONC-1, resolved): a compiler thread adding a dependency to a layout's list while the runtime invalidates that layout. If the invalidator's traversal misses the concurrently added dependency, compiled code survives with an assumption the shape transition already violated. Missing the dependency is not a performance defect; it is unsoundness.

The corrected protocol publishes additions through a per-layout seqlock and bounds invalidation by a retry loop, with a safepoint handshake as the escalation path:

- **Per-layout sequence lock**: each CanonicalLayoutHandle's dependency list carries an even/odd sequence counter. A compiler thread adding a dependency: (1) reads the counter; (2) if odd, spins with a cpu-relax hint (an invalidation traversal or another publish is in progress); (3) installs the new node by Copy-on-Write of the list it observed; (4) publishes with a release increment of the counter to the next even value.
- **Invalidator traversal**: the invalidator (shape transition handler) reads the counter (must be even, else spins briefly), records it, traverses the published list, and re-reads the counter. If the counter changed, the traversal overlapped a publication window and is retried. Because publishes use release ordering and the re-check uses acquire ordering, any dependency published before the invalidator's final re-read is visible to a completed traversal.
- **Publication bound (soundness argument)**: a shape transition must not complete its invalidation step until one traversal finishes with an unchanged sequence counter. An add that started before the transition either published before the successful traversal (and is seen) or is still mid-publish (counter odd; invalidator spins or retries). An add that starts after the successful traversal targets the new layout generation and is rejected by the install-time generation check.
- **Generation check on install**: a dependency records the layout generation it was compiled against. If the generation advanced between the compiler's read and its publish, the dependency is stale on arrival; the compiler discards it and re-derives from current state.
- **Escalation to safepoint handshake (rare)**: if a layout's sequence counter is contended beyond a bounded spin count, the invalidator escalates to the thread-local handshake (section 3): compiler threads pause at their next safepoint flag check, the invalidator traverses quiescent lists, and compilers resume. This path is expected to be rare; its existence removes reliance on unbounded live-lock behavior.

#### 2.2 Inline Cache (IC) Stub Array (Atomic Pointers)
ICs are shared across different compiled versions of a method.
- **Structure**: The IC Stub Array holds atomic pointers to the current fast-path stub.
- **Update Protocol**: When a mutator thread experiences an IC miss, it constructs a new stub in the Stub Pool. It then publishes the new stub using an atomic store (`atomic_store_explicit(..., memory_order_release)`). 
- **Read Protocol**: Compiled code loads the stub pointer using `atomic_load_explicit(..., memory_order_acquire)`. This guarantees that mutators never see a partially constructed stub, and no locks are held during execution.

#### 2.3 Layout Registry (Sharded Concurrent Hash Table)
The registry interns layout descriptors to produce `CanonicalLayoutHandle`s.
- **Mechanism**: A sharded lock-free hash table. The shard is selected by hashing the layout description. 
- **Collision Resolution**: If two threads attempt to intern the same layout simultaneously, one wins the atomic CAS (Compare-And-Swap) on the shard bucket. The loser discards its duplicate descriptor and uses the winner's handle.

#### 2.4 Code Cache Global Allocator (TLABs + Lock-Free Stack)
- **Thread-Local Allocation Buffers (TLABs)**: Compiler threads allocate code bytes locally within their TLABs without any synchronization.
- **Global Free-List**: When a TLAB is exhausted, the compiler thread requests a new large chunk (e.g., 256KB) from the global free-list. The free-list is implemented as a lock-free Treiber stack. Push and pop operations use atomic CAS, ensuring sub-microsecond allocation latency.

### 3. Thread-Local Handshake & Safepoints
Compiler threads cannot be ignored by the GC. If a compiler thread is allocating GC-managed metadata (like IC stubs), it must respect safepoints.
- **Handshake Mechanism**: We use a thread-local flag (e.g., `ThreadLocalHandshake::requested`). The GC sets this flag globally. Compiler threads check this flag at safe points (e.g., between compiling methods, or after allocating large metadata blocks).
- **Action**: If requested, the compiler thread pauses its work, flushes its local allocation buffers, and waits at a safepoint barrier until the GC finishes its phase.

### 4. CONC-1 Invariants
| ID | Invariant |
|---|---|
| **CONC-INV-01** | **No Global Mutexes on Hot Paths**: Neither mutator execution nor compiler IR traversal may acquire a global mutex. |
| **CONC-INV-02** | **Atomic IC Publication**: IC stub pointers must be updated and read using atomic operations with appropriate memory ordering (release/acquire). |
| **CONC-INV-03** | **Epoch-Based Reclamation**: Memory retired from lock-free structures (DependencyMap, ICs) must only be freed after a full GC epoch/safepoint has passed. |
| **CONC-INV-04** | **Compiler Safepoint Compliance**: Compiler threads must respond to thread-local handshake requests and pause at safepoints when allocating GC-managed metadata. |
| **CONC-INV-05** | **Lock-Free Code Allocation**: Code Pool TLAB exhaustion must be resolved via a lock-free global free-list, not a mutex. |
| **CONC-INV-06** | **Invalidation Completeness**: A shape transition must not complete its invalidation step until one dependency traversal has finished with an unchanged per-layout sequence counter. A dependency installed before the transition started must either be visible to that traversal or force a retry. |
| **CONC-INV-07** | **Stale Install Rejection**: A dependency publication that observes an advanced layout generation must be discarded by the compiler, never installed. |

---

## DEBUG-1: Debugging & Profiling Tooling Interface (DWARF)
**Version 1.0 · Core Execution Contract**

### 1. Fundamental Design Philosophy
With Adaptive Representation Selection (ARS), Region-Based Deoptimization, and Stencil T1, the physical state of the program diverges wildly from the source-level semantics. The JIT must dynamically generate and expose **DWARF 5** debug information and **JIT profiling markers** so that debuggers (GDB/LLDB) and profilers (perf, VTune) can function correctly without knowing the JIT's internal architecture.

### 2. DWARF Generation Strategy
DWARF is not written to disk; it is generated in memory and exposed via the GDB JIT Interface (`__jit_debug_descriptor`).

#### 2.1 T0 (Interpreter)
Standard, static DWARF is emitted for the interpreter loop. Virtual registers are mapped to the VM stack frame.

#### 2.2 T1 (Stencil Baseline)
Because T1 uses a fixed physical map (or simple location allocation), the mapping from VM registers to physical registers/stack slots is deterministic based on the stencil selected.
- **Pre-computed Fragments**: At VM startup, we pre-compile DWARF `.debug_info` and `.debug_line` fragments for every stencil variant.
- **Stitching**: When T1 emits code, it copies the corresponding DWARF fragments into the in-memory debug section, patching the PC ranges to match the generated code.

#### 2.3 T2/T3 (Optimizing Tiers)
The compiler generates DWARF dynamically during MIR emission.
- **DWARF 5 Location Lists**: This is mandatory for ARS. A variable's physical location and representation change over its live range. 
  - *Example*: Variable `v` is in `RAX` (I64) from PC `0x100-0x110`, then spilled to `[RSP+16]` (TAGGED) from `0x110-0x120`. The Location List explicitly maps these PC ranges to the physical locations and representations.
- **DWARF Expressions**: For complex ARS transitions (e.g., a value that must be unboxed on the fly for the debugger), we emit DWARF expressions (e.g., `DW_OP_breg0`, `DW_OP_lit16`, `DW_OP_shr`, `DW_OP_stack_value`) that instruct the debugger how to reconstruct the source-level value from the physical state.

### 3. Region-Based Deopt & DWARF
When a Region's entry is patched with a `JMP deopt_stub`, the original physical bytes remain in the Code Pool.
- **Original Code**: The DWARF for the original bytes remains perfectly valid. If a debugger sets a breakpoint inside the patched region, it will hit the `JMP`, deopt, and resume in T0.
- **Deopt Stubs**: Deopt stubs are marked in DWARF as compiler-generated (no source line mapping). If the debugger steps into a deopt stub, it sees the runtime state reconstruction logic.

### 4. Profiler Integration (perf / VTune)
Profilers need to map physical PCs to method names and source lines.
- **perf (Linux)**: The JIT maintains a `/tmp/perf-<pid>.map` file. Every time a `CodeBlob` is published, the JIT appends a line: `<start_addr> <size> <method_name>`.
- **VTune (Intel)**: The JIT uses the Intel JIT Profiling API (`iJIT_NotifyEvent`) to register the method name, start address, size, and line number table upon publication.
- **Invariant**: A profiler must never see an "unknown" PC for JIT-compiled code. All executable bytes must be registered.

### 5. DEBUG-1 Invariants
| ID | Invariant |
|---|---|
| **DEBUG-INV-01** | **Complete PC Coverage**: Every physical byte of executable code must have a valid DWARF mapping to a source location, or be explicitly marked as compiler-generated (e.g., deopt stubs, trampolines). |
| **DEBUG-INV-02** | **ARS Location Accuracy**: DWARF Location Lists must accurately reflect the physical register/stack location and representation (TAGGED, I64, F64, REF_OFF) of every variable at every safepoint. |
| **DEBUG-INV-03** | **Dynamic Registration**: All published CodeBlobs must be immediately registered with OS-level profiler interfaces (perf map, VTune API, GDB JIT descriptor). |
| **DEBUG-INV-04** | **Deopt Transparency**: Breakpoints set in optimized code must safely trigger deoptimization and resume execution in T0 without corrupting the debugger's state. |

---

## SIMD-1: Vectorization Strategy for Tagged Values
**Version 1.1 · Retcon of SIMD-INV-01 and homogeneity-gated Strategy B**

### 1. Fundamental Design Philosophy
NaN-boxing is hostile to SIMD **for mixed or integer-tagged data**. The 1.0 philosophy ("we do not attempt to SIMD process tagged values directly") was too absolute, and it is retconned here (finding SIMD-RETCON, resolved; see also NUM-1).

The decisive fact: NV-1 floating-point values are stored as **native IEEE-754 binary64 bits**. A float is not boxed in any sense that SIMD arithmetic can observe; its tag is its value. Consequently, homogeneous tagged-float SIMD arithmetic is bit-exact with unboxed SIMD arithmetic, subject only to reserved-space collision canonicalization (section 4). Direct SIMD math on **homogeneous tagged-float lanes** is therefore legal. Direct SIMD math on **integer lanes** is legal only through masked payload arithmetic with explicit overflow fallback (section 3.2), because the 0xFFFC tag occupies bits that participate in wrap-around. Direct SIMD math on **mixed or unknown** lanes remains prohibited.

### 2. The NV-1 Float Advantage
Recall from NV-1 that floating-point values are stored as **direct binary64 representations**. The "tag" for a float is simply its native IEEE-754 bits. The reserved tag space (`0xFFFC...`) is only used for integers, references, and special values.
- **Crucial Insight**: If an array contains *only* floats, a SIMD load of 4x F64 (or 2x F64 on RV64) works natively. The bits are already in the correct format for the FPU. No unboxing is required for the float bits themselves!

### 3. Vectorization Strategies

#### 3.1 Strategy A: Structural Array Specialization (Primary Path)
Driven by AOSS and the Profiler (PROF-1), the compiler proves an array is homogeneous.
- **Mechanism**: The array's `CanonicalLayoutHandle` transitions from `Array<TAGGED>` to `Array<F64>` or `Array<I64>`.
- **Physical Layout**: Elements are stored natively (8 bytes each, no tags). 
- **Vectorization**: Standard Auto-Vectorization and SLP apply. The compiler emits `vmovupd` (x86) or `ldr q0` (AArch64) to load 4x/2x elements, performs SIMD math, and stores them back.
- **Profitability**: This is the fastest path. It yields native C/C++ SIMD performance.

#### 3.2 Strategy B: Direct Tagged SIMD (Retconned: Floats Free, Integers Masked)

If the array remains `Array<TAGGED>`, SIMD is still available, with different rules per element kind.

**B-F: Float lanes (zero-cost vectorization).** Because tagged floats are raw binary64 bits, a PACKED_TAGGED load (vmovupd of 4x64 into YMM, or 8x64 into ZMM) followed directly by vaddpd / vmulpd / vfmadd computes correct results for every non-colliding lane. The upper 16 bits of a valid float can never equal a reserved tag: an IEEE-754 value with an all-ones exponent is a NaN, and NV-1 canonicalizes colliding NaN patterns at the value boundary (NUM-1 section 1). After the math, if any lane's result bit pattern falls in the reserved tag region 0xFFFC-0xFFFF, a single masked blend replaces those lanes with the canonical NaN:

```asm
; YMM0, YMM1 = 4x tagged-f64 operands
vaddpd      YMM2, YMM0, YMM1     ; Direct math on tagged float bits
; Collision canonicalization (elidable when provably collision-free, section 4):
vpsrld      YMM3, YMM2, 48       ; per-lane top 16 bits, replicated in dword view
vpcmpud     k1, YMM3, [tag_hi_bounds], 5   ; k1 = lanes whose top16 is in 0xFFFC..0xFFFF
vmovdqu32   YMM2{k1}, [canonical_nan_broadcast] ; blend canonical NaN into colliding lanes
```
(AArch64 uses cmhi + bsl; RV64 vector uses vmsltu + vmerge. The canonicalization is one compare and one masked blend.)

**B-I: Integer lanes (AVX-512 masked payload arithmetic).** Tagged integers are 0xFFFC + signed 48-bit payload. 8x64-bit tagged integers fit one ZMM register. The sequence is:

1. **Load**: vmovupd 8x tagged words into ZMM0.
2. **Extract**: vpandq ZMM1, ZMM0, [payload_mask] isolates the 48-bit payloads.
3. **Compute**: vpaddq / vpsubq / vpmullq on the payloads.
4. **Overflow check**: vpcmpq against the sign-extension pattern of bit 47 produces a mask of overflowed lanes; kortestw branches to the scalar boxing fallback for the whole vector if any lane overflowed.
5. **Re-tag**: vporq ZMM0, ZMM1, [tag_0xFFFC_broadcast] re-applies 0xFFFC to all 8 lanes simultaneously.

Cost: for add/subtract, the extract and re-tag are two extra 64-bit lane ops per vector plus one fused compare-branch. This is profitable only under the gating rules of section 3.4.

**B-X: Mixed or unknown lanes are prohibited.** If profiling cannot establish homogeneity (section 3.4), Strategy B is not applied. There is no mixed-lane tagged SIMD.

#### 3.2.1 PACKED_TAGGED as a First-Class Representation

To let the register allocator keep tagged vectors in SIMD registers across loop iterations without scalar unboxing, PACKED_TAGGED is added as a first-class register representation in LLIR/MIR, alongside TAGGED, I64, F64, REF_OFF, REF_RAW:

| Property | Value |
|---|---|
| Width | 128/256/512 bits (2/4/8 NV-1 lanes) |
| Register class | Vector (XMM/YMM/ZMM) |
| Lane domain | Homogeneous F64 or homogeneous I64 payloads; never mixed |
| GC interaction | No references may live in a PACKED_TAGGED value; safepoint maps describe it as non-rooted |
| Spill | Full vector spill/reload; no lane splitting |
| Legal ops | Lane-wise FP/int arithmetic, blends, shuffles, lane extract/insert |
| Illegal ops | Anything requiring per-lane tag interpretation (e.g. LOAD_FIELD on a lane) |

ARS may promote TAGGED scalars into PACKED_TAGGED lanes when the SLP pass (section 3.3) proves lane homogeneity; demotion back to scalar TAGGED at observable boundaries applies the same canonicalization rules as scalar boxing.

#### 3.3 Strategy C: Superword Level Parallelism (SLP)
SLP packs independent scalar operations within a single basic block into SIMD instructions.
- **Mechanism**: If ARS has unboxed scalars into FPRs/GPRs (e.g., `a = x + y; b = z + w;`), the MIR instruction scheduler and SLP pass pack `x, y, z, w` into a SIMD register, perform the adds, and unpack.
- **Constraint**: SLP only operates on values that ARS has already committed to unboxed representations (I64, F64). It does not operate on TAGGED values.

#### 3.4 Strategy B Profitability Gating (Required)

Strategy B's guard and mask overhead is only amortized by real vector work. Ungated, it is slower than scalar execution on short loops (finding SIMD-1, resolved). Strategy B on Array<TAGGED> may be selected only when all of the following hold:

1. **Trip count**: the loop's profiled iteration count (PROF-1 section 2, Memory/Vector domain) exceeds SIMD_TAGGED_MIN_TRIP (default 64), or the trip count is statically provable to exceed it.
2. **Homogeneity**: the element profile for the accessed array proves at least 99.99% homogeneous lane domain (float or integer) across the profile window, with zero observed foreign-tag stores in the current window.
3. **Body width**: the loop body contains at least two vectorizable lane operations, so guard overhead amortizes.
4. **No interior serialization**: the loop must not contain per-element layout guards that would serialize the vector.

Otherwise the compiler falls back to Strategy A (if AOSS can transition the layout) or to scalar unboxed loops (ARS). The gating decision and its inputs are recorded in region metadata so deopt reasoning can cite them.

Strategy A (structural Array<F64>/Array<I64> specialization, NUM-1 section 3) has no such gate beyond standard vectorization profitability, because it carries no per-access tag cost.

### 4. NaN Canonicalization in SIMD
When SIMD floating-point math produces a NaN, the hardware may generate a NaN payload that collides with our reserved NV-1 tag space.
- **Normalization Step**: Before storing a SIMD float result back to a `TAGGED` context (or an `Array<TAGGED>`), the compiler must emit a normalization sequence.
- **x86-64 Implementation**:
  ```asm
  ; YMM0 contains 4x F64 results
  vpcmpeqd YMM1, YMM0, [reserved_tag_mask] ; Check for reserved NaN patterns
  vptest   YMM1, YMM1                      ; Test if any lane collided
  jz       .no_collision                    ; If zero, no collision, skip
  ; Slow path: blend with canonical NaN 0x7FF8000000000000
  vblendvpd YMM0, YMM0, [canonical_nan], YMM1
  .no_collision:
  vmovupd  [mem], YMM0
  ```
- **Optimization**: If the result is stored directly into an `Array<F64>` (Strategy A), and the array is never read as `TAGGED`, the normalization can be deferred or elided entirely, relying on the VM's deferred normalization rules.

### 5. SIMD-1 Invariants
| ID | Invariant |
|---|---|
| **SIMD-INV-01** | **No Direct Tagged SIMD Math on Mixed or Unknown Lanes** (retconned): SIMD arithmetic must not operate on lanes of mixed or unproven tag domain. Homogeneous tagged-float SIMD math is legal (lanes are raw binary64 bits), subject to collision canonicalization. Tagged-integer SIMD math is legal only via masked payload arithmetic with explicit 48-bit overflow fallback. |
| **SIMD-INV-02** | **Integer Overflow Fallback**: Tagged-integer vector math must detect any 48-bit payload overflow per vector and fall back to scalar boxing for the whole vector. Silent wrap-through of the tag bits is a correctness violation. |
| **SIMD-INV-03** | **NaN Canonicalization**: SIMD floating-point results that cross into a `TAGGED` observable boundary must be normalized to the VM canonical NaN (`0x7FF8000000000000`) if they collide with the reserved tag space. |
| **SIMD-INV-04** | **Guard Deoptimization**: Guarded SIMD paths on `Array<TAGGED>` must deoptimize to scalar code if the SIMD type-check guard fails. They must not silently produce incorrect results. |
| **SIMD-INV-05** | **Profitability Gating**: Strategy B on tagged arrays requires the section 3.4 gate (trip count, homogeneity, body width). Ungated tagged-array vectorization is a specification violation. |
| **SIMD-INV-06** | **PACKED_TAGGED Purity**: A PACKED_TAGGED value must never contain heap references. Safepoint maps must classify it as non-rooted. |
| **SIMD-INV-07** | **Canonicalization Elision Proof**: Eliding the NaN collision canonicalization requires a compiler proof that no lane can produce a reserved-space bit pattern; the proof is recorded in region metadata. |


Defining the exception handling, inline cache evolution, memory model mapping, and barrier fast-paths completes the physical execution and runtime integration contract. These subsystems are where the theoretical elegance of the IR meets the harsh realities of OS unwinders, megamorphic workloads, hardware memory models, and GC concurrency.

Here is the formal, exhaustive specification for **EXC-1**, **IC-1**, **MEM-1**, and **BARRIER-1**.

---

## EXC-1: Zero-Cost Exception Handling & Unwind Tables
**Version 1.0 · Core Execution Contract**

### 1. Fundamental Design Philosophy
Exceptions are rare; the happy path must have **absolute zero overhead**. We do not use setjmp/longjmp or explicit exception state threading in the IR. Instead, we rely entirely on **table-driven unwinding** provided by the OS, using dynamically generated metadata that maps physical PCs to catch handlers and deoptimization targets.

### 2. Unwind Table Generation & Registration
The JIT must generate standard OS unwind metadata so that when a native exception (or a JIT-thrown exception) unwinds through JIT code, the OS can correctly restore the stack pointer and frame pointer.

#### 2.1 Target-Specific Unwind Formats
- **Linux/ELF (x86-64, AArch64, RV64)**: Generate **DWARF Call Frame Information (CFI)** (`.eh_frame`).
- **Windows/PE (x86-64)**: Generate **`.pdata` and `.xdata`** (RUNTIME_FUNCTION and UNWIND_INFO structures).

#### 2.2 Tier-Specific Unwind Strategies
- **T0 (Interpreter)**: The interpreter loop has a single, static, pre-compiled unwind descriptor registered at VM startup.
- **T1 (Stencil Baseline)**: Because T1 enforces a strict, uniform frame layout (the Fixed Physical Map), **all T1 code shares a single, static unwind descriptor**. When a T1 CodeBlob is published, we do not generate new unwind info; we just register the existing T1 descriptor for the new code range.
- **T2/T3 (Optimizing Tiers)**: The MIR emitter dynamically generates DWARF CFI / `.pdata` records alongside the machine code. 
  - *Prologue*: Emits CFI directives for stack allocation and callee-saved register pushes.
  - *Safepoints/Deopts*: Emits CFI directives to reconstruct the virtual frame if unwinding stops at a safepoint.

#### 2.3 Dynamic Registration Protocol
When a CodeBlob is published to the Code Pool (RX view):
1. The JIT allocates the unwind metadata in the Metadata Pool (RW).
2. The JIT calls the OS-specific registration API:
   - Linux: `__register_frame(unwind_info_ptr)`
   - Windows: `RtlAddFunctionTable(pdata_ptr, count, base_addr)`
3. When the CodeBlob is reclaimed by the Sweeper, the JIT calls the corresponding unregistration API.

### 3. Exception Dispatch & The Exception Table
When a JIT-compiled method throws an exception, it does not call a generic runtime function. It jumps to a **Throw Stub**.

#### 3.1 The Throw Stub
1. The Throw Stub captures the current physical PC and stack pointer.
2. It queries the **Exception Table** in the current `CodeBlobHeader`.
3. The Exception Table is a sorted array of `(PC_Start, PC_End, Handler_Offset, Deopt_ID)` tuples.
4. The runtime performs a binary search to find the innermost enclosing `try-catch` or `try-finally` block.
5. If a handler is found, the stack is unwound to that frame, and execution jumps to the handler.
6. If no handler is found in the current CodeBlob, the OS unwinder takes over, using the DWARF/`.pdata` metadata to walk up the stack to the caller.

#### 3.2 Interaction with Region-Based Deoptimization
If an exception is thrown, and the PC falls inside a **Deopt Stub** (because the region was invalidated):
- The Deopt Stub is marked in the Exception Table with a special `Deopt_ID`.
- The runtime recognizes this, reconstructs the T0 interpreter frame, and **transfers the pending exception into the T0 exception state**.
- T0 then resumes execution and handles the exception using the interpreter's standard exception dispatch.

### 4. EXC-1 Invariants
| ID | Invariant |
|---|---|
| **EXC-INV-01** | **Zero Happy-Path Cost**: No instructions are executed on the happy path to maintain exception state. All mapping is done via out-of-band tables. |
| **EXC-INV-02** | **Complete PC Coverage**: Every executable byte in the Code Pool must be covered by a valid unwind descriptor. |
| **EXC-INV-03** | **Atomic Registration**: Unwind tables must be registered with the OS *before* the CodeBlob's RX pointer is published to mutators. |
| **EXC-INV-04** | **Deopt-Exception Safety**: If unwinding hits a deopt stub, the runtime must correctly reconstruct the T0 frame and preserve the exception object. |

---

## IC-1: Megamorphic Fallback & Inline Cache Evolution
**Version 1.0 · Core Execution Contract**

### 1. Fundamental Design Philosophy
Inline Caches (ICs) must evolve gracefully from monomorphic to megamorphic without causing unbounded memory growth, infinite deopt storms, or catastrophic performance cliffs. The IC state machine is strictly bounded, and megamorphic execution relies on optimized generic stubs, not unbounded polymorphic switches.

### 2. The IC State Machine
Every property access and call site maintains a state machine driven by the Profiler (PROF-1).

| State | Condition | T0/T1 Execution | T2/T3 Compilation Strategy |
|---|---|---|---|
| **Uninitialized** | Never executed | Generic runtime lookup | N/A |
| **Monomorphic** | 1 unique LayoutID observed | 1-tap IC check | Emit single `GuardLayout` + direct offset load. |
| **Polymorphic** | 2 to $N$ LayoutIDs (e.g., $N=4$) | Linear scan of $N$ entries | Emit a switch statement of $N$ `GuardLayout` checks. |
| **Megamorphic** | $> N$ LayoutIDs observed | Hash-table or generic lookup | **Stop specializing.** Emit a "Generic Region" using dictionary lookups. |

### 3. Megamorphic Execution Strategy
When a site becomes megamorphic, T3 **must not** compile a 50-way switch statement. Instead, it compiles a **Generic Region**.

#### 3.1 The Generic Region
- The Generic Region abandons AOSS shape specialization for this specific site.
- It emits a call to a highly optimized, JIT-compiled **Megamorphic Stub**.
- The Megamorphic Stub uses a fast, lock-free hash table (keyed by LayoutID) to resolve the field offset or call target.
- *Alternative*: If the language semantics permit, the Generic Region can use a "dictionary object" layout where properties are stored in a hash map attached to the object, and the JIT emits direct hash-table lookup instructions.

#### 3.2 Preventing Deopt Storms
If a site is marked Megamorphic, the AOSS DependencyMap **stops tracking** shape transitions for that specific site. 
- If the object's shape changes, the Generic Region does not deopt. It simply performs the dictionary lookup and continues.
- This breaks the "compile $\rightarrow$ deopt $\rightarrow$ compile" loop.

### 4. IC Memory Management
IC state is not embedded in the executable code (which would bloat the Code Pool and complicate patching). It is stored in the **IC Side-Table** in the Metadata Pool.
- The compiled code contains a `LOAD_IC_STUB(index)` instruction.
- The IC Side-Table holds the current state and the pointer to the active fast-path stub.
- When a CodeBlob is reclaimed, its associated IC Side-Table entries are freed.

### 5. IC-1 Invariants
| ID | Invariant |
|---|---|
| **IC-INV-01** | **Bounded Polymorphism**: The compiler will never emit a polymorphic guard chain longer than $N$ entries. It must transition to Megamorphic execution. |
| **IC-INV-02** | **Megamorphic Stability**: Once a site is marked Megamorphic, it cannot be reverted to Monomorphic/Polymorphic, and it does not trigger deoptimization on shape changes. |
| **IC-INV-03** | **Side-Table Separation**: IC state must reside in the RW Metadata Pool, not the RX Code Pool. |
| **IC-INV-04** | **Atomic IC Updates**: Updates to the IC Side-Table must be atomic with respect to mutator threads. |

---

## MEM-1: Hardware Memory Model Mapping
**Version 1.0 · Core Execution Contract**

### 1. Fundamental Design Philosophy
UGB defines the source-level memory model (assumed to be at least as strong as C11/Java: acquire/release semantics for synchronization, sequential consistency for volatile). The JIT must map these abstract semantics to hardware-specific fences (x86-TSO, ARM-Weak, RVWMO) without over-synchronizing, while strictly preventing the optimizer from reordering operations in violation of the model.

### 2. IR Representation of Memory Order
In VSDG and SoN, memory operations and fences carry an explicit `MemoryOrder` tag:
- **Relaxed**: No ordering guarantees.
- **Acquire**: Subsequent loads/stores cannot be reordered before this operation.
- **Release**: Preceding loads/stores cannot be reordered after this operation.
- **AcqRel**: Both Acquire and Release.
- **SeqCst**: Acts as Release + Acquire, and prevents all Store-Load reordering globally.

### 3. Optimization Rules (SoN / LLIR)
The optimizer must respect the following hard constraints when reordering memory operations:
1. **Acquire Load**: Cannot be moved *up* (before) any preceding memory operation. Can be moved *down* past other loads, but not past stores.
2. **Release Store**: Cannot be moved *down* (after) any succeeding memory operation. Can be moved *up* past other stores, but not past loads.
3. **SeqCst**: Cannot be reordered with *any* other memory operation across the fence.
4. **Relaxed**: Can be freely reordered by the optimizer, subject only to data dependencies.

### 4. Target-Specific Lowering (MIR)
The MIR instruction selector maps the abstract `MemoryOrder` to hardware instructions.

#### 4.1 x86-64 (TSO - Total Store Order)
Hardware already guarantees Acquire/Release semantics for normal loads/stores.
- **Relaxed / Acquire / Release**: Normal `MOV` instructions. (Zero overhead).
- **SeqCst**: Requires a `MFENCE` instruction, OR using a `LOCK` prefix on the store (e.g., `LOCK XCHG`).
- **Atomic RMW**: `LOCK XADD`, `LOCK CMPXCHG` inherently provide SeqCst.

#### 4.2 AArch64 (Weak Memory Model)
Hardware requires explicit instructions for ordering.
- **Relaxed**: `LDR` / `STR`
- **Acquire**: `LDAR` (Load-Acquire)
- **Release**: `STLR` (Store-Release)
- **AcqRel**: `LDAR` / `STLR`
- **SeqCst**: `LDR` / `STR` followed by `DMB ISH` (Data Memory Barrier, Inner Shareable).

#### 4.3 RV64 (RVWMO)
Hardware uses bits in the instruction encoding or explicit fences.
- **Relaxed**: `LW` / `SW`
- **Acquire**: `LW` with `.aq` bit set.
- **Release**: `SW` with `.rl` bit set.
- **SeqCst**: `FENCE RW, RW` instruction.

### 5. MEM-1 Invariants
| ID | Invariant |
|---|---|
| **MEM-INV-01** | **Semantic Preservation**: The optimizer must never reorder an Acquire load before a preceding store, or a Release store after a succeeding load. |
| **MEM-INV-02** | **Target Correctness**: The MIR emitter must emit the exact hardware instructions required to satisfy the `MemoryOrder` tag for the target ISA. |
| **MEM-INV-03** | **No Redundant Fences**: The optimizer must eliminate redundant fences (e.g., an Acquire load immediately followed by a SeqCst fence). |

---

## BARRIER-1 (Revised): Fully Concurrent Load Barriers
**Version 1.2 · Correcting the Fully Concurrent GC Contract**
**1.2 change: the fast path now verifies the reference tag before testing GC color bits. The 1.1 fast path false-triggered on tagged integers (finding BARRIER-1, resolved). See §2.0.**

### 1. The Mathematical Trick: 3 Free Bits from Alignment
Recall from **HEAP-1** that all heap entities are strictly **8-byte aligned**. 
Because the NV-1 reference payload is a 48-bit byte offset, and the offset is always a multiple of 8, the **lower 3 bits of the payload are always `000`**.

We can hijack these 3 zero-bits to store **Concurrent GC Colors** directly inside the NV-1 reference itself. This gives us a ZGC-style colored pointer mechanism without expanding the 64-bit word or shrinking the 256 TiB heap (since $2^{45} \text{ offsets} \times 8 \text{ bytes} = 256 \text{ TiB}$).

#### 1.1 The 3-Bit Color Allocation
| Bit | Name | Meaning |
|-----|------|---------|
| 0 | `Marked0` | Part of concurrent marking cycle 0 |
| 1 | `Marked1` | Part of concurrent marking cycle 1 |
| 2 | `Remapped` | Object has been relocated; offset needs resolution |

When the GC concurrently moves an object, it updates the object's header with a forwarding offset and changes the color bits in the heap. When a mutator loads a reference, it checks these bits.

### 2. The Load Barrier Fast-Path
Because the colors are embedded in the lower 3 bits of the NV-1 word, the Load Barrier is incredibly cheap. It requires exactly **one `TEST` instruction and one conditional branch**.

#### 2.0 Tag Precedence: The Barrier Classifies Before It Tests (Correctness Correction)

The 1.1 fast path executed `test al, 0x07` on the loaded 64-bit word without first verifying that the word is a heap reference. That is operationally broken, and the flaw is corrected here (finding BARRIER-1, resolved).

NV-1 encodes immediate integers as `0xFFFC` in bits 63-48 with a signed 48-bit payload in bits 47-0. The integer `5` is the word `0xFFFC000000000005`. Its lower three bits are `101`, so `test al, 0x07` sets ZF=0 and branches to the slow path. Every odd integer, and many even integers whose payload low bits are set, false-triggers the barrier. The CPU does not know that the value is "supposed to be a reference"; it only sees bits. On integer-heavy workloads this is a systematic slow-path storm, not a statistical accident.

The corrected fast path performs tag classification before the color test. A word is color-tested only when its 16-bit tag equals `0xFFFD` (heap reference). Integers, special immediates, and floating-point values never reach the color test because their tags differ. The slow path is therefore entered only when a genuine reference carries an unresolved GC color.

#### 2.1 x86-64 Assembly (The Fast Path)
```asm
; RAX contains the loaded 64-bit NV-1 word from memory.
; Bits 63-48: tag. For references the tag is 0xFFFD and bits 2-0 carry GC colors.

; 1. Classify: is this word a heap reference at all?
mov r8, rax
shr r8, 48                  ; Extract the 16-bit tag
cmp r8, 0xFFFD              ; Heap reference?
jne .fast_path_clean        ; No (int, float, special): no GC color exists. Fast exit.

; 2. It is a reference. Now, and only now, test the color bits.
test al, 0x07               ; Test the 3 GC color bits
jnz .load_barrier_slow      ; Any color set: resolve via slow path

; 3. FAST PATH: clean reference. The lower 3 bits are 0, so RAX holds the
; pure (0xFFFD << 48) | 48-bit offset. Reconstruct and load.
and rax, 0x0000FFFFFFFFFFFF ; Clear the tag, keep the offset
lea rbx, [heap_base + rax]  ; Reconstruct the native address
mov rcx, [rbx + field_offset]
```
**Cost:** 1 `MOV`, 1 `SHR`, 1 `CMP`, 1 `JNE`, then 1 `TEST` and 1 `JNZ` on the reference path only. The non-reference exit is a highly predictable not-taken branch for reference-typed loads, and the color test remains exactly as cheap as in 1.1 for genuine references. The added shift-and-compare pair occupies 2-3 cycles on the reference fast path and overlaps with the load latency. This is the correct price for eliminating a slow-path storm on every tagged non-reference word.

#### 2.2 The Load Barrier Slow-Path
If the `Remapped` bit (or marking bits) is set, the mutator must resolve the true location of the object.

```asm
.load_barrier_slow:
; 1. Extract the 48-bit offset (mask out the tag and the color bits)
mov r8, rax
and r8, 0x0000FFFFFFF8   ; Clear lower 3 bits (colors) and upper 16 bits (tag)

; 2. Reconstruct the native pointer to the object header
lea r9, [heap_base + r8]

; 3. Read the object header to find the forwarding offset or true state
mov r10, [r9]            ; Load the 128-bit header (first 64 bits)

; 4. Call the runtime Load Barrier Resolver
; The resolver checks the header, finds the new offset, updates the 
; mutator's local register/stack slot with the clean offset, and 
; returns the corrected NV-1 reference in RAX.
call rt_load_barrier_resolve
jmp .resume_fast_path
```

### 3. Interaction with NV-1 and ARS (Adaptive Representation Selection)
How does this affect the compiler's unboxed live ranges?

1. **Tagged References (`TAGGED`)**: The Load Barrier operates directly on the 64-bit NV-1 word. The 3 color bits are checked, and if clean, the value is used as-is.
2. **Unboxed Offsets (`REF_OFF`)**: If ARS decides to keep a reference as an unboxed 48-bit offset in a GPR to avoid repeated tag extraction, the Load Barrier still applies. The compiler simply emits the `TEST` on the raw offset register.
3. **Raw Pointers (`REF_RAW`)**: If ARS reconstructs a raw native pointer (`heap_base + offset`), the Load Barrier **must** execute *before* the pointer is reconstructed. You cannot safely use a raw pointer if the object is concurrently moving. 
   - **INV-BARRIER-NEW**: `REF_RAW` values must never be materialized until the Load Barrier has verified the `REF_OFF` is clean.

### 4. GC Write Barrier Synergy (SATB / Generational)
The Load Barrier handles *reads* of moving objects. The Write Barrier (defined previously) handles *writes* for the concurrent marker and generational remembered sets.
- **Concurrent Marking**: If the GC is doing concurrent marking (e.g., SATB or COLORED), the Load Barrier ensures the mutator never reads a stale object. If the mutator *writes* a reference, the Write Barrier logs the old value or marks the card.
- **No Read-Barrier for Non-Moving**: If the engine ever runs in a mode where the GC is *not* moving (e.g., a non-moving concurrent marker), the `Remapped` bit is never set, and the Load Barrier fast-path remains exactly the same (just checking marking bits if required, or elided entirely if the GC design permits).

### 5. Updated BARRIER-1 Invariants

| ID | Invariant |
|---|---|
| **BARRIER-INV-01** | **Fast-Path Purity (Write)**: The write barrier fast-path must not execute memory writes unless the slow-path is taken. |
| **BARRIER-INV-02** | **Load Barrier Presence**: Because the GC is fully concurrent and moving, every heap reference load must pass through the Load Barrier fast-path (checking the 3 alignment bits). |
| **BARRIER-INV-03** | **Alignment Guarantee**: The Load Barrier relies on the invariant that all heap objects are 8-byte aligned, guaranteeing the lower 3 bits of the offset are available for GC colors. |
| **BARRIER-INV-04** | **No Raw Pointers Across Barriers**: A `REF_RAW` (native pointer) must never be materialized or used for memory access until the Load Barrier has verified the underlying `REF_OFF` is clean (no `Remapped` bit set). |
| **BARRIER-INV-05** | **Elision Safety**: The Load Barrier can only be elided by the compiler if it mathematically proves the object is thread-local, newly allocated in the current TLAB, and cannot possibly be seen or moved by the concurrent GC. |
| **BARRIER-INV-06** | **Tag Precedence**: The color test (`test al, 0x07`) may only execute on words whose 16-bit tag equals `0xFFFD`. A non-reference word must exit the barrier on the fast path without a color test. A barrier implementation that color-tests arbitrary NV-1 words is incorrect. |
| **BARRIER-INV-07** | **Slow-Path Classification**: The load barrier slow path is entered only for words already classified as references (invariant 06). Resolution logic may assume a genuine heap reference. |
| **BARRIER-INV-08** | **Numeric Array Load Elision**: Loads from `Layout_NumericArray_F64` or `Layout_NumericArray_I64` payloads (NUM-1) contain no references and take no load barrier. The layout check that selects the raw-load path substitutes for the barrier. |
---

## SNAP-1: Native Image & Snapshot Specification
**Version 1.0 · Core Execution Contract**

### 1. Fundamental Design Philosophy
A Native Image is a complete, self-contained executable that includes:
- Pre-compiled machine code (all tiers)
- A serialized heap snapshot (pre-allocated objects)
- All metadata (AOSS dependencies, IC stubs, layout registry, profiling data)
- Debug and profiling information (DWARF, perf maps)

The goal is **near-zero startup**: the application begins execution with all hot code already compiled, all inline caches already populated, and all objects already allocated. The only remaining work is runtime initialization (thread-local state, GC setup, assumption validation).

**Key Insight**: Our NV-1 48-bit heap-relative offsets are already position-independent. This eliminates the biggest pain point of traditional native images (pointer relocation). We only need to set `heap_base` correctly on startup, and all references in the snapshot are automatically valid.

---

### 2. Snapshot Composition
A snapshot file (`.snap`) contains five distinct sections:

| Section | Contents | Position-Independent? |
|---------|----------|----------------------|
| **Code Image** | All CodeBlobs (T1/T2/T3/T4), stencils, deopt stubs, unwind tables | Partially (requires relocation for absolute addresses) |
| **Heap Image** | Serialized heap objects (classes, prototypes, constants, closures, strings) | Yes (uses NV-1 48-bit offsets) |
| **Metadata Image** | AOSS dependency maps, IC stub arrays, layout registry, type profiles | Partially (contains pointers to code/heap) |
| **Debug Image** | DWARF info, perf maps, source line tables | Yes (uses relative offsets) |
| **Header** | Version, checksum, entry point, relocation table, assumption list | N/A |

#### 2.1 Code Image
The code image contains all compiled methods, organized by tier:
- **T1 Stencils**: Pre-assembled stencil templates (shared across all instances).
- **T2/T3 CodeBlobs**: Optimized machine code with embedded region metadata, safepoint maps, and deopt stubs.
- **T4 Trace Code**: Specialized trace code with guard metadata.
- **FFI Trampolines**: Pre-generated trampolines for known foreign function calls.

**Relocation Requirements**:
- Branch targets (relative offsets) are already position-independent.
- Absolute addresses (e.g., `heap_base`, external symbols) require relocation.
- We emit a relocation table that lists all absolute addresses and their fixup types.

#### 2.2 Heap Image
The heap image is a serialized dump of the VM heap at a safepoint:
- **Object Layout**: Each object is serialized as `[Header (128-bit)][Payload...]`.
- **References**: All heap references are NV-1 48-bit offsets, which are already position-independent.
- **Special Objects**: Classes, prototypes, interned strings, and pre-allocated constants are included.

**GC State**:
- The entire snapshot heap is treated as **old generation** on startup (already promoted).
- The mark bitmap is initialized to "all marked" (no need to re-trace).
- The card table is initialized to "clean" (no young-to-old references to scan).
- If the snapshot includes young objects (e.g., pre-allocated for performance), they are explicitly marked in the header.

#### 2.3 Metadata Image
The metadata image contains all runtime metadata:
- **AOSS DependencyMap**: Maps `CanonicalLayoutHandle` to lists of dependent regions.
- **IC Stub Arrays**: Pre-populated inline caches with monomorphic/polymorphic stubs.
- **Layout Registry**: Interned layout descriptors with their canonical handles.
- **Type Profiles**: Pre-collected profiling data (type frequencies, call site counts).

**Relocation Requirements**:
- Pointers to CodeBlobs and heap objects require relocation.
- We use the same relocation table as the code image.

#### 2.4 Debug Image
The debug image contains all debugging and profiling information:
- **DWARF 5**: Complete debug info for all CodeBlobs, including ARS location lists.
- **Perf Maps**: `/tmp/perf-<pid>.map` entries for all methods.
- **VTune JIT API**: Registration data for Intel VTune.

This is already position-independent (uses relative offsets).

#### 2.5 Header
The snapshot header contains:
- **Magic Number**: Identifies the file as a snapshot.
- **Version**: Snapshot format version (for compatibility checking).
- **Checksum**: Integrity verification.
- **Entry Point**: The bytecode PC or method ID to start execution.
- **Relocation Table**: List of absolute addresses to fix up.
- **Assumption List**: List of assumptions that must be validated on startup (e.g., "class X exists", "method Y has signature Z").
- **Heap Base Offset**: The offset of the heap image within the snapshot file.

---

### 3. Serialization Protocol
The snapshot is generated by T5 (or a dedicated snapshot tool) after the application has reached a stable state (e.g., after warm-up).

#### 3.1 Serialization Steps
1. **Reach a Global Safepoint**: Pause all mutator threads.
2. **Freeze the Heap**: No further mutations are allowed.
3. **Serialize the Heap Image**:
   - Traverse all live objects (using the GC's mark bitmap).
   - For each object, serialize the header and payload.
   - Record the object's offset within the heap image.
4. **Serialize the Code Image**:
   - Collect all active CodeBlobs from the code cache.
   - Emit the machine code and metadata.
   - Generate the relocation table.
5. **Serialize the Metadata Image**:
   - Serialize the AOSS dependency map, IC stubs, layout registry, etc.
   - Generate relocations for pointers to code/heap.
6. **Serialize the Debug Image**:
   - Emit DWARF info, perf maps, etc.
7. **Write the Header**:
   - Compute the checksum.
   - Write the entry point, relocation table, assumption list, etc.
8. **Resume Execution**: The application can continue running (the snapshot is now complete).

#### 3.2 Incremental Serialization
For large applications, we support incremental snapshots:
- **Base Snapshot**: Contains the core runtime and common libraries.
- **Delta Snapshot**: Contains only the changes since the base snapshot (new classes, new methods, modified objects).
- On startup, we load the base snapshot, then apply the delta.

This allows fast iteration during development (rebuild only the changed parts).

---

### 4. Startup / Deserialization Protocol
When the native image is executed, the runtime performs the following steps:

#### 4.1 Startup Steps
1. **Map the Snapshot File**:
   - Use `mmap` (or equivalent) to map the snapshot file into memory.
   - The code image is mapped as executable (RX).
   - The heap and metadata images are mapped as read-write (RW).

2. **Apply Relocations**:
   - Parse the relocation table from the header.
   - For each relocation, fix up the absolute address:
     - `heap_base` is set to the start of the heap image.
     - External symbols (e.g., libc functions) are resolved via the dynamic linker.
   - This step can be parallelized across multiple threads.

3. **Initialize the Runtime**:
   - Set up thread-local state (GC buffers, safepoint flags, etc.).
   - Initialize the GC (mark bitmap, card table, remembered sets).
   - Initialize the AOSS subsystem (load the dependency map).
   - Initialize the IC subsystem (load the stub arrays).

4. **Validate Assumptions**:
   - Parse the assumption list from the header.
   - For each assumption, check that it still holds:
     - "Class X exists" → Check the class registry.
     - "Method Y has signature Z" → Check the method descriptor.
     - "Layout L has field F at offset O" → Check the layout registry.
   - If any assumption fails, **abort snapshot loading** and fall back to normal startup (discard the snapshot, start from bytecode).

5. **Jump to Entry Point**:
   - If all assumptions are valid, jump to the entry point (a pre-compiled T3 method or a trace).
   - The application begins execution with near-zero startup overhead.

#### 4.2 Startup Time Breakdown
| Step | Estimated Time |
|------|----------------|
| Map snapshot file | ~1 ms |
| Apply relocations | ~5-10 ms (parallelized) |
| Initialize runtime | ~2-5 ms |
| Validate assumptions | ~1-2 ms |
| **Total** | **~10-20 ms** |

This is "near zero" compared to traditional JIT startup (which can take seconds for large applications).

---

### 5. Relocation and Position Independence
The key to a robust native image is minimizing relocation work. Our architecture has several advantages:

#### 5.1 NV-1 Heap References
All heap references are 48-bit offsets from `heap_base`. These are already position-independent:
- No relocation needed for heap references.
- Just set `heap_base` correctly on startup.

#### 5.2 Position-Independent Code (PIC)
The code image uses PIC wherever possible:
- Branch targets use relative offsets (already position-independent).
- Data references use RIP-relative addressing (x86-64) or ADRP (AArch64).
- Only absolute addresses (e.g., external symbols) require relocation.

#### 5.3 Relocation Table
The relocation table lists all absolute addresses that need fixup:
```
RelocationEntry {
    offset: u64,          // Offset within the code/metadata image
    type: enum {
        HEAP_BASE,        // Fix up to heap_base
        EXTERNAL_SYMBOL,  // Fix up to external symbol (resolved by dynamic linker)
        CODE_BLOB,        // Fix up to another CodeBlob
        HEAP_OBJECT,      // Fix up to a heap object (should be rare, use offsets instead)
    },
    symbol_name: string,  // For EXTERNAL_SYMBOL
}
```

On startup, we iterate through the relocation table and apply the fixups. This is a simple, fast process.

---

### 6. GC Integration
The snapshot must integrate seamlessly with our concurrent, generational GC.

#### 6.1 Heap State on Startup
- The entire snapshot heap is treated as **old generation**.
- The mark bitmap is initialized to "all marked" (no need to re-trace).
- The card table is initialized to "clean" (no young-to-old references to scan).
- The remembered set is empty (no old-to-young references).

#### 6.2 New Allocations
After startup, new allocations follow the normal GC protocol:
- New objects are allocated in the young generation.
- Write barriers track young-to-old references.
- The GC runs concurrently as usual.

#### 6.3 Snapshot Heap Compaction (Corrected: Non-Relocatable by Contract)

The 1.0 text stated that the GC treats the snapshot heap like any other old-generation heap and relocates objects with reference updates. That is contradictory and unsafe, and it is corrected here (finding SNAP-1, resolved).

A shared base snapshot is mapped `PROT_READ` across processes. Relocating an object requires writing a forwarding pointer into read-only memory, which faults. If the OS resolves such faults by Copy-on-Write, every faulting process silently privatizes the shared pages, which destroys exactly the memory-sharing benefit the shared snapshot exists to provide. There is no configuration in which moving snapshot objects is correct.

Therefore, snapshot heap memory is strictly non-relocatable:

- **Pinning by page descriptor**: every page belonging to a snapshot heap image is marked `PINNED | IMMORTAL` in its page descriptor at map time. The evacuation candidate selector never selects pinned pages. This is selection-time exclusion, not fault-time discovery.
- **Marking still traces snapshot pages**: reachability analysis must traverse snapshot objects, because snapshot objects can reference runtime-allocated objects and runtime objects can reference snapshot objects. Marking never mutates snapshot object payloads or headers. Mark bits for snapshot pages live in the GC side tables (HEAP-1 side metadata), which are runtime-private, writable, and per-process.
- **Fragmentation is absorbed, not compacted**: fragmentation inside the snapshot image is permanent by design. Dead snapshot objects are not reclaimed by the running GC. The remedy for a fragmented or stale base snapshot is snapshot rebuilding (§6.4), an offline operation, not a GC compaction phase.
- **Snapshot-to-young references use an external remembered set**: snapshot pages are read-only, so no card mark can be stored into a snapshot card table. References from snapshot objects into the mutable heap are tracked in a runtime-private, per-process snapshot remembered set allocated in normal memory. It is populated by the write barrier when a mutator stores a snapshot-derived reference into a mutable object, and it is scanned as a root source during young-generation collection.
- **Write barrier obligation on snapshot targets is removed**: a mutator can never store into a snapshot object; a store whose target page is a snapshot page traps (Snapshot Violation, SNAP-INV-09). The only snapshot-related barrier obligation on the fast path is the remembered-set barrier for stores of snapshot-derived references into mutable objects, which the generational barrier already performs.

#### 6.4 Snapshot Rebuilding
If the application runs for a long time and the snapshot heap becomes fragmented or outdated, we can rebuild the snapshot:
- Serialize the current heap state.
- Re-compile hot methods with updated profiling data.
- Write a new snapshot file.

This can be done offline (e.g., during a maintenance window).

---

### 7. Dynamic Code and Class Loading
If the application loads new classes or code at runtime (e.g., via `eval`, dynamic class loading, or plugin systems), the snapshot cannot contain them.

#### 7.1 Fallback to JIT
- The runtime detects that a class/method is not in the snapshot.
- It falls back to normal JIT compilation (T0 → T1 → T2 → T3).
- The newly compiled code is added to the code cache as usual.

#### 7.2 Assumption Invalidation
- If a dynamically loaded class invalidates an assumption in the snapshot (e.g., adds a new subclass that breaks a devirtualization), the AOSS dependency map triggers region-based deopt.
- The affected regions are re-compiled with the new information.

#### 7.3 Closed-World vs. Open-World
- **Closed-World**: The snapshot contains all possible code and classes. No dynamic loading is allowed. This is the fastest startup, but least flexible.
- **Open-World**: The snapshot contains the core application, but allows dynamic loading. This is more flexible, but may require more deopts.

We support both modes, depending on the application's needs.

---

### 8. Assumption Validation
The snapshot was built with certain assumptions (e.g., "class X has layout Y"). If the runtime environment differs (e.g., different library versions), these assumptions may be invalid.

#### 8.1 Assumption Types
| Assumption Type | Example | Validation Method |
|-----------------|---------|-------------------|
| Class Existence | "Class X exists" | Check the class registry |
| Method Signature | "Method Y has signature Z" | Check the method descriptor |
| Layout Stability | "Layout L has field F at offset O" | Check the layout registry |
| Call Target | "Call site C targets method M" | Check the method registry |
| Type Profile | "Variable V is always type T" | Check the type profile (optional, can be re-validated lazily) |

#### 8.2 Validation Protocol
On startup, we iterate through the assumption list and validate each one:
- If all assumptions are valid, proceed with snapshot loading.
- If any assumption fails, abort snapshot loading and fall back to normal startup.

#### 8.3 Lazy Validation
For performance-critical assumptions (e.g., type profiles), we can defer validation:
- On startup, we skip the validation.
- When the assumption is first used (e.g., a guard check), we validate it.
- If the validation fails, we deopt and re-compile.

This reduces startup time at the cost of potential deopts later.

---

### 9. Incremental and Shared Snapshots
For large applications or multi-tenant environments, we support incremental and shared snapshots.

#### 9.1 Incremental Snapshots
- **Base Snapshot**: Contains the core runtime and common libraries.
- **Delta Snapshot**: Contains only the changes since the base snapshot (new classes, new methods, modified objects).
- On startup, we load the base snapshot, then apply the delta.

This allows fast iteration during development (rebuild only the changed parts).

#### 9.2 Shared Snapshots
- Multiple applications can share a common base snapshot (e.g., the JavaScript runtime, common libraries).
- Each application has its own delta snapshot (application-specific code and data).
- The base snapshot is mapped read-only (shared across processes) and is pinned and non-relocatable per 6.3 and SNAP-INV-08/09/10.
- The delta snapshot is mapped read-write (private to each process).

This reduces memory usage and startup time for multi-tenant environments. Sharing remains beneficial only if the shared pages are never written; the non-relocatability contract is what keeps that true for the process lifetime. References from the shared base into per-process delta heaps are prohibited at snapshot-construction time (the base must be self-contained or reference only other shared bases), which removes the need for a shared cross-process remembered set entirely.

---

### 10. SNAP-1 Invariants

| ID | Invariant |
|----|-----------|
| **SNAP-INV-01** | **Position Independence**: All heap references in the snapshot use NV-1 48-bit offsets, requiring no relocation. |
| **SNAP-INV-02** | **Relocation Completeness**: All absolute addresses in the code and metadata images are listed in the relocation table. |
| **SNAP-INV-03** | **Assumption Validation**: All assumptions in the snapshot are validated on startup. If any fail, the snapshot is discarded. |
| **SNAP-INV-04** | **GC Consistency**: The snapshot heap is in a consistent GC state (all objects marked, no young-to-old references). |
| **SNAP-INV-05** | **Dynamic Loading Fallback**: If a class/method is not in the snapshot, the runtime falls back to normal JIT compilation. |
| **SNAP-INV-06** | **Incremental Compatibility**: Delta snapshots can be applied to base snapshots without corruption. |
| **SNAP-INV-07** | **Debug Integration**: The debug image provides complete DWARF info for all snapshot code. |
| **SNAP-INV-08** | **Snapshot Non-Relocatability**: No GC phase may select a snapshot page as an evacuation or compaction candidate. Snapshot pages are pinned and immortal for the lifetime of the mapping. A GC that relocates a snapshot object is incorrect by definition. |
| **SNAP-INV-09** | **Shared Immutability**: A base snapshot mapped for sharing is mapped read-only. Mutator stores whose target page is a snapshot page trap (Snapshot Violation). The engine must not rely on Copy-on-Write to handle such stores. |
| **SNAP-INV-10** | **External Snapshot Remembered Set**: Card marks and remembered references for snapshot pages reside in runtime-private memory outside the snapshot image. No GC or barrier protocol may write GC state into shared read-only snapshot pages. |
---

## UGB-1: Universal Guest Bytecode Specification
**Version 1.0 · Core Execution Contract**

### 1. Fundamental Design Philosophy
UGB is designed around three non-negotiable pillars:
1. **Universal Representation**: It must be expressive enough to map the semantics of JS, Python, Ruby, Lua, and custom DSLs without losing fidelity.
2. **Capability-Based Security**: There is zero ambient authority. All access to resources (I/O, FFI, memory mapping, network) requires a first-class Capability object.
3. **Register-Based Execution**: It operates on a register-based VM model, dynamically allocating virtual registers per method, perfectly aligning with the NV-1 64-bit value model and the JIT's Adaptive Representation Selection (ARS).

### 2. The Capability Security Model (Polyglot Isolation)
In a polyglot engine, if Python code can access the filesystem, and JS code shares the same global namespace or runtime environment, a malicious JS script could exploit the Python runtime to escape its sandbox. UGB prevents this via **Object Capabilities**.

#### 2.1 How Capabilities Work
- A Capability is a standard heap object (with a specific `LayoutID`, e.g., `Layout_Capability`).
- It contains an **Authority Mask** (e.g., `AUTH_FILE_READ`, `AUTH_FFI`, `AUTH_NETWORK`) and a **Resource Handle** (e.g., a file descriptor, a socket, a C function pointer).
- **No Global Functions**: There is no global `open_file()` or `ffi_call()`. To open a file, you must invoke a method on a `FileSystem` capability.
- **Delegation**: A host (the C++/Rust embedder) creates capabilities and passes them to the entry point of a script. The script can delegate (pass) these capabilities to child functions or other languages, but it cannot forge them.

#### 2.2 Polyglot Boundary Security
When JS calls Python:
1. The JS code must explicitly pass a Capability to the Python entry point if the Python code needs to do anything privileged.
2. The Python code runs in an isolated lexical scope. It cannot "reach out" to grab ambient authority.
3. If the Python code attempts an FFI call without an `FFI_Capability`, the UGB `CAP_CHECK` opcode throws a `SecurityError`.

### 3. Execution State & Frame Layout
UGB is register-based. Every method invocation creates a **Frame**.

#### 3.1 Frame Structure
```text
[ Frame Header ]
  - Method ID
  - Return Address (PC in caller)
  - Caller Frame Pointer
  - Capability Environment Pointer (Lexical scope of authorities)
[ Virtual Registers ]
  - Reg_0 to Reg_N (Each is a 64-bit NV-1 slot)
  - Includes Arguments, Locals, and Temporaries
[ Upvalue Pointers ]
  - Pointers to closed-over variables from enclosing scopes
```

#### 3.2 Register Allocation
- The method header declares `arg_count` and `register_count`.
- The interpreter allocates `register_count * 8` bytes on the VM stack.
- All virtual registers are initially treated as `TAGGED` (NV-1) values. The JIT (T2/T3) will later apply ARS to keep them unboxed in physical CPU registers where profitable.

### 4. Instruction Encoding
To balance L1 I-cache density with the need for large methods, UGB uses a **Hybrid Variable-Length Encoding**.

- **Standard Instruction (95% of code)**: 
  `[ 1-byte Opcode ] [ 1-byte Reg A ] [ 1-byte Reg B ] ...`
  Supports up to 256 virtual registers per method. Extremely compact.
- **Extended Instruction (Prefix 0xFF)**: 
  `[ 0xFF ] [ 1-byte Opcode ] [ 2-byte Reg A ] [ 2-byte Reg B ] ...`
  Supports up to 65,536 virtual registers. Used for massive methods or complex FFI calls.

### 5. The Opcode Taxonomy (116 Opcodes)
UGB defines exactly 116 opcodes, grouped into 12 functional domains.

#### 5.1 Constants & Literals (10)
| Opcode | Name | Operands | Semantics |
|--------|------|----------|-----------|
| `0x00` | `LOAD_NULL` | `dst` | `dst = Null` |
| `0x01` | `LOAD_UNDEF` | `dst` | `dst = Undefined` |
| `0x02` | `LOAD_TRUE` | `dst` | `dst = True` |
| `0x03` | `LOAD_FALSE` | `dst` | `dst = False` |
| `0x04` | `LOAD_INT8` | `dst, imm8` | `dst = SignExtend(imm8)` |
| `0x05` | `LOAD_INT16` | `dst, imm16` | `dst = SignExtend(imm16)` |
| `0x06` | `LOAD_INT32` | `dst, imm32` | `dst = SignExtend(imm32)` |
| `0x07` | `LOAD_INT64` | `dst, imm64` | `dst = imm64` (Requires extended encoding) |
| `0x08` | `LOAD_FLOAT` | `dst, imm64` | `dst = binary64(imm64)` |
| `0x09` | `LOAD_CONST` | `dst, pool_idx` | `dst = ConstantPool[pool_idx]` |

#### 5.2 Registers & Variables (12)
| Opcode | Name | Operands | Semantics |
|--------|------|----------|-----------|
| `0x10` | `MOVE` | `dst, src` | `dst = src` |
| `0x11` | `LOAD_LOCAL` | `dst, local_idx` | `dst = Frame[local_idx]` |
| `0x12` | `STORE_LOCAL` | `local_idx, src` | `Frame[local_idx] = src` |
| `0x13` | `LOAD_UPVALUE` | `dst, upval_idx` | `dst = *UpvaluePtr[upval_idx]` |
| `0x14` | `STORE_UPVALUE` | `upval_idx, src` | `*UpvaluePtr[upval_idx] = src` |
| `0x15` | `LOAD_GLOBAL` | `dst, name_idx` | `dst = GlobalEnv[name_idx]` |
| `0x16` | `STORE_GLOBAL` | `name_idx, src` | `GlobalEnv[name_idx] = src` |
| `0x17` | `LOAD_ARG` | `dst, arg_idx` | `dst = Args[arg_idx]` |
| `0x18` | `STORE_ARG` | `arg_idx, src` | `Args[arg_idx] = src` |
| `0x19` | `LOAD_THIS` | `dst` | `dst = ThisBinding` |
| `0x1A` | `STORE_THIS` | `src` | `ThisBinding = src` |
| `0x1B` | `LOAD_ENV` | `dst, env_idx` | `dst = CapabilityEnv[env_idx]` |

#### 5.3 Arithmetic & Bitwise (16)
*All operate on NV-1 TAGGED values. The JIT unboxes them to I64/F64 if ARS proves it is safe.*
| Opcode | Name | Operands | Semantics |
|--------|------|----------|-----------|
| `0x20` | `ADD` | `dst, a, b` | `dst = a + b` (Handles int/float/string concat) |
| `0x21` | `SUB` | `dst, a, b` | `dst = a - b` |
| `0x22` | `MUL` | `dst, a, b` | `dst = a * b` |
| `0x23` | `DIV` | `dst, a, b` | `dst = a / b` |
| `0x24` | `MOD` | `dst, a, b` | `dst = a % b` |
| `0x25` | `POW` | `dst, a, b` | `dst = a ** b` |
| `0x26` | `NEG` | `dst, a` | `dst = -a` |
| `0x27` | `ABS` | `dst, a` | `dst = |a|` |
| `0x28` | `MIN` | `dst, a, b` | `dst = min(a, b)` |
| `0x29` | `MAX` | `dst, a, b` | `dst = max(a, b)` |
| `0x2A` | `BIT_AND` | `dst, a, b` | `dst = a & b` |
| `0x2B` | `BIT_OR` | `dst, a, b` | `dst = a | b` |
| `0x2C` | `BIT_XOR` | `dst, a, b` | `dst = a ^ b` |
| `0x2D` | `BIT_SHL` | `dst, a, b` | `dst = a << b` |
| `0x2E` | `BIT_SHR` | `dst, a, b` | `dst = a >>> b` (Logical) |
| `0x2F` | `BIT_SAR` | `dst, a, b` | `dst = a >> b` (Arithmetic) |

#### 5.4 Comparison & Logic (8)
| Opcode | Name | Operands | Semantics |
|--------|------|----------|-----------|
| `0x30` | `EQ` | `dst, a, b` | `dst = (a == b)` (Abstract equality) |
| `0x31` | `NE` | `dst, a, b` | `dst = (a != b)` |
| `0x32` | `LT` | `dst, a, b` | `dst = (a < b)` |
| `0x33` | `LE` | `dst, a, b` | `dst = (a <= b)` |
| `0x34` | `GT` | `dst, a, b` | `dst = (a > b)` |
| `0x35` | `GE` | `dst, a, b` | `dst = (a >= b)` |
| `0x36` | `IS_NULL` | `dst, a` | `dst = (a == Null)` |
| `0x37` | `IS_UNDEF` | `dst, a` | `dst = (a == Undefined)` |

#### 5.5 Control Flow (10)
| Opcode | Name | Operands | Semantics |
|--------|------|----------|-----------|
| `0x40` | `JUMP` | `offset` | `PC += offset` |
| `0x41` | `JUMP_TRUE` | `cond, offset` | `if (cond) PC += offset` |
| `0x42` | `JUMP_FALSE` | `cond, offset` | `if (!cond) PC += offset` |
| `0x43` | `JUMP_NULL` | `val, offset` | `if (val == Null) PC += offset` |
| `0x44` | `JUMP_NOT_NULL`| `val, offset` | `if (val != Null) PC += offset` |
| `0x45` | `SWITCH_TABLE` | `val, table_idx` | Jump via dense jump table |
| `0x46` | `SWITCH_SPARSE` | `val, table_idx` | Jump via sparse hash/map |
| `0x47` | `CALL` | `dst, target, argc` | `dst = target(args...)` |
| `0x48` | `TAIL_CALL` | `target, argc` | Tail-recursive call |
| `0x49` | `RETURN` | `val` | Return `val` to caller |

#### 5.6 Objects & Arrays (14)
| Opcode | Name | Operands | Semantics |
|--------|------|----------|-----------|
| `0x50` | `ALLOC_OBJ` | `dst, layout_id` | Allocate object with given layout |
| `0x51` | `ALLOC_ARRAY` | `dst, len` | Allocate array of length `len` |
| `0x52` | `ALLOC_STRING` | `dst, len, enc` | Allocate string (Latin-1/UTF-16) |
| `0x53` | `ALLOC_CLOSURE`| `dst, func_id` | Allocate closure (upvalues bound later) |
| `0x54` | `LOAD_FIELD` | `dst, obj, offset` | `dst = obj[offset]` |
| `0x55` | `STORE_FIELD` | `obj, offset, val` | `obj[offset] = val` |
| `0x56` | `LOAD_INDEX` | `dst, arr, idx` | `dst = arr[idx]` (Bounds checked) |
| `0x57` | `STORE_INDEX` | `arr, idx, val` | `arr[idx] = val` (Bounds checked) |
| `0x58` | `LOAD_LENGTH` | `dst, obj` | `dst = obj.length` |
| `0x59` | `HAS_FIELD` | `dst, obj, name` | `dst = obj.hasOwnProperty(name)` |
| `0x5A` | `DELETE_FIELD` | `obj, name` | `delete obj[name]` |
| `0x5B` | `ITER_NEXT` | `dst, iter` | `dst = iter.next()` |
| `0x5C` | `ITER_DONE` | `dst, iter` | `dst = iter.done` |
| `0x5D` | `ALLOC_ITER` | `dst, obj` | `dst = obj[Symbol.iterator]()` |

#### 5.7 Capabilities & Security (10)
*The core of the polyglot security model.*
| Opcode | Name | Operands | Semantics |
|--------|------|----------|-----------|
| `0x60` | `CAP_CREATE` | `dst, auth, handle`| Create new capability with authority mask |
| `0x61` | `CAP_CHECK` | `cap, req_auth` | Throws `SecurityError` if `cap` lacks `req_auth` |
| `0x62` | `CAP_DELEGATE` | `dst, cap, sub_auth`| Create a derived capability with reduced authority |
| `0x63` | `CAP_REVOKE` | `cap` | Revokes the capability and all its delegates |
| `0x64` | `CAP_INVOKE` | `dst, cap, method` | Invoke a method on the capability's resource |
| `0x65` | `CAP_SEAL` | `cap` | Prevents further delegation of this capability |
| `0x66` | `CAP_GET_AUTH` | `dst, cap` | `dst = cap.authority_mask` |
| `0x67` | `CAP_DROP` | `cap` | Explicitly drop a capability (helps GC) |
| `0x68` | `CAP_WRAP` | `dst, obj, auth` | Wrap a raw object in a capability envelope |
| `0x69` | `CAP_UNWRAP` | `dst, cap` | Extract the raw object (requires authority check) |

#### 5.8 Exceptions & Unwinding (6)
| Opcode | Name | Operands | Semantics |
|--------|------|----------|-----------|
| `0x70` | `THROW` | `val` | Throw exception `val` |
| `0x71` | `TRY_BEGIN` | `catch_offset` | Push exception handler to `catch_offset` |
| `0x72` | `TRY_END` | | Pop exception handler |
| `0x73` | `CATCH` | `dst` | `dst = thrown_exception` |
| `0x74` | `FINALLY_BEGIN`| | Mark start of finally block |
| `0x75` | `FINALLY_END` | | Mark end of finally block |

#### 5.9 Closures & Environments (6)
| Opcode | Name | Operands | Semantics |
|--------|------|----------|-----------|
| `0x80` | `MAKE_CLOSURE`| `dst, func, upvals`| Bind upvalues to closure |
| `0x81` | `CLOSE_UPVALUES`| `reg` | Close all upvalues pointing to `reg` |
| `0x82` | `OPEN_UPVALUE` | `dst, reg` | Create open upvalue for `reg` |
| `0x83` | `LOAD_ENV` | `dst, env_idx` | Load from capability environment |
| `0x84` | `STORE_ENV` | `env_idx, src` | Store to capability environment |
| `0x85` | `CLONE_ENV` | `dst, src` | Clone a capability environment scope |

#### 5.10 FFI & Foreign Interop (8)
*All FFI operations require an `FFI_Capability`.*
| Opcode | Name | Operands | Semantics |
|--------|------|----------|-----------|
| `0x90` | `FFI_LOAD_LIB` | `dst, cap, name` | Load foreign library (Requires `AUTH_FFI`) |
| `0x91` | `FFI_LOOKUP` | `dst, cap, lib, sym`| Lookup symbol in library |
| `0x92` | `FFI_CALL` | `dst, cap, ptr, args`| Call foreign function pointer |
| `0x93` | `FFI_CAST` | `dst, cap, val, type`| Cast value to foreign type |
| `0x94` | `FFI_PIN` | `cap, obj` | Pin object in memory (prevents GC move) |
| `0x95` | `FFI_UNPIN` | `cap, obj` | Unpin object |
| `0x96` | `FFI_CALLBACK` | `dst, cap, func` | Create a callable foreign callback |
| `0x97` | `FFI_SYSCALL` | `dst, cap, num, args`| Direct OS syscall (Requires `AUTH_SYSCALL`) |

#### 5.11 Type & Reflection (8)
| Opcode | Name | Operands | Semantics |
|--------|------|----------|-----------|
| `0xA0` | `TYPE_OF` | `dst, val` | `dst = typeof(val)` |
| `0xA1` | `INSTANCE_OF` | `dst, val, type` | `dst = val instanceof type` |
| `0xA2` | `IS_TYPE` | `dst, val, tag` | `dst = (NV1_TAG(val) == tag)` |
| `0xA3` | `AS_TYPE` | `dst, val, tag` | Cast/Coerce `val` to `tag`, throws on failure |
| `0xA4` | `GET_PROTOTYPE` | `dst, obj` | `dst = obj.__proto__` |
| `0xA5` | `SET_PROTOTYPE` | `obj, proto` | `obj.__proto__ = proto` |
| `0xA6` | `GET_OWN_PROP` | `dst, obj, name` | Get property descriptor |
| `0xA7` | `ENUM_PROPS` | `dst, obj` | `dst = Object.keys(obj)` |

#### 5.12 GC, Safepoints & Meta (8)
| Opcode | Name | Operands | Semantics |
|--------|------|----------|-----------|
| `0xB0` | `SAFEPOINT` | | Explicit GC yield point (used in long loops) |
| `0xB1` | `WRITE_BARRIER`| `obj, val` | Explicit GC write barrier (JIT may elide) |
| `0xB2` | `GC_ALLOC` | `dst, size` | Raw allocation (bypasses standard alloc) |
| `0xB3` | `YIELD` | `val` | Yield to coroutine/scheduler |
| `0xB4` | `DEBUG_BREAK` | | Trigger debugger breakpoint |
| `0xB5` | `NOP` | | No operation |
| `0xB6` | `HALT` | | Halt execution immediately |
| `0xB7` | `SOURCE_MAP` | `line, col` | Debugging metadata (ignored in execution) |

---

### 6. JIT Integration & Capability Optimization
How does the JIT (T2/T3/T4) handle UGB, especially the capability overhead?

#### 6.1 Capability Elision via AOSS
The `CAP_CHECK` opcode is a potential performance bottleneck if executed on every FFI call. 
- **AOSS Integration**: When a function receives a `FileSystem` capability, AOSS tracks it as a `SPECIALIZED` value. 
- **Guard Insertion**: The JIT inserts a single `GuardLayout(cap, Layout_Capability)` and a `GuardAuth(cap, AUTH_FILE_READ)` at the function entry.
- **Elision**: Inside the function body, all subsequent `CAP_CHECK` opcodes for that same capability are **eliminated** by Sea of Nodes' GVN (Global Value Numbering). The JIT replaces the UGB `CAP_INVOKE` with a direct native call, completely bypassing the runtime check.
- **Invalidation**: If the capability is revoked (`CAP_REVOKE`), the AOSS DependencyMap invalidates the specialized region, forcing a deopt to the generic, checked path.

#### 6.2 Polyglot Inlining
If JS calls Python, and both are compiled by T3:
- The FFI trampoline is analyzed. If the semantic adapters (exception translation, type conversion) are proven to be no-ops (e.g., both languages use NV-1 integers), the trampoline is **inlined**.
- The Python function body is directly embedded in the JS method's SoN graph.
- Result: Cross-language calls execute at native, intra-module speeds.

---

#### 6.3 Capability Version Epochs: Cheap T0 Enforcement (Corrected Addition)

Section 6.1 covers JIT tiers, where AOSS guards eliminate per-call CAP_CHECK cost. It does not cover T0, where no specialization exists. The 1.0 text left T0 enforcing revocation through full authority-mask validation on every privileged operation, which is slow, or not enforcing it at all, which violates UGB-INV-01. Both outcomes are unacceptable, and the fix is the Capability Version Epoch (finding UGB-1, resolved).

**Object model.** The Capability object's auxiliary header word (revised HEAP-1 128-bit header) reserves 16 bits as the version_epoch, plus 1 bit as the revoked flag. The epoch is a saturating up-counter.

**Runtime model.** The runtime's capability table stores, per capability: authority_mask, resource_handle, epoch (atomic u16), revoked (atomic flag), and a delegation link to its parent. CAP_REVOKE and CAP_SEAL are the only operations that mutate epoch or flag: revocation increments the epoch (saturating at 0xFFFF, where the capability becomes permanently invalid) and sets revoked. Delegation creates a child whose revocation propagates by incrementing every ancestor's epoch.

**T0 fast path.** Each UGB CAP_CHECK / CAP_INVOKE / FFI_* site in a method owns a one-slot epoch cache in the method's metadata (not in the frame, so it is shared across activations of the same method). The interpreter check is:

1. Load the site's cached epoch (thread-local metadata read).
2. Load the capability's current epoch (single 16-bit load) and its revoked flag.
3. If equal and not revoked: proceed. Total cost: two loads, one compare, one predicted branch.
4. On mismatch: slow path. Re-read the full authority table. If the capability moved to a new epoch with the required authority still present, update the cache and proceed. If it was revoked (or authority is insufficient), throw SecurityError (UGB section 5.7).

A stale cache can only cause a slow path, never a security hole: the authoritative state is the capability object's epoch, and the cache is validated against it on every check. Revocation therefore costs one increment, and every T0 site observes it on its next check with no broadcast, no code patching, and no full-mask evaluation.

**JIT tiers.** T2/T3 keep the AOSS guard model of section 6.1. The epoch is folded into the guard: GuardEpoch(cap, epoch_at_compile) is GVN'd with the layout guard, and CAP_REVOKE's epoch increment invalidates dependent regions through the AOSS DependencyMap exactly like a layout transition (CONC-1 publication rules apply).

### 7. UGB Invariants

| ID | Invariant |
|---|---|
| **UGB-INV-01** | **Capability Completeness**: No privileged operation (FFI, I/O, Syscall) can be executed without a valid, unrevoked capability passed explicitly as an operand. |
| **UGB-INV-02** | **Value Uniformity**: All virtual registers hold exactly one 64-bit NV-1 value. There are no "hidden" types or unboxed values at the UGB level. |
| **UGB-INV-03** | **Control Flow Safety**: All jumps and branches must target valid bytecode offsets. The interpreter and JIT must verify CFG integrity before execution. |
| **UGB-INV-04** | **Exception Safety**: Every `TRY_BEGIN` must have a matching `TRY_END`. Unwinding must correctly restore the capability environment and virtual register state. |
| **UGB-INV-05** | **Safepoint Coverage**: Every loop back-edge and function call must contain or be dominated by a `SAFEPOINT` or implicit safepoint, ensuring the concurrent GC can make progress. |
| **UGB-INV-06** | **Semantic Preservation**: The JIT must never optimize away a `CAP_CHECK` unless it has mathematically proven the capability's validity and authority via AOSS. |
| **UGB-INV-07** | **Epoch Cache Safety**: A cached capability epoch may only ever cause a slow path, never a bypass. Every capability-consuming site must validate its cached epoch against the capability object's current epoch before the privileged operation executes. Revocation must be observable to all tiers within one check of the next privileged use. |

---


---

## NUM-1: Numeric Execution Contract (The Numeric Retcon)
**Version 1.0 · Adds PACKED_TAGGED, AOSS Numeric Array Specialization, barrier elision, stride-aware ABCE, ARS live-range splitting, CAP_FAST_MATH**

### 1. Purpose and Scope

This section consolidates the numeric-execution contract of the engine. It retcons the SIMD-INV-01 prohibition (SIMD-1 section 1, version 1.1), specifies the unboxed numeric array layouts inside AOSS, and defines the elision and loop optimizations that together let numeric kernels run at native memory-bandwidth and compute throughput. The engine's numeric targets are scalar numeric code, tight loops, physics simulation, and array processing. Dense linear algebra libraries (BLAS/LAPACK) are explicitly out of scope: the auto-vectorizer on numeric arrays is the supported path for such kernels.

### 2. The Value-Model Facts the Retcon Rests On

1. **Floats are transparent.** An NV-1 float is a native IEEE-754 binary64 bit pattern. SIMD float arithmetic on lanes drawn from a float-homogeneous tagged array requires no unboxing; the tag is the value. The only obligation is collision canonicalization when a result lane lands in the reserved tag region (SIMD-1 section 3.2 B-F).
2. **Integers are opaque.** An NV-1 integer is 0xFFFC plus a signed 48-bit payload. SIMD integer arithmetic must mask to the payload, compute, range-check, and re-tag (SIMD-1 section 3.2 B-I), with a whole-vector scalar fallback on 48-bit overflow.
3. **References are invisible to numeric lanes.** A numeric lane context never contains references (SIMD-INV-06), so numeric SIMD values impose no GC obligations.

### 3. AOSS Numeric Array Layout Transition

#### 3.1 Layout_NumericArray_F64 and Layout_NumericArray_I64

When AOSS observes that an array is populated exclusively with numbers (no references, no null, no undefined, no mixed domains), it triggers a layout transition:

- **New canonical layouts**: `Layout_NumericArray_F64` and `Layout_NumericArray_I64`, interned in the Layout Registry like any other canonical layout handle (REVISED section 2: hash-assisted lookup, canonical handle identity).
- **Physical layout**: the array header carries the numeric element kind; the payload is a contiguous block of raw 64-bit unboxed values with no NV-1 tags. Element i is at `header_end + 8*i`, preserving the HEAP-1 alignment contract.
- **Transition protocol**: the transition runs at a safepoint. The payload is converted from tagged to raw form (floats bit-copied; integers sign-extended from their 48-bit payloads and stored as i64; any stored value outside the i64-preserving domain keeps the array in tagged form). The array's layout handle is swapped atomically, and the AOSS DependencyMap is published through the CONC-1 section 2.1.1 handshake.
- **Reverse transition**: a store of a non-numeric value (reference, null, undefined, boxed object) into a numeric array converts the array back to the tagged array layout. The check on the store fast path is one tag compare against the expected numeric domain; the slow path performs the reverse migration. This preserves language semantics: numeric arrays are an optimization, never an observable type.

#### 3.2 GC Contract

- **Payload non-tracing**: the GC's mark phase consults the layout handle. For `Layout_NumericArray_F64` and `Layout_NumericArray_I64`, the payload is raw data and is never scanned as references. A raw float bit pattern or i64 can never be misinterpreted as a heap reference, which removes a whole class of conservative-scanning hazards.
- **Header tracing**: the header and any reference-bearing metadata are traced as usual.
- **Snapshot interaction**: numeric arrays may live inside snapshot images; their payload bytes are serialized raw, and the non-relocatability contract (SNAP-INV-08) applies unchanged.

### 4. Barrier Elision for Numeric Kernels

- **Write barrier elimination**: a `Layout_NumericArray_*` payload contains zero heap references, so a store into its payload cannot create a young-to-old or old-to-young pointer. When LLIR lowers `STORE_INDEX` whose target layout is a numeric array, the Write Barrier Elision pass deletes the barrier instruction outright. The generated store sequence is a raw `vmovupd`/`movq` with no GC side effects.
- **Load barrier elimination**: loads from a numeric array payload take no load barrier (BARRIER-INV-08). Where escape analysis (VSDG domain) additionally proves the array is thread-local and newly allocated, the layout pre-check is hoisted out of the loop entirely, reducing steady-state element access to a single unboxed load.
- **What is never elided**: the bounds checks subject to section 6, and the store fast-path tag check of section 3.1 (reverse-transition safety).

### 5. PACKED_TAGGED and the Unboxed Loop Body

For a loop over a numeric array, the JIT (T2/T3) emits:

1. Direct unboxed SIMD loads (`vmovupd`) into `F64`/`PACKED_TAGGED` vector registers.
2. Lane arithmetic without tag manipulation (floats) or with the masked-payload sequence (integers).
3. Boxing back to `TAGGED` only when a value flows into a generic variable, an observable boundary, or a non-numeric context.

ARS live ranges keep values unboxed across the loop (NV-1 section 5.3 hybrid model); the conversion-amortization argument (NV-1 section 7.1) applies with n = iterations.

### 6. Stride-Aware ABCE

Bounds checks inside tight loops destroy instruction-level parallelism. The SoN induction-variable analysis detects canonical counted loops (`i = start; i < len; i += step`) and, when the following conditions hold, hoists a single range proof to the loop pre-header and emits the body's `LOAD_INDEX`/`STORE_INDEX` without per-iteration bounds checks:

1. The induction variable is monotone with a stride provably not crossing zero between iterations.
2. The accessed index is an affine function of the induction variable: `idx = c0 + c1 * i`.
3. The pre-header proof `min(idx) >= 0 and max(idx) < len` is established once (`if len > 0` style guard, or the degenerate empty-range skip).
4. The array reference is loop-invariant and no intervening store can resize the array (length is immutable per allocation in this engine; a resize is a new object, which changes the layout handle and invalidates the region).

The proof and its preconditions are recorded in region metadata. Any violated precondition falls back to per-access checks. This is the standard ABCE pass assigned to the LLIR domain (T2/T3 section: LLIR "Safety Elision"), with the stride-aware induction extension defined here.

### 7. ARS Live-Range Splitting

A value used as an object in one region and as a number in another must not be pinned to `TAGGED` in a GPR across both. ARS implements live-range splitting:

- If value `v` is used for object manipulation in block A and for arithmetic in block B, ARS splits the live range: `v` stays `TAGGED` in a GPR for A, and a single `UNBOX` emits an unboxed `F64`/`I64` copy at B's entry.
- GVN ensures the unbox is not duplicated when B is reached on multiple paths; PRE hoists it when partially redundant.
- Deopt maps record both fragments, so deoptimization reconstructs the tagged value from whichever fragment is live (OSR-1 section 7 state-reconstruction rules apply unchanged).

### 8. CAP_FAST_MATH

IEEE-754 strictness (NV-1 section 4.3) forbids reassociation, FMA contraction, and denormal flushing by default. UGB introduces the `CAP_FAST_MATH` capability bit in the authority mask space. When a function is invoked under a capability carrying `CAP_FAST_MATH`, the JIT is permitted, within that function's regions:

1. Reassociation of floating-point operations (algebraic identities that hold over the reals, e.g. `(a+b)+c` to `a+(b+c)`).
2. FMA contraction: `vfmadd` replacing separate multiply-then-add pairs.
3. Denormal flush-to-zero (DAZ/FTZ mode around the region).
4. Non-preservation of NaN payloads beyond the NV-1 canonicalization contract.

The capability is explicitly delegated (UGB section 2) and revoked like any other: revocation increments the epoch (UGB-1 section 6.3) and invalidates dependent regions through AOSS. Without this capability, the default remains strict IEEE-754 semantics with canonical NaN results. The fast-math authorization is recorded per region; a deopt out of a fast-math region re-executes the remaining computation under strict semantics from the deopt point.

### 9. Updated Numeric Contract

| Metric | Previous architecture | Contract after NUM-1 |
|---|---|---|
| Scalar float math | Tagged with per-op unbox/box | Unboxed across live ranges via ARS splitting (section 7); near-native |
| Scalar int math | 48-bit immediate, boxing on overflow | Unboxed I64 live ranges with 48-bit range check at boundaries; boxed only on overflow |
| Tagged array processing | Prohibited (SIMD-INV-01 1.0) | Float lanes free (B-F), integer lanes masked (B-I), both gated by section 3.4 of SIMD-1 |
| Unboxed array processing | Not specified | `Layout_NumericArray_F64/I64` (section 3): raw payload, native SIMD |
| GC overhead in numeric loops | Write + load barriers per access | Zero: payload non-tracing (3.2) plus barrier elision (section 4) |
| Bounds checks | Per access | Hoisted by stride-aware ABCE (section 6) |
| FMA / reassociation | Banned | Enabled under `CAP_FAST_MATH` (section 8), default remains strict |

### 10. NUM-1 Invariants

| ID | Invariant |
|---|---|
| **NUM-INV-01** | A `Layout_NumericArray_*` payload is never scanned, traced, or interpreted as references by any GC phase. |
| **NUM-INV-02** | A store into a numeric array of a value outside its numeric domain triggers the reverse layout transition (or the store traps in a frozen context); it never corrupts the payload domain. |
| **NUM-INV-03** | Write barriers are elided on numeric-array stores only when the target layout is proven `Layout_NumericArray_*` by the store's layout guard; the guard is not optional. |
| **NUM-INV-04** | Stride-aware ABCE may remove a bounds check only when the section 6 preconditions are proven and recorded in region metadata. |
| **NUM-INV-05** | `CAP_FAST_MATH` transformations apply only inside regions invoked under a valid, unrevoked capability carrying the bit; strict semantics are the default everywhere else. |

---

## REVIEW RESOLUTIONS REGISTER
**Version 1.0 · Maps the external architecture review findings to their resolutions**

| Finding | Severity claimed | Resolution location | Status |
|---|---|---|---|
| BARRIER-1: load barrier color-test false positives on tagged integers | Critical correctness | BARRIER-1 sections 2.0/2.1 (tag precedence fast path); BARRIER-INV-06/07/08 | RESOLVED |
| SNAP-1: shared read-only snapshot vs. relocating GC | Critical correctness | SNAP-1 section 6.3 (non-relocatable snapshot pages, external remembered set); SNAP-INV-08/09/10 | RESOLVED |
| PROF-1: missing T2/T3 back-edge counting starves T4 trace selection | Critical correctness | PROF-1 section 3.2 (sampled back-edge counter); PROF-INV-07/08 | RESOLVED |
| UGB-1: T0 capability revocation cost (full mask check vs. no check) | Suboptimal | UGB-1 section 6.3 (16-bit capability version epoch, site epoch cache); UGB-INV-07 | RESOLVED |
| T1-OSR: 32-bit OSR counter inside stencils explodes the Stencil Pool | Suboptimal | OSR-1 section 6.1 (shared OSR Trampoline Stub, Metadata Pool counters); OSR-INV-06 | RESOLVED |
| SIMD-1: Strategy B guard cost exceeds benefit on short loops | Suboptimal | SIMD-1 section 3.4 (profitability gating); SIMD-INV-05 | RESOLVED |
| SIMD-RETCON: direct tagged SIMD math prohibition is unnecessary for floats | Retcon | SIMD-1 version 1.1 sections 1/3.2/3.2.1 (B-F, B-I, PACKED_TAGGED); SIMD-INV-01/02 retconned; NUM-1 section 2 | RESOLVED |
| VSDG-OVERLAY: overlay interaction with base-graph passes undefined | Ambiguity | REVISED section 1 (transaction API, event log, commit-time sync, revision counters) | RESOLVED |
| CONC-1: dependency add vs. invalidation traversal race in the RCU map | Ambiguity / race | CONC-1 section 2.1.1 (seqlock publish, retry bound, generation check, handshake escalation); CONC-INV-06/07 | RESOLVED |
| FFI-1: cross-language inlining preconditions unproven | Ambiguity | FFI-1 section 5 (SemanticCompatibilityMatrix); INV-FFI-007; Universal Engine 6.3 retraction | RESOLVED |
| NUM-1: numeric performance contract (AOSS numeric arrays, barrier elision, stride-aware ABCE, ARS splitting, CAP_FAST_MATH) | Enhancement | NUM-1 sections 3-9; NUM-INV-01..05 | SPECIFIED |

Every resolution above is enforced by the reference implementation in this repository where the subsystem is implemented (see README.md for the implemented-subsystem map), and is otherwise a binding specification for implementations of this design.
