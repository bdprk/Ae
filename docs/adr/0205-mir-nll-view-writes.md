# ADR-0205: View Borrows Die After Their Last Use (M2.5 L1, First Slice)

- **Date**: 2026-11-14
- **Status**: Implemented (Windows-verified; Linux coverage rides the CI matrix on the next push)
- **Milestone**: M2.5 L1 — "NLL: borrows die after their last use" (see `docs/borrow-checker-design.md` §2.4)

## Context

The parser registered every persistent borrow with a scope-based lifetime: a view binding (`slice_view`, `tokenize`) froze its base from the binding until the scope ended, and any write to the base in between was rejected on sight — `cannot assign to '...' while it is borrowed`. That rule rejects programs that are safe: a write after the view's last read releases bytes nobody can observe anymore. The design's adversarial matrix (§3.2/§3.1) pinned these exact shapes as the differential baseline an NLL relaxation must loosen, and ADR-0193's region pass plus the M2.4 drop pass provided the graph machinery to compute the true lifetime.

Two rules bound the slice:

- **Safety**: a write must stay rejected while the view can still be read later on some path — the write releases the bytes the view points at.
- **Precision**: a view created after the write looks at the new bytes, so it never conflicts; and a write is only on the path from an introduction to a read when the introduction dominates the write (a read without a dominating introduction is a definite-assignment rejection the parser already makes).

## Options

| Option | Pros | Cons | Resource Cost |
|---|---|---|---|
| Keep the scope rule | No new code | Rejects safe programs (the known false-positive class) | — |
| Runtime drop flags for views | Precise | Hidden state in generated C, slower than the branch it replaces; contradicts §2.5 (borrow safety entirely compile-time) | Runtime: flag per view |
| Defer the conflict to the graph; adjudicate with NLL regions | Precise and compile-time; reuses ADR-0193's regions and M2.4's move walk | Parser and checker now cooperate (deferral + adjudication plumbing) | Compile: one backwards-liveness pass per function |

## Decision

1. **Borrows carry a kind.** `BorrowKind::{View, Container}`: view-kind borrows hang off view bindings and get NLL regions; container-kind borrows (for-in containers, token-loop bases, match subjects, strvec element reads, closure captures) keep the scope rule and their on-sight rejections. The kinds cannot mix on one base except through a struct (a field view plus an element read of another field), and the deferral is refused whenever any container borrow is active on the base.
2. **The parser defers; the graph decides.** At the scalar whole-variable assignment, a conflict whose borrows are all view-kind is recorded (`base, offset, name`) instead of rejected. When the function is assembled, `borrowscope::adjudicate_deferred` builds the CFG, computes the view's NLL region (backwards liveness of the view slot — views are declare-only, so there are no kills), and rejects the write iff the view was introduced before the write (same block: earlier statement; otherwise the introduction block dominates the write's block) and may still be read (reads after the write in the block, or live on entry to any successor). Closures adjudicate against their own graph with the enclosing function's deferred list restored afterwards.
3. **Diagnostics keep their shape.** A surviving conflict surfaces as `cannot assign to '...' while a view binding of it may still be read` at the write's position. The verifier (`BorrowRegions::run`) shares the same coverage rule, so an accepted program can no longer disagree; the scope-region check stays for the container borrows.
4. **Enforcement scope.** Whole-variable writes, moves, and struct-field writes on view-borrowed bases are relaxed, and the places are field-granular (L2): a field write releases the written field's bytes only, so it overlaps a view of that very field and never a view of a sibling field — `p.note = "y";` while `p.name` is viewed is allowed. A whole-variable write or move re-homes every field's bytes, so it overlaps every field view. A move hands the buffer to a new owner that may die before the view does, so a move conflicts exactly when the view can still be read later on some path — the same coverage rule, with the statement's move effects (from the shared move walk) joining the release sites in the check. Element writes and container-borrow conflicts keep the on-sight rejections; nested fields (`a.b.c`) stay on the variable-granularity rule with an honest message (the design's "ambiguous source means rejection" trade-off).

## Evidence

- `cargo test --workspace --locked` — 278 tests, 0 failures, including three hand-built-CFG unit tests for the region math (conflict inside a view's lifetime; write after the last use outside the region; a loop container's scope region unchanged) and the flipped baseline cases: `tests/run/borrow_write_after_last_use.ae` (allowed, runs) and `tests/ui/borrow_write_during_view.ae` (the rejection keeps its position).
- `python scripts/diff_fuzz.py --seeds 40` — four-way differential, 0 failures; the fuzzer's view-heavy seeds now parse through the deferral path.
- `python scripts/verify_generated_c.py` — 41 PASS / 0 FAIL.
- `tests/run/borrow_move_after_last_use.ae` (allowed, runs) and `tests/ui/borrow_move_during_view.ae` (the move rejection keeps its position) pin the §3.3 pair, mirroring the write pair. The walk's Declare/Assign/Return arms now record their moves, so the drop pass elides those releases too.
- `field_granularity_borrows_freeze_their_own_field_only` pins the L2 trio: two field views of different fields coexist, a write to the unviewed field is allowed while both are dead, the viewed field's write while read later is rejected, and the whole-struct move while a field view is read later is rejected.
- Single-platform verification (Windows). The relaxation changes parser acceptance, not C templates; the sanitizer sweep adds nothing platform-specific here.

## Consequences

- The language accepts the write-after-last-use shape (the §3.2 baseline flips to a run-corpus allowance); `docs/current-language.md` and `docs/resource-model.md` record the rule.
- The NLL region computation is now load-bearing, not verification-only; the L1 move relaxation and field-granularity work (L2) build on the same regions.
- Revisit criteria: if a false negative is ever demonstrated (a write the region wrongly allows), the dominator + liveness combination is the first suspect; the unit tests pin both faces so the failure localizes.

## Slice 3 (2026-11-14): `&mut` lending calls are release sites

The mut receiver and `&mut` argument calls kept their on-sight rejection — the mutation happens inside the callee, invisible to the CFG walk. This slice brings them onto the graph:

1. **The signature table identifies the release sites.** A call whose receiver or argument lends a struct mutably (the callee's parameter type is `StructMut`) may write any field, so the call is a whole-variable release on the lent slot. The release scan reads the four tables every `Expr::Call` id can come from: free functions and trait impls (`functions`), inherent methods (`generics.inherent`), generic function instances (`fn_done`), generic method instances (`generic_impl_done`). Closure calls (`FnCall`) stay read-only: fn types cannot carry `&mut` (ADR-0189's bare-name parameter grammar), and borrow captures are read-only by construction (a capture the body writes is a value capture or is rejected — `closure_write_borrowed`).
2. **The parser defers when only view borrows are active.** Both deferral sites (the `&mut self` receiver on an owned variable, the `&mut name` argument) defer when `only_view_borrows_active` — the active borrows are all persistent views, no statement-scoped call or mut borrows, no containers. Same-statement mixes keep the pinned rejection (`mut_borrow_shared_call`). The deferral additionally requires the callee to be visible to the release scan (`signature_in_release_tables`): a deferral the graph cannot see would be an unsound allowance, so an unresolvable callee keeps the on-sight rejection.
3. **The adjudication is evaluation-order aware.** The language pins left-to-right operand and argument evaluation (current-language.md), and every backend hoists subexpressions in that order, so the release walk is a `MoveTracker` (`ReleaseTracker`) over the statement in evaluation order: a mut call kills the views of its lent base from that point on, and a view read after the kill is the conflict — while a read before the call sees the bytes intact. The released bases also join the statement's write targets, so the existing region rule (introduction dominates, live at the write) covers cross-statement reads; the block's **terminator** (an `if`/`while` condition, a `return`'s expression) is scanned the same way, with the release landing at the block's end and liveness read from the successors. The verifier (`BorrowRegions::run`) applies the identical scan, so the two faces cannot disagree.

**Evidence**: six unit tests in `borrowscope.rs` pin the covered/after-last-use/ordered/terminator/position faces on hand-built CFGs; the language corpus pins all six end-to-end faces — `tests/run/borrow_mut_call_after_last_use.ae`, `tests/run/borrow_mut_arg_after_last_use.ae`, `tests/run/borrow_mut_call_read_before.ae` (allowed, run) and `tests/ui/borrow_mut_call_during_view.ae`, `tests/ui/borrow_mut_arg_during_view.ae`, `tests/ui/borrow_mut_call_read_after_in_statement.ae` (rejected at the call site). 290 tests, 41/41 `verify_generated_c`, 40-seed four-way differential all green; Windows-verified, Linux rides CI.

**Fixed on the way**: the `MapPut` arm of the move walk still truncated with `?` — the value's walk was abandoned whenever the key's walk found nothing, the exact pattern the failure log documents for the `Binary` arm. Both operands walk now.

## Slice 4 (2026-11-14): views bind once — the declare-only invariant enforced

A self-built soundness probe broke the region math's core assumption: **`var h = slice_view(t, 0, 1); h = slice_view(s, 0, 3);` parses** — the view slot is reassigned to a different base. Two failure directions followed from the one gap:

- **Unsound allow** (demonstrated with freed-memory reads): the recorded view borrows only carry the *first* declare's base, so a write to the *reassigned* base is never covered — `s = "other"` while `h` views `s`'s bytes compiled, and the later `print(h)` read the released buffer.
- **False positive** (latent): with no kill points, the old view's region lives forever, so a write between two reassignments of one slot is rejected even though no view of it is live.

The minimal fix restores the invariant instead of building kill-point NLL (an L4+ option): a view binding is immutable — `var h = <view>` is rejected at the declaration ("a view binding cannot be mutable; declare it with 'let'"), and any assignment into a `StrView`/`Tokens` slot is rejected at the assignment ("a view binding cannot be reassigned; declare a new view with 'let'"). The cursor type is covered by the same checks. `let`-bound views were already protected by the immutable-binding rule; the slot check is the second line of defense for any path the assignment form can reach.

**Evidence**: the probe's exact shape is `tests/ui/view_rebind_mutable.ae` (rejected at the declaration) and `tests/ui/view_reassign_let.ae` (rejected at the write); the corpus holds no `var`-bound views, so nothing else changes. 290 tests, 60-seed four-way differential, 41/41 `verify_generated_c` all green; the 1,000-seed run over seeds 5000–5999 finished with 0 failures before this slice.
