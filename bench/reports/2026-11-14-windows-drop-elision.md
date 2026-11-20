# Benchmark report: 2026-11-14 Windows — M2.4 drop elaboration (release elision)

- Status: exploratory; platforms tested: Windows only (single-platform measurement)
- Source revision and working tree state: commit `5b0f273` (fix(movewalk): a plain read must not abandon the move walk), clean tree
- Raw results: [bench/results/2026-11-14-windows-drop-elision-alloc.json](../results/2026-11-14-windows-drop-elision-alloc.json), [bench/results/2026-11-14-windows-drop-elision-straight.json](../results/2026-11-14-windows-drop-elision-straight.json)
- Purpose and scope: what does drop elaboration (ADR-0204) buy for the programs it was built for — fewer release calls in the shipped C, and does eliding the provably-dead ones show in wall-clock time? It does not measure compile time, code size beyond release-call counts, or any other optimization's effect.

## Environment

- OS: Windows 11 (build 26300), x86-64
- CPU/memory/power policy: not recorded (single desktop machine, default power plan)
- Compiler: gcc via the AE driver; AE binary `target/debug/ae.exe` built with `cargo build -p ae` (dev profile, the same binary the tests use)
- Cache state: AE content-addressed cache warm; builds go through `build --debug` (-O0 object code), timings measure the produced binaries' execution only

## Reproduction

```text
cd <repo>
cargo build -p ae
python bench/drop_elision_timings.py --mode alloc --rounds 11
python bench/drop_elision_timings.py --mode straight --rounds 11 --pairs 20000
```

Two workloads, same source through both backends (`AE_CODEGEN=tree` for the conservative one; the MIR backend with drop elaboration is the default):

- `alloc` — 1,000,000 loop iterations; each allocates three owned strings, moves two into a by-value call (`sink(a) + sink(b)`), and holds the third (`hold(c)`). The graph proves two of the three scope-end releases dead every iteration.
- `straight` — 20,000 declare-then-move pairs in one function; no loop, no allocation beyond the 20,000 one-time constructions, so the workload is almost exactly the dead releases themselves.

Binaries are built once per backend; runs are interleaved (mir, tree) over 11 rounds; every run's stdout is validated against the first run's (`34666670` and `108890` respectively — identical across backends). Wall-clock around each child process via `time.perf_counter`.

## Results

`alloc` (1M iterations):

| backend | median | min | max | release sites in main |
|---|---|---|---|---|
| mir (elision) | 0.3123 s | 0.2981 s | 0.3753 s | **1** |
| tree (conservative) | 0.3196 s | 0.3035 s | 0.3629 s | 3 |

`straight` (20,000 moved bindings):

| backend | median | min | max | release sites in main |
|---|---|---|---|---|
| mir (elision) | 0.0108 s | 0.0103 s | 0.3682 s | **0** |
| tree (conservative) | 0.0108 s | 0.0099 s | 0.6436 s | 20,000 |

## Interpretation and limitations

- The structural result is exact and repeatable: drop elaboration removes the release call sites the graph proves dead — 2 of 3 per iteration in `alloc`, all 20,000 in `straight` — while the tree backend ships them and relies on move-zeroed slots making them no-ops.
- The runtime result is marginal on these workloads. A dead release is a function call that finds a null pointer and returns; against 3,000,000 string allocations (`alloc`) or process start-up plus 20,000 constructions (`straight`), 2M/20k elided calls sit inside run-to-run noise. The `alloc` medians differ by ~2% (mir faster); the `straight` medians are equal. Max values include scheduler outliers (0.37–0.64 s), so medians and mins, not maxima, carry the comparison.
- The elision's value as measured here is shipped-C shape (fewer calls to read, JIT-free branches to execute) rather than wall-clock time; a workload dominated by non-allocating release sites at higher frequency would be needed to make the runtime delta measurable, and that workload does not represent the language's typical cost profile (ADR-53's allocation-heavy text work dominates).
- Single-platform measurement; the pass emits no new C templates, so platform variance in the *difference* is expected to be small, but it is unmeasured here.

## Consequences

- No claims of universal speedup are made; the milestone's acceptance rests on the differential tests (byte-identical stdout across four execution modes) and the release-site counts above.
- The workload generator and raw JSON are committed: `bench/drop_elision_timings.py` re-derives both tables with the commands above.
