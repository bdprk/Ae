# General Borrow Checker Design (Stage C Core Design Draft)

Status: **Designed** (2026-11-12); this document is a design, not an implementation commitment. Implementation and acceptance proceed according to the phases in §6, and every step must expand tests according to the checklist in `resource-model.md §9` before making any claims. The rules of evidence follow §22 of the proposal document.

## 0. What Questions This Document Answers

Section 4 of `resource-model.md` records the **current implementation**: borrows are block-scoped read-only aliases, tracked by variable ID, prohibited from escaping, and lacking field granularity. While safe, this set of rules has limited expressiveness—parameterized containers (`fn first(v: &vec[T])`), storing borrows inside structs, returning borrows, and mutable borrows are all unsupported.

The goal of Stage C is to upgrade the restricted rules into a **general borrow checker**, while maintaining three non-negotiable bottom lines:

1. **Every rejection in safe code must be explainable, and every allowance must be reliable** (false negatives are strictly disallowed, while false positives may be tightened first and relaxed later);
2. **Requiring no lifetime annotations from the user** is a goal, but not a pre-guarantee—if a class of programs requires annotations to be checked reliably, the design prefers adding annotations over relaxing checks;
3. Every relaxation must be preceded by adversarial test cases and rejection tests before touching the implementation (§20.4 Rule 1).

## 1. Current State Assessment (2026-11-12, Post-ADR-35/52/61)

| Capability | Current Status | Evidence |
|---|---|---|
| Read-only borrow parameters | `&string`, `&vec`, `&vec[string]`, `&vec[f64]`, `&[i64; N]` | Full test suite |
| Generic borrows | `&T` resolved via instantiation to one of the above four (ADR-35/36, ADR-61) | `generic_borrow.ae` |
| Field borrows | Variable granularity only: `slice_view(p.name, …)` borrows `p` as a whole, writing to any field is rejected (ADR-52) | `field_view.ae` |
| Borrow escape | Prohibited: cannot return, cannot store via `let`, cannot enter containers | `resource-model.md` §4 |
| Mutable borrows | Non-existent (container modifications are done via `push`/index assignment on owned values themselves) | — |
| Borrow tracking | `(variable id, scope depth)`, reclaimed at block end | `borrows` in `frontend.rs` |
| Lifetime annotations | None (un-borrowable escapes are the sole reason) | — |

**Known Conservative Points** (behaviorally safe but overly restrictive): Field writes are intercepted as a whole by the "presence of a borrow on the struct"; borrow granularity does not include fields and elements; the `slice_view` chain cannot distinguish between "borrowing `p.name`" and "borrowing `p`".

## 2. Target Model

### 2.1 Shape of a Borrow

```text
Loan  = { place: Place, mode: {Shared, Mut}, origin: Origin }
Place = Var(id) | Place.field(name) | Place.index(constrained, see 2.4)
Origin= StaticBlock(scope) | Outlives(parameter index)   // the latter is introduced only for escaping borrows
```

- **Shared**: Multiple can coexist; prohibits the establishment of Mut borrows and any writes during their lifetime.
- **Mut**: Exclusive; prohibits any other Loans (even reads) and movement of the base variable during its lifetime.
- The first version **only introduces Shared**; Mut is an independent phase (2.5) because exclusivity checks require modifying built-in modification paths (`push`/index assignment will shift to using Mut loans), which has a wide impact radius.

### 2.2 Constraints (The Core Alternative to Lifetime Annotations)

Escaping borrows introduce **ordered constraints** rather than named lifetimes:

- `Return(Borrowed(param p))` requires `origin(returned borrow) ⊇ origin(p)`—borrows can only be returned from entities that "live at least as long as a certain parameter";
- Borrowing into a struct field → that struct instance carries an origin, with construction, movement, and dropping all following the ownership mechanism, and dropping requires proving the absence of shared borrows beforehand (compile-time + drop-path assertions);
- **Users never write annotations**: Constraints are uniquely determined by "where the borrow came from", with no cases involving multiple selectable inputs. If a case arises in the future where two parameters could potentially serve as the source (e.g., `fn longer(a: &string, b: &string) -> &string`), **reject** the function—it requires explicit annotation choices or returning owned values. In ambiguous cases, we prefer rejection (Rust uses `'a` to make a choice, whereas we substitute rejection + owned values; this is a trade-off between usability and reliability, explicitly stated).

### 2.3 Flow Sensitivity and Merging

- Borrows die after their **last use** (NLL semantics), rather than at block end—this is the first step of relaxation, preventing "borrows inside loops that are used by the end of the loop body" from occupying the entire loop;
- Control flow join points take the **union** of active loans across branches; loop back-edges use fixed-point iteration, with an iteration upper bound plus non-convergence leading to rejection (conservative termination, with rejection messages specifying which loan failed to converge);
- Partial moves and field granularity share the same Place tree: moving field `a` of `s` only freezes the place `s.a` without affecting sibling fields—**provided** that every intermediate node on the place tree cannot have escaping aliases (container elements remain un-borrowable in the first version, see 2.4).

### 2.4 Expressiveness Ladder (Independently Accepted per Level)

| Level | New Capability | Prerequisite Evidence |
|---|---|---|
| L0 (Current) | Block-scoped read-only borrows | Existing |
| L1 | NLL: Borrows die after their last use | Differential testing: L1 allows the set of valid programs rejected by L0, output remains unchanged |
| L2 | Field-granularity loans: Borrowing `s.a` does not freeze `s.b` | Adversarial test cases + ASan/UBSan |
| L3 | Escaping borrows: Returning borrows (see constraints in 2.2), borrowing into structs | Lifetime correctness suite + drop-path leak/double-free tests |
| L4 | Element borrows (read-only borrows of container elements, `v[i]` views borrowing `v`) | Container drop-path adversity tests |
| L5 | Mut borrows and exclusivity | Exclusivity adversity matrix + data race precondition review (alias correctness only prior to concurrency) |

**Every level must**: First write positive/negative/adversarial test case lists → Use differential testing to prove the "tightened set remains invariant" → Dual-platform verification → Synchronously rewrite `resource-model.md` → ADR.

### 2.5 Interaction with Drops

- Drop points remain unchanged (end of scope, early return, `?`, overwrite assignment, slot forfeiture after move); values reaching a drop point while a borrow is active are **not dropped** (reference counting is not part of the design—shared borrows are temporary aliases, not ownership); once a borrow dies, it is handled by the existing mechanisms of its enclosing scope;
- If a host value of an escaping borrow (L3) still has active loans when dropped: compile-time proof that this is impossible is required (the loan's origin must be shorter than the host's origin); paths where this cannot be proven are rejected. No runtime sentinels are added—this differs from the current "moved sentinel": **borrow safety is entirely shifted to compile time**, retaining only moved sentinels at runtime.

### 2.6 Concurrency Prerequisites

`resource-model.md §7` remains unchanged: prior to the introduction of concurrency, the exclusivity of Mut loans is merely single-threaded alias correctness. The design mapping Mut/Shared directly to `Send`/`Sync` constraints when threading is introduced is deferred to the Stage C concurrency section and is not pre-committed here.

## 3. Adversarial Test Case Matrix (Write Tests Before Implementation)

Each level must cover at least:

1. **Alias pairs**: Two shared loans of the same base coexisting for reads → Allowed; one of them introducing a write path (prior to L5) → Rejected;
2. **Write after borrow**: `let v = slice_view(s, …); s = other;` → Rejected (Allowed after last use starting from L1);
3. **Move after borrow**: Moving the base while a borrow is active → Rejected; moving after the last use → Allowed (L1);
4. **Escape to return**: Returning a parameter borrow → Allowed (constraint holds); returning a local borrow → Rejected (insufficient origin); ambiguous source → Rejected (2.2);
5. **Escape to container**: Storing a borrow in a struct field → Old instance unusable after host is moved, drop order correct → Allowance set + Rejection set;
6. **Loop fixed point**: A borrow surviving across a back-edge (e.g., an accumulator view) → Allowed; a borrow re-established every iteration but referenced by the previous iteration → Rejected, indicating non-convergence;
7. **Partial moves**: Moving `s.a` then borrowing `s.b` → Allowed (L2); borrowing `s.a` → Rejected;
8. **Container elements** (prior to L4): Any pattern lending out elements → Rejected, with error messages explaining the current level;
9. **Generic cross-product**: `&T` instantiated as the four container types + future L3 behavior consistent within generic bodies (instantiation-by-instantiation checking still acts as a fallback);
10. **Differential baseline**: All existing L0 rejection cases **re-run** after L1/L2—relaxation may only reduce rejections, and any existing rejections classified as "false positives" must be individually named in the ADR with reasons provided for their allowance.

## 3.1 L2 Adversarial Case List (field-granularity loans — written before implementation)

Each case names its expected verdict under L2 and what pins it. The tests land
with the implementation; the list is the contract.

**Allowances (positive)**

- A1. `let h = slice_view(p.name, ..); p.note = "y"; print(h);` — writing a
  *different* field never dangles a view of `p.name`; allowed at L2 (rejected
  at L0/L1 by the variable-granularity rule).
- A2. `let h = slice_view(p.name, ..); print(h); p.note = "y";` — same shape,
  write after last use; allowed at L1 already through the variable-granularity
  region, stays allowed.
- A3. Two field views of *different* fields coexist: `slice_view(p.name, ..)`
  and `slice_view(p.note, ..)`; both readable; writes to the *unviewed* third
  field allowed.
- A4. A view of `p.name` and a move of a *different* struct `q` — unrelated
  variables, unaffected (regression guard).

**Rejections (negative)**

- R1. Write the *viewed field*: `p.name = "x";` while a view of `p.name` is
  still readable → rejected (the write releases the viewed bytes).
- R2. Move the whole struct while any field view is readable:
  `let q = p;` → rejected (the move re-homes every field's bytes).
- R3. Write the whole struct (fresh value in) while a field view is readable →
  rejected (releases every field).
- R4. A field view escapes (stored/returned) → rejected, same as L0.

**Adversarial**

- X1. Nested fields `a.b.c`: a view of `a.b.c` freezes writes to `a.b.c` and
  to `a.b` and to `a` (the intermediate places own the leaf's storage);
  writes to a *sibling* leaf `a.b.d` — L2's stretch goal, may stay rejected
  with an honest message (the design's "ambiguous source means rejection").
- X2. A field of an array/vector element (`v[0].name`) — elements stay
  un-borrowable at L2; the existing rejection message must not regress.
- X3. The move walk's field granularity: `p.a = p.b;` (a field-to-field move)
  moves `p.b` and re-initializes `p.a` — reads of `p.a` afterwards are legal,
  reads of `p.b` rejected; the borrow regions must respect the same places.
- X4. Differential: every L1 rejection re-runs under L2 — relaxations only
  reduce rejections; each newly-allowed program gets a run-corpus entry.

## 4. Implementation Hosting

- The frontend already has a prototype CFG (branch merging/loop fixed-points have been run on the move set); the borrow checker reuses the same skeleton, replacing `borrows: Vec<(id, depth)>` with `Map<Place, Loan>` + live sets.
- Diagnostic formatting follows the `file/line/column` contract (ADR-48); error messages must specify: what is borrowed, where it is borrowed to, and which usage causes the conflict.
- Check cost budget: Lessons learned from `constraint_audit_timings.py` (O(N²) copying nearly overshadowed the audit cost)—borrow analysis is performed per function; generic declarations are not re-analyzed (only the borrow surface diff is analyzed per instantiation). Use the same benchmark script for timing upper bounds.

## 5. Explicitly Out of Scope

- Lifetime **annotation syntax** (if L3 proves it necessary, discuss in a separate ADR; do not add it silently).
- Interior mutability (e.g., `RefCell`) and shared ownership (`Rc`)—the deterministic release model does not support these; introducing them would invalidate §4.5.
- Borrowing under concurrency (`Send`/`Sync`)—see the concurrency section in Phase C.
- Interaction with `unsafe` (raw pointer borrows)—see the FFI phase.

## 6. Progression Order and Acceptance Criteria

1. **L1 (NLL)**: Minimal relaxation; prioritize the user experience gain where "borrows in loops no longer occupy the entire loop"; commit the differential test suite.
2. **L2 (Field Granularity)**: Directly address the "rejecting writes to other fields" boundary defined in ADR-52.
3. **L3 (Escape)**: The core risk of this design—start with the "returning borrows" subset (simplest constraints); tracking the full origin of loans carried by containers will be a separate project.
4. L4/L5 will proceed according to dependencies, each with its own ADR.

Acceptance for each level = All test cases for that level in the §3 matrix + existing rejection diffs + full suite on both platforms + rewrite of `resource-model.md` + ADR. **Relaxations without a differential baseline are not accepted**—"fewer rejections" must be predicated on a case-by-case demonstration that "these programs are indeed safe," rather than simply counting a looser implementation as a success.