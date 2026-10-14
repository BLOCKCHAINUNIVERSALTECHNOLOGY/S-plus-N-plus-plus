# S+N++ Roadmap

**Goal:** light + fast language for coding, game logic, and systems glue.

| Version | Status |
|---------|--------|
| **0.1.x** | Frozen solid core |
| **0.2.0** | Released — professional jump |
| **0.3.0** | Released — language depth (methods, enums, match, modules, Result) |
| **0.3.1** | **Current** — quality close, tooling and embedder improvements |

---

## Version 0.2.0 — Released

All priority tracks done:

1. Stack traces
2. `std.fs` / `std.json` / `std.test` + `snp test`
3. `snp bench` + Arc list/map
4. Embedder FFI API + `docs/FFI.md`
5. Registry download + `~/.cache/snp` + `snp fetch`
6. MIR-only path + const-fold / dead blocks
7. WASM CFG + deeper LLVM IR text

Also: `VERSION` / `snp --version` aligned, `docs/CHANGELOG.md`.

### Known 0.2 limits (not bugs — scope)

- `extern` boundary: Int / Bool / String / Unit only
- WASM/LLVM are **text** emitters (no linked native object by default)
- Ownership is moves + definite assignment, not full borrow checker
- Stdlib intentionally small

---

## Version 0.3 — Released

Theme: **language depth** without losing “light + fast”.

### 0.3 Language
- [x] Methods on structs (`impl Type { fn method(self, ...) }`) — 0.3 started
- [x] Enums / tagged unions (`enum E { A, B(T) }`, match variants)
- [x] Match **guards** (`pat if cond => ...`)
- [x] Struct patterns
- [x] Or-patterns (`1 | 2 | 3`) (`Type { field }`, `Type { field: bind }`)
- [x] Nested modules + `pub` visibility (depth ≤ 8)
- [x] `Result` prelude (`Result.Ok(Int)` / `Result.Err(String)`)

### 0.3 Compiler
- [ ] Full span on every sema error (not only parse)
- [x] Optional `Engine` isolate (per-embedder registry, not process-global only)
- [x] MIR more opts (copy prop light, jump threading)

### 0.3 Runtime / tooling
- [x] Soft recursion / memory guards for embedders
- [x] `snp fmt` and documented style guide
- [x] `///` doc comments + `snp doc` minimal
- [x] More golden examples (game loop, plugin host)

### 0.3 Quality bar
- [ ] ≥ 100 automated tests
- [x] CHANGELOG kept current
- [x] README quickstart ≤ 2 minutes

### Later (0.4+)
- Full borrow checker / lifetimes
- Traits / interfaces
- LSP, public registry hosting
- Optional `llvm-sys` / Cranelift object emit

---

*Ship 0.3.1 clean. Grow 0.4 features one at a time.*
