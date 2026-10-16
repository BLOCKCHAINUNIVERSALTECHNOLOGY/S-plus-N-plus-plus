# S+N++ Roadmap

**Goal:** light + fast language for coding, game logic, and systems glue.

| Version | Status |
|---------|--------|
| **0.1.x** | Frozen solid core |
| **0.2.0** | Professional jump (stack, stdlib, FFI, registry client, MIR) |
| **0.3.0** | Language depth (methods, enums, match, modules, Result) |
| **0.3.1** | Quality close (Engine, guards, fmt/doc, MIR opts) |
| **0.4.0** | Traits, where-bounds, borrow messages, tests≥100, LSP |
| **0.5.0** | Native/JIT research, registry-serve |
| **0.5.1** | registry-publish + index checksums |
| **0.5.2** | local registry integration + cranelift research stub |
| **0.5.3** | nested module call paths (`a.b.v()`) |
| **0.6.0** | `?`, list map/filter/fold, exhaustive match, `const` |
| **0.7.0** | generics, `use`, `defer`, string ops |
| **0.8.0** | `|>` pipeline, list literals, assert |
| **0.9.0** | `..=`, if-let, Option, panic/todo |
| **1.0.0** | stable surface + expanded integration tests |
| **1.1.0** | `+=`, loop, while-let, list_slice |
| **1.2.0** | `&&`/`||`, range, list_rev/contains |
| **1.3.0** | `%`, str_join, list_sort, list_sum |
| **1.4.0** | min/max/any/all, str_index/substring, clone |
| **1.5.0** | map_len/keys/values/remove, str_repeat, list_flatten |
| **2.0.0** | find/zip/merge, empty checks, 2.0 milestone |
| **2.1.0** | if-expression, list_unique/chunk, str_lines |
| **2.1.1** | pipeline + integration upgrade |
| **2.2.0** | **Current** — match-expression, list_sort_by |

---

## Version 0.5 — Systems scale

### Native / JIT research
- [x] `docs/NATIVE.md` — Cranelift preferred path
- [x] `Backend::NativePlan` + `snp emit-native` (MIR → native-ish plan text)
- [x] Cranelift research stub (`backend::cranelift_research`)
- [ ] Optional `cranelift` Cargo feature (real codegen, later)

### Deeper borrow
- [x] Method receiver moves non-copyable `self`
- [x] Clearer move diagnostics (0.4+)
- [ ] Full lifetime / borrow checker (later)

### Registry hosting
- [x] `snp registry-serve [dir] [port]` — local HTTP registry (protocol v1)
- [x] Auto index from `packages/<name>/<ver>/`
- [x] `snp registry-publish` + on-disk index JSON
- [ ] Public hosted instance (ops, not language)

### Later
- Full borrow checker / lifetimes
- Cranelift object emit behind feature flag
- Public registry hosting (deployment)


## Current: **2.5.0** — method sugar + list_take/drop + str_trim
