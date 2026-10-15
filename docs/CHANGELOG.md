# Changelog

## [0.5.0] — 2026-10-14

### Added
- **Native/JIT research**: `docs/NATIVE.md`, `Backend::NativePlan`, `snp emit-native`
- **Registry hosting**: `snp registry-serve [dir] [port]` (local HTTP, protocol v1)
- Deeper ownership: method receivers move non-copyable values

### Notes
- Native plan is **not** executable machine code — research bridge to Cranelift
- Registry server binds `127.0.0.1` only by default

## [0.4.0] — 2026-10-14

Traits, default methods, `where Type: Trait`, borrow diagnostics, tests ≥100, `snp lsp`.

## [0.3.1] — 2026-10-14

Engine isolate, soft guards, MIR jump threading, fmt/doc, golden examples.

## [0.3.0] — 2026-10-13

Methods, enums, match (guards, struct/or patterns), nested mod + pub, Result prelude.

## [0.2.0] — prior

Stack traces, stdlib, FFI, registry client, MIR, LLVM/WASM text backends.
