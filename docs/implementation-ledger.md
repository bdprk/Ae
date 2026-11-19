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

- [ ] M0.1.01. **Index files.** Load two different main.ae files and resolve each diagnostic to its own path.
- [ ] M0.1.02. **normalize revision IDs.** Edit one file twice and reject spans retained from the first revision.
- [ ] M0.1.03. **map byte spans.** Select the exact UTF-8 bytes for a token after a non-ASCII character.
- [ ] M0.1.04. **render tabs and multibyte text.** Underline a bad token following a tab and a wide character against a pinned display policy.
- [ ] M0.1.05. **pin JSON diagnostics.** Parse check --json output and assert path, range and stable error identifier.

Integration gate: Editor and terminal point to the same token.

## M0.2 Parser migration

Entry dependency: M0.1.

- [ ] M0.2.01. **Inventory grammar.** Map each production to a current example or a documented gap.
- [ ] M0.2.02. **port literals.** Compare decoded values and spans for escaped and unterminated strings.
- [ ] M0.2.03. **expressions.** Parse unary minus nested inside multiplication and compare both parse trees.
- [ ] M0.2.04. **patterns.** Pin nested variant payload bindings and the span of a malformed pattern.
- [ ] M0.2.05. **declarations.** Compare function and generic struct boundaries across both parsers.
- [ ] M0.2.06. **modules.** Resolve an imported declaration from examples/modules/main.ae through the new parser.
- [ ] M0.2.07. **recover after bad token.** Inject a missing delimiter before a valid declaration and require bounded recovery.
- [ ] M0.2.08. **switch CLI route.** Run the CLI corpus through both routes before removing the old default.

Integration gate: Differential corpus matches and malformed files terminate.

## M1.1 Resolution

Entry dependency: M0.2.

- [ ] M1.1.01. **Intern symbols.** Intern repeated spellings to one symbol while retaining separate occurrence spans.
- [ ] M1.1.02. **allocate definition IDs.** Assign distinct IDs to same-named items in different modules.
- [ ] M1.1.03. **collect declarations.** Resolve a call to a later declaration and reject duplicate definitions.
- [ ] M1.1.04. **resolve imports.** Resolve a renamed import and flag two imports binding the same local name.
- [ ] M1.1.05. **detect cycles.** Render the closed dependency path for a three-module cycle.
- [ ] M1.1.06. **resolve locals and shadowing.** Resolve an inner binding to itself and restore the outer binding on exit.

Integration gate: Forward references and ambiguous names have specified outcomes.

## M1.2 Types

Entry dependency: M1.1.

- [ ] M1.2.01. **Intern types.** Canonicalize repeated vector types while keeping different element types distinct.
- [ ] M1.2.02. **infer locals.** Infer a local from its initializer and point at an incompatible reassignment.
- [ ] M1.2.03. **unify calls.** Bind repeated parameters consistently and reject conflicting argument types.
- [ ] M1.2.04. **occurs check.** Reject an equation requiring a finite type to contain itself.
- [ ] M1.2.05. **type return paths.** Diagnose the branch that returns an incompatible value.
- [ ] M1.2.06. **reject unconstrained variables.** Require an annotation when an empty constructor leaves its element type unknown.

Integration gate: Type dumps stable across repeated builds.

## M1.3 Generics

Entry dependency: M1.2.

- [ ] M1.3.01. **Bind parameters.** Keep a nested type parameter distinct from an outer one with the same spelling.
- [ ] M1.3.02. **collect obligations.** List the required operations of a bounded generic call.
- [ ] M1.3.03. **check generic body.** Reject an unused generic function that invokes an operation absent from its bounds.
- [ ] M1.3.04. **test overlap.** Reject two applicable implementations and identify both declarations.
- [ ] M1.3.05. **canonicalize instances.** Emit one instance for two call sites using the same concrete arguments.
- [ ] M1.3.06. **limit recursion.** Stop expanding a growing type chain and print the instantiation trace.

Integration gate: Invalid body rejected even when unused, repeated instances share code.

## M1.4 Patterns and closures

Entry dependency: M1.2.

- [ ] M1.4.01. **Lower destructuring.** Bind fields from a nested variant once in declaration order.
- [ ] M1.4.02. **check exhaustiveness.** Report a concrete uncovered variant and detect a redundant catch-all arm.
- [ ] M1.4.03. **classify captures.** Distinguish read, write and move captures in separate fixtures.
- [ ] M1.4.04. **infer closure environment.** Inspect environment fields and reject a returned closure borrowing a dead local once loan checks land.
- [ ] M1.4.05. **lower invocation.** Call a closure twice when permitted and reject a second call after consuming a move-only capture.

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

- [ ] M3.1.01. **Build core without allocator.** Link a scalar program without allocator symbols and inspect its imports.
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

- [ ] M3.3.01. **Parse manifest.** Report a malformed dependency at its manifest location.
- [ ] M3.3.02. **path dependencies.** Resolve a sibling package relative to its manifest from a different working directory.
- [ ] M3.3.03. **version constraints.** Check inclusive and exclusive endpoints with pinned version fixtures.
- [ ] M3.3.04. **solve graph.** Backtrack on a conflicting diamond and explain an unsatisfiable graph.
- [ ] M3.3.05. **lock hashes.** Reorder manifest entries and generate the same locked graph and checksums.
- [ ] M3.3.06. **offline build.** Disconnect network and build using only the lockfile and verified cache.
- [ ] M3.3.07. **verify downloaded content.** Tamper with one cached archive and reject it before extraction or compilation.

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
