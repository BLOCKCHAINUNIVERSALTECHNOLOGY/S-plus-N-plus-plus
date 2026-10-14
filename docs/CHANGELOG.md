# Changelog

## [0.3.1] — 2026-10-14

### Added
- Per-embedder `ffi::Engine` host registry isolate
- Soft recursion and VM stack guards
- MIR jump-threading optimization
- `snp fmt`, `snp doc`, `//` comments and style guidance
- Golden examples for game loops, plugin hosts and documentation extraction

### Fixed
- Deterministic FFI tests when the process-global host registry is accessed in parallel
- Removed a duplicate unreachable enum-expression checking branch
- Aligned documentation version metadata with the 0.3.1 release

## [0.3.0] — 2026-10-13

### Added
- Struct **methods** (`impl Type { fn method(self, ...) }`)
- **Enums** + variant patterns (`Option.Some(n)`)
- Match **guards** (`pat if cond => ...`)
- **Struct patterns** (`Player { hp, name }` / rename)
- **Or-patterns** (`1 | 2 | 3 => ...`)
- Nested **`mod` blocks** + **`pub`** (max depth 8; private default inside modules)
- **Result** prelude convention: `Result.Ok(Int)` / `Result.Err(String)`

### Notes
- Or-patterns do not bind variables across alternatives
- Private functions are only callable from the same module path

# Changelog

## [0.2.0] — 2026-10-13

### Added
- Runtime **stack traces** on errors
- `std.fs` (`fs_read_text`, `fs_write_text`, `fs_exists`)
- `std.json` (`json_stringify`, `json_parse` — minimal)
- `std.test` (`assert_eq`, `assert_true`, `assert_false`)
- `snp test` runner (`tests/snp/*.snp`)
- `snp bench` micro-harness
- List/Map **Arc COW** (`make_mut`) for fewer deep clones
- Stable **embedder FFI API** (`register_host`, arg helpers, `docs/FFI.md`)
- Registry **download + on-disk cache** (`PackageCache`, `snp fetch`)
- **MIR-only** execution path + const-fold / dead-block opts
- Deeper **LLVM IR** and **WASM text** CFG backends
- `snp --version` / `VERSION = 0.2.0`

### Changed
- Primary codegen is HIR → MIR → optimize → bytecode (AST bytecode path retained only as dead-code reference)
- Diagnostics report multi-line runtime traces

### Notes
- Zero external crate dependencies remain the default
- Native object emit (llvm-sys / Cranelift) still optional future work

## [0.1.0] — prior

Solid core: lexer/parser/sema, structs, match, closures+captures, multi-file `mod`,
concurrency, packages, HIR/MIR foundation, host FFI builtins, `emit-llvm` / `emit-wasm` stubs.
