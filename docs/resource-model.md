# Resource Model (Formal Specification)

This document is the **specification** for ownership and deallocation: it describes what the current implementation actually guarantees, where checks are performed, and which properties are not yet guaranteed. Any behavior inconsistent with this document is a defect; for any property marked "not guaranteed," the implementation must not imply its existence in documentation or external communications. See [ADR-5/6/8](decisions.md) for design motivations.

## 1. Classification of Values

| Category | Type | Storage | Copy Semantics |
| --- | --- | --- | --- |
| Scalar | `i64`, `bool`, `f64` | Stack | Copy |
| Owned | `string`, `vec`, `vec[string]`, `vec[f64]`, `map[string, i64]`, `map[string, f64]`, struct, enum | Stack handle + Heap buffer | Move (**No implicit copy**) |
| Read-only Borrow | `&string` (view), `&vec`, `&vec[string]`, `&vec[f64]`, `&Struct`, `&[i64; N]` | Stack pointer + length | Read-only alias, never deallocated |
| Exclusive Borrow | `&mut Struct` (parameter or `&mut self`, ADR-0185) | Stack pointer | Write-through for the call, exclusive while live |
| Fixed Array | `[i64; N]` | Stack (inline) | Cannot be moved or copied as a whole |
| Closure / `fn(T) -> R` | closure literal, function-typed parameter (ADR-0188/0189) | No storage (a name for a function id) | Not a value: cannot be stored, returned, or reassigned |
| Raw Pointer | `*const T`, `*mut T` (M2.12) | Stack address | Copy, comparable; **untracked** — no ownership, no borrows |

The criterion for "Owned" is `Type::owned()`: it determines exactly three things—whether assignment/passing moves the value, whether deallocation is inserted upon scope exit, and whether use-after-move is rejected. New types must define these three semantics simultaneously.

## 2. Transfer of Ownership

Moves occur at the following syntactic positions, and nowhere else:

1. `let x = e;` / `x = e;` where `e` evaluates to an owned **bare variable** (e.g., `let b = a;`).
2. Pass-by-value arguments (where the parameter type is owned): `f(a)`, `f("literal")`, `f(a + b)`.
3. `return e;` (where `e` is an owned bare variable or literal).
4. Initializer expressions for owned fields in struct literals.
5. `string`/`vec`/`vec[string]` payloads in enum construction.
6. `map` keys are **copied** by the runtime upon insertion (not moved: the caller retains their own string).

**Temporary owned values** produced by literals and built-ins like `+`, `slice`, or `read_file` are deallocated at the end of the statement that consumes them (`drop_owned_temp`); temporary values passed to view parameters are deallocated immediately after the call returns.

## 3. State After Move

- **Compile-time**: Each function maintains a `moved: HashSet<var_id>`. Accessing, reading, borrowing, accessing fields/elements of, or using as a receiver a variable that has been moved will trigger an error. Reassignment (`x = e;`) clears this flag because the owned target is deallocated before the write, which is equivalent to re-initialization. The move set is tracked by variable ID and **does not distinguish between fields**: once an entire struct is moved, none of its fields can be used.
- **Runtime**: Deallocation and "hand-off" of `string`/`vec`/`vec[string]`/`map` clear the handle (setting `ptr`/`data`/`slots` to NULL and length to zero). A null-pointer check is inserted before every read; if triggered, it reports `use of moved <type>` and terminates. This fallback covers paths missed by compile-time checks (e.g., moving then using within a loop).
- **Structs and enums have no runtime sentinels**: They rely solely on compile-time checks. If move tracking across functions or loops is incomplete, erroneous use of a struct will not be caught at runtime—this is a known gap, not a design goal.

## 4. Borrowing

Borrowing is a read-only alias with no independent storage. Two mechanisms coexist since M2.5 (ADR-0205): borrows are *registered* as `(variable id, scope depth)` and reclaimed at block end — that registry drives the on-sight rejections (moves, writes, container mutation while borrowed) — while *deferred* conflicts between persistent views and whole-variable writes/moves are decided by the CFG's NLL last-use region for the view:

- Sources: `&string`/`&vec`/`&vec[string]`/`&vec[f64]`/`&[i64; N]`/`&Struct` parameters (parameters are borrows within the function body), generic `&T` parameters (ADR-0184 family), `slice_view` views, `match` bindings, loop variables in `for x in v`, **borrow captures of a closure literal** (a persistent borrow on the outer variable for the closure's live range, ADR-0188), and built-in reads of string/vector arguments.
- `&mut Struct` (ADR-0185) is the exclusive form: the callee writes fields through the pointer for the duration of the call; the lender must be a `var` bare struct variable, no other borrow may overlap the call, and a `&` borrow never upgrades to `&mut`. A `&mut` lending call is a release site for the viewed bytes of that struct (see below).
- Borrows are recorded as `(variable id, scope depth)`; at the end of a block, borrows established within that block are reclaimed based on depth.
- Prohibitions during a borrow: `push`/`pop`, index writes, field writes, pass-by-value, `return`, or establishing expressions that would move the variable—all report `cannot assign/modify/move ... while it is borrowed`. Determinations are based on **variable ID**, so writing to any field of a struct is blocked by the "borrow exists on struct" check (conservative but safe).
- **Whole-variable writes to a view's base are flow-sensitive since M2.5 L1 (ADR-0205)**: the parser defers the conflict instead of rejecting it, and the CFG's NLL region for the view decides — since M2.5 L2 the places are field-granular: a field write only overlaps a view of that very field, so `p.note = "y";` while `p.name` is viewed is allowed. A write or move is rejected (`cannot assign/modify/move to '...' while a view binding of it may still be read`) only when the view was introduced before the write on every path (its introduction dominates the write) and can still be read later on some path. A write after the view's last use is allowed; a view created after the write reads the new bytes and never conflicts. Moves and the other borrow prohibitions keep the on-sight rejections (later L1 slices).
- **Views bind once since M2.5 slice 4 (ADR-0205)**: view slots are declare-only — the NLL regions assume no kills, so a view binding cannot be mutable and cannot be reassigned (both rejected at parse time). The soundness probe showed a reassigned view both overstates the old region (false positives) and silently escapes the recorded base (freed-memory reads); the invariant is now enforced by the parser.
- **`&mut` lending calls are release sites since M2.5 (ADR-0205)**: a call whose receiver or argument lends a struct mutably may write any field, so it releases the viewed bytes of that struct at the call point. When only persistent string views are active on the base, the same NLL rule decides: allowed after the view's last use, rejected while it may still be read. The language pins left-to-right evaluation, so within one statement a view read before the call sees the bytes intact and a read after the call is rejected (the ordered release walk). Same-statement call-argument or `&mut` borrows keep the on-sight rejections. A deferral is only sound when the graph's signature scan can see the callee (free functions and trait impls, inherent methods, generic instances); a callee outside those tables keeps the on-sight rejection.
- Borrows must not escape: They cannot be returned, nor stored in a `let` (e.g., `cannot store a borrowed vector in a variable`); thus, the lifetime of a borrowed value cannot exceed the block in which it was created. This is the **sole** reason why lifetime parameters are currently unnecessary.
- Struct fields and vector elements **can never be moved out** (`cannot move a field out of a struct` / `cannot move an element out of a vector`): they can only be borrowed as a whole or read in-place. A field **write** consumes an owned variable on its right side — the field owns the bytes after the write, the source is forfeited, and a field read on the right side is the field-move rejection above (copy with `slice` first).

## 5. Deallocation Points

Deallocation of owned values is **deterministic and idempotent** (handles are cleared after deallocation, making subsequent deallocations no-ops):

| Location | Content Deallocated |
| --- | --- |
| End of block | Owned variables declared in that block (in order of declaration) |
| Normal function exit | Owned parameters in the parameter scope |
| `return e;` | All live owned variables in the function, except the one moved out via `e` |
| Error path of `?` | Same as above: all live owned variables, then return the result enum as-is |
| `break`/`continue` | Owned variables declared inside the loop body (live values outside the loop are unaffected) |
| End of each loop iteration | Owned variables declared within the loop body block (same as a normal block) |
| Overwrite assignment `x = e;` | The old value of `x` (any owned target) |
| Struct field assignment / Vector element overwrite | The old value being overwritten |
| Enum whole-assignment, after `match` | Old payload / payload deallocated according to the current variant |
| Program exit (end of `main` block) | Local owned values in `main` |
| `exit(code);` | All live owned variables (flush stdout, then deallocate, then terminate) |

`forfeit` is a paired operation that "clears the slot without deallocating" after a hand-off, used when moving a value from one slot to another (e.g., moving a whole struct, taking an `Ok` payload via `?`, or moving out an enum payload).

## 6. Termination Semantics

- Runtime errors (out-of-bounds, overflow, division-by-zero, use-after-move, allocation failure, UTF-8 boundaries) uniformly trigger `ae_fail`: prints `ae:line:col: message` and exits with status code 1.
- **Termination does not guarantee cleanup**: There is no destructor stack unwinding; buffers not deallocated upon process exit are reclaimed by the OS. This is consistent with §4.5 "Abnormal termination does not guarantee cleanup" and is the cost of "not introducing unwind."
- Allocation failure is terminal, except for built-ins that return values (e.g., `try_*` returns `Err("out of memory")`); see [ADR-16](decisions.md).
- There are no `panic`/`catch`/destructors, nor recoverable runtime errors: recoverable errors must be expressed via `Result`-shaped enum values ([ADR-11](decisions.md)).

## 7. Concurrency and `unsafe`

The current language has **no** concurrency, threads, or shared mutable state, so "sendable/shareable" constraints are not required: any owned value lives within a single thread. Introducing concurrency will require extending this document (defining which types are `Send`, which are `Sync`, and how locks interact with borrows) before implementation.

`unsafe` exists since M2.12 (`spec/unsafe.md`, ADR-0207): `unsafe {}` blocks, `unsafe fn`, raw pointers, `extern "C"` declarations/exposures, and `#[repr(C)]`. The boundary does not weaken this model — it sits beside it:

- Raw pointers are untracked addresses: creating one from a reference is safe, reading or writing through one requires `unsafe`. They carry no ownership, so the move set and the borrow registry never see them.
- Dereferencing into the tracked world (binding the pointee back to an owned variable) re-enters the checks of this document from that point.
- The declaration-time guards are the safe-code shield: a pointer to an owned value is rejected (its borrow is untracked), so a dangling deref cannot be spelled in safe code; the interpreter's Moved-slot trap is the armed backstop for when pointer-to-owned lands.
- Owned values crossing `extern "C"` are rejected in both directions (the C side cannot honor their release logic); `#[repr(C)]` structs — which are the legal crossing shape — carry no owned fields for the same reason.

## 8. Explicitly Non-Guaranteed Properties

- No cross-function lifetime checking: `&string` parameters cannot be stored, so escaping is syntactically impossible, but the compiler does not "prove" this; it relies on the prohibition against storing borrows in variables or return values.
- No alias analysis: A single string may have multiple views, all read-only; as long as a view exists, writes to the base variable are prohibited, but views are not mutually exclusive.
- No partial moves: Cannot move out a single field of a struct.
- No loop fixed-point iteration: As an alternative, **the loop body must not move owned values declared outside the loop**, unless they are re-initialized before the end of the loop body. This rule eliminates the entire class of "reading a moved value in the second iteration" errors, at the cost of rejecting some actually safe patterns (e.g., moves at the end of a loop body). Diagnostics point to the loop keyword; see [ADR-20](decisions.md).
- No runtime sentinels for structs/enums; the move set at branch merges is a conservative union (a move on any path is treated as a move).
- Deallocation order is the order of declaration, not reverse order; therefore, **do not** rely on deallocation order across variables (currently, all deallocations are independent).
- Memory safety currently relies on: compile-time move/borrow checking + runtime handle sentinels + leak testing via allocation counts. These are all covered by tests but **do not constitute** a formal proof.

## 9. Checklist for Adding New Features

When adding any feature involving values to the language, answer each of the following items one by one:

1. What is the `owned()` behavior for the new type? Have the three aspects—move, drop, and use-after-move check—all been implemented?
2. Does the new move position need to update `live_owned()` (the drop manifest for returns/`?`)?
3. Do new statement forms insert drops across all jump paths (block termination, `return`, `?`, `break`/`continue`, `match` arms)?
4. Are new borrowing forms registered in `borrows` and reclaimed at the end of the block?
5. Do new builtins drop temporary owned values when consuming or borrowing them (`drop_owned_temp`)?
6. Do failure paths (including allocation failure) leave no dangling handles, just like success paths?
7. Is the block count for leak verification (`scripts/verify_generated_c.py --leak`) still equal to the baseline?
8. Have the documentation files (this document and `current-language.md`) been