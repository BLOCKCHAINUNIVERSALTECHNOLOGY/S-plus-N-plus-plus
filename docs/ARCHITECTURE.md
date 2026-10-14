# S+N++ Architecture

## Pipeline

```
.snp source
    │
    ▼
┌─────────┐
│  Lexer  │  → Token stream (keywords, ops, literals)
└────┬────┘
     ▼
┌─────────┐
│ Parser  │  → AST (Expr, Stmt, Function, Program)
└────┬────┘
     ▼
┌─────────┐
│  Sema   │  → Type check + ownership moves + definite assignment
└────┬────┘
     ▼
┌─────────┐
│   VM    │  → Bytecode compile + stack machine + runtime
└────┬────┘
     │
     ├─ backend::LlvmIr   → textual .ll (llc / clang)
     └─ backend::WasmText → textual .wat skeleton
     │
     ├── threads / channels (OS threads + mpsc)
     └── stdlib (io, string, collections, concurrency)
```

## Crate layout

| Module | Responsibility |
|--------|----------------|
| `ast` | Types, expressions, statements, values |
| `lexer` | Tokenization |
| `parser` | Recursive-descent → AST |
| `sema` | Static analysis (types, ownership, init) |
| `vm` | Bytecode + interpreter |
| `stdlib` | Built-in module registry |
| `package` | Manifests, lockfiles, local deps |
| `registry` | HTTP registry client protocol |
| `cli` | `snp` command surface |
| `diagnostics` | Structured errors (spans coming next) |

## Design goals

- **Light**: zero external crates by default
- **Fast startup**: interpret bytecode, no heavy JIT required for v1
- **Solid**: real ownership checks, definite assignment, concurrency
- **Scalable**: clear module boundaries so LLVM / HIR can slot in later
