# File map — S+N++ 2.5.0

## Root
- `Cargo.toml` — package version **2.5.0**, binary `snp`
- `src/main.rs` — entry → `cli::run`
- `src/lib.rs` — modules + `VERSION = "2.5.0"`

## Compiler pipeline (`src/`)
| Module | Role |
|--------|------|
| `lexer/` | Tokenize source |
| `parser/` | AST (incl. method calls, tuples, for (i,x), match-expr, `_\|`) |
| `ast/` | Expr, Stmt, Type, Value, Pattern |
| `sema/` | Types, ownership, method/list/str desugar, match-expr desugar |
| `hir/` | High-level IR |
| `mir/` | CFG / basic blocks |
| `vm/` | Bytecode + builtins + execute |
| `cli/` | run / test / bench / registry |
| `diagnostics/` | errors |
| `backend/` | optional emit |
| `ffi/` `lsp.rs` `loader/` `registry/` `package/` `stdlib/` | tooling |

## Examples
- `examples/v06_impact.snp` … `v25_impact.snp` — version demos
- `examples/game_loop.snp`, `traits.snp`, `modules.snp`, …

## Tests
- `tests/integration.rs` — end-to-end pipeline tests
- `tests/snp/*.snp` — script tests
- Unit tests live mainly in `src/vm/mod.rs` (`mod tests`)

## Docs
- `docs/ZIP_GUIDE_HI.md` — **Hindi/easy guide for this ZIP**
- `docs/CHANGELOG.md`, `ROADMAP.md`, `ARCHITECTURE.md`, `NEXT_VERSIONS.md`
