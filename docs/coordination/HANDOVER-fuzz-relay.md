# Handover — fuzz relay

- Date: 2026-11-21 15:27 +08:00
- Branch: `agent/fuzz-relay`
- Worktree: the dedicated fuzz-relay worktree of this checkout
- Area claim: docs

## What is done

- Recorded seven original log prefixes in the implementation ledger and verified output from the restarted evaluate-3, ai and Ubuntu WSL lanes.
- Identified overlapping segments. The Windows one-hour process was kept and the duplicate 3600-hour process was stopped.
- Continued evaluate-2 at 622800, eva at 409536 and evaluate-1 at 515799 after their terminal summaries. Windows resumed at 953000 with a fresh log.
- Added the full frontend differential gate, stabilized generated output and routed the compiler's impl-body parameter handling through the shared method declaration tree (`f8a234a`).
- Continued evaluate-3 at 722356 and ai at 808862 after their zero-failure terminal summaries.
- Ubuntu completed the bounded 929886–949999 segment with zero failures. The next one-hour segment starts at 1000000 and stops before 1100000. Keep Windows below 1000000.
- Bounded statement-tree nesting and switched loop jumps to shared statement nodes before attaching ownership cleanup and loop targets.
- The Windows replay completed all 1,080 missing seeds. The 953000 segment then completed 4,969 seeds with zero failures and continued at 957969.
- Module assembly and resolution now share the entry token stream in `emit-resolve`. The same entry points accept the compiler's existing token slice.
- Literal `print` statements and boolean-literal `assert` statements now lower from shared statement and expression nodes.
- Extended shared lowering to scalar operator trees, using the existing prefix and infix type checks.
- Scalar local reads in shared output trees use resolver definitions that match active compiler slots.
- Scalar binding initializers and return values now use shared expression nodes, including resolved local reads.
- Scalar branch and loop conditions now use shared expression nodes and the existing boolean type check.
- The general expression entry lowers outermost scalar expressions at semicolons, commas, closing parentheses, closing brackets and range separators.
- Nested scalar lowering uses the enclosing expression budget. A call with 128 nodes succeeds and one with 129 retains the existing diagnostic.
- Shared expression lowering now covers string literals and literal-only string operations.
- Bare owned local reads and moves now use resolver definitions and the compiler's existing borrow checks. Compound owned expressions still use typed lowering.
- Safe direct calls to same-module functions with scalar arguments and scalar or string results now lower through resolver definitions and compiler signatures.
- Shared direct calls now accept owned string temporaries and borrow string views from temporaries or resolved locals.
- Shared direct calls stage owned string moves and deferred view conflicts until the enclosing expression lowers successfully.
- All seven lanes were restarted from the next seeds after 93,428 seeds completed with zero differential failures. See the ledger for the ranges.
- evaluate-2 resumed at 634915 after completing 12,115 seeds with zero failures.
- eva resumed at 416112 after completing 6,576 seeds with zero failures.
- evaluate-1 resumed at 527527 after completing 11,728 seeds with zero failures.

## What is half-done

- The remaining active runs still need terminal summaries. Reconcile unique seed IDs across logs before crediting coverage.
- The Ubuntu 916600 relay ended without a terminal summary. Its contiguous observed prefix is recorded in the ledger.
- ADR-0229 slice 5 still needs broader statement, call, and ownership-sensitive lowering before M0.2.08 can close.

## Next step

```powershell
ssh instabox-evaluate-3 'tail -n 20 ~/ae/bench/results/fuzz-segment-start745647.log'
wsl -d Ubuntu -- bash -lc 'tail -n 20 ~/ae/bench/results/fuzz-segment-start1066059.log'
Get-Content ../../ae/bench/results/2026-11-21-distributed-campaign/windows-baseline-start970018.log -Tail 20
cargo test --workspace --locked
```

## Evidence

- The original log prefixes contain 57,351 distinct seed verdicts, with zero differential failures and zero environmental verdicts.
- See the November 21 distributed regression campaign entry for lane starts and initial counts.
- Windows and Ubuntu WSL each passed 482 workspace tests and matched the pre-method compiler byte for byte on 50 generated C examples and 646 CLI invocations.
- Both platforms passed 483 workspace tests and byte parity on the same corpus after the statement depth and loop-jump changes.
- Both platforms passed the same suite and corpus after token-stream reuse. Windows also matched the previous resolver's output on eight `emit-resolve` inputs, including a module with imports.
- Both platforms passed 483 tests and the full frontend corpus after literal statement lowering. Windows also matched six focused inputs covering supported literals and malformed statements.
- Scalar operator lowering passed 483 tests and the full corpus on Windows and Ubuntu. Windows also matched 27 focused invocations, and the Ubuntu candidate passed a 40-seed differential smoke run.
- Resolved scalar reads passed 484 tests and the full corpus on both platforms. Windows matched 24 additional focused invocations, and Ubuntu candidate fuzz covered 40 more seeds without failures.
- Shared scalar initializers and returns passed 484 tests and the full corpus on both platforms. Windows matched 32 focused invocations, and Ubuntu candidate fuzz passed 40 seeds.
- Shared scalar conditions passed the same 484 tests and corpus on both platforms, 28 focused Windows comparisons and 40 candidate fuzz seeds on Ubuntu.
- The general expression route passed 484 tests and the same full corpus on both platforms. Windows matched 28 focused assignment, index, range, call and complexity comparisons. Ubuntu candidate fuzz passed 40 seeds.
- Nested scalar lowering passed 484 tests and the full corpus on both platforms. Windows matched 16 focused invocations, including the 128- and 129-node cases.
- Shared string expressions passed 484 tests and the full corpus on both platforms. Windows matched 24 focused comparisons. Ubuntu generated C passed the 41 sanitizer/leak checks.
- Bare owned locals passed 484 tests and the full corpus on Windows and Ubuntu. Windows matched 24 focused comparisons, including use-after-move and deferred borrow cases. Ubuntu passed all 41 generated-C sanitizer/leak checks.
- Direct calls passed 484 tests and the full corpus on both platforms. Windows matched 28 focused calls and diagnostic cases. Ubuntu candidate fuzz passed 40 seeds.
- Ubuntu generated C passed 41 sanitizer/leak cases with zero leaked blocks.

## Risks and blockers

- Multiple lanes replay seed ranges. Terminal summaries establish completion and elapsed time.
- Ubuntu is the validation distribution. The default docker-desktop distribution fails to mount its VHD.
- The original Windows log was replaced while its producer was live. Use the earlier checkpoint and the completed bounded replay to reconcile coverage.

## Files touched

- `docs/implementation-ledger.md`
- `AGENT_PROGRESS.md`

## Latest validation

- String call arguments passed 484 tests, 50 generated C example comparisons, and 646 CLI comparisons on Windows and Ubuntu. Windows matched 24 focused invocations. Ubuntu passed 41 generated-C sanitizer and leak checks.

- Staged string moves passed 484 tests, 50 C examples, 646 CLI invocations, and 28 focused comparisons on each platform. Ubuntu passed 41 sanitizer and leak checks.

- Deferred string view moves passed the same Windows and Ubuntu corpus and focused checks. The general expression entry now consumes indexed expression roots from the resolver module.

- Resolver expression roots passed 484 tests, 50 generated C examples, 646 frontend CLI invocations, and 28 focused comparisons per platform. Ubuntu passed 41 sanitizer and leak checks and 40 candidate fuzz seeds.

- Indexed output statements and loop jumps passed 484 tests, 50 generated C example comparisons, 646 CLI comparisons, and 32 focused comparisons per platform. Ubuntu passed 41 sanitizer and leak checks. The resolver still parses opaque closure bodies during its walk. Exposing those bodies to compiler lowering is the next frontend step.

- The 15:27 batch covered 78,348 distinct seeds with zero differential failures and zero environmental verdicts. Active successors are eva 429299 (PID 3614912), evaluate-1 550709 (3338406), evaluate-2 658941 (1679986), evaluate-3 745647 (343010), ai 818844 (418711), Windows 970018 (5512, exclusive ceiling 1000000), and Ubuntu 1066059 (guest 913946, exclusive ceiling 1100000). Use the Windows baseline log named above. The separate Windows 970018 probe is excluded.

- The resolver now retains closure statement trees. The compiler uses the same index for regular and closure bodies. Both platforms passed the full frontend corpus and 12 focused closure comparisons, and Ubuntu passed 41 sanitizer and leak checks. Next, lower numeric vector literals from shared expression nodes.

- Numeric vector literals now lower from the resolver tree. Both platforms passed the full frontend corpus and 40 focused comparisons, including valid and invalid complexity-boundary cases. Ubuntu passed 41 sanitizer and leak checks and 40 candidate fuzz seeds. Next, lower resolved numeric array and vector index reads.

- Resolved numeric index reads passed the full frontend corpus and 32 focused comparisons on each platform. Ubuntu passed 41 sanitizer and leak checks and 40 candidate fuzz seeds. Next, share the cached resolver index with closure subparsers that use the same token stream.

- The lazy resolver index is now shared by closure subparsers and same-source generic instantiations. Both platforms passed the full corpus and 32 focused comparisons. Ubuntu passed 41 sanitizer and leak checks. Next, lower numeric vector values and moves at direct-call boundaries.

- Direct calls with numeric vector value parameters or returns now lower from shared trees. Both platforms passed the full corpus and 36 focused comparisons. Next, lower borrowed numeric vector arguments through the resolver.

- Read-only vectors and fixed arrays now lower through resolved borrow nodes. Both platforms passed the full corpus and 44 focused comparisons. Ubuntu passed 41 sanitizer and leak checks. Next, carry statement-scoped struct borrow effects through shared calls.

- Read-only struct arguments now stage statement-scoped borrows during shared call lowering. Both platforms passed the full corpus and 36 focused comparisons. Next, stage mutable struct arguments and preserve deferred view-conflict ordering.

- Mutable struct arguments now lower from shared call nodes. Both platforms passed byte parity and 40 focused comparisons, including mixed move and mutate conflict order. Ubuntu passed 41 sanitizer and leak checks. Next, route owned struct value arguments and returns through the same shared call path.
- At 16:17, evaluate-1 PID 3338406 could not be observed because SSH closed the connection. Recheck that PID and its terminal log before launching the next segment.
