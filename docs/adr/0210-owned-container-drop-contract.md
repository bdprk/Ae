# ADR-0210: Drop Contract for Source-Level Owning Containers

- Date: 2026-11-18
- Status: Proposed; no user-defined destructor is implemented by this ADR.
- Scope: the ownership and destruction prerequisites for ADR-0208's source-level containers.

## Evidence and problem

`Type::owned` in `frontend/mod.rs` classifies all structs as owned, and generated code in `frontend/c_output.rs` emits a per-struct release and forfeit helper. The release helper recursively releases known owned fields; for a raw-pointer field it only writes zero. Thus a struct holding a pointer from `ae_std_alloc` is move-tracked as a struct, but its generated cleanup does not free the allocation. Existing MIR drop elaboration operates on those generated releases; it does not infer ownership through the pointer or supply a user-defined destructor.

The scalar storage primitives alone are therefore insufficient for a safe `Vec[T]`: explicit `free` calls cannot guarantee cleanup on every normal control-flow edge and can double-free if the language permits another path to free the same allocation. This ADR specifies the contract to implement and test before such an API is exposed.

## Decision

Use statically resolved drop glue for each concrete owning type. A future language feature will permit a type to define an explicitly declared destructor, called exactly once for each initialized value that reaches a normal destruction point. The precise syntax, eligibility rules, and dispatch mechanism must be specified before implementation; this ADR does not introduce syntax or assert existing support. Glue for a concrete generic instantiation resolves its element behavior statically. No per-instance callback is required by default.

The destructor has exclusive access to the still-initialized receiver and runs before automatic destruction of its fields. A container destructor is responsible for dropping exactly its initialized elements, then freeing its allocation exactly once. After its destructor returns, the compiler drops ordinary owned fields in declaration order. A destructor must not independently destroy fields that the compiler will subsequently destroy. The exact interface to drop an individual generic element is part of the future spec and must not be exposed as safe until its initialization, aliasing, and type requirements are enforced.

## Control-flow and state requirements

- A successful move transfers the entire live value and its cleanup obligation; the source must not run the destructor. Assignment into a live destination destroys the old value before taking ownership of the replacement, with evaluation order specified and tested.
- Normal scope exit, early return, `?` error propagation, `break`, and `continue` destroy every live local leaving scope exactly once. Nested aggregates recursively destroy their initialized fields. A partially initialized aggregate must never run a destructor requiring a fully initialized receiver; its initialized fields still need cleanup.
- The existing C backend's zeroed-slot/idempotent-release convention is not sufficient for arbitrary user destructors: a zeroed moved-from representation is not necessarily a valid receiver. The implementation must use sound liveness/drop flags or an equivalent proof on every path, including `Mixed` control-flow joins and the tree backend. The interpreter must implement the same observable normal-exit semantics or explicitly reject the feature until it does.
- Specify abnormal termination separately. Current panic defaults to process termination; this ADR does not promise stack unwinding, destructor execution after process termination, or recovery from a destructor trap.
- A container tracks `0 <= len <= cap`; only `[0, len)` contains initialized elements. Allocation size and offsets use checked arithmetic and obey size and alignment requirements. Growth, failed allocation and partial initialization preserve a valid cleanup state. Any raw-pointer access remains inside a documented unsafe boundary.
- A safe client cannot clone the owning handle, observe it after move, use an invalidated view, or cause a duplicate release through method calls. A raw pointer itself remains copyable and does not own its pointee.

## Required adversarial matrix

Before claiming the protocol implemented, positive and rejection tests must cover: scope exit; move into and out of a function; return; reassignment and self-referential evaluation; both sides of a conditional move with a later join; loops with `break` and `continue`; `?` on success and error paths; nested owned aggregates; empty and partially initialized buffers; growth and allocation failure; destructor order and exactly-once counters; invalid borrow/move/alias attempts; and generic instantiations for both trivial and owned elements. Verify both compiled code paths, interpreter behavior where supported, leaks and use-after-free with sanitizers, and Windows/Linux. Record unsupported paths explicitly rather than treating a trap as equivalence.

## Rollback and open work

Existing built-in containers remain the migration fallback until the complete source container passes these gates. Before implementation, settle destructor syntax and visibility, the element-drop operation, drop-flag representation, interactions with mutable borrowing and reentrancy, constructor failure, and a precise abort policy. None of those questions authorizes weakening safe-code guarantees.
