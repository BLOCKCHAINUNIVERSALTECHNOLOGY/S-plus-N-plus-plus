# S+N++

**S+N++** **v0.2.0** — a readable, statically-oriented systems language (Rust-based compiler + bytecode VM).

Goals: simple syntax, clear diagnostics, safe ownership, practical concurrency.  
Light enough for tools & game logic; solid enough for real systems work.

## Project layout

```text
S-N-/
├── Cargo.toml          # binary `snp` + library `splusnpp`
├── README.md
├── SPEC.md             # language specification
├── REGISTRY_PROTOCOL.md
├── docs/
│   └── ARCHITECTURE.md # pipeline & module map
├── examples/           # .snp samples
├── tests/
│   └── integration.rs  # end-to-end pipeline tests
└── src/
    ├── main.rs         # thin CLI entry
    ├── lib.rs          # crate root + runtime tables
    ├── ast/            # types, expr, stmt, value
    ├── lexer/          # tokenizer
    ├── parser/         # recursive-descent → AST
    ├── sema/           # typecheck + ownership + definite assignment
    ├── vm/             # bytecode compiler + stack VM
    ├── stdlib/         # std.io / string / collections / concurrency
    ├── package/        # snp.toml, lockfiles, deps
    ├── registry/       # HTTP registry protocol client
    ├── cli/            # snp run|check|build|repl|init|lock
    └── diagnostics/    # structured errors (spans next)
```

## Quick start

```bash
cargo test
cargo build --release
./target/release/snp run examples/hello.snp
./target/release/snp run examples/new_features.snp
```

## Language features (v1)

| Area | Features |
|------|----------|
| Types | `Int`, `String`, `Bool`, `Unit`, `List`, `Map`, `Channel`, `Thread`, `&T`, `&mut T` |
| Control | `if` / `else` / `else if`, `while`, `for i in a..b`, `break`, `continue`, `return` |
| Ops | arithmetic, comparison, unary `-` / `!`, indexing `xs[i]` / `m["k"]` |
| Ownership | moves, definite assignment, branch-sensitive init |
| Concurrency | `spawn`, `join`, `channel`, `send`, `receive` |
| Stdlib | `std.io`, `std.string`, `std.collections`, `std.concurrency` |
| Tooling | `snp run` · `check` · `build` · `repl` · `init` · `lock` · `package-check` |

---

## Version 1.0 milestone

इस milestone में repository का executable अब thin binary entrypoint और testable library crate में विभाजित है। Current implementation में lexer, parser, AST, type checker, definite-assignment analysis, ownership/move analysis, bytecode compiler, stack VM और CLI workflow मौजूद हैं।

Language features में functions, typed parameters, `Int`, `String`, `Bool`, `Unit`, `Channel` और `Thread` values, arithmetic/comparison operators, `if`, `else`, `while`, `return`, lexical block scope, shadowing, branch-sensitive initialization, recursive calls, immutable/mutable reference syntax, ownership moves, `spawn`, `join`, `channel`, `send` और `receive` शामिल हैं।

## Worker-channel example

Workers को अब channel handles और अन्य values के साथ launch किया जा सकता है:

```snp
fn worker(ch: Channel) {
    send(ch, 99);
}

fn main() {
    let ch = channel();
    let task = spawn("worker", ch);
    join(task);
    print(receive(ch));
}
```

`spawn("worker", arguments...)` named function को अलग OS thread पर चलाता है और बाकी arguments forward करता है। `join(handle)` completion की प्रतीक्षा करता है। `channel()` synchronized message channel बनाता है।

## Build and test

Rust और Cargo install होने के बाद:

```bash
cargo test
cargo build --release
```

## Run

```bash
cargo run -- run examples/hello.snp
cargo run -- check examples/hello.snp
cargo run -- build examples/hello.snp
cargo run -- repl
cargo run -- run examples/stdlib_import.snp
cargo run -- check examples/stdlib_import.snp
cargo run -- run examples/string_collections.snp
cargo run -- check examples/string_collections.snp
cargo run -- run examples/advanced_collections.snp
cargo run -- check examples/advanced_collections.snp
```

## Architecture

```text
S+N++ source
    -> lexer and tokens
    -> recursive-descent parser
    -> AST
    -> type / definite-assignment / ownership checks
    -> bytecode compiler
    -> stack-based VM
    -> runtime services: threads and synchronized channels
```

The crate is organized as a testable library plus a thin binary entrypoint. Source files may begin with imports such as `import std.io;` and `import std.concurrency;`. The resolver currently recognizes `std.io`, `std.string`, `std.collections`, and `std.concurrency`; this registry is the contract that the future package manager will extend. `std.string` provides `str_len`, `str_contains`, `str_upper`, `str_lower`, `str_trim`, and `str_concat`. `std.collections` provides immutable-style `list`, `list_push`, `list_len`, `list_get`, `list_set`, `list_concat`, `map`, `map_set`, `map_get`, and `map_has` operations. Lists and maps are returned as new values when updated, so the original collection remains available under the ownership model. The AST, lexer, parser, type checker, runtime and CLI now have dedicated module implementations. `snp build file.snp` validates and emits a `.snpbc` bytecode artifact with the `SNPBC1` format marker; `snp check` performs frontend validation, while `snp run` executes through the bytecode VM. Future production milestones include loading serialized artifacts directly, explicit closure capture, richer diagnostics, standard library modules and an LLVM/native backend.

विस्तृत language rules और milestone details के लिए [SPEC.md](SPEC.md) देखें।

## Packages

S+N++ projects may contain a `snp.toml` manifest. Initialize one with `snp init <directory> [name]`; the command creates `src/main.snp` and a package manifest. The current manifest format uses a `[package]` section with `name`, `version`, and `entry`, plus a `[dependencies]` section whose values are local package paths.

Use `snp package-check [snp.toml]` to validate the manifest, resolve every local dependency, read the package entry file, and verify that non-standard imports belong to declared dependency namespaces. Standard modules continue to use `std.*`, while a dependency named `util` may expose imports such as `util.math`. This is intentionally a local-only foundation; registry downloads, lockfiles, cryptographic checksums, and semver solving are future milestones.

Example manifest:

```toml
[package]
name = "demo"
version = "0.1.0"
entry = "src/main.snp"

[dependencies]
util = "../util"
```

## Semantic versions and lockfiles

Package dependencies may use an inline table with a local path and a semantic-version constraint:

```toml
[dependencies]
util = { path = "../util", version = "^0.1.0" }
```

S+N++ currently supports exact versions such as `0.1.2`, caret ranges such as `^0.1.0`, tilde ranges such as `~0.1.2`, lower bounds such as `>=0.1.0`, and `*`. Run `snp lock path/to/snp.toml` to resolve local dependencies and generate `snp.lock`. `snp package-check path/to/snp.toml` validates the lockfile when it exists, including the version constraint, dependency path and manifest checksum.

The lockfile is deterministic and records each resolved package, exact version, source path and checksum. Remote registry resolution is now available through a transport-neutral filesystem registry fixture. A registry uses the layout `REGISTRY/<package>/<version>/snp.toml`; set `SNP_REGISTRY` to its root and use `snp lock` or `snp package-check`.

Registry dependencies use this form:

```toml
[dependencies]
app_core = { registry = "default", version = "^1.0.0" }
```

The resolver walks registry dependencies recursively, selects the highest available version satisfying each constraint, records transitive packages in `snp.lock`, and reports missing versions, dependency cycles, malformed metadata, and incompatible constraints for the same package name. Registry lock entries include a manifest checksum and are validated by `package-check`.

The HTTP registry protocol is specified in [REGISTRY_PROTOCOL.md](REGISTRY_PROTOCOL.md). Protocol v1 uses `GET /v1/index/{package}.json` for version discovery, `GET /v1/packages/{package}/{version}/snp.toml` for canonical metadata, and `GET /v1/packages/{package}/{version}/download` for archives. Each index entry includes an exact version, metadata URL, archive URL, dependency constraints and a `sha256:` archive checksum. The typed client implementation in `src/registry.rs` now performs HTTP `GET` requests through the system `curl` transport, maps non-2xx status codes to registry errors, validates protocol identity and package response identity, and parses the index JSON using a dependency-free parser. It also performs highest-compatible-version selection. Archive extraction and cache I/O will build on this client contract.
