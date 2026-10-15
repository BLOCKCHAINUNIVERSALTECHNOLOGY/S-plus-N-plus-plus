# S+N++ v0.5.0 — Complete File Map & Change Inventory

**Version:** 0.5.0  
**Binary:** `snp` | **Library:** `splusnpp`  
**Tests:** ~107 (unit + integration)

This document lists **every** important path, what changed across 0.2→0.5, and where features live.

---

## Root

| File | Role |
|------|------|
| `Cargo.toml` | version **0.5.0**, bin `snp`, lib `splusnpp` |
| `Cargo.lock` | lockfile |
| `README.md` | quickstart |
| `SPEC.md` | language specification |
| `REGISTRY_PROTOCOL.md` | registry HTTP protocol |
| `.gitignore` | ignores `target/` |

---

## `docs/`

| File | Role |
|------|------|
| **FILEMAP.md** | this inventory |
| **ROADMAP.md** | 0.1 → 0.5+ checklist |
| **CHANGELOG.md** | release notes per version |
| **ARCHITECTURE.md** | pipeline aligned with vision diagrams |
| **NATIVE.md** | **0.5** JIT/native research (Cranelift path) |
| **FFI.md** | embedder host API |
| **LSP.md** | `snp lsp` editor integration |
| **STYLE.md** | style + `snp fmt` |

---

## `src/` — compiler & runtime

| Path | Role | Key 0.3–0.5 features |
|------|------|----------------------|
| `ast/mod.rs` | AST | Pattern::{Struct,Or,Variant}, TraitDef, Visibility, trait_bounds |
| `lexer/mod.rs` | tokens | `pub`, `trait`, `where`, `\|`, `//` comments |
| `parser/mod.rs` | AST build | nested mod, traits, where-clause, match patterns |
| `sema/mod.rs` | types + ownership + traits | Result prelude, expand defaults, where-check, move diagnostics |
| `hir/mod.rs` | HIR | or-pattern expansion |
| `mir/mod.rs` | CFG MIR | const-fold, jump threading, dead blocks |
| `backend/mod.rs` | emitters | LLVM, WASM, **NativePlan (0.5)** |
| `vm/mod.rs` | bytecode VM | Arc List/Map, soft recursion/stack limits |
| `stdlib/mod.rs` | std modules | fs, json, test, string, collections |
| `loader/mod.rs` | multi-file | trait expand on load |
| `package/mod.rs` | snp.toml / lock | — |
| `registry/mod.rs` | client + **host** | `serve_local` **0.5** |
| `cli/mod.rs` | CLI | lsp, fmt, doc, emit-native, registry-serve |
| `diagnostics/mod.rs` | errors | stack traces, sema prefixes |
| `ffi/mod.rs` | hosts | `Engine` isolate, exec lock |
| `fmtutil.rs` | fmt/doc | — |
| `lsp.rs` | **LSP server** | hover + diagnostics |
| `lib.rs` / `main.rs` | crate root | `VERSION = "0.5.0"` |

---

## Version → what landed

### 0.2
Stack traces, std.fs/json/test, bench, Arc collections, FFI, registry **client**, MIR, LLVM/WASM text.

### 0.3.0
`impl` methods, enums, match guards, struct patterns, or-patterns, nested `mod`+`pub`, Result prelude.

### 0.3.1
`Engine`, soft guards, jump threading, `snp fmt`/`doc`, STYLE, golden examples.

### 0.4.0
Traits, default methods, `where Type: Trait`, borrow messages, tests≥100, `snp lsp`.

### 0.5.0 (this ZIP)
- `docs/NATIVE.md` + `snp emit-native`
- `snp registry-serve`
- deeper method-receiver moves

---

## CLI surface

```text
snp --version
snp run|check|build <file.snp>
snp test | snp bench
snp fmt | snp doc
snp lsp
snp emit-llvm | emit-wasm | emit-native
snp registry-serve [dir] [port]    # default port 4873
snp fetch | lock | init | repl
```

---

## Build

```bash
cargo test
cargo build --release
# if noexec mount:
CARGO_TARGET_DIR=/tmp/snp-target cargo build
```

## GitHub paste

Extract ZIP → copy contents of `S-N-/` into repo root → commit as `v0.5.0`.
