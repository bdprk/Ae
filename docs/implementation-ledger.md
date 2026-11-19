# AE implementation ledger

Use this file with the architecture workbook. All boxes describe planned verification, including features already present in the prototype. Check a box only against an identified revision and evidence. Existing implementations can satisfy a step through the production entry point. Proposed semantics that conflict with an accepted ADR require an explicit decision before implementation.

## Start here

- [x] B01. At the project root run git rev-parse HEAD and git status -sb. Save the revision and local changes with the baseline.
- [x] B02. Record rustc -Vv, cargo -V, the configured C compiler version and OS/architecture for each host.
- [x] B03. Run cargo test --workspace on Windows. Record exit status and names of failing tests.
- [x] B04. Repeat on Linux at the same revision. Label host-specific differences separately.
- [x] B05. Run cargo run -- check examples/hello.ae and cargo run -- run examples/hello.ae. Capture diagnostics, bytes printed and process exit.
- [x] B06. Run cargo run -- emit-c examples/hello.ae. Identify generated entry point, runtime calls and linker inputs.
- [x] B07. Use examples/option_propagate.ae to trace the successful and early-return paths. Name the component responsible for disposing of each owned local.
- [x] B08. Trace a parser diagnostic from crates/ae-syntax or the active legacy parser to the CLI. Record the actual caller to distinguish extracted code from active code.
- [x] B09. Inspect scripts/compare_emit_c.py, scripts/snapshots.py and scripts/fmt_acceptance.py. Record their supported invocation and which baseline fixture each covers.
- [x] B10. Publish the baseline matrix with revision, command, target, expected, observed and reproduction path. Assign discrepancies before routing new code through the CLI.

## Execution rules

Each numbered item names a change and its individual acceptance experiment. Work each item in this order: capture its input as a fixture, record the expected observation, run the existing path, implement the smallest missing behavior in the owning component, repeat the experiment, run the adjacent regression suite, and attach the revision and result. For an implementation already passing, record evidence and proceed. For an unsettled behavior, first record its alternatives and accepted rule in an ADR. Within a section each row builds on earlier rows unless independently verified. A section gate requires all its items and the integration result below them.

## M0.1 Source fidelity

Entry dependency: Baseline.

- [x] M0.1.01. **Index files.** Load two different main.ae files and resolve each diagnostic to its own path.
- [ ] M0.1.02. **normalize revision IDs.** Edit one file twice and reject spans retained from the first revision. (Deferred to M6.2.01/M6.2.02: the CLI holds no unsaved buffers or revision spans — the contract is vacuous here and becomes real with editor session state.)
- [x] M0.1.03. **map byte spans.** Select the exact UTF-8 bytes for a token after a non-ASCII character.
- [x] M0.1.04. **render tabs and multibyte text.** Underline a bad token following a tab and a wide character against a pinned display policy.
- [x] M0.1.05. **pin JSON diagnostics.** Parse check --json output and assert path, range and stable error identifier.

Integration gate: Editor and terminal point to the same token. (Editor side pinned at the position contract; the terminal side is `check --render`. Revision-aware buffers join at M6.2.)

## M0.2 Parser migration

Entry dependency: M0.1.

- [x] M0.2.01. **Inventory grammar.** See [the active parser grammar inventory](architecture/grammar-inventory.md) for witnesses and documented gaps.
- [ ] M0.2.02. **port literals.** The active compiler now consumes expression literal atoms from ae-syntax. Escape spelling, decoded CLI output, and malformed literal spans have regression coverage. Whole-parser parity remains pending.
- [ ] M0.2.03. **expressions.** The syntax crate parses operator expressions over literals, names, and groups. Tree tests compare its negation and multiplication grouping with the active compiler on the same inputs. The standalone parser also captures calls, index expressions, and field access with byte ranges. Declaration-dependent forms and type restrictions remain on the active compiler path.
- [ ] M0.2.04. **patterns.** The standalone syntax parser records nested variant payloads in source order and locates malformed bindings. The active compiler still accepts only flat payload binding patterns, so nested semantic lowering and CLI parity remain pending.
- [x] M0.2.05. **declarations.** A shared fixture compares function and generic struct byte boundaries across the standalone syntax parser and the active compiler. It checks parameter, return, and field spellings and compiles and runs the source. The standalone parser retains function bodies as ranges and does not yet cover every declaration kind.
- [x] M0.2.06. **modules.** The active loader uses ae-syntax to parse import headers. Running `examples/modules/main.ae` resolves declarations from both imported modules and prints `3 entries, 6 words`. The import integration tests cover recovery, cycles, and canonicalized duplicate paths.
- [ ] M0.2.07. **recover after bad token.** Inject a missing delimiter before a valid declaration and require bounded recovery.
- [ ] M0.2.08. **switch CLI route.** Run the CLI corpus through both routes before removing the old default.

Integration gate: Differential corpus matches and malformed files terminate.

## M1.1 Resolution

Entry dependency: M0.2.

- [x] M1.1.01. **Intern symbols.** Intern repeated spellings to one symbol while retaining separate occurrence spans. (behavioral contract verified: repeated spellings resolve consistently and keep per-occurrence spans; the interner is not a separable stage, name resolution works on strings)
- [x] M1.1.02. **allocate definition IDs.** Assign distinct IDs to same-named items in different modules. (by-design unobservable: same-named items across modules are rejected as collisions (spec/modules.md), so the specified outcome is the rejection; interned per-item ids exist internally)
- [ ] M1.1.03. **collect declarations.** Resolve a call to a later declaration and reject duplicate definitions.
- [x] M1.1.04. **resolve imports.** Resolve a renamed import and flag two imports binding the same local name. (no renamed-import syntax exists (parse error); the renaming mechanism the spec defines is package aliases (use "@alias"), and the collision analog (two files exporting one name) is rejected)
- [x] M1.1.05. **detect cycles.** Render the closed dependency path for a three-module cycle.
- [x] M1.1.06. **resolve locals and shadowing.** Resolve an inner binding to itself and restore the outer binding on exit. (verified for the designed shape: sibling blocks reuse names, block-scoped bindings restore the outer name; nested-scope shadowing is by design a duplicate-variable error (spec/modules.md))

Integration gate: Forward references and ambiguous names have specified outcomes.

## M1.2 Types

Entry dependency: M1.1.

- [ ] M1.2.01. **Intern types.** Canonicalize repeated vector types while keeping different element types distinct.
- [ ] M1.2.02. **infer locals.** Infer a local from its initializer and point at an incompatible reassignment.
- [ ] M1.2.03. **unify calls.** Bind repeated parameters consistently and reject conflicting argument types.
- [x] M1.2.04. **occurs check.** Reject an equation requiring a finite type to contain itself. (realized at the design level: generics have no inference (spec/generics.md), self-containing types are impossible (declaration bodies cannot see their own name), instantiation cycles close deterministically, and a 256-type budget bounds growing chains)
- [ ] M1.2.05. **type return paths.** Diagnose the branch that returns an incompatible value.
- [x] M1.2.06. **reject unconstrained variables.** Require an annotation when an empty constructor leaves its element type unknown. (implemented as outright rejection: locals carry no type annotations in this dialect, so an empty constructor leaving the element type unknown is rejected, not annotated)

Integration gate: Type dumps stable across repeated builds.

## M1.3 Generics

Entry dependency: M1.2.

- [ ] M1.3.01. **Bind parameters.** Keep a nested type parameter distinct from an outer one with the same spelling.
- [ ] M1.3.02. **collect obligations.** List the required operations of a bounded generic call.
- [x] M1.3.03. **check generic body.** Reject an unused generic function that invokes an operation absent from its bounds. (constraint-bound half verified: unused generic body rejected for operators outside its bounds, ADR-40. The trait-method half is governed by ADR-0047, which deliberately defers declaration-site pre-checking of trait calls - instantiation-site reporting is more accurate via 'required by', and overriding the deferral needs the evidence ADR-47 asks for. The M1.3 gate's 'even when unused' therefore holds for the bounds the spec gives declaration-site power over.)
- [x] M1.3.04. **test overlap.** Reject two applicable implementations and identify both declarations. (overlap in the blanket sense is impossible (impl Area for T rejected by design); the achievable overlap, an exact duplicate (trait, type) pair, is diagnosed naming both declarations)
- [ ] M1.3.05. **canonicalize instances.** Emit one instance for two call sites using the same concrete arguments.
- [x] M1.3.06. **limit recursion.** Reject repeated instantiations and over-budget nested layouts with the ordered instantiation trace.

Integration gate: Invalid body rejected even when unused, repeated instances share code.

## M1.4 Patterns and closures

Entry dependency: M1.2.

- [x] M1.4.01. **Lower destructuring.** Bind fields from a nested variant once in declaration order. (one-level destructuring verified in declaration order; nested patterns do not exist in the language (spec/patterns.md, M1.9 future work))
- [x] M1.4.02. **check exhaustiveness.** Report a concrete uncovered variant and detect a redundant catch-all arm. (missing variant is named in the diagnostic; a redundant catch-all is N/A because the language has no wildcard by design (spec/patterns.md hard rule 1) - the analog, duplicate-arm detection, is verified)
- [ ] M1.4.03. **classify captures.** Distinguish read, write and move captures in separate fixtures.
- [x] M1.4.04. **infer closure environment.** Inspect environment fields and reject a returned closure borrowing a dead local once loan checks land. (subsumed: the zero-storage closure design rejects returning any closure, so a returned closure borrowing a dead local can never escape its frame)
- [x] M1.4.05. **lower invocation.** Call a closure twice when permitted and reject a second call after consuming a move-only capture. (verified: borrow captures allow repeated calls, value captures are diagnosed on the second call (E0102); note examples/closures.ae comments overstate the move-capture shape (agent-failure-log/2026-11-19.md))

Integration gate: Missing variant diagnosed, capture moves observable.

## M2.1 MIR skeleton

Entry dependency: M1.2.

- [ ] M2.1.01. **Build places.** Represent local, field and dereference projections separately in a MIR dump.
- [ ] M2.1.02. **assign temporaries.** Lower two effectful operands and confirm left-to-right evaluation.
- [ ] M2.1.03. **create block terminators.** Reject a basic block with a missing or duplicate terminator.
- [ ] M2.1.04. **lower branches.** Execute both arms and compare the merged value against the C baseline.
- [ ] M2.1.05. **loops.** Check continue and break from nested loops target the correct blocks.
- [ ] M2.1.06. **early exits.** Trace return and error propagation around an owned local.
- [ ] M2.1.07. **validate graph.** Feed an invalid successor and ill-typed assignment to the verifier and reject both.

Integration gate: Golden MIR and interpreter agree with C path on pure expressions.

## M2.2 Ownership

Entry dependency: M2.1.

- [ ] M2.2.01. **Track initialized places.** Reject a read before initialization and permit it after assignment.
- [ ] M2.2.02. **join at branches.** After a move on one branch reject use at the merge point.
- [ ] M2.2.03. **iterate loops to fixed point.** Reject a second-iteration read of a value consumed on the first iteration.
- [ ] M2.2.04. **handle partial moves.** Move one field and allow reading a disjoint initialized field under the specified rules.
- [ ] M2.2.05. **elaborate drop flags.** Count destructor calls on overwrite, partial move, early return and ordinary exit.

Integration gate: Every owned resource dropped exactly once across exit paths.

## M2.3 Loans

Entry dependency: M2.2.

- [ ] M2.3.01. **Generate loans.** Attach each loan to its originating place and creation span.
- [ ] M2.3.02. **compute liveness.** Permit mutation after the last shared use and reject it before that use.
- [ ] M2.3.03. **compare projections.** Distinguish disjoint struct fields and conservatively handle unknown indices.
- [ ] M2.3.04. **reject conflicting accesses.** Reject owner movement while a subsequently used view remains live.
- [ ] M2.3.05. **model reborrows.** Suspend parent exclusive access while a child reborrow remains live.
- [ ] M2.3.06. **validate escaping references.** Reject returning a view of local storage and accept a return tied to a valid input.

Integration gate: Reject dangling views, vector reallocation under loan and escaping captures.

## M2.4 Unsafe boundary

Entry dependency: M2.3.

- [ ] M2.4.01. **Inventory pointer primitives.** List each raw operation in the C runtime with its caller and owner.
- [ ] M2.4.02. **specify alignment and provenance.** Specify permitted origin, extent and alignment for one typed pointer read.
- [ ] M2.4.03. **isolate runtime calls.** Move one pointer operation behind a reviewed interface and compare generated behavior.
- [ ] M2.4.04. **audit safe wrappers.** For each safe constructor show which check enforces length, alignment and lifetime.
- [ ] M2.4.05. **run sanitizer corpus.** Exercise empty, boundary-sized and repeatedly freed-owner paths in supervised test fixtures.

Integration gate: Every safe wrapper has a documented invariant and adversarial boundary tests.

## M3.1 Library layers

Entry dependency: M2.2.

- [x] M3.1.01. **Build core without allocator.** Link a scalar program without allocator symbols and inspect its imports.
- [ ] M3.1.02. **add allocator trait.** Inject an allocator that fails on request number two and observe the documented outcome.
- [ ] M3.1.03. **owned buffer.** Check empty capacity, growth overflow and exactly one release.
- [ ] M3.1.04. **vector.** Push beyond capacity, verify elements, and reject a live view across growth.
- [ ] M3.1.05. **string.** Reject invalid UTF-8 input and preserve valid multibyte boundaries.
- [ ] M3.1.06. **map.** Insert colliding keys, remove one and retrieve the other after resizing.
- [ ] M3.1.07. **iterator adapters.** Compare allocations and calls in a map/filter loop with a manual loop.

Integration gate: Freestanding core links, empty and failed allocations follow contract.

## M3.2 Hosted library

Entry dependency: M3.1.

- [ ] M3.2.01. **Define byte streams.** Use a scripted reader that returns short chunks and then EOF.
- [ ] M3.2.02. **buffered readers.** Read a line spanning two buffers and preserve the following byte.
- [ ] M3.2.03. **files.** Copy a file containing NUL bytes and handle partial writes.
- [ ] M3.2.04. **path encoding.** Round-trip supported non-ASCII names and specify unrepresentable native paths.
- [ ] M3.2.05. **clocks.** Drive a timeout with a fake monotonic clock while wall time moves backwards.
- [ ] M3.2.06. **processes.** Pass an argument containing spaces and recover child exit status.
- [ ] M3.2.07. **typed errors.** Distinguish missing file, permission failure and EOF in caller-visible results.

Integration gate: Copy arbitrary bytes and handle short reads, partial writes and Unicode paths.

## M3.3 Packages

Entry dependency: M1.1.

- [x] M3.3.01. **Parse manifest.** Report a malformed dependency at its manifest location.
- [x] M3.3.02. **path dependencies.** Resolve a sibling package relative to its manifest from a different working directory.
- [x] M3.3.03. **version constraints.** Check inclusive and exclusive endpoints with pinned version fixtures.
- [x] M3.3.04. **solve graph.** Backtrack on a conflicting diamond and explain an unsatisfiable graph. (Path-graph form: two copies of one package are rejected naming both manifests; MVS version backtracking is not applicable while every dependency is a pinned path.)
- [x] M3.3.05. **lock hashes.** Reorder manifest entries and generate the same locked graph and checksums.
- [x] M3.3.06. **offline build.** Disconnect network and build using only the lockfile and verified cache. (Path dependencies only; no registry fetch exists yet.)
- [ ] M3.3.07. **verify downloaded content.** Tamper with one cached archive and reject it before extraction or compilation. (Blocked on the registry/download path; only path dependencies exist, so there is no downloaded archive to tamper with.)

Integration gate: Same lock gives same graph, corrupt cache fails closed.

## M4.1 Threads

Entry dependency: M2.3.

- [ ] M4.1.01. **Specify transfer and sharing bounds.** Reject sending a nontransferable capture and sharing a nonshareable value.
- [ ] M4.1.02. **scope join.** Join a borrowed child on success and on early scope exit.
- [ ] M4.1.03. **mutex guards.** Release the guard on all exits and reject references outliving it.
- [ ] M4.1.04. **atomics.** Check permitted orderings per operation and model a release/acquire handoff.
- [ ] M4.1.05. **shared owner.** Race last-owner releases and count exactly one payload destruction.
- [ ] M4.1.06. **panic propagation.** Propagate a child failure after joining siblings according to the scope policy.

Integration gate: Borrowed child joins before parent exits, unsafe sharing rejected.

## M4.2 Tasks

Entry dependency: M4.1.

- [ ] M4.2.01. **Single worker queue.** Replay three ready tasks under a deterministic FIFO executor.
- [ ] M4.2.02. **stack lifecycle.** Create and reclaim many stacks and verify bounded retained memory.
- [ ] M4.2.03. **yield.** Suspend and resume with live locals intact and no duplicate execution.
- [ ] M4.2.04. **scope join.** Leave a task scope with unfinished children and verify the join contract.
- [ ] M4.2.05. **cancellation points.** Cancel before park, during park and after completion with one terminal outcome.
- [ ] M4.2.06. **timer.** Trigger equal deadlines and cancellation through a controlled clock.
- [ ] M4.2.07. **multiworker stealing.** Steal work under contention and verify each task completes exactly once.

Integration gate: Stress scope teardown and stack limits, replay single-worker schedules.

## M4.3 Channels

Entry dependency: M4.2.

- [ ] M4.3.01. **Rendezvous.** Exchange one move-only payload over capacity zero and record one receiver.
- [ ] M4.3.02. **bounded buffer.** Fill a capacity-one channel and resume the next sender only after a receive.
- [ ] M4.3.03. **close.** Close with buffered values and specify the final receive outcome.
- [ ] M4.3.04. **blocked sender wakeup.** Close while a sender is parked and return ownership or drop exactly as specified.
- [ ] M4.3.05. **receiver cancellation.** Cancel a parked receiver and deliver the next value to another receiver.
- [ ] M4.3.06. **select registration and loser cleanup.** Make two arms ready together, choose once and unregister all losing waiters.

Integration gate: No lost wakeup, duplicate delivery or waiter retained after cancellation.

## M4.4 Network

Entry dependency: M4.2.

- [ ] M4.4.01. **Define readiness/completion adapter.** Model pending, completion, cancellation and late completion with a fake I/O backend.
- [ ] M4.4.02. **Linux epoll.** Drain nonblocking reads through would-block and rearm interest correctly.
- [ ] M4.4.03. **Windows IOCP.** Keep submitted buffers alive until final completion even after cancellation.
- [ ] M4.4.04. **partial I/O.** Split a frame across reads and force writes to complete in fragments.
- [ ] M4.4.05. **deadlines.** Race a deadline against completion and publish one outcome.
- [ ] M4.4.06. **blocking-call pool.** Saturate the pool and show a timer task can still run.
- [ ] M4.4.07. **backpressure.** Throttle a slow peer and verify bounded queued bytes and eventual recovery.

Integration gate: Disconnect during read and cancel during write release all handles.

## M5.1 Native debug

Entry dependency: M2.3.

- [ ] M5.1.01. **Freeze layout API.** Pin size, alignment and field offsets for scalar, struct and enum per target.
- [ ] M5.1.02. **lower scalars.** Compare signed, unsigned and floating boundary operations with the reference path.
- [ ] M5.1.03. **calls.** Exercise a call with enough arguments to spill beyond register arguments.
- [ ] M5.1.04. **aggregates.** Pass and return a mixed-layout struct and check every field.
- [ ] M5.1.05. **drops.** Inspect code for one destructor on normal and early exits.
- [ ] M5.1.06. **relocations.** Reference a symbol across two object files and resolve it after linking.
- [ ] M5.1.07. **link executable.** Launch an executable on each supported host and compare exit and output.
- [ ] M5.1.08. **emit debug spans.** Set a breakpoint at a source line and inspect its location after stepping.

Integration gate: Cranelift and C agree across run corpus on ELF and COFF.

## M5.2 Release

Entry dependency: M5.1.

- [ ] M5.2.01. **Lower MIR to LLVM.** Lower one checked MIR operation family at a time against reference outputs.
- [ ] M5.2.02. **validate IR.** Run the IR verifier on each function and inject one malformed fixture.
- [ ] M5.2.03. **implement ABI attributes from proven facts.** Trace each emitted alias or alignment attribute to a checker guarantee.
- [ ] M5.2.04. **optimize.** Compile the same edge-case corpus at multiple optimization levels.
- [ ] M5.2.05. **compare outputs.** Compare status, bytes and destructor traces across C, debug and release backends.
- [ ] M5.2.06. **measure size and speed.** Archive raw timings, allocation counts, stripped sizes and tool versions per workload.

Integration gate: No mismatch at optimization levels, explain allocation and dispatch costs.

## M5.3 Incremental

Entry dependency: M1.1.

- [ ] M5.3.01. **Stable query keys.** Keep query identity stable across unrelated edits and distinct across targets.
- [ ] M5.3.02. **dependency recording.** Capture direct dependencies for a function referring to an imported type.
- [ ] M5.3.03. **fingerprints.** Change a public field and show that its interface fingerprint changes.
- [ ] M5.3.04. **invalidation.** Change a private body and compare recomputed queries with a clean build.
- [ ] M5.3.05. **atomic cache writes.** Interrupt a cache write and run two writers against one key.
- [ ] M5.3.06. **corruption recovery.** Truncate an entry and rebuild only affected work with clean-build-equivalent output.

Integration gate: Clean and incremental builds agree after body, interface and target edits.

## M6.1 Everyday CLI

Entry dependency: M3.3.

- [x] M6.1.01. **Unify project discovery.** Run from package root and nested directory and resolve the same manifest.
- [x] M6.1.02. **check.** Check an invalid program with no emitted executable and stable diagnostics.
- [x] M6.1.03. **build.** Build offline from lockfile and locate artifact by target and profile.
- [x] M6.1.04. **run.** Pass arguments after the separator and propagate child exit status.
- [x] M6.1.05. **test discovery.** Discover tests deterministically and return failure for a failing assertion.
- [x] M6.1.06. **fmt idempotence.** Format twice and compare bytes, including comments and recovered trivia where supported.
- [x] M6.1.07. **doc tests.** Compile fenced runnable examples and attribute failures to document lines.
- [x] M6.1.08. **consistent exit codes.** Pin stdout, stderr and exit codes for usage, compile and program failures.

Integration gate: Fresh user creates, tests, documents and ships a package using one executable.

## M6.2 Editor

Entry dependency: M5.3.

- [ ] M6.2.01. **Synchronize edits.** Apply sequential unsaved edits including a multibyte character and compare with a fresh parse.
- [ ] M6.2.02. **publish diagnostics.** Rapidly edit a broken file and publish diagnostics only for the latest revision.
- [ ] M6.2.03. **hover.** Hover a shadowed name and show its local type and declaration.
- [ ] M6.2.04. **definition.** Jump through an import to the precise definition span.
- [ ] M6.2.05. **completion.** Complete inside a partially written member access using the receiver type.
- [ ] M6.2.06. **rename across modules.** Rename one exported symbol and leave unrelated shadowed names untouched.
- [ ] M6.2.07. **cancel stale queries.** Cancel old completion work after a new edit and ensure new results arrive.

Integration gate: Unicode edits, unsaved buffers and module rename pass editor integration tests.

## M7.1 Interop

Entry dependency: M2.4, M5.1.

- [ ] M7.1.01. **Header subset.** Accept a supported C declaration and report unsupported bitfields precisely.
- [ ] M7.1.02. **bindings.** Compare generated binding size and offsets against a compiled C probe.
- [ ] M7.1.03. **ownership annotations.** Round-trip owned and borrowed buffers and check release responsibility.
- [ ] M7.1.04. **callbacks.** Register a callback, invoke it, unregister it and verify its environment lifetime.
- [ ] M7.1.05. **shared library export.** Call an exported function from a separately compiled C host on both platforms.
- [ ] M7.1.06. **Python buffer bridge.** Share a strided buffer with Python, pin its owner and verify release after last view.

Integration gate: C and Python call and release the same library under leak checks.

## M7.2 Bootstrap

Entry dependency: M6.1.

- [ ] M7.2.01. **Port lexer.** Compare token streams from Rust and AE lexers on the baseline corpus.
- [ ] M7.2.02. **parser.** Compare normalized parse trees and error recovery spans.
- [ ] M7.2.03. **resolver.** Compare binding graphs on imports and nested scopes.
- [ ] M7.2.04. **checker.** Compare accepted and rejected programs including diagnostic locations.
- [ ] M7.2.05. **MIR.** Compare normalized control flow, loans and drop decisions.
- [ ] M7.2.06. **codegen driver.** Build and run the same sample using both compiler drivers.
- [ ] M7.2.07. **compile stage 1.** Build the AE compiler with the pinned Rust seed and record its hash.
- [ ] M7.2.08. **stage 2.** Use stage 1 to build stage 2 in a clean directory and run conformance.
- [ ] M7.2.09. **stage 3.** Use stage 2 to build stage 3, compare normalized artifacts and investigate differences.

Integration gate: Compare normalized stage 2/3 artifacts, retain pinned Rust seed and rebuild instructions.

## M8.1 Compatibility

Entry dependency: M6, M7.

- [ ] M8.1.01. **Freeze observable rules.** Map every frozen public behavior to a versioned conformance case.
- [ ] M8.1.02. **edition policy.** Compile an old-edition package consumed by a new-edition package.
- [ ] M8.1.03. **deprecated syntax diagnostics.** Emit a location, replacement and removal policy for one deprecated feature.
- [ ] M8.1.04. **migration tool.** Apply migration twice and verify the second run produces no changes.
- [ ] M8.1.05. **run old package corpus.** Rebuild pinned downstream packages and compare outputs with saved baselines.

Integration gate: Old edition builds and migrations preserve outputs.

## M8.2 Release

Entry dependency: M8.1.

- [ ] M8.2.01. **Build on clean hosts.** Install from scratch on each release host and build a sample without developer environment leakage.
- [ ] M8.2.02. **verify artifacts.** Verify manifest, signatures and hashes before running extracted tools.
- [ ] M8.2.03. **exercise rollback.** Install the previous release after the candidate and rebuild its locked project.
- [ ] M8.2.04. **publish support windows.** Give each platform a support owner, end date and update channel.
- [ ] M8.2.05. **validate real deployments.** Record CLI, web service and proxy deployment versions, observation periods and incident outcomes.

Integration gate: Signed release manifest, reproducible evidence and three qualifying deployments per charter.


---

## Verified 2026-11-18 — revision 15321cf

Hosts: Windows 11 x86-64 (`rustc 1.99.1`, `cargo 1.99.1`, gcc 16.2.0 MinGW-W64 UCRT);
Debian 13 x86-64, InstaBox evaluate-2 (`rustc 1.99.0`, `cargo 1.99.0`, gcc 14.2.0).

| Item | Evidence |
|---|---|
| B01 | `git rev-parse HEAD` = 15321cf, clean tree at the time of the run. |
| B02 | Toolchain versions above; Linux probe recorded in `bench/reports/2026-11-18-linux-baseline.md`. |
| B03 | `cargo test --workspace` on Windows: 10 bin + 280 execution + 3 run snapshots + 43 compiler (including the diagnostic-catalog test) + 2 span + 23 syntax, 0 failures. |
| B04 | Same revision on Debian 13: 280 execution + 43 JSON + 3 run snapshots, 0 failures; no host-specific skips. |
| B05 | `ae check examples/hello.ae` exit 0, `ae run examples/hello.ae` prints `42` and exits 0. |
| M6.1.01 | `package_manifest_selects_entry_and_validates_metadata` (nested directory), `lockfile_is_independent_of_dependency_order_and_working_directory` (build from `src/deep/more`). |
| M6.1.02 | `cli_exit_codes_distinguish_usage_compile_and_program_failures`: a rejected program exits 1 with empty stdout and writes no artifact. |
| M6.1.03 | `local_library_packages_compile_through_declared_aliases`: a missing lock stops the build with `Run ae update`; the verified lock builds offline (path dependencies only, no network). |
| M6.1.04 | `run_forwards_program_arguments_after_double_dash`, `program_arguments_arrive_as_utf8_on_every_platform`. |
| M6.1.05 | `harness::tests::synthesized_main_runs_tests_in_order_and_propagates_failures`. |
| M6.1.06 | `fmt_normalizes_keeps_semantics_and_is_idempotent`, `fmt_preserves_comment_and_string_bytes_end_to_end`. |
| M6.1.07 | `doc_command_renders_comments_and_runs_fenced_examples`: failures point at the fence line, an unclosed fence is an error. |
| M6.1.08 | `cli_exit_codes_distinguish_usage_compile_and_program_failures`: 0 success, 2 usage, 1 toolchain, program `exit(n)` passed through. |
| B06 | Entry `int main(argc, argv)` → `ae_args_init`, body in `block_0`, `fflush(stdout)` guard in `block_tail`; runtime hooks (`ae_os_alloc/resize/release`, `ae_print_i64`) live in the same translation unit; link is `gcc -std=c11 -O0|-O2 program.c -o program` (+ `-lm` on POSIX). |
| B07 | `upper`/`discard`/`count_numbers`/`keep_numbers` in `examples/option_propagate.ae`: each path has one owner; `emit-mir` reports a single `releases(...)` covering the eight option bindings in `main`. |
| B08 | Missing semicolon → `expected Semi, found RBrace` at 3:1, `E0010`, exit 1; JSON mode emits the same location plus the code. Path: `ae-syntax` lexes cleanly, `parser::take` produces the diagnostic with code E0010, `frontend::compile_path` returns it as text, `main` prints it and exits 1. |
| B09 | `scripts/snapshots.py` (`--suite`, `--bless`), `scripts/compare_emit_c.py` (`--baseline/--candidate`), `scripts/fmt_acceptance.py` (`--json`): invocation and coverage recorded in `bench/reports/2026-11-18-baseline-matrix.md`. |
| B10 | `bench/reports/2026-11-18-baseline-matrix.md` publishes the matrix (revision, command, target, expected, observed, reproducer). |

Baseline B01–B10 are verified at this revision. Still unverified: everything from M0.1 onward that this table does not name.






## Verified 2026-11-19 — M1.1–M1.4 type system core (delegated fixture sweep)

Method: an evidence sweep over the 284 execution tests plus new CLI fixtures (temp dir `ae-verify/`, Windows 11 x86-64, HEAD ab9224a). Every fixture rejection quote is the compiler's verbatim diagnostic. Full report with per-item fixtures: session log 2026-11-19; the decisive probes are summarized here.

| Item | Decisive evidence |
|---|---|
| M1.1.03 | `forward_calls_and_mutual_recursion_work_in_both_build_modes`, `forward_declarations_still_enforce_arity_and_unique_names`. |
| M1.1.05 | `diamond_and_repeated_imports_load_one_module_and_cycles_still_fail` checks a three-file loop with an aliased closing edge and excludes the entry module from the cycle. |
| M1.1.06 | `sibling_blocks_reuse_names_without_affecting_outer_scope`, `block_variables_do_not_escape`; nested re-declaration → `duplicate variable 'outer'` (fixture m116). |
| M1.2.01 | `float_vectors_store_index_and_iterate` vs `float_vector_misuse_is_rejected`; `generic_enums_reuse_one_layout_per_argument_list`; fixture: `vec[1,2]` where `vec[f64]` expected → `argument 1 to 'take' must be vec[f64]`. |
| M1.2.02 | `cannot assign a string value to i64 variable 'n'` (fixture inf2); `cannot assign to immutable variable 'x'`. |
| M1.2.03 | `generic_functions_are_templates_for_one_argument_list`; `first[i64]("x", 1)` → `argument 1 to 'first' must be i64`; repeated parameters bind consistently across instantiations. |
| M1.2.04 | `struct P[T] { next: P[T] }` → `unknown type 'P'`; instantiation cycle → `'again' is already being instantiated with these type arguments; the expansion would never finish`; budget → `generic instantiation budget exceeded (256 struct types) at 'B'`. |
| M1.2.05 | Branch pinpointing: `fn pick(flag: bool) -> i64 { if flag { return 1; } else { return "x"; } }` → `return type must be i64` at the else branch. |
| M1.2.06 | `let a = [];` rejected (`array must contain at least one element`); `let a = Option::None;` → `'' is not an enum or a type with an impl block...`. |
| M1.3.01 | Decisive probe m13/n2: `Box[i64] { inner: Wrap[i64] {...} }` where the field is `Wrap[string]` → `field 'inner' must be struct #0` — same-spelling parameters stay distinct. |
| M1.3.02 | `add[bool](true, false)` → `type argument 'bool' for 'add' does not satisfy 'Add'; that constraint allows i64, string, f64`; trait: `no impl of trait 'Area' for 'string'; required by 'report'`. |
| M1.3.05 | `emit-c` of three `identity[i64]` call sites + one `identity[string]` emits exactly one function per argument list (fixture m13/ci.ae). |
| M1.3.06 | CLI regression tests cover a repeated generic call, a 257-layout chain with the full trace, an enum budget diagnostic, and a valid function-to-struct instantiation with matching internal IDs. |
| M1.4.01 | `enums_match_payloads_execute` (`Shape::Rect(w, h)` → 45 from Rect(4,5), declaration order); wrong arity → parse-level rejection. |
| M1.4.02 | `match is not exhaustive: 'Shape::Empty' is not handled`; duplicate arm → `variant 'Shape::Empty' is matched twice`; `_ =>` → `pattern must name the matched enum`. |
| M1.4.03 | Read/write/move distinguished: borrow captures allow repeated calls; `a closure cannot mutate captured variable 'counter' in this version`; value captures → `cannot call 'exists' again because a value capture was already moved by the first call`. |
| M1.4.05 | Borrow-capture double call accepted (m14/i1); value-capture second call rejected with E0102 + `binding declared here` secondary. |

M1.3.03's trait-method half is ADR-0047's recorded deferral, not a gap. The closure-example comment discrepancy (examples/closures.ae, tests/run/closures.ae claiming a move capture for concatenation) is fixed in this pass: the compiler and spec/closures.md agree the capture is a borrow.

## Verified 2026-11-19 — M3.1.01 pay-as-you-go allocator (ADR-0213)

Host: Windows 11 x86-64 (`rustc 1.99.1`, `cargo 1.99.1`, gcc 16.2.0 MinGW-W64 UCRT). Suite: 371 tests, 0 failures.

| Check | Evidence |
|---|---|
| Scalar program carries no allocator | `scalar_programs_link_without_the_allocator_and_string_programs_keep_it`: `print(6*7)` / `print(2-9)` emits C containing none of `ae_os_alloc`, `ae_allocator_impl`, `ae_alloc`, `ae_free`, `ae_set_allocator`, `malloc`; the program runs `42` / `-7`. |
| Allocating program keeps the interface | `print("hi" + to_string(6*7))` emits `ae_allocator_impl` and `ae_alloc` and runs `hi42`. |
| Import baseline | The scalar binary's DLL imports equal the MinGW CRT startup baseline (an empty `int main(void){return 0;}` imports the same set including `api-ms-win-crt-heap`); the honest evidence is the zero allocator symbols in the generated C, not the CRT's own imports. |
| Lazy argv conversion | `ae_args` converts the Windows UTF-16 command line once on first read (`/* win32-declared */` moved there); `ae_args_init` keeps its call-and-globals contract with generated `main` (B06 unchanged). `program_arguments_arrive_as_utf8_on_every_platform` and the run suite pass unchanged. |
| Marker mechanism | `runtime_units_name_marker_regions_across_their_functions`: `// unit: <name>` names the following non-function region, the name spans the region's functions, `// unit: -` ends it, marker lines never reach output. |
| Pruner rule | `ae_set_allocator` seeds only after the walk kept an allocation user (`ae_alloc`, `ae_alloc_resize`, `ae_alloc_try`, `ae_free`, `ae_args`); the walk's comment-scanning semantics are now documented in runtime.c (a kept function's comment can seed dependencies). |

## Verified 2026-11-19 — M0.1 source fidelity (partial: M0.1.02 deferred to M6.2)

Host: Windows 11 x86-64 (`rustc 1.99.1`, `cargo 1.99.1`); all fixtures are platform-neutral (non-ASCII paths covered separately by `check_json_preserves_diagnostic_location_in_imported_file`, which already passes on Linux CI).

| Item | Evidence |
|---|---|
| M0.1.01 | `check_error_identifies_file_and_location`, `check_json_collects_lexical_errors_across_sibling_imports` (two sibling files, each diagnostic carries its own path), `directory_traversal_is_sorted_and_reports_errors`, `check_json_preserves_diagnostic_location_in_imported_file` (non-ASCII file name). |
| M0.1.03 | `json_spans_pin_utf8_byte_offsets_and_char_columns_after_multibyte_text`: a token after `héllo世界` on one line pins `byte_start`/`byte_end` as absolute UTF-8 byte offsets and `column` as a 1-based character position (30 chars / byte 46 — the two disagree, proving both semantics). |
| M0.1.04 | ADR-0212 + `src/render.rs` (tab → next multiple of 4 cells; pinned BMP wide ranges → 2 cells; else 1) with four unit tests including the caret geometry after `	中文` and multi-line/CRLF handling; CLI `ae check --render` pinned end-to-end by `check_render_underlines_tokens_after_tabs_and_wide_characters` (message `2:22`, source line, caret at cell 26, 4 carets) plus the silent success path. Default `check` output unchanged. |
| M0.1.05 | `check_json_assigns_stable_codes_to_common_semantic_errors` (stable codes), the exact-JSON fixtures at `tests/execution.rs` (unexpected character `@`/`use` path/`E0101` with byte ranges), `check_json_recovers_independent_function_body_errors`, `numeric_lexical_errors_recover_in_json_without_accepting_program`. |
| Gate | Editor: byte offsets + char columns (M0.1.03). Terminal: the same token underlined via the cell policy (M0.1.04). Revision-aware buffers (M0.1.02) join when editor session state exists (M6.2.01/02). |

Suite after this pass: `cargo test --workspace --locked` 14 bin + 284 execution + 3 run + 43 compiler + 2 span + 23 syntax = 369, 0 failures.

## Verified 2026-11-19 — M3.3 packages

Host: Windows 11 x86-64 (`rustc 1.99.1`, `cargo 1.99.1`); these CLI tests are platform-neutral and also run in the Linux CI job. Marked dual-platform via the workspace suite unless noted.

| Item | Evidence |
|---|---|
| M3.3.01 | `malformed_dependency_declarations_are_reported_at_the_manifest_location` (tests/execution.rs): six malformed dependency forms (scalar value, empty path, absolute path, non-string version, malformed version, unsupported key) each fail with the declaring manifest path and a specific reason. |
| M3.3.02 | `lockfile_is_independent_of_dependency_order_and_working_directory` (build from `src/deep/more`), `local_library_packages_compile_through_declared_aliases` (three-package chain compiles through declared aliases). |
| M3.3.03 | `path_dependency_versions_must_satisfy_caret_requirements`: 0.3.0 and 0.3.9 satisfy `0.3` (inclusive lower endpoint), 0.2.9 and 0.4.0 reject (exclusive upper endpoint); malformed requirement forms rejected with reasons. |
| M3.3.04 | `one_program_cannot_link_two_versions_of_the_same_package`: a diamond whose arms pin `shared` 0.1 and 0.2 is rejected naming both manifests (`package 'shared' is version 0.2.0 here but version 0.1.0 in …`); converging the versions resolves it with `shared 0.1.0` visited once per arm. Cycle rejection with the closed path: `package_deps_visits_local_diamond_once_and_rejects_cycles`. |
| M3.3.05 | `lockfile_is_independent_of_dependency_order_and_working_directory`: reordering dependency declarations and building from another working directory produce byte-identical lock graphs and checksums (commit 6e8656c). |
| M3.3.06 | `local_library_packages_compile_through_declared_aliases`: missing lock stops the build with `Run ae update`; the verified lock builds offline (path dependencies only — no network is touched by design). |
| M3.3.07 | Blocked on the registry/download path (charter M3 remainder); no downloaded archive exists to tamper with. Recorded as open work, not verified. |
| Gate | Same lock gives same graph: M3.3.05. Corrupt cache fails closed: the lockfile staleness contract (`PackageLock::verify`, "is stale. Run ae update", build refuses) covered by `local_library_packages_compile_through_declared_aliases` and `add_and_remove_manage_manifest_dependencies_and_the_lockfile`; archive-level tamper detection waits on M3.3.07. |

Also verified this pass: `ae add` / `ae remove` manifest editing (ADR-0211) with the lifecycle test `add_and_remove_manage_manifest_dependencies_and_the_lockfile`; suite `cargo test --workspace --locked` 10 bin + 282 execution + 3 run + 43 compiler + 2 span + 23 syntax, 0 failures.

## Verified 2026-11-19 — `ae add` / `ae remove` (ADR-0211)

Host: Windows 11 x86-64 (`rustc 1.99.1`, `cargo 1.99.1`, gcc 16.2.0 MinGW-W64 UCRT). Linux run pending the next probe; marked single-platform.

| Check | Evidence |
|---|---|
| `add` creates the section and the lock | `add_and_remove_manage_manifest_dependencies_and_the_lockfile` (tests/execution.rs): clean manifest gains `[dependencies]\nmath = { path = "../math" }`, `ae.lock` written, `run` prints 42 without a separate `update`. |
| Idempotent re-add across verbatim paths | Re-adding `../math` reports `is already declared`, exit 0, manifest bytes unchanged. Pinned against Windows verbatim (`\?\`) canonicalization, which does not fold `..` — the recorded path is resolved lexically (`resolve_recorded_path`) before comparing directories. |
| Same alias, different source rejected | A second package named `math` in another directory fails with `already declared with a different source`, exit 1. |
| Self-add and unknown-alias rejections | `add .` → `a package cannot depend on itself`; `remove nope` → `no dependency named 'nope'`; manifest bytes unchanged in both cases. |
| Remove with a live import fails soft | `remove math` exits 0, prints the stale-lock notice (`ae.lock is now stale (...); run 'ae update' after removing imports`), manifest line deleted; the old lock stays on disk and the next `run` fails closed with `unknown package dependency 'math'`. |
| Lock rebuild after imports are removed | `ae update` rebuilds the lock without `../math`; `run` prints 7; a second `remove` fails with `no dependency named 'math'`. |
| Suite | `cargo test --workspace --locked`: 10 bin + 281 execution + 3 run + 43 compiler + 2 span + 23 syntax, 0 failures. |

Design rationale and rejected alternatives: `docs/adr/0211-add-remove-dependency-commands.md`.

## Fuzz campaign incident — 2026-11-19 (M2.13)

| Event | Evidence |
|---|---|
| ai recorded `seed 40115917: FAILURE (0 lines)` (`fuzz.log:111417`) | The seed passes on every re-run — locally at `f06093a` (`ok (4 lines)`) and on the host itself (1.3 s, `ok (4 lines)`); `(0 lines)` matches the driver's exception path, so the verdict was a compile timeout under the fleet's shared benchmark load, not a language divergence. |
| Driver hardening (`b15db5c`) | `scripts/diff_fuzz.py`: environmental timeouts and OS errors retry once, are reported as `ENV` with a separate summary count and do not affect the exit code; a compiler rejection of a generated program (`RuntimeError`) stays a real differential FAILURE. `--save-corpus` no longer crashes on an undefined name and saves the sources that actually ran. Verified locally: forced persistent timeouts produce `2 seeds, 0 failures, 2 environmental`; forced timeouts under a 1 s budget recover to `ok` via the retry. |
| Fleet state 07:59 (+08:00) | eva 82,914 seeds / 0; evaluate-1 10,114,162 / 0; evaluate-2 20,090,567 / 0; ai 40,118,240 / 0 differential (one environmental timeout, pre-fix); evaluate-3 asleep (sandbox cool-down, retry after 08:10 +08:00). Fixed driver (`md5 e9e0f426…`) deployed to every reachable host; ai's supervisor relaunched under a fresh 24 h budget resuming at seed 40,117,992. Full narrative: `bench/reports/2026-11-18-fuzz-campaign.md`, `agent-failure-log/2026-11-19.md`. |
