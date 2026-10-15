# Native / JIT Research (S+N++ 0.5)

## Goal
Keep **light + fast startup** (bytecode VM default) while opening a path to **native code** for hot loops / embedders.

## Current backends (stable)

| Backend | Output | Status |
|---------|--------|--------|
| Bytecode VM | in-memory ops | **default** |
| LLVM IR text | `.ll` | emit only (no link) |
| WASM text | `.wat` | emit only |

## Research directions

### A. Cranelift (preferred for light embeds)
- MIR basic blocks → Cranelift CLIF
- Integer + branch subset first
- Feature flag `cranelift` (optional dependency)
- Pros: pure Rust, no system LLVM, good for WASM/native

### B. llvm-sys / inkwell
- MIR → existing `.ll` emitter → bitcode → object
- Pros: mature optimizer
- Cons: heavy dependency, system LLVM version hell

### C. Custom micro-JIT
- Only for hot `while`/`for` integer loops
- mmap + dynasm-style emitters
- Highest maintenance cost — **not** first choice

## Recommended 0.5 plan

1. **Document** mapping MIR → machine ops (this file + `backend::native_plan`)
2. **Stub** `Backend::NativePlan` text report (what would be emitted)
3. **Later 0.5.x**: optional `cranelift` feature behind Cargo flag
4. Keep VM as default forever for scripting / tools

## MIR → native sketch

```
MirInstr::Binop(Add, a, b)  →  add reg_a, reg_b
MirInstr::Branch            →  cmp + jcc
MirTerminator::Return       →  ret
Call                        →  call / ABI stub
```

Register allocation: linear scan over MIR locals (research).

## Non-goals (0.5)
- Full GC / JIT deoptimize
- Cross-compile matrix in CI
