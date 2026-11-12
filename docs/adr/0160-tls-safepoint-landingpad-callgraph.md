# ADR-0160: Compiler Runtime ABI Primitives (Thread Local Storage, Safepoints, Landing Pads, Call Graphs)

## Status
Accepted

## Context
Under Milestone M2 (Self-hosting compiler runtime integration), high-performance execution of multithreaded programs and unwinding relies on runtime ABI descriptors:
1. Thread Local Storage (TLS): modeling ELF TLS models (`LocalExec`, `InitialExec`, `LocalDynamic`, `GeneralDynamic`) and TCB offsets.
2. GC Safepoint Polling: loop backedge and function prologue polling points to permit cooperative garbage collection and thread suspension.
3. Landing Pads: DWARF exception handling personality routine descriptors tracking action table offsets, catch filters, and cleanup actions.
4. Call Graph Analysis: edge frequency weighting, hot-call detection, and recursion detection for whole-program interprocedural optimization.

## Decision
1. **TlsDescriptor (`std/core/ir_thread_local.ae`)**: Compact model index and thread control block offset representation.
2. **SafepointPoll (`std/core/ir_safepoint.ae`)**: Polling instruction placement descriptor with dedicated polling register binding.
3. **LandingPad (`std/core/ir_landing_pad.ae`)**: Action table offset, type filter identification, and cleanup status encapsulation.
4. **CallGraphEdge (`std/core/ir_call_graph.ae`)**: Edge call frequency and direct recursion detection.

## Consequences
- The compiler runtime ABI bridge is fully specified and operable in `core` without dynamic allocations.
- Verified via test suite `tests/run/batch_core_compiler_runtime.ae`.
