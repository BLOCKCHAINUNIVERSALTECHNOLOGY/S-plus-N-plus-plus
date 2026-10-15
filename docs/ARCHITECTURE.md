# S+N++ Architecture

**Version:** 0.4.0  
Aligned with the project vision diagrams (compiler front/back-end, memory model, IL/JIT path).

---

## 1. Classic compiler pipeline (vision ↔ S+N++)

```
 High-level language (.snp)
        │
        ▼
 ┌──────────────────┐     ┌─────────────────┐
 │ Lexical Analyzer │────▶│ Symbol / tables │  (env, structs, enums, traits)
 │ (lexer)          │     └─────────────────┘
 └────────┬─────────┘
          ▼
 ┌──────────────────┐
 │ Syntax Analyzer  │  parser → AST
 └────────┬─────────┘
          ▼
 ┌──────────────────┐     ┌─────────────────┐
 │ Semantic Analyzer│────▶│ Error Handler   │  diagnostics + stack traces
 │ (sema)           │     └─────────────────┘
 └────────┬─────────┘
          ▼
 ┌──────────────────┐
 │ Intermediate     │  HIR → MIR (CFG)
 │ Code Generation  │
 └────────┬─────────┘
          ▼
 ┌──────────────────┐
 │ Code Optimizer   │  const-fold, jump threading, dead blocks
 └────────┬─────────┘
          ▼
 ┌──────────────────┐
 │ Target / Runtime │  bytecode VM  |  LLVM IR text  |  WASM text
 └────────┬─────────┘
          ▼
     Execution / Assembly path (future native object)
```

| Vision block | S+N++ module |
|--------------|--------------|
| Lexical Analyzer | `src/lexer` |
| Syntax Analyzer | `src/parser` → AST |
| Semantic Analyzer | `src/sema` (types, ownership, traits) |
| Intermediate Code | `src/hir` + `src/mir` |
| Code Optimizer | `mir::optimize` |
| Target Code Generator | `src/vm` bytecode, `src/backend` (.ll / .wat) |
| Symbol Table Manager | sema env + Program structs/enums/traits |
| Error Handler | `src/diagnostics` |

Front-end = blue path (lexer → sema).  
Back-end = gray path (MIR → bytecode / LLVM / WASM).

---

## 2. Memory model (vision ↔ design choice)

Vision diagram contrasts **GC languages** (Python, Java, Swift/C#) with **ownership** (C++, Rust).

**S+N++ choice: ownership + moves** (Rust-like), not a tracing GC.

| Area | Behavior |
|------|----------|
| Stack | locals, call frames |
| Heap-like values | `List` / `Map` via **Arc COW** (shared until mutation) |
| Structs / enums | values + field maps |
| Threads / channels | OS threads + `mpsc` |
| Soft limits | recursion depth + value-stack caps (`Engine` / `set_runtime_limits`) |

Future (0.5+): optional GC interface only if embedders need it — not the default model.

---

## 3. IL → execution paths

Vision shows:

```
Intermediate Language → JIT → Native Code
                         ↘ Cache
                         ↘ Execution
```

**Today (0.4):**

```
MIR / bytecode  ──interpret──▶  VM execution   (primary, light + fast startup)
        │
        ├── emit-llvm  → .ll text
        └── emit-wasm  → .wat text
```

**Later (roadmap):**

```
bytecode / MIR  →  optional JIT / Cranelift / llvm-sys object
                     → native code + code cache
```

JIT workflow diagram (profiler, register allocator, scheduler) maps to a **future** backend; not required for the language to stay light.

---

## 4. Current crate layout

| Module | Responsibility |
|--------|----------------|
| `ast` | Types, expr, stmt, patterns, traits, values |
| `lexer` / `parser` | Tokens → AST |
| `sema` | Types, ownership, definite assignment, traits |
| `hir` / `mir` | Intermediate reps + opts |
| `vm` | Bytecode + stack VM |
| `backend` | LLVM IR / WASM text |
| `stdlib` / `loader` / `package` / `registry` | Modules, deps, fetch |
| `ffi` | Host registry + `Engine` isolate |
| `cli` / `diagnostics` / `fmtutil` | Tooling |

---

## 5. Design goals (unchanged)

- **Light** — small core, optional heavy backends  
- **Fast startup** — bytecode interpret first; JIT optional later  
- **Solid** — ownership, traits, modules, Result convention  
- **Scalable** — front-end / IR / back-end boundaries match the vision diagrams  

---

*Vision PDF diagrams are the north star for pipeline shape; implementation prioritizes a working VM + clean IR before full JIT.*
