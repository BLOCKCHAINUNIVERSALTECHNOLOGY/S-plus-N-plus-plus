# Phase A — Production confidence

## Goals
1. Fuzz / integration suite growth
2. Sandbox flags for `fs_*` / network
3. Versioned bytecode artifact (`SNPBC2`)
4. FILEMAP + release ZIP + changelog discipline

## Sandbox
```bash
snp run --sandbox file.snp
# or
SNP_SANDBOX=1 snp run file.snp
SNP_ALLOW_FS=0 snp run file.snp
```

Embedder:
```rust
use splusnpp::vm::{set_sandbox, Sandbox};
set_sandbox(Sandbox { allow_fs: false, allow_net: false });
```

Blocked when `allow_fs = false`:
`fs_read_text`, `fs_write_text`, `fs_read_lines`, `fs_append_text`, `fs_exists`, `fs_remove`

## Bytecode ship
```bash
snp build file.snp   # writes file.snp.snpbc
```

Header:
```
SNPBC2
version=2.24.0
funcs=N
fn main
---
...
```

`inspect_artifact(bytes)` validates header.

## Still open in Phase A
- Full binary bytecode encode/decode (not Debug dump)
- Fuzz corpus automation
- Network API surface (when added) must call `require_net`
