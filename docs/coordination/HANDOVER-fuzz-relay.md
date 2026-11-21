# Handover — fuzz relay

- Date: 2026-11-21 12:40 +08:00
- Branch: `agent/fuzz-relay`
- Worktree: the dedicated fuzz-relay worktree of this checkout
- Area claim: docs

## What is done

- Recorded seven original log prefixes in the implementation ledger and verified output from the restarted evaluate-3, ai and Ubuntu WSL lanes.
- Identified overlapping segments. The Windows one-hour process was kept and the duplicate 3600-hour process was stopped.
- Continued evaluate-2 at 622800, eva at 409536 and evaluate-1 at 515799 after their terminal summaries. Windows resumed at 953000 with a fresh log.
- Added the full frontend differential gate, stabilized generated output and routed the compiler's impl-body parameter handling through the shared method declaration tree (`f8a234a`).
- Continued evaluate-3 at 722356 and ai at 808862 after their zero-failure terminal summaries.
- Resumed Ubuntu at 929886 from the last contiguous verdict. The successor is capped at 949999.
- Bounded statement-tree nesting and switched loop jumps to shared statement nodes before attaching ownership cleanup and loop targets.
- The Windows replay completed all 1,080 missing seeds. The 953000 segment then completed 4,969 seeds with zero failures and continued at 957969.
- Module assembly and resolution now share the entry token stream in `emit-resolve`. The same entry points accept the compiler's existing token slice.
- Literal `print` statements and boolean-literal `assert` statements now lower from shared statement and expression nodes.
- Extended shared lowering to scalar operator trees, using the existing prefix and infix type checks.

## What is half-done

- The remaining active runs still need terminal summaries. Reconcile unique seed IDs across logs before crediting coverage.
- The Ubuntu 916600 relay ended without a terminal summary. Its contiguous observed prefix is recorded in the ledger.
- ADR-0229 slice 5 still needs typed lowering over resolved statements and DefIds before M0.2.08 can close.

## Next step

```powershell
ssh instabox-evaluate-3 'tail -n 20 ~/ae/bench/results/fuzz-segment-start722356.log'
wsl -d Ubuntu -- bash -lc 'tail -n 20 ~/ae/bench/results/fuzz-segment-start929886.log'
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
- Ubuntu generated C passed 41 sanitizer/leak cases with zero leaked blocks.

## Risks and blockers

- Multiple lanes replay seed ranges. Terminal summaries establish completion and elapsed time.
- Ubuntu is the validation distribution. The default docker-desktop distribution fails to mount its VHD.
- The original Windows log was replaced while its producer was live. Use the earlier checkpoint and the completed bounded replay to reconcile coverage.

## Files touched

- `docs/implementation-ledger.md`
- `AGENT_PROGRESS.md`
