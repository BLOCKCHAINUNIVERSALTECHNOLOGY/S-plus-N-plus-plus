# S+N++ FFI / Embedder Guide (v0.2)

## Why this API

Game engines and tools often need a **small scripting language** that can call
host logic (damage formulas, save slots, UI hooks) without linking a full C ABI
toolchain. S+N++ uses a **safe host registry**: Rust functions registered by
name, invoked from `extern "C"` declarations inside `.snp` scripts.

No `unsafe`, no `libloading` required for the common case.

## Language syntax

```snp
extern "C" fn damage(base: Int, mult: Int) -> Int;

fn main() -> Int {
    return damage(10, 3);
}
```

Rules:
- ABI string must be `"C"` (only supported ABI in 0.2).
- Declaration ends with `;` (no body).
- Name must match a host registration **before** `execute` / `snp run`.

## Supported types across the boundary

| S+N++ type | Host `Value`        |
|------------|---------------------|
| `Int`      | `Value::Int(i64)`   |
| `Bool`     | `Value::Bool(bool)` |
| `String`   | `Value::Str(...)`   |
| `Unit`     | `Value::Unit` (return) |

**Not** supported on extern params/returns in 0.2: `List`, `Map`, structs,
closures, channels, threads.

## Rust embedder API

```rust
use splusnpp::ffi::{self, HostFn};
use splusnpp::{execute, Value};

fn damage(args: &[Value]) -> Result<Value, String> {
    ffi::expect_arity(args, 2, "damage")?;
    let base = ffi::arg_int(args, 0, "damage")?;
    let mult = ffi::arg_int(args, 1, "damage")?;
    Ok(Value::Int(base * mult))
}

fn main() {
    ffi::register_host("damage", damage);

    let out = execute(r#"
        extern "C" fn damage(base: Int, mult: Int) -> Int;
        fn main() -> Int { return damage(10, 3); }
    "#).expect("script failed");

    assert_eq!(out, Value::Int(30));
}
```

### API surface

| Function | Role |
|----------|------|
| `register_host(name, f)` | Add / replace binding |
| `unregister_host(name)` | Remove binding |
| `has_host(name)` | Query |
| `list_hosts()` | Sorted names |
| `reset_registry()` | Back to built-ins only |
| `call_host(name, args)` | Direct invoke |
| `resolve_externs(names)` | Preflight declarations |
| `execute_with_hosts(src, &[(name, f)])` | Register then run |
| `arg_int` / `arg_bool` / `arg_str` | Typed argument extract |
| `expect_arity` | Arity check |

### Built-in hosts

Always present after `reset_registry()`:

- `host_abs(x: Int) -> Int`
- `host_clamp(x: Int, lo: Int, hi: Int) -> Int`
- `host_strlen(s: String) -> Int`

Example: `examples/ffi_host.snp`.

## CLI

Scripts that only use built-in hosts work with:

```bash
snp run examples/ffi_host.snp
```

Custom hosts require linking against `splusnpp` as a library (embedder binary).

## Design notes

- Registry is process-global (`OnceLock` + `Mutex`). Fine for one embedder; for
  multi-isolate hosts, wrap scripts in separate processes or extend later with
  an `Engine` object (0.3+).
- Errors from host functions surface as VM runtime errors with **stack traces**.
