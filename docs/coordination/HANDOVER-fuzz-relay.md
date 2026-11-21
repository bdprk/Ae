# Handover — fuzz relay

- Date: 2026-11-21 20:39 +08:00
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
ssh instabox-evaluate-3 'tail -n 20 ~/ae/bench/results/fuzz-segment-start779893.log'
wsl -d Ubuntu -- bash -lc 'tail -n 20 ~/ae/bench/results/fuzz-segment-start1127538.log'
Get-Content ../../ae/bench/results/2026-11-21-distributed-campaign/windows-baseline-start982506.log -Tail 20
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

- Owned struct value arguments and returns now lower from shared calls. Both platforms passed the full corpus and 40 focused comparisons. Next, lower struct constructor nodes from the resolver tree.

- At 19:25, seven terminal logs covered 78,897 seeds with zero differential or environmental failures. Active starts and PIDs: eva 435878 (1342854), evaluate-1 562107 (3693383), evaluate-2 670902 (2051232), evaluate-3 757011 (686040), ai 823803 (816250), Windows 975869 (34036), and Ubuntu 1092844 (guest 11). The evaluate-3 and ai continuations are the next expected to finish.

- evaluate-3 and ai completed a further 16,254 seeds without failures. Their verified successors are 768241 (PID 1026848) and 828827 (PID 1219720).

- Shared lowering now handles named struct constructors. Both platforms passed the full corpus and 36 focused comparisons. Next, consume field access nodes and preserve the compiler field path semantics. The Ubuntu relay remains bounded at 1100000.

- Scalar field reads now lower from shared postfix trees. Next, handle read-only owned-field expressions while preserving the original move diagnostics. Ubuntu reached its ceiling with 7,156 additional successful seeds and resumed at 1100120 (guest PID 14, host PID 26140), stopping before 1200000.

- Shared string field reads passed the full corpus and 36 focused comparisons on both platforms. Next, consume resolved enum constructor nodes. The seven active relay PIDs were verified live.

- Concrete enum constructors passed the full frontend corpus and 36 focused comparisons on both platforms. Next, handle enum values at shared direct-call boundaries.

- Enum value calls passed the full byte gate and 32 focused comparisons on both platforms. Next, connect shared lowering at the match-subject boundary while preserving pattern ownership.

- Match subjects and arm patterns now reuse the shared index. The full byte gate and 36 focused comparisons passed on both platforms. Next, feed trailing propagation nodes into the typed propagation route.

- Shared propagation passed the full corpus and 36 focused comparisons. Next, consume indexed for-loop operand nodes while preserving iteration mode and ownership.

- Indexed for-loop operands passed the full corpus and 36 focused comparisons. The diagnostic binding-range map retains its prior scope. Next, consume fixed-array initializer trees.

- Fixed-array initializers passed full corpus and focused parity on both platforms. Next, consume indexed assignment targets and preserve the existing write checks. The three early remote lanes and the Windows lane should be checked for completion around 20:25.

- Indexed store indices and keys passed 36 focused comparisons and the full byte gate on both platforms. Next, consume shared field target nodes for typed stores.

- The 20:25 batch recorded 36,698 seeds and zero failures. Verified successor starts and PIDs: eva 442316 (3642392), evaluate-1 573904 (4049278), evaluate-2 682728 (2410459), and Windows 982506 (12116). Next terminal checks are evaluate-3 768241 (1026848) and ai 828827 (1219720) around 20:29.

- Direct field stores now use indexed target names. Next, route pointer-store targets from the statement tree while preserving unsafe checks and existing diagnostics. The 20:29 batch covered 21,260 successful seeds, with verified successors 779893 (evaluate-3 PID 1381435) and 838435 (ai PID 1516911).

- Pointer-store target indexing passed the full corpus and 24 focused comparisons. Next, replace the remaining local statement dispatch for indexed output and jump nodes with shared statement kinds.

- Ubuntu finished 27,418 successful seeds at 20:39. Its verified successor starts at 1127538 (guest PID 14, host PID 9564), with an exclusive ceiling of 1200000.

- Output and loop jumps now select their lowering from shared statement kinds. Both platforms passed the full byte gate and 32 focused comparisons. Next, index shared let and var binding metadata for statement lowering.

- Indexed let and var headers passed full corpus parity and 32 focused comparisons. Next, use the shared for statement metadata for loop binding and iteration dispatch.


## Remaining typed-parse sites (audit 2026-11-21, relay continuation; framing corrected)

The 36 `self.expression(0, …)` sites in `parser.rs` are NOT "unshared" —
each consumes the shared tree indirectly: `expression()` consults
`lower_shared_value` at operand starts, which reads the resolver's
`values` map. The audit's real value is the per-construct location list
for the remaining typed-parse entry points whose operand shapes the
shared lowering does not cover yet. The authoritative work list is the
"Next:" pointer at the end of each handover bullet (the incremental
expansion of `shared_expression_start` and the `values` coverage), not
this site count.

Per-construct locations (for the next slices):
- concurrency statements: TaskYield ×2, PoolSpawn ×1, ChanClose ×2 — the
  chan_close handle now lowers through the shared route (relay continuation)
- stores (value sides): Store ×2, StrVecStore ×2, VecStore ×1,
  FieldStore ×6, Star (pointer store) ×1
- compound assignment: PercentEqual ×2; plain Assign ×2
- containers and builtins: Array ×1, VecPop ×3, MapPut ×2, Sort ×1
- control flow tail: condition ×1, Return ×1, For ×2,
  Assert ×1, Print ×1, match_statement ×1
- Declare ×1

Relay continuation (2026-11-21, the takeover agent): the `exit` builtin's
argument now lowers through `lower_shared_value` with an RParen boundary
(Windows 484/0, WSL2 484/0) — the Exit ×2 entry above is resolved, leaving
34 sites.
