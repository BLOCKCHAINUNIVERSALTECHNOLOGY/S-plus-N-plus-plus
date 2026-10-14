# S+N++ v0.3.1 — Complete File Map & Inventory

**Repository:** Sanjaynain754/S-N-  
**Version:** 0.3.1  
**Language:** S+N++ (Rust compiler + bytecode VM)  
**Binary:** `snp` | **Library:** `splusnpp`

This document lists **every** source file, what it does, and where it belongs.

---

## Root

| File | Role |
|------|------|
| `Cargo.toml` | Package metadata, version **0.3.1**, bin `snp`, lib `splusnpp` |
| `Cargo.lock` | Dependency lock (minimal deps) |
| `README.md` | Quickstart (≤2 min), feature table, layout |
| `SPEC.md` | Language specification |
| `REGISTRY_PROTOCOL.md` | Package registry HTTP protocol |
| `.gitignore` | Ignores `target/`, artifacts |

---

## `docs/` — Documentation

| File | Role |
|------|------|
| `docs/ROADMAP.md` | Version roadmap 0.1 → 0.4+ with checkboxes |
| `docs/CHANGELOG.md` | 0.2.0, 0.3.0, **0.3.1** release notes |
| `docs/ARCHITECTURE.md` | Compiler pipeline & module map |
| `docs/FFI.md` | Embedder FFI guide + ABI rules |
| `docs/STYLE.md` | Style guide (`snp fmt`, naming, modules) |
| `docs/FILEMAP.md` | **This file** — full inventory |

---

## `src/` — Compiler & Runtime (Rust)

| File | Role | Major features |
|------|------|----------------|
| `src/main.rs` | Thin CLI entry → `cli::run` | — |
| `src/lib.rs` | Crate root, `VERSION = "0.3.1"`, module exports | Engine re-export |
| `src/ast/mod.rs` | AST: Expr, Stmt, Pattern, Function, Struct, Enum, Value | MatchArm, Visibility, module_path, Pattern::{Struct,Or,Variant} |
| `src/lexer/mod.rs` | Tokenizer | `pub`, `\|` (Pipe), `//` comments, `#` comments |
| `src/parser/mod.rs` | Recursive-descent parser | nested `mod`, `pub`, or-patterns, struct patterns, guards, methods |
| `src/sema/mod.rs` | Typecheck + ownership + definite assignment | Result prelude, private visibility, function-scoped errors |
| `src/hir/mod.rs` | High-level IR | Or-pattern expansion |
| `src/mir/mod.rs` | Mid-level IR (CFG) | const-fold, **jump threading**, dead blocks |
| `src/backend/mod.rs` | LLVM IR text + WASM text emitters | `snp emit-llvm`, `snp emit-wasm` |
| `src/vm/mod.rs` | Bytecode compiler + stack VM | Arc List/Map, closures, soft recursion/stack limits |
| `src/stdlib/mod.rs` | std modules (io, string, collections, fs, json, test) | — |
| `src/loader/mod.rs` | Multi-file `mod` loading | — |
| `src/package/mod.rs` | `snp.toml`, lockfile | — |
| `src/registry/mod.rs` | HTTP registry fetch + `~/.cache/snp` | `snp fetch` |
| `src/cli/mod.rs` | CLI commands | run, check, build, repl, test, bench, fmt, doc, fetch, emit-* |
| `src/diagnostics/mod.rs` | Error formatting + stack traces | `format_sema_error` |
| `src/ffi/mod.rs` | Host registry + **`Engine` isolate** | register_host, execute_with_hosts |
| `src/fmtutil.rs` | `snp fmt` + `snp doc` helpers | format_source, extract_docs |

---

## `examples/` — Sample programs (.snp)

| File | Demonstrates |
|------|----------------|
| `hello.snp` | Basics |
| `new_features.snp` | Early feature set |
| `full_features.snp` | structs, for-in, match, interpolation |
| `closures.snp` | Closures + captures |
| `methods.snp` | `impl` methods |
| `enums.snp` | Enums + match |
| `match_guards.snp` | `pat if cond =>` |
| `struct_match.snp` | Struct patterns |
| `or_patterns.snp` | `1 \| 2 \| 3` |
| `modules.snp` | Nested `mod` + `pub` |
| `result_conv.snp` | Result.Ok / Result.Err |
| `game_loop.snp` | Game-style loop (0.3.1 golden) |
| `plugin_host.snp` | Module as plugin API |
| `docs_demo.snp` | `///` doc comments |
| `fs_json.snp` | fs + json stdlib |
| `bench_arith.snp` / `bench_list.snp` | `snp bench` |
| `ffi_host.snp` | extern + host |
| `stdlib_import.snp` | std imports |
| `string_collections.snp` | string/list/map APIs |
| `advanced_collections.snp` | list_set / map |
| `multi/main.snp` + `multi/utils.snp` | Multi-file project |

---

## `tests/`

| File | Role |
|------|------|
| `tests/integration.rs` | End-to-end Rust integration tests |
| `tests/snp/test_math.snp` | `snp test` assert samples |
| `tests/snp/test_json.snp` | json assert samples |

---

## Version history (what shipped where)

### 0.1.x — Core freeze
Lexer, parser, basic VM, ownership moves, concurrency primitives.

### 0.2.0 — Professional jump
Stack traces, std.fs/json/test, `snp test` / `snp bench`, Arc collections, FFI docs, registry cache, MIR path, LLVM/WASM text backends.

### 0.3.0 — Language depth
- Methods (`impl`)
- Enums + variant patterns
- Match guards
- Struct patterns
- Or-patterns
- Nested modules + `pub` (depth ≤ 8)
- Result prelude

### 0.3.1 — Quality close (current)
- Function-scoped sema errors
- `ffi::Engine` isolate
- Soft recursion / stack guards
- MIR jump threading
- `snp fmt`, `snp doc`, `//` comments, STYLE.md
- Golden examples (game_loop, plugin_host, docs_demo)

### 0.4+ (planned, not in this ZIP)
Full borrow checker, traits, LSP, public registry hosting, native object emit.

---

## CLI cheatsheet

```text
snp --version
snp run <file.snp>
snp check <file.snp>
snp build <file.snp>
snp test [tests/snp]
snp bench <file.snp>
snp fmt <file.snp>
snp doc <file.snp>
snp emit-llvm <file.snp>
snp emit-wasm <file.snp>
snp fetch <package>
snp repl
```

---

## Build

```bash
cargo test
cargo build --release
./target/release/snp run examples/hello.snp
```

**Note:** If the mount is `noexec`, build with:
`CARGO_TARGET_DIR=/tmp/snp-target cargo build`
