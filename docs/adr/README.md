# Architecture Decision Records (ADR) Index

Each record follows: Context → Decision → Rationale → Consequences and Revisit Criteria. New decisions are appended to the end.

One file per decision, four-digit numbering. Use `docs/adr/TEMPLATE.md` for new ADRs. The legacy consolidated file `docs/decisions.md` has been split into this format.

| ID | Title |
|---|---|
| [ADR-1](0001.md) | C backend as a swappable bootstrap solution |
| [ADR-2](0002.md) | Integers checked by default, no silent wrapping |
| [ADR-3](0003.md) | `string` as the first owned type; move semantics first |
| [ADR-4](0004.md) | Build cache keys based on generated C code |
| [ADR-5](0005.md) | `for` range loop bounds evaluated once, counter is immutable |
| [ADR-6](0006.md) | Built-in functions occupy reserved names |
| [ADR-7](0007.md) | Compiler worker threads use large stacks |
| [ADR-8](0008.md) | Strings are now NUL-terminated C-strings; length-carrying representation is the established evolution path |
| [ADR-9](0009.md) | Summary of empirical end-to-end costs for C backend and backend evaluation triggers (does not constitute a backend decision) |
| [ADR-10](0010.md) | Enums use unions of "tag + variant-specific payload struct", `match` is exhaustively enforced |
| [ADR-11](0011.md) | Recoverable errors expressed via pre-defined result enums; `+` performs copies for borrowed strings |
| [ADR-12](0012.md) | Read-only borrowed parameters (`&string`, `&vec[string]`) and directory traversal |
| [ADR-13](0013.md) | Path algebra and file metadata/organization operations built-in |
| [ADR-14](0014.md) | Use `?` to propagate recoverable errors |
| [ADR-15](0015.md) | `map[string, i64]` hash map |
| [ADR-16](0016.md) | Allocator interface and allocation failure semantics |
| [ADR-17](0017.md) | UTF-8 character boundaries and character-based truncation |
| [ADR-18](0018.md) | Cross-file visibility explicitly exported via `pub` |
| [ADR-19](0019.md) | Resource model as a specification document rather than an implementation byproduct |
| [ADR-20](0020.md) | Loop bodies prohibited from moving out owned values from the outer scope (replaces loop fixed-point) |
| [ADR-21](0021.md) | Generic enums monomorphized by argument |
| [ADR-22](0022.md) | Sorting, clocks, environment variables, and append-writes |
| [ADR-23](0023.md) | `exit(status_code)` termination statement |
| [ADR-24](0024.md) | Standard input and working directory |
| [ADR-25](0025.md) | Full Linux validation and strict ISO C corrections |
| [ADR-26](0026.md) | JSON path queries (read-only) |
| [ADR-27](0027.md) | Cache entries include validation; bypass cache if compiler is unrecognized |
| [ADR-28](0028.md) | Cache size limits and LRU (Least Recently Used) eviction |
| [ADR-29](0029.md) | `vec[string]` construction and subprocess execution |
| [ADR-30](0030.md) | Generic structs use the same monomorphization as generic enums |
| [ADR-31](0031.md) | Generic functions resolve their own source code at the call site |
| [ADR-32](0032.md) | `set_cwd` is an explicit process-level side effect; `set_env` is not |
| [ADR-33](0033.md) | Constraints are shape checks of arguments at the call site, not proofs for generic bodies |
| [ADR-34](0034.md) | `?` propagates based on "two-variant shape"; `Option` and result enums follow the same rule |
| [ADR-35](0035.md) | `&T` resolves to existing borrows during instantiation; borrowed parameters can be forwarded directly |
| [ADR-36](0036.md) | `find` returns byte indices via result values; no sentinels introduced |
| [ADR-37](0037.md) | `f64` semantics defined before implementation; textual representation unified by runtime |
| [ADR-38](0038.md) | Reduce input to improve compilation speed; avoid relying on larger caches or more translation units |
| [ADR-39](0039.md) | System headers trimmed by reachability; no manual Win32 declarations |
| [ADR-40](0040.md) | Constraints undergo capability coverage checks at declaration, but are still not traits |
| [ADR-41](0041.md) | Generic declarations share lexical results by source file to eliminate O(N²) frontend duplication |
| [ADR-42](0042.md) | `fmt` only rearranges whitespace; idempotency guaranteed by construction, no full check required |
| [ADR-43](0043.md) | Tests run via text-synthesized `main`; zero semantic changes to frontend |
| [ADR-44](0044.md) | Literal concatenation fusion—program text does not enter the allocator |
| [ADR-45](0045.md) | `to_string` numbers use stack buffers; zero heap allocation when concatenated with literals |
| [ADR-46](0046.md) | `map_inc` converges counting into a single lookup; starts from zero if key is missing |
| [ADR-47](0047.md) | Trait/impl static dispatch—resolved at instantiation, no method tables |
| [ADR-48](0048.md) | Editor diagnostics—structured output via `check --json` |
| [ADR-49](0049.md) | `split`/`lines` single-block arena—allocations reduced to constant count based on token count |
| [ADR-50](0050.md) | Runtime annotations stripped with function pruning; names in annotations no longer treated as dependency roots |
| [ADR-51](0051.md) | C compiler invoked with relative filenames within a temporary directory |
| [ADR-52](0052.md) | Read-only views can follow string fields of structs (borrowing the entire variable) |
| [ADR-53](0053.md) | Streaming token cursor `tokenize`—lazy splitting with zero allocations per token |
| [ADR-54](0054.md) | `args()` converts command line to UTF-8 on Windows |
| [ADR-55](0055.md) | `print` writes out as UTF-16 on Windows consoles |
| [ADR-56](0056.md) | Map load factor reduced from 0.75 to 0.5 |
| [ADR-57](0057.md) | Map status bits moved out of slots; slots compressed from 32 bytes to 24 bytes |
| [ADR-58](0058.md) | Map miss reason text changed to read-only constants; no allocations on misses |
| [ADR-59](0059.md) | `build` defaults to optimized builds; `--debug` reverts to fast builds; optimization flags moved before the `--` separator |
| [ADR-60](0060.md) | Math built-ins v1—11 thin wrappers, IEEE semantics, `-lm` added to Linux link line |
| [ADR-61](0061.md) | `vec[f64]`—owned vector container for `f64` elements |
| [ADR-62](0062.md) | `ae test` adds name filtering (`test <file.ae> [filter]`) |
| [ADR-63](0063.md) | General borrow checker design draft (Phase C core, Status: Designed, Not Implemented) |
| [ADR-64](0064.md) | `min`/`max` math built-ins—runtime implementation, NaN ignored + ±0 total order tie-breaking |
| [ADR-65](0065.md) | `map[string, f64]`—second map shape and pre-defined `FloatResult` enum |
| [ADR-66](0066.md) | Prelude enum layouts emitted on-demand—small programs no longer carry unused result types |
| [ADR-67](0067.md) | Language design charter—Rust's safety model + Go's simplicity and concurrency |
| [ADR-68](0068.md) | Freeze list before M1 acceptance |
| [ADR-69](0069.md) | Frontend split into submodules by responsibility (zero behavioral changes) |
| [ADR-70](0070.md) | Establish workspace boundaries using transitional compiler libraries |
| [ADR-71](0071.md) | Extract lexer crate |
| [ADR-72](0072.md) | Progressive migration of source locations and structured diagnostics |
| [ADR-73](0073.md) | Distinguish between circular imports and completed duplicate imports |
| [ADR-74](0074.md) | Move the formatter into the syntax crate |
| [ADR-75](0075.md) | Load import graphs without recursive host calls |

