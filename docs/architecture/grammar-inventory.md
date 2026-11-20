# M0.2.01 — Grammar Inventory of the Active Parser

Status: verified against revision d50be35 (2026-11-19); the M0.2.01 deliverable of the parser-migration plan. Scope: the compiler front end as it exists today —
`crates/ae-compiler/src/frontend/{parser,expression,types,declarations,closures,mod,loader}.rs`
plus the token surface in `crates/ae-syntax/src/lib.rs`. Every production
maps to (a) a witness (example file, run test, or `tests/execution.rs` test
name) or (b) a documented gap. Syntax claims marked **[probe]** were verified
by running `target/debug/ae.exe check` on a fixture in a temp directory;
the quoted error text is the probe's actual output.

## Summary

The active parser accepts **~120 distinct productions**: 12 top-level item
forms, 19 type-expression forms, 23 statement forms, ~50 expression forms
(6 literal kinds, 4 unary/prefix forms, 5 postfix forms, 16 binary operators
in 9 precedence tiers, plus ~90 name-dispatched builtins that are ordinary
identifiers in the grammar), and 2 pattern forms. **Witness coverage is high
for everything on the mainstream path**: of the productions a user program
can reach, only 3 accepted forms lack any witness (`var` parameter slots,
compound assignment on struct fields `s.f += v`, map put through a field
`s.counts[k] = v`), and 6 documented gaps are rejection-only. **Accidental
syntax findings**: (1) `Kind::Pipe` and `Kind::Or` are double-booked — `|`
is both bitwise-or (precedence 3) and the closure-literal opener, and `||`
is both logical-or and the empty closure parameter list, disambiguated by a
one-token lookahead for `->`; (2) `&` is triple-booked (bitwise-and,
call-argument borrows, address-of casts); (3) ~90 builtins (`len`, `push`,
`map_get`, `json_get`, …) plus `pub`, `Self`, `const`, `T` are **not
keywords** — they are `Kind::Ident` strings dispatched inside the parser and
frozen by the `RESERVED` table; (4) `print` is the one real keyword with
statement-only grammar and a special bare-text literal form.

Parser entry points (for the migration's port order):

| Phase | File:function |
|---|---|
| Lex | `crates/ae-syntax/src/lib.rs: lex_inner` |
| `use` scan | `frontend/loader.rs: scan_uses` (Text + Semi only) |
| Signature pass | `frontend/declarations.rs: collect_functions` (+ `parse_trait_def`, `collect_impl`, `parse_struct_def`, `parse_enum_def` in `types.rs`) |
| Body pass | `frontend/declarations.rs: program` → `function_rest` → `frontend/parser.rs: block_inner` |
| Expressions | `frontend/expression.rs: expression / expression_inner` |
| Types | `frontend/types.rs: parameter_type / named_type / return_annotation / pointer_type` |
| Closures | `frontend/closures.rs: closure_literal / closure_body` (probe-parse twice: borrow probe, then value probe) |
| Generics | `frontend/types.rs: instantiate_enum / instantiate_struct`, `frontend/declarations.rs: instantiate_fn / instantiate_impl_method` |

All citations below are `file:line` inside `crates/ae-compiler/src/frontend/`
unless noted. `exec#name` = test function in `tests/execution.rs`.

---

## 1. Lexical surface (crates/ae-syntax/src/lib.rs)

Keywords (`lex_inner`, lib.rs:229-235): `fn let var mut print if else while
return for in break continue assert true false use struct enum match trait
impl unsafe const as extern`. Note what is *not* a keyword: `pub`, `Self`,
`self`, `const`-as-declaration is `Const` but only read in pointer types,
`T` and every builtin are plain idents.

| Token | Spellings | Witness |
|---|---|---|
| Number | `[0-9]+` → i64; overflow E0005 | every example |
| Float | digits `.` digits, or exponent `1e3`/`1E-3`/`1e-2`; E0006/7/8 | `examples/float_math.ae`; **[probe]** `1e3` accepted |
| Text | `"…"`; escapes `\n \t \r \\ \"` only (E0002); raw newline/NUL E0004; unterminated E0003 | `exec#print_handles_strings_with_escapes_and_unicode`; **[probe]** `\u0041` → "unsupported string escape" |
| Comments | `//` line only. No block comments, no `///` tokens (doc comments are re-scanned from raw source by `has_doc_comment_above`, lib.rs:70, for E0704) | `tests/run/unsafe_fn_call.ae`; **[probe]** `/* hi */` rejected |
| Operators | two-char first: `== != <= >= -> && \|\| .. => ::` then single `+ - * / % < > = ! & \| ^` and `<< >>`, compound `+= -= *= /= %=` | `exec#bitwise_operators_and_precedence` |
| **[probe] hex/binary/underscore literals** | `0x10` lexes as `Number(0)`, `Ident("x10")` → "expected Semi, found Ident(\"x10\")" | — |

---

## 2. Top-level items

| # | Production | Parser | Witness |
|---|---|---|---|
| 2.1 | `use "path";` header(s), file-leading only; `@alias` prefix inside the path string resolves via package manifest (`loader.rs: resolve_import`); no `as` clause | `loader.rs: scan_uses`; skipped in-parser by `declarations.rs: skip_uses` (`pos += 3`) | `examples/modules/main.ae:3`; `tests/run/batch_core_compiler_asm.ae:4`; **[probe]** `use "x.ae" as n;` → "expected ';' after 'use' path"; `use "@std/io";` parses, fails at path resolution |
| 2.2 | `pub` item modifier (fn/struct/enum/trait/impl/extern) — an Ident, not a keyword | `declarations.rs: take_pub` (33) | `examples/modules/report.ae`; `tests/run/trait_bounds.ae:1` |
| 2.3 | `#[repr(C)]` on a struct (non-generic, non-empty fields, C-shaped fields only) | `declarations.rs: collect_functions` 164-188; `types.rs: parse_struct_def` 448 | `tests/run/generic_layout_query.ae:5`; `exec#repr_c_attribute_guards_and_extern_struct_rejections` |
| 2.4 | `#[no_mangle]` on an `extern "C" fn` **with** body (exposure) | `declarations.rs:180-265` | `exec#no_mangle_exposure_runs_bidirectionally_and_stays_c_visible`; `exec#no_mangle_attribute_misuse_is_rejected` |
| 2.5 | `fn name(params) -> T { body }`; `main` has no return arrow and no params (**[probe]** `fn main() -> i64` → "expected LBrace, found Arrow"); params `name: Type`, optional `var` slot modifier, ≤64 | `declarations.rs:307-355` + `function_rest` 960 | `examples/functions.ae`; `exec#functions_execute_recursion_and_ordered_arguments` |
| 2.6 | `unsafe fn name(...)` — requires `///` doc comment above (E0704), body parses normally, call sites need `unsafe` | `declarations.rs:279-287`; lib.rs `has_doc_comment_above` | `tests/run/unsafe_fn_call.ae`; `exec#unsafe_fn_doc_comment_is_enforced_and_a_documented_one_runs` |
| 2.7 | `extern "C" fn name(params) -> T;` declaration (no body; ABI string must be `"C"`; params/return must pass `c_abi_shape`) | `declarations.rs:194-269` | `tests/run/extern_c_call.ae:6`; `tests/run/ptr_offset.ae:10` |
| 2.8 | `#[no_mangle] extern "C" fn name(params) -> T { body }` exposure with body | `declarations.rs:243-265` | `exec#no_mangle_exposure_runs_bidirectionally_and_stays_c_visible` |
| 2.9 | generic fn `fn name[T, U: Bound + Trait](params) -> T { body }` — body skipped at collect, parsed per instantiation; params are single uppercase letters; bounds `Add Eq Ord Copy` or a declared trait; constraints legal on functions only | `declarations.rs:290-305`; `types.rs: type_params_with_bounds` 226 | `examples/generic_fn.ae`; `exec#generic_functions_are_templates_for_one_argument_list` |
| 2.10 | `struct Name { field: Type, ... }` and generic `struct Name[T, U] { ... }`; trailing comma accepted; `#[repr(C)]` combinator | `types.rs: parse_struct_def` 448 | `examples/methods.ae:7`; `tests/run/nested_generics.ae` |
| 2.11 | `enum Name { Variant, Variant(T, ...), }` — payloads flat: i64/bool/f64/string/vec/vec[string] or param; ≤16 per variant, ≤256 variants; generic `enum Name[T]` | `types.rs: parse_enum_def` 171 | `examples/enums.ae`; `exec#generic_enums_instantiate_per_argument` |
| 2.12 | `trait Name { fn method(p: T, ...) -> T; }` — first param must be `T`, no default bodies, ≤8 params | `declarations.rs: parse_trait_def` 383 | `tests/run/trait_bounds.ae:1` |
| 2.13 | `impl Trait for Type { fn method(...) -> T { body } }` — signatures must match the trait exactly (T substituted); coherence: impl lives with trait or type | `declarations.rs: collect_impl` 713 | `tests/run/trait_bounds.ae:9`; `exec#trait_dispatch_resolves_per_type_and_matches_the_hand_written_call` |
| 2.14 | inherent `impl Type { fn f(self) ...; fn g(&self)...; fn h(&mut self)...; fn assoc()...; fn drop(&mut self) -> i64 }` — struct or enum target, same-file coherence, ≤8 params, no borrow returns; `drop` fixed signature, not `pub`, not callable | `declarations.rs: collect_inherent_impl` 463 | `tests/run/methods.ae`; `examples/struct_mut_borrow.ae`; `exec#user_defined_drop_runs_on_scope_exit_and_respects_move` |
| 2.15 | generic impl `impl Name[K, V] { ... }` — method bodies stored as token ranges, parsed per instantiation; enum targets by-value receivers only | `declarations.rs: collect_generic_impl` 619 | `tests/run/generic_impl.ae`; `tests/run/enum_generic_impls.ae` |
| — | **GAP**: no `const` item, no nested items, no `mod` blocks, no `impl Trait for Type` where Type is generic-instantiated via `[]` in trait-impl position | `declarations.rs:156-193` **[probe]** `const N = 5;` → "expected Fn, found Const" | — |

Multi-file model: no module system — a `use`d file is loaded depth-first and
its `pub` items join the shared function/type tables
(`loader.rs: load_program`; `exec#modules_import_transitively_and_reject_cycles_or_conflicts`,
`exec#diamond_and_repeated_imports_load_one_module_and_cycles_still_fail`).

---

## 3. Type expressions

| # | Production | Parser | Witness |
|---|---|---|---|
| 3.1 | `i64`, `f64`, `bool`, `string` | `types.rs: named_type` 5 | everywhere |
| 3.2 | `vec` (i64 elements) | `named_type` 16 | `examples/vectors.ae` |
| 3.3 | `vec[string]`, `vec[f64]` | `named_type` 20-28 | `examples/float_stats.ae:7` |
| 3.4 | `map[string, i64]`, `map[string, f64]` | `named_type` 30-43 | `examples/map_count.ae`; **[probe]** `map[string, bool]` rejected (also `exec` rejection list line 2622) |
| 3.5 | user struct / enum name (visibility-checked) | `named_type` 88-101 | everywhere |
| 3.6 | generic instantiation `Name[arg, ...]` (enum args restricted to scalars/vec; struct args via `named_type`) | `named_type` 44-80; `types.rs: type_argument` 334 | `tests/execution.rs:1179 generic_structs_instantiate…`; `examples/generic_impl.ae` |
| 3.7 | `Self` inside impl (type position) | `named_type` 81-87 | `tests/run/self_type.ae:4` |
| 3.8 | type parameter `T` (substituted at instantiation) | `named_type` 10 via `subst` | `examples/generic_fn.ae` |
| 3.9 | `*const T` / `*mut T` — parameter and return annotations; pointee via `named_type` | `types.rs: pointer_type` 518; `return_annotation` 715 | `tests/run/ptr_offset.ae:10` |
| 3.10 | `fn(T, U) -> R` function type — params via `callable_parameter_type` (named or borrow forms); return may not be borrow/fn | `types.rs: parameter_type` 555-575 | `examples/function_types.ae:9` |
| 3.11 | `&string` (view param) | `parameter_type` 599 | `exec#read_only_string_parameters_do_not_consume_their_arguments` (`twice(text: &string)`) |
| 3.12 | `&vec`, `&vec[string]`, `&vec[f64]` | `parameter_type` 605-619 | `examples/generic_borrow.ae:10` |
| 3.13 | `&Struct`, `&mut Struct` (incl. generic `&Pair[args]`); `&mut` on non-struct rejected | `parameter_type` 625-698 | `examples/struct_borrow.ae`; `examples/struct_mut_borrow.ae` |
| 3.14 | `&[i64; N]` borrowed fixed array, N 1..4096; arrays are **parameter-only**, born only from `let`-array literals | `parameter_type` 702-710 | `examples/borrow.ae:1`; `exec#arrays_support_checked_read_write_and_expression_order` |
| 3.15 | `&T` where T is the generic's parameter (spelling alias: string→view, vec→refs) | `parameter_type` 576-598 | `exec#generic_borrow_parameters_resolve_at_each_call_site` |
| 3.16 | return annotations: named types + pointer types; **no** borrows (`return_annotation` 719 rejects `&`) | `types.rs: return_annotation` | everywhere; **[probe]** confirmed |
| — | **GAP**: no tuples, no slices `&[T]`, no `Option[T]` as a *type annotation* anywhere except as the generic enum instantiation `Option[T]` (3.6), no type annotations on `let`/`var` at all | **[probe]** `let x: i64 = 1;` → "expected Equal, found Colon" | — |

---

## 4. Statements (`parser.rs: block_inner` 286-936)

| # | Production | Parser | Witness |
|---|---|---|---|
| 4.1 | `let name = expr;` — no annotation; name checked against RESERVED/struct/enum names | `parser.rs:296-362` | everywhere |
| 4.2 | `let name = [e, e, ...];` **array literal form** — only in let/var initializers, i64 elements only, 1..4096, length is part of the type | `parser.rs:306-319` | `examples/borrow.ae:6`; `tests/run/batch_core_search_sort.ae:7`; **[probe]** `var a = [1,2,3]` also accepted; as an argument → "expected expression" |
| 4.3 | `let name = expr?;` — propagation binding; enum must be `{Ok(T), Err(string)}` or `{Some(T), None}` shaped (user enums count, `exec#:1464 make_maybe`); binds payload | `parser.rs:325-336` + `propagate_statement` 957 | `examples/option_propagate.ae:13`; `exec#question_mark_propagates_none_as_well_as_err` |
| 4.4 | `var name = expr;` mutable binding | `parser.rs:296-297` | everywhere |
| 4.5 | view bindings: initializers of type StrView/Tokens (`slice_view`, `tokenize`) — must be `let`, never reassigned | `parser.rs:342-344, 682-684` | `tests/run/slice_view.ae`; `examples/tokens.ae` |
| 4.6 | assignment `name = expr;` (scalars, strings move, vecs move, enums, whole structs; self-assignment-in-statement rejected E0102) | `parser.rs:636-698` | everywhere |
| 4.7 | compound assign `name += -= *= /= %= expr;` — i64 all; f64 all but `%=;` string only `+=` | `parser.rs:649-675` | `exec#…` (compound exercised in run suite; e.g. `tests/run/batch_core_ranges.ae`) |
| 4.8 | compound assign on struct field `s.f += v;` | `parser.rs:587-614` | **GAP (accepted, unwitnessed)** |
| 4.9 | field store `s.f = v;` on owned var and through `&mut` borrow (`parser.rs:520-548`) | `parser.rs:549-621` | `examples/struct_mut_borrow.ae` |
| 4.10 | element store `v[i] = e;` for array/vec/vec[f64]/vec[string] variables | `parser.rs:460-496` | `examples/vectors.ae` |
| 4.11 | map put `m[k] = v;` (key string or view; value i64/f64) | `parser.rs:446-459` | `tests/execution.rs:2599` (`counts["seen"] = 1`) |
| 4.12 | map put through field `s.f[k] = v;` | `parser.rs:559-574` | **GAP (accepted, unwitnessed)** |
| 4.13 | pointer store `*p = v;` — unsafe only, `*mut` var, non-owned pointee | `parser.rs:709-734` | `tests/run/unsafe_pointer_write.ae:12` |
| 4.14 | bare call statement `name(args)?;` — `?` mandatory (result never dropped); whole-expr propagation only | `parser.rs:430-441` | `examples/option_propagate.ae:18` (`maybe_find(flag)?;`); **[probe]** `?` mid-expression rejected |
| 4.15 | `exit(code);` — i64 status, flushes, releases | `parser.rs:370-377` | `examples/grep_cli.ae`; `exec#exit_sets_the_status_and_flushes_what_was_printed` |
| 4.16 | builtin mutation statements `push(v, e); pop(v); sort(v); reverse(v);` — statement-only (**[probe]** `push` in expr → "unknown function 'push'") | `parser.rs:380-428` | `examples/vectors.ae`; `exec#sort_and_reverse_order_vectors_in_place` |
| 4.17 | `print(expr);` — int/bool/f64/string/view only (E0206) | `parser.rs:913-929` | everywhere |
| 4.18 | `print("text");` bare-text fast form (no borrow, verbatim bytes) | `parser.rs:915-918` | every example |
| 4.19 | `assert(cond);` — bool only | `parser.rs:905-912` | `exec#assert_passes_and_fails_at_the_right_location` |
| 4.20 | `return expr;` — forbidden in `main` (**[probe]**, `parser.rs:701`); type must equal fn return | `parser.rs:700-708` | everywhere |
| 4.21 | `if cond { } else { }` + `else if` chains; condition bool (E0205); no parens, no bare-else-expr | `parser.rs: if_statement` 937 | `exec#else_if_chains_evaluate_in_order`; **[probe]** `else print(2);` rejected |
| 4.22 | `while cond { }` — bool, loop-move fixed point (ADR-20) | `parser.rs:756-774` | `examples/control-flow.ae` |
| 4.23 | `for x in start..end { }` — int range; bounds i64 expressions, evaluated once in order | `parser.rs:866-896` | `examples/loops.ae:4`; `exec#for_range_bounds_evaluate_once_in_order` |
| 4.24 | `for x in vec_var { }` — bare variables of vec (i64 by value), vec[f64], vec[string] (views), and borrowed forms `&vec`/`&vec[string]`/`&vec[f64]` params; container borrowed for the loop | `parser.rs:814-865` | `exec#for_in_iterates_vector_elements_by_value`; `exec#borrowed_string_vectors_can_be_iterated_and_read` |
| 4.25 | `for t in cursor { }` — token cursor (`tokens(text, sep)`), bare cursor variable only; per-iteration view binding | `parser.rs:783-813` | `examples/tokens.ae:15`; `exec#token_cursor_nested_under_a_range_restarts_on_each_iteration` |
| 4.26 | `break;` / `continue;` — loop depth ≥ 1, semicolon required | `parser.rs:897-904` | `exec#break_and_continue_control_loop_flow`; `examples/loops.ae` |
| 4.27 | `match var { Enum::Variant(binds) => { } ... }` — subject must be a bare enum variable (**[probe]** `match get()` → "match needs a bare enum variable"); exhaustive, each variant once; instantiated enums matched by declaration name | `parser.rs: match_statement` 995 | `examples/enums.ae`; `exec#enums_match_payloads_execute` |
| 4.28 | `unsafe { }` block statement — depth-gated ops inside; W0702 warning when body has no unchecked op | `parser.rs:735-753` | `tests/run/unsafe_block.ae`; `exec#unnecessary_unsafe_block_warns_without_failing_the_compile` |
| — | **GAP**: no free-standing block `{ }` as a statement (**[probe]** → "expected declaration, assignment, print, or '}'"); no `match` as expression (**[probe]** "expected expression"); no `unsafe` as expression (**[probe]**); no labeled loops/`break label`; no `while let`; no `do`/`loop` | — | — |

---

## 5. Expressions (`expression.rs: expression_inner` 91-1529)

### 5.1 Precedence tiers (binding loosest → tightest)

| Tier | Operators | Operand/type rules | Witness |
|---|---|---|---|
| 1 | `\|\|` | bool only | everywhere |
| 2 | `&&` | bool only | everywhere |
| 3 | `\|` bitwise-or | i64 only | `exec#bitwise_operators_and_precedence` |
| 4 | `^` | i64 | same |
| 5 | `&` bitwise-and | i64 | same |
| 6 | `== != < <= > >=` | i64/i64, f64/f64; `== !=` also bool/bool and string/view; strings lower to `Expr::StrEq` | `exec#string_equality_compares_content`; `exec#booleans_and_short_circuit_evaluation_are_correct` |
| 7 | `<< >>` | i64 | `exec#bitwise_operators_and_precedence` |
| 8 | `+ -` | i64 both; f64 both (no mixing, E0203); `+` with string/view operands = concat (consumes owned strings, copies views, `plus_operand_depth` rule) | `exec#owned_strings_move_concatenate_and_release`; `exec#concat_reads_fields_and_equality_does_not_consume` |
| 9 | `* / %` | i64; f64 for `* /` only; `%` i64-only | `exec#modulo_and_negation_follow_documented_semantics` |
| 10 (unary) | `-e` (int/f64), `!e` (bool) | operands parsed at min=10, so `-a * b` = `(-a) * b` | `exec#modulo_and_negation…`; `exec#rejects_type_errors` (`!1`, `-true`) |
| postfix | `.f`, `.m(..)`, `[i]` (var-only), `as *T`, `p.deref()` | see below | — |

Associativity: left (loop at expression.rs:1441). `&&`/`||` short-circuit
(`exec#booleans_and_short_circuit_evaluation_are_correct`).

### 5.2 Primary expressions

| # | Production | Parser | Witness |
|---|---|---|---|
| 5.1 | integer / float / `true` / `false` / string literals | `expression.rs:95-103` | everywhere |
| 5.2 | `(expr)` grouping | `expression.rs:1127` | everywhere |
| 5.3 | variable reference (move-checked when owned and no operator follows) | `expression.rs:1090-1124` | everywhere |
| 5.4 | `-` / `!` unary | `expression.rs:109-122` | everywhere |
| 5.5 | `&var as *const T` / `&mut var as *mut T` address-of cast — operand must be a bare local variable; pointee must match; owned pointee rejected; the `as` is mandatory | `expression.rs:128-176` | `tests/run/unsafe_pointer_deref.ae:9`; `tests/run/unsafe_pointer_write.ae` |
| 5.6 | `vec[e, e, ...]` vector literal — element type from first element (i64/f64; mixing rejected), empty `vec[]` = i64 vec, ≤65536 | `expression.rs:181-215` | `examples/vectors.ae:3`; `examples/option_propagate.ae:23` (`vec[1, 2, 3]`) |
| 5.7 | struct literal `Name { f: e, ... }` — any order, each exactly once, all required; trailing comma accepted (**[probe]**) | `types.rs: struct_literal` 139 | `tests/run/methods.ae:55` |
| 5.8 | generic struct literal `Name[a, b] { ... }` | `expression.rs:342-387` | `tests/execution.rs:1185` |
| 5.9 | `Self { ... }` in impl | `expression.rs:388-406` | `tests/run/self_type.ae:5` |
| 5.10 | enum literal `Name::Variant(e, ...)` — payload count exact, trailing comma rejected (**[probe]**); `Name(args)` where Name is a struct is rejected ("is a struct, not a function") | `types.rs: enum_literal` 108 | `examples/enums.ae` |
| 5.11 | generic enum literal `Name[args]::Variant(...)` / `::None` / `::None()` | `expression.rs:289-341` | `examples/option_propagate.ae:4` (`Option[string]::Some`, `Option[string]::None()`) |
| 5.12 | `Self::Variant(...)` (enum impl, plain and generic-template) | `expression.rs:222-272` | `tests/run/enum_generic_impls.ae:26` (`Self::None`) |
| 5.13 | closure literal `|p: T, q: U| -> R { body }` and `|| -> R { body }` (`Or`+`Arrow` lookahead, expression.rs:108); param types mandatory; captures inferred by double probe-parse (borrow probe, then value probe); no mutation of captures; body must return on all paths | `closures.rs: closure_literal` 47 | `tests/run/closures.ae`; `examples/function_types.ae:19` |
| 5.14 | closure call `name(args)` where name: Closure — captures re-materialized at call site (value captures move) | `closures.rs: closure_call` 273 | `tests/run/closures.ae:11` (`add(3, 4)`); `exec#closure_captures_vector_and_index_expression` |
| 5.15 | fn-typed call `name(args)` where name: Fn — through interned signature pointer | `expression.rs:1059-1067` | `examples/function_types.ae:10` (`f(x)`) |
| 5.16 | function name as value argument (non-capturing closure or matching fn; extern/unsafe fns excluded) | `declarations.rs: function_value_argument` 1180 | `exec#closure_captures_function_value_and_its_argument` |

### 5.3 Postfix and calls

| # | Production | Parser | Witness |
|---|---|---|---|
| 5.17 | field read `e.f` (owned / &Struct / &mut Struct bases; enum has no fields) | `expression.rs:1169-1194, 1385-1394` | everywhere |
| 5.18 | method call `e.m(args)` — receiver is hidden arg 0; auto-borrow for `&self`/`&mut self` (mut needs `var` binding; borrow conflicts deferred to NLL graph, M2.5); by-value receiver moves | `expression.rs:1224-1383` | `tests/run/methods.ae:56`; `tests/run/struct_mut_borrow.ae` |
| 5.19 | method call on generic instantiation (template + origin args) | `expression.rs:1231-1251` | `tests/run/generic_impl.ae`; `tests/run/enum_generic_impls.ae` |
| 5.20 | explicit destructor call rejected ("explicit call to destructor 'drop' is not allowed") | `expression.rs:1253-1260` | `exec#user_defined_drop_rejects_direct_call_and_pub` |
| 5.21 | pointer postfix `p.deref()` (unsafe, E0700), `p.offset(n)` (element stride), `p.byte_offset(n)` (bytes) — unsafe-gated | `expression.rs:1177-1206` | `tests/run/ptr_offset.ae:17-21`; `tests/run/unsafe_pointer_deref.ae:12` |
| 5.22 | pointer retag `p as *const U` / `as *mut U` — only cast form in the language; non-pointer LHS leaves `as` unconsumed (**[probe]**: a stray `as` surfaces as a statement error) | `expression.rs:1138-1167` | `tests/run/ptr_retag.ae:18` (`raw as *mut f64`) |
| 5.23 | plain call `name(args)` — arity/type exact, ≤64 args, visibility, extern/unsafe calls need `unsafe` (E0701) | `expression.rs:1068-1088` + `declarations.rs: call_arguments` 1223 | everywhere |
| 5.24 | generic call `name[T, U](args)` — mandatory type args (**[probe]** omitted → "'…' is a generic function; write its type arguments") | `expression.rs:1035-1053` | `examples/generic_fn.ae`; `tests/run/trait_bounds.ae:21` (`combine_flags[Flags]`) |
| 5.25 | associated call `Type::f(args)` / `Self::f(args)`; generic `Name[args]::f(args)` | `expression.rs: associated_call` 12; 246-263; 314-338; 356-382 | `tests/run/methods.ae:81` (`Counter::starting`); `exec#self_associated_call_on_plain_enum_resolves_through_the_impl_block` |
| 5.26 | trait-method call inside a generic body (dispatch through `(trait, method, substituted type)`) | `declarations.rs: resolve_trait_call` 870 | `tests/run/bitwise_traits.ae:13`; `exec#trait_dispatch_resolves_per_type…` |
| 5.27 | indexing `v[i]` — **bare variables only** (array/vec/vec[f64]/vec[string]/borrowed forms); indices i64 (E0201); bounds-checked | `expression.rs:1099-1121` | `examples/vectors.ae`; `exec#arrays_support_checked_read_write_and_expression_order` |
| — | **GAP**: no chained indexing `v[0][1]` (**[probe]** "expected RParen, found LBracket"); no indexing of fields/call results `s.items[0]`, `lines(t)[0]` (**[probe]**); no map-index read `m[k]` (**[probe]** — only `map_get`); no postfix `[..]` at all in the postfix loop | — | — |

### 5.4 Name-dispatched builtins (all `Kind::Ident`; frozen by `RESERVED`, mod.rs:199)

These are grammar-relevant because each has a dedicated parse arm with fixed
shape, and none can be shadowed or redefined:

| Group | Names (arity/shape) | Witness |
|---|---|---|
| lengths | `len(e)` (string/view/vec/map/array; tokens cursor rejected) | `exec#len_reports_array_lengths_and_string_sizes` |
| string ops | `read_file(w)`, `write_file(w, c)`, `bytes(w)`, `contains(w, p)`, `starts_with(w, p)`, `ends_with(w, p)`, `slice(w, i, j)`, `split(w, sep)`, `trim(w)`, `find(w, p)`→IntResult, `count_lines_with(w, p)`, `lines(w)`, `join(strvec-var, sep)`, `utf8_len(w)`, `utf8_prefix(w, n)` | `exec#contains_and_slice_cover_text_processing`, `exec#fused_split_and_lines_keep_their_edge_cases`, `exec#utf8_length_and_prefix_count_characters_not_bytes`, `examples/split_arena.ae:13` |
| views | `slice_view(base, i, j)` → StrView (base = string var or struct-var field; borrows base) | `tests/run/slice_view.ae`; `exec#slice_views_are_zero_copy_and_borrow_tracked` |
| cursors | `tokenize(base, sep)` → Tokens (same base rule) | `examples/tokens.ae` |
| vectors | `strvec_new()`, `f64vec_new()` | `exec#string_vectors_start_empty_and_copy_borrowed_elements`; `examples/float_stats.ae` |
| maps | `map_new()`, `f64map_new()`, `map_get(m, k)`→result, `map_has(m, k)`, `map_remove(m, k)`, `map_inc(m, k, d)`, `keys(m)` — place = var or var.field | `examples/map_count.ae`; `exec#map_inc_counts_and_returns_the_new_value`; `exec#maps_survive_storage_in_struct_fields` |
| fallible io/fs | `try_read_file`, `try_write_file`, `try_append_file`, `try_list_dir`, `try_file_info`, `try_copy_file`, `try_rename_path`, `try_remove_file`, `try_remove_dir`, `try_create_dir` → prelude result enums | `exec#fallible_file_io_reports_errors_as_values`; `exec#file_operations_create_copy_rename_and_remove` |
| paths | `path_exists`, `path_is_dir`, `path_join`, `path_name`, `path_parent`, `set_cwd` | `exec#path_algebra_joins_names_and_parents` |
| process | `args()`, `env(name)`, `read_stdin()`, `cwd()`, `run_command(prog, strvec-var)`, `now_ms()`, `now_s()` | `exec#split_and_args_produce_string_vectors`, `exec#environment_lookup_separates_missing_from_empty`, `exec#run_command_reports_status_and_output`, `exec#clock_reports_a_plausible_epoch` |
| json | `json_get`, `json_int`, `json_bool`, `json_len`, `json_has` | `exec#json_paths_select_members_elements_and_escapes` |
| conversions | `parse_int(w)`, `to_string(i/f)`, `to_f64(i)`, `to_i64(f)`→IntResult | `examples/float_math.ae`; `exec#float_rejections_keep_conversions_explicit` |
| math (ADR-60) | `sqrt abs floor ceil round exp log sin cos` (1), `pow atan2 min max` (2); all-f64 args | `exec#math_builtins_compute_report_domain_errors` |
| layout | `c_size_of[T]()`, `c_align_of[T]()`, `c_offset_of[T]("field")` — bracket type-argument form; scalar fast path for i64/f64/bool inside generics | `tests/run/generic_layout_query.ae`; `exec#repr_c_layout_queries_and_extern_prototypes` |
| unsafe exprs | `ptr_write(p, v)`, `ptr_drop_in_place(p)` — unsafe-only, `*mut` | `exec#mir_branch_observes_writes_through_raw_pointers` (tests/execution.rs:6030); `exec#ptr_drop_in_place_drops_struct_in_place_and_requires_unsafe` |

---

## 6. Patterns (match arms only)

| # | Production | Parser | Witness |
|---|---|---|---|
| 6.1 | `EnumName::Variant` unit pattern (instantiated enums use the **declaration** name, not the display name) | `parser.rs:1009-1019` | `examples/option_propagate.ae:31` (`Option::Some(s)` / `Option::None`) |
| 6.2 | `EnumName::Variant(a, b)` identifier bindings, exact payload arity, extra comma rejected (**[probe]**); string→StrView, vec→borrowed refs (read-only) | `parser.rs:1025-1055` | `exec#enums_match_payloads_execute` (`Shape::Circle(r)`, `Shape::Rect(w, h)`) |
| — | **GAP**: no wildcard `_` (**[probe]** "pattern must name the matched enum"), no guards (**[probe]** "expected FatArrow, found If"), no literal patterns, no nested patterns (nested payload types rejected at declaration, **[probe]**), no `|`-alternation, no binding-by-value/move-out | — | — |

---

## 7. Kind-enum suggests, parser rejects (documented gaps)

| Kind / token | Suggests | Reality |
|---|---|---|
| `Const` | const declarations/items | only `*const T` in pointer types (**[probe]** "expected Fn, found Const") |
| `As` | general value casts (`x as i64`) | pointer retags only; `&v as *const T` prefix form |
| `Question` | `?` in expressions | statement-tail propagation only (let-tail / call-stmt-tail); main rejected |
| `DotDot` | range expressions / open ranges | only `for x in a..b` statement header (**[probe]** "expected Semi, found DotDot") |
| `Hash` | general attributes | `#[repr(C)]` and `#[no_mangle]` only, by exact name |
| `Print` (keyword) | printable expression | statement-only; reserved name |
| `Text` escapes | full escape set | `\n \t \r \\ \"` only (**[probe]** `\u0041` E0002) |
| two-char lexemes | `!=` etc. | fine — but there is **no** `&=`/`\|=`/`^=`/`<<=`/`>>=`: elements/fields reject compound ops (**[probe]** `v[0] += 1` → "expected Equal, found PlusEqual") |
| lexer numbers | numeric suffixes/bases | no `0x`, no `_`, no suffixes (**[probe]**) |
| `Trait`/`Impl` | default methods, generic impls over params | trait methods are signature-only (`;`); impl targets concrete |
| `Match` | match expressions / guards / wildcards | statement-only, exhaustive, identifier-bind patterns only |

## 8. Parser accepts, Kind enum does not suggest (accidental syntax)

| Finding | Detail |
|---|---|
| `|` / `||` double duty | bitwise-or tier 3 **and** closure syntax; `||` → empty closure params detected via next-token `Arrow` (expression.rs:108). A closure opener is decided in the *primary* position, bitwise-or in the *binary loop* — the new parser must keep both contexts. |
| `&` triple duty | bitwise-and (tier 5), call-argument/param borrows (`&x`, `&mut x`), address-of prefix (`&v as *const T`). |
| Ident-keywords | `pub`, `Self`, `self`, `const`-in-`*const`, `T` (single uppercase = type param), plus every builtin in §5.4 — all `Kind::Ident`; the `RESERVED` list (mod.rs:199) is part of the grammar contract. |
| `exit(code)` statement | not a builtin call arm — a statement-position Ident special case (parser.rs:370). Same for `push/pop/sort/reverse` (parser.rs:380). |
| `[i64; N]` array type and `[1,2,3]` literal | exist only in parameter annotations and let/var initializers respectively; invisible everywhere else. |
| `c_size_of[T]()` bracket form | type arguments in square brackets on a *call*, unique to the three layout queries. |
| Name-shaped statement grammar | statement dispatch is by (a) keyword, (b) known-variable name, (c) builtin statement name, (d) otherwise "must be a call ending in `?`" — an error-path-sensitive dispatch the new parser must reproduce for diagnostics. |
| Trailing-comma asymmetry | accepted: struct literals, enum declaration bodies, vec literals, struct declaration bodies; rejected: call args, enum payloads, match payloads, fn/closure/trait param lists (**[probe]** "too many arguments to 'f'" for `f(1,)`). |
| Shadowing | duplicate names are rejected across *nesting*, not just one scope (**[probe]** "duplicate variable 'x'" inside an if-block), contradicting the E0104 help text "inner blocks may shadow"; sibling blocks may reuse names (`exec#sibling_blocks_reuse_names_without_affecting_outer_scope`). Closure bodies allow shadowing via `shadow_ok` (closures.rs:171). |
| `var` parameters | `fn f(var x: i64)` — accepted (writable slot), **no witness in examples/tests/run**. |
| `let`/`var` arrays | `var a = [1,2,3];` accepted (**[probe]**), arrays then mutable via element stores. |
| `pub impl` | accepted (`take_pub` before `impl`), semantically a no-op (**[probe]**). |
| `use "@alias"` | syntactically a Text token; `@` prefix is a package-manifest concern inside the loader, invisible to the parser proper. |
| Doc comments | `///` is not a token; only `unsafe fn` precondition presence is enforced by re-scanning raw source (E0704). |

## 9. Rejection probes (all run via `target/debug/ae.exe check`, temp fixtures)

| Fixture | Error (verbatim) | Meaning for M0.2 |
|---|---|---|
| `let x = flag ? 1 : 2;` | `'?' needs a result or option value, found bool` | `?` binds to the whole let-initializer |
| `let y = match …;` | `expected expression` | match is statement-only |
| `let r = 1..5;` | `expected Semi, found DotDot` | no range expressions |
| `let x: i64 = 1;` | `expected Equal, found Colon` | no let annotations |
| `const N = 5;` (top) | `expected Fn, found Const` | no const items |
| `use "x" as n;` | `expected ';' after 'use' path` | no use-alias clause |
| `/* */` | `expected declaration, assignment, print, or '}'` | no block comments |
| `0x10` | `expected Semi, found Ident("x10")` | no hex |
| `v[0][1]` | `expected RParen, found LBracket` | no chained index |
| `print(s.items[0])` / `lines(t)[0]` | `cannot print a value of type vec…` (stray `[` left) | no postfix index |
| `m["a"]` (read) | `cannot print a value of type map[string, i64]` | no map index read |
| `_ =>` in match | `pattern must name the matched enum ('E::Variant')` | no wildcard |
| `E::A(a) if a > 0` | `expected FatArrow, found If` | no guards |
| `enum Outer { W(Inner) }` | `enum variant payloads must be i64, bool, f64, string, vec, vec[string], or a type parameter` | no nested payloads |
| `let x = unsafe { 1 };` | `expected expression` | unsafe is statement-only |
| `v[0] += 1;` | `expected Equal, found PlusEqual` | no compound element assign |
| `f(1,)` | `too many arguments to 'f'` | no call trailing comma |
| `E::A(1,)` | `variant 'E::A' takes 1 payload value(s)` | no payload trailing comma |
| `fn main() -> i64` | `expected LBrace, found Arrow` | main has no return annotation |
| `let x = print(1);` | `expected expression` | print is statement-only |
| `match get() {…}` | `match needs a bare enum variable as its subject` | subject is a slot, not a value |
| `else print(2);` | `expected LBrace, found Print` | else requires a block |
| `{ let x = 2; }` free block | `expected declaration, assignment, print, or '}'` | no block statements |
| `if true { let x = 2; }` under outer `x` | `duplicate variable 'x'` | no nested shadowing |
| `1 + f()?` (f→enum) | `arithmetic requires integer operands` | `?` never mid-expression |
| `f()?;` in main | `'?' needs the enclosing function to return E…` | propagation needs matching return |

## 10. Gap list (condensed)

Accepted but unwitnessed (port these with care; behavior locked only by code):

1. `var` parameter slots (`fn f(var x: i64)`) — declarations.rs:215, 312, 982.
2. Compound assignment on struct fields `s.f += v;` — parser.rs:587-614.
3. Map put through a struct field `s.f[k] = v;` — parser.rs:559-574.

Rejection-only gaps (spec'd by absence; new parser should keep the errors):

4. No tuples, no slice types, no `const` items, no type annotations on `let`.
5. No match wildcards/guards/literal/nested patterns; match is statement-only
   with a bare-variable subject.
6. No postfix indexing at all (chained, field, call-result); no map-index read.
7. No range expressions, no ternary, no `?` in expression position, no
   labeled loops, no block expressions/statements.
8. Lexer: no block comments, no hex/bin/underscore/suffixed literals, only
   five string escapes.

Witnesses for everything else are cited inline in §2-§6.
