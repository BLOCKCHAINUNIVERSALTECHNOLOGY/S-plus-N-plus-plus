# S+N++ Roadmap

**Goal:** light + fast language for coding, game logic, and systems glue.

| Version | Status |
|---------|--------|
| **0.1.x** | Frozen solid core |
| **0.2.0** | Professional jump (stack, stdlib, FFI, registry client, MIR) |
| **0.3.0** | Language depth (methods, enums, match, modules, Result) |
| **0.3.1** | Quality close (Engine, guards, fmt/doc, MIR opts) |
| **0.4.0** | Traits, where-bounds, borrow messages, tests≥100, LSP |
| **0.5.0** | **Current** — native/JIT research, deeper borrow, registry hosting |

---

## Version 0.5 — Systems scale

### Native / JIT research
- [x] `docs/NATIVE.md` — Cranelift preferred path
- [x] `Backend::NativePlan` + `snp emit-native` (MIR → native-ish plan text)
- [ ] Optional `cranelift` Cargo feature (future 0.5.x)

### Deeper borrow
- [x] Method receiver moves non-copyable `self`
- [x] Clearer move diagnostics (0.4+)
- [ ] Full lifetime / borrow checker (later)

### Registry hosting
- [x] `snp registry-serve [dir] [port]` — local HTTP registry (protocol v1)
- [x] Auto index from `packages/<name>/<ver>/`
- [ ] Public hosted instance (ops, not language)

### Later
- Full borrow checker / lifetimes
- Cranelift object emit behind feature flag
- Public registry hosting (deployment)
