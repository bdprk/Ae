# ae engineering architecture and delivery workbook

Planning baseline: 2026-11-18, revision 58024fe. Read alongside [the charter](plan.md), [current semantics](current-language.md), [platform evidence](platform-matrix.md) and the [specification](../spec/). This workbook proposes implementation order and gates. A checked-in source file establishes presence. Passing behavior requires a dated test result.

## Product contract

ae is a native programming language combining Rust-style memory safety and zero-cost abstractions with Go-style concise syntax, concurrency and integrated tools. Protect safe code first. Make allocations, copies, blocking and dynamic dispatch discoverable. Infer local details while keeping public contracts readable. Support broad application domains through libraries, target profiles and foreign interfaces. Treat ten million lines as a possible maintenance burden. Measure progress in programs people can build, ship, upgrade and debug.

Make explicit compromises. Reject ambiguous escaping borrows until signatures can express them. Start with scoped threads before lightweight tasks. Ship a narrow supported platform matrix before expanding it. Permit C toolchain dependencies during bootstrap. Accept slower release compilation when a separate fast development build exists. Reopen each compromise against its milestone gate.

## Starting position and first work queue

The root Cargo package uses ae-compiler. The workspace already includes ae-span and ae-syntax. The compiler still carries C runtime sources. The README describes generic specialization, limited shared borrowing, static trait calls, CLI tools and a native C11 path. Inventory each against executable evidence. In particular the desktop charter's historical statement about absent M0 crates is stale.

1. Pin 58024fe as the comparison point. Record Rust, C compiler, OS, CPU, flags and checkout cleanliness in a baseline report.
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

## Ordered engineering backlog

Within each milestone execute rows in order unless the dependency column permits independent work. Each arrow is a separate checkable implementation step. Use the ten-step delivery rule above for each semantic change.

| Slice | Depends on | Small steps | Exit evidence |
|---|---|---|---|
| M0.1 Source fidelity | Baseline | Index files → normalize revision IDs → map byte spans → render tabs and multibyte text → pin JSON diagnostics | Editor and terminal point to the same token |
| M0.2 Parser migration | M0.1 | Inventory grammar → port literals → expressions → patterns → declarations → modules → recover after bad token → switch CLI route | Differential corpus matches and malformed files terminate |
| M1.1 Resolution | M0.2 | Intern symbols → allocate definition IDs → collect declarations → resolve imports → detect cycles → resolve locals and shadowing | Forward references and ambiguous names have specified outcomes |
| M1.2 Types | M1.1 | Intern types → infer locals → unify calls → occurs check → type return paths → reject unconstrained variables | Type dumps stable across repeated builds |
| M1.3 Generics | M1.2 | Bind parameters → collect obligations → check generic body → test overlap → canonicalize instances → limit recursion | Invalid body rejected even when unused, repeated instances share code |
| M1.4 Patterns and closures | M1.2 | Lower destructuring → check exhaustiveness → classify captures → infer closure environment → lower invocation | Missing variant diagnosed, capture moves observable |
| M2.1 MIR skeleton | M1.2 | Build places → assign temporaries → create block terminators → lower branches → loops → early exits → validate graph | Golden MIR and interpreter agree with C path on pure expressions |
| M2.2 Ownership | M2.1 | Track initialized places → join at branches → iterate loops to fixed point → handle partial moves → elaborate drop flags | Every owned resource dropped exactly once across exit paths |
| M2.3 Loans | M2.2 | Generate loans → compute liveness → compare projections → reject conflicting accesses → model reborrows → validate escaping references | Reject dangling views, vector reallocation under loan and escaping captures |
| M2.4 Unsafe boundary | M2.3 | Inventory pointer primitives → specify alignment and provenance → isolate runtime calls → audit safe wrappers → run sanitizer corpus | Every safe wrapper has a documented invariant and adversarial boundary tests |
| M3.1 Library layers | M2.2 | Build core without allocator → add allocator trait → owned buffer → vector → string → map → iterator adapters | Freestanding core links, empty and failed allocations follow contract |
| M3.2 Hosted library | M3.1 | Define byte streams → buffered readers → files → path encoding → clocks → processes → typed errors | Copy arbitrary bytes and handle short reads, partial writes and Unicode paths |
| M3.3 Packages | M1.1 | Parse manifest → path dependencies → version constraints → solve graph → lock hashes → offline build → verify downloaded content | Same lock gives same graph, corrupt cache fails closed |
| M4.1 Threads | M2.3 | Specify transfer and sharing bounds → scope join → mutex guards → atomics → shared owner → panic propagation | Borrowed child joins before parent exits, unsafe sharing rejected |
| M4.2 Tasks | M4.1 | Single worker queue → stack lifecycle → yield → scope join → cancellation points → timer → multiworker stealing | Stress scope teardown and stack limits, replay single-worker schedules |
| M4.3 Channels | M4.2 | Rendezvous → bounded buffer → close → blocked sender wakeup → receiver cancellation → select registration and loser cleanup | No lost wakeup, duplicate delivery or waiter retained after cancellation |
| M4.4 Network | M4.2 | Define readiness/completion adapter → Linux epoll → Windows IOCP → partial I/O → deadlines → blocking-call pool → backpressure | Disconnect during read and cancel during write release all handles |
| M5.1 Native debug | M2.3 | Freeze layout API → lower scalars → calls → aggregates → drops → relocations → link executable → emit debug spans | Cranelift and C agree across run corpus on ELF and COFF |
| M5.2 Release | M5.1 | Lower MIR to LLVM → validate IR → implement ABI attributes from proven facts → optimize → compare outputs → measure size and speed | No mismatch at optimization levels, explain allocation and dispatch costs |
| M5.3 Incremental | M1.1 | Stable query keys → dependency recording → fingerprints → invalidation → atomic cache writes → corruption recovery | Clean and incremental builds agree after body, interface and target edits |
| M6.1 Everyday CLI | M3.3 | Unify project discovery → check → build → run → test discovery → fmt idempotence → doc tests → consistent exit codes | Fresh user creates, tests, documents and ships a package using one executable |
| M6.2 Editor | M5.3 | Synchronize edits → publish diagnostics → hover → definition → completion → rename across modules → cancel stale queries | Unicode edits, unsaved buffers and module rename pass editor integration tests |
| M7.1 Interop | M2.4, M5.1 | Header subset → bindings → ownership annotations → callbacks → shared library export → Python buffer bridge | C and Python call and release the same library under leak checks |
| M7.2 Bootstrap | M6.1 | Port lexer → parser → resolver → checker → MIR → codegen driver → compile stage 1 → stage 2 → stage 3 | Compare normalized stage 2/3 artifacts, retain pinned Rust seed and rebuild instructions |
| M8.1 Compatibility | M6, M7 | Freeze observable rules → edition policy → deprecated syntax diagnostics → migration tool → run old package corpus | Old edition builds and migrations preserve outputs |
| M8.2 Release | M8.1 | Build on clean hosts → verify artifacts → exercise rollback → publish support windows → validate real deployments | Signed release manifest, reproducible evidence and three qualifying deployments per charter |

## What makes this a general-purpose language

| Domain | First deliverable | Added capability | Promotion test |
|---|---|---|---|
| CLI and automation | Directory report with streaming input | Paths, processes, error values, distribution | Run unattended on Windows and Linux with non-ASCII filenames |
| Network services | Concurrent bounded work server | Tasks, TLS via maintained library, deadlines, metrics | Load, cancel and restart with bounded memory and graceful shutdown |
| Systems and embedded | Freestanding byte parser | core, volatile access, linker scripts, custom panic handler | Boot on emulator, report image size, run without OS or heap |
| Desktop and mobile | Small native editor using host UI bindings | Event-loop integration, callbacks, packaging, cross compilation | Install on target device and release resources after window closure |
| Browser and plugins | Parser compiled to WebAssembly | Linear-memory ABI, host imports, size controls | Browser and native produce identical output for the same corpus |
| Data and scientific computing | Native array kernel callable from Python | Strided buffers, alignment, vectorization, numeric libraries | Match reference tolerance and count copies across the boundary |
| Games, graphics and audio | Interactive frame loop | GPU bindings, arenas, explicit frame allocations, real-time profiling | Record frame-time distribution and allocation count on named hardware |
| Storage and infrastructure | Restartable indexed store | Durable I/O, synchronization, crash consistency | Recover committed data across simulated interrupted writes |
| Accelerators and distributed workloads | Host-side compute and worker clients | Vendor FFI, serialization, retries, observability | Compare CPU reference and recover from a disconnected worker |

Each domain starts as an independently versioned package and example unless it needs a new compiler invariant. Promote a target from experimental to supported only with a named maintainer, repeatable build, conformance run and release artifact. Separate host support from target support. A Windows compiler emitting a foreign object establishes cross-codegen only. Execution on the target establishes runtime support.

## Performance and safety evidence

For iterator fusion, generic dispatch, closures, Result propagation and borrowed slices, maintain three equivalent programs: idiomatic ae, manually lowered ae and a hand-written native reference. Check outputs first. Inspect calls, allocations, copies and layout. Report multiple runs, dispersion, input size, compiler flags and hardware. Apply the charter's Rust comparison budget per named workload. For compilation report cold, warm no-change, single-body edit and public-interface edit separately. For concurrency track throughput, tail latency, parked tasks, memory per task and cancellation latency under saturation.

Maintain compile-fail suites for aliasing and lifetime rules, interpreter/codegen differential suites, deterministic scheduler interleavings and runtime sanitizer jobs. Minimize failures into versioned regressions. Treat proofs of individual invariants, empirical test results and unchecked assumptions as separate evidence. A backend optimization using alias metadata needs a documented derivation from checked MIR.

## Long-term maintenance and sequencing

The critical path is baseline → typed bodies → checked MIR → sound runtime boundaries → concurrent hosted programs. Package resolution and diagnostic tooling can progress alongside MIR work. Begin a second backend after the MIR corpus can expose divergence. Keep each prior compiler path available until the replacement passes production-entry tests. Prioritize the CLI as the first daily-use program, a service as the second and a native library embedded in another language as the third.

For each release publish a support table (target, installer, tested runtime, known limitations), an unsafe inventory, benchmark deltas and a migration note. Keep a minimal reproducer on every correctness issue. Triage wrong code, memory unsafety and data loss ahead of convenience work. Track unowned packages and unsupported platforms explicitly. Every quarter choose one user workload, profile its largest obstacle, deliver one end-to-end improvement and archive the measurements.

For each future feature open a work item with an ID, user program, prerequisites, touched interfaces, semantics, implementation steps, acceptance fixture, compatibility risk, rollback and evidence link. Estimate after a prototype exposes the unknowns. Keep the next milestone decomposed to individual changes and expand later milestones at their entry review. Release only when the gates pass. No calendar date or source-line target substitutes for evidence.
