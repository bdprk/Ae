# ADR-0193: Borrow-Region Liveness Along CFG Edges

## Status

Implemented (Windows-verified: the full `cargo test --workspace --locked` suite — 205 tests — plus the 137-file run-suite/examples sweep, all clean; Linux coverage rides the CI matrix on the next push).

## Context

ADR-0192's move walk gave the CFG flow-sensitive move states. The borrow checker's other half is borrows: the parser registers them with two lifetimes — **persistent** (string views, token cursors, `for`-in containers, `match` subjects) lasting until their scope or loop ends, and **call-argument** borrows lasting one statement — and enforces writes/moves against its registers in the same single pass. The graph needs the same facts as regions: a write to a variable whose borrow region covers the write's block is the rejection the checker exists to make.

## Decision

**Persistent borrows are computed per block to a fixed point with union meets.** A borrow is live on entry to a block if *any* incoming path keeps it live — the parser's registers are scope-based, and a scope encloses both branch paths, so union (not the move walk's intersection) is the faithful meet. Introductions are recognized from the statements the tree carries: `let v = slice_view(base, ..)` / `tokenize(base, ..)` bind a view whose base is borrowed (recognized through the `Declare` init expression, including through field reads and concat operands), `for` borrows its container, `for-tokens` its base, `match` its subject.

**The pass is verification-only in this slice**: it computes the regions and panics on nothing — the parser's registers enforce — so the write-through-borrow checks can be developed against computed regions (the next slice's rejections) instead of being woven into the parser's single pass. Running inside every compile, it doubles as a regression tripwire: a lowering change that breaks region computation surfaces on the suite immediately.

**Loop back-edges keep regions live** through the fixed point, which is exactly the parser's "the container is borrowed for the whole loop" rule expressed on the graph.

## Evidence

Re-runnable:

- `cargo test --workspace --locked` — 205 passing.
- Manual sweep: all 137 run-suite and example files compile (the walk runs inside every compile and never fires).

## Consequences

- The write-through-borrow rejection (the parser's "cannot modify while borrowed" rule, including the call-borrow statement scoping) can now be derived from regions and compared against the parser the way the move walk is.
- Known gaps, stated rather than implied: call-argument borrows are not yet region-tracked (they are statement-scoped and the parser enforces them within that statement); region kills at scope ends are implicit (the tree has no scope-exit markers distinct from the block structure, so regions run to the end of the enclosing loop/function — conservative in the parser's own direction).
- Next: the write-through-borrow check over these regions, or MIR places/rvalues.
