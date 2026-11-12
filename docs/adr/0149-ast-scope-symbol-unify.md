# ADR-0149: AST Node Kinds, Scope Trees, Symbol Bindings, and Type Unification

## Context
Under the Standard Library Blueprint (Domain C & M2 Self-Hosting Compiler frontend), semantic analysis and type inference require:
1. AST node category classification (`AstNodeKind`, `is_declaration`, `is_statement`).
2. Lexical scope tree hierarchy and nesting depth accounting (`LexicalScope`, `lexical_scope_root`, `lexical_scope_new_child`).
3. Symbol binding mutability classifications (`SymbolBindingKind`, `can_reassign`).
4. Primitive builtin type unification and numeric predicate checking (`BuiltinTypeId`, `builtin_type_can_unify`).

## Design
- `std/core/ast_node_kind.ae`: `AstNodeKind`, `ast_node_is_declaration`, `ast_node_is_statement`.
- `std/core/scope_tree.ae`: `LexicalScope`, `lexical_scope_new_child`, `lexical_scope_is_root`.
- `std/core/symbol_table.ae`: `SymbolBindingKind`, `symbol_binding_can_reassign`.
- `std/core/type_unify.ae`: `BuiltinTypeId`, `builtin_type_is_numeric`, `builtin_type_can_unify`.
