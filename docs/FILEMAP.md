# File map — S+N++ 2.13.0

## Root
- `Cargo.toml` — package version **2.13.0**, binary `snp`
- `src/main.rs` — entry → `cli::run`
- `src/lib.rs` — modules + `VERSION = "2.13.0"`

## Compiler pipeline (`src/`)
| Module | Role |
|--------|------|
| `lexer/` | Tokenize (comments `//` `#`, floats, …) |
| `parser/` | AST: generics `fn[T]` / `struct[T]` / `enum[T]`, methods, match, pipeline |
| `ast/` | Expr, Stmt, Type (`Generic`, `StructApp`, `EnumApp`, `ListOf`, …), Value |
| `sema/` | Types, ownership, traits, **monomorphize** (fns + structs), desugars |
| `hir/` | High-level IR |
| `mir/` | CFG / basic blocks |
| `vm/` | Bytecode + builtins + execute |
| `cli/` | run / check / build / test / bench / registry |
| `diagnostics/` | error spans + source snippets |
| `loader/` | multi-file `mod` + full pipeline (check → mono → execute) |
| `backend/` `ffi/` `lsp.rs` `registry/` `package/` `stdlib/` | tooling |

## Generics (2.10–2.13)
| Feature | Where |
|---------|--------|
| `fn id[T](x: T) -> T` | parser + sema + **mono → `id__Int`** |
| `struct Box[T] { value: T }` | parser + sema + **mono → `Box__Int`** |
| `enum Opt[T] { None, Some(T) }` | parser + sema (typecheck; enum mono optional next) |
| Traits + `where` | `src/sema/mod.rs` expand + bounds |

## Examples (recent)
| File | Demo |
|------|------|
| `examples/v29_generics.snp` | function generics |
| `examples/v29_traits.snp` | traits / defaults |
| `examples/v30_mono.snp` | function monomorphization |
| `examples/v31_generic_struct.snp` | generic structs |
| `examples/v32_generic_enum.snp` | generic enums |
| `examples/v33_struct_mono.snp` | struct monomorphization |
| `examples/v26_float.snp` … `v28_*.snp` | float, stdlib, methods, spans |
| `examples/traits.snp`, `game_loop.snp`, `tool_todo.snp` | older demos |

## Tests
- `tests/integration.rs` — end-to-end
- `tests/snp/*.snp` — script tests
- Unit tests: mainly `src/vm/mod.rs` (`mod tests`) — **~184+** lib tests

## Docs
| File | Content |
|------|---------|
| `docs/ZIP_GUIDE_HI.md` | Hindi guide for this ZIP |
| `docs/CHANGELOG.md` | version history |
| `docs/ROADMAP.md` | forward plan |
| `docs/ARCHITECTURE.md` | pipeline overview |
| `docs/HARDEN.md` | harden notes |
| `docs/FILEMAP.md` | this file |

## Upload to GitHub
1. Unzip this archive
2. Copy contents into repo root (or replace tree)
3. `cargo test` then `cargo build --release`
4. `./target/release/snp run examples/v33_struct_mono.snp`
