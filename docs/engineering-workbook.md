# AE engineering architecture and delivery workbook

Planning baseline: 2026-11-18, revision 660cec0. Read alongside [the charter](plan.md), [current semantics](current-language.md), [platform evidence](platform-matrix.md) and the [specification](../spec/). This workbook proposes implementation order and gates. A checked-in source file establishes presence. Passing behavior requires a dated test result.

## Product contract

AE is a native programming language combining Rust-style memory safety and zero-cost abstractions with Go-style concise syntax, concurrency and integrated tools. Protect safe code first. Make allocations, copies, blocking and dynamic dispatch discoverable. Infer local details while keeping public contracts readable. Support broad application domains through libraries, target profiles and foreign interfaces. Treat ten million lines as a possible maintenance burden. Measure progress in programs people can build, ship, upgrade and debug.

Make explicit compromises. Reject ambiguous escaping borrows until signatures can express them. Start with scoped threads before lightweight tasks. Ship a narrow supported platform matrix before expanding it. Permit C toolchain dependencies during bootstrap. Accept slower release compilation when a separate fast development build exists. Reopen each compromise against its milestone gate.

## Starting position and first work queue

The root Cargo package uses ae-compiler. The workspace already includes ae-span and ae-syntax. The compiler still carries C runtime sources. The README describes generic specialization, limited shared borrowing, static trait calls, CLI tools and a native C11 path. Inventory each against executable evidence. In particular the desktop charter's historical statement about absent M0 crates is stale.

1. Pin 660cec0 as the comparison point. Record Rust, C compiler, OS, CPU, flags and checkout cleanliness in a baseline report.
2. Run cargo test --workspace on Windows and Linux. Preserve failures with exact reproducer, target and exit code.
3. Run the existing check, emit-c, run, fmt and test routes on a shared fixture. Capture stdout, stderr, exit status and diagnostics independently.
4. Exercise the unary precedence change with nested prefix and binary operators. Compare the production parser and the extracted parser on the same source.
5. Trace examples/hello.ae from CLI argument parsing through frontend, C emission, linking and process exit. Record the implementing module at each handoff.
6. Index current-language.md rules by a passing fixture and a failing fixture. Mark missing coverage explicitly.
7. Classify each M0 crate by manifest membership, actual CLI caller and differential coverage. Port one missing production path at a time.
8. Freeze a representative ownership corpus covering return, break, assignment, error propagation and field views. Capture drop counts as well as output.
9. Publish the baseline and select the first unsupported safety rule with a minimized test. Close M0 only after normal entry points pass the same corpus.

## Component contracts

| Boundary | Input and output | Invariant and failure contract |
|---|---|---|
| Driver and package loader | CLI, manifest, lockfile to module graph | Stable module identities, cycle trace, explicit target and profile |
| Source database | File revisions to spans and line maps | Byte offsets scoped to a revision, Unicode display positions computed at presentation |
| Lexer and parser | UTF-8 to tokens and recoverable syntax tree | Retain trivia for formatting, bounded recovery, diagnostics have primary spans |
| Resolution and HIR | Syntax to scoped definitions and resolved expressions | Distinguish modules, values and types, report ambiguous paths at use |
| Type checker | HIR to typed bodies, substitutions and obligations | Check generic bodies against declared bounds, diagnose unsatisfied obligations at call site |
| MIR | Typed body to control-flow graph of places and operations | Explicit move, borrow, drop and unwind or abort edges, validate before code generation |
| Ownership checker | MIR to initialization and loan facts | Reject use after move, overlapping exclusive access, escaped locals and uninitialized reads |
| Layout and specialization | Concrete types and target to layout and instance graph | Cache by canonical arguments and target, bound expansion, explain cycles |
| Backends and linker | Checked MIR to object or executable | Common ABI description, equivalent observable semantics, source locations in debug builds |
| Runtime | Allocator, task, I/O and panic hooks | Document every unsafe precondition, thread affinity and resource owner |
| Tools and IDE | Shared queries to diagnostics, formatting, hover and navigation | Revision-aware cancellation, consistent spans, deterministic machine-readable output |

Use a one-way dependency chain from driver to frontend, semantic analysis and code generation. Put layout and target data behind a shared read-only interface. Keep the platform runtime separately linkable. Initially enforce these boundaries as modules. Split a crate when tests or consumers require independent compilation.

## Language decisions to specify before general availability

- Values move by default. Specify Copy eligibility recursively. Cloning owned storage is explicit. Destructors run deterministically in specified order. For partial moves track initialization per field and forbid unsupported projections.
- Shared loans permit simultaneous reads. Exclusive loans require unique access for their live region. Specify reborrows, owner mutation and invalidation by container growth. Express output-to-input lifetime relations in public interfaces where elision yields multiple candidates.
- Define overflow, shifts, conversions, NaN comparisons, UTF-8 boundaries, indexing failure and evaluation order per profile. Give each operation a pass, trap or diagnostic oracle.
- Prefer local inference, algebraic enums, exhaustive matching, Result and Option. Specify propagation cleanup on every early exit. Resolve method ambiguity deterministically. Specialize generic calls and reserve dynamic dispatch for an explicit interface.
- Specify panic policy per target, foreign unwind barriers and destructor behavior during failure. Separate recoverable I/O errors from violated invariants.
- Stable C ABI initially covers explicitly represented scalars, structures, opaque handles and pointer-length pairs. Define alignment, ownership transfer, nullable values, thread affinity and callback lifetimes. Keep native generic layout private.
- Define safe concurrency through ownership transfer, shared-access bounds and scoped joining. Give cancellation, channel close and waiting precise observable behavior before choosing scheduler optimizations.

## Delivery rule for each numbered slice

Land one observable behavior per change. (1) Link the governing rule or open a decision record. (2) Write a minimal accepted program. (3) Write the nearest rejected or failing program. (4) Record expected diagnostics, values or resource events. (5) Change the owning component. (6) Run its narrow tests. (7) Exercise the public CLI on the same fixtures. (8) Run the platform matrix affected by the change. (9) Update spec, compatibility notes and evidence. (10) Commit the slice with a reversible migration boundary. Split a slice again whenever its acceptance requires two independently shippable behaviors.
