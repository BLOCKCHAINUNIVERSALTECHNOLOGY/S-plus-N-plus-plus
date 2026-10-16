# Changelog

## [2.6.0] — 2026-10-15 — Float + parse + tool

### Added
- **`Float`** type / literals (`1.5`), arith + compare with Int promotion
- **`str_parse_int`**, **`str_parse_float`**, **`int_to_str`**, **`float_to_str`**
- **`floor`**, **`ceil`**, **`round`**
- **`examples/tool_todo.snp`** — small weighted TODO score tool
- **`examples/v26_float.snp`**

### Notes
- Parser errors already include line/column
- Struct methods continue via `impl` / `Type.method` desugar

# Changelog

## [2.5.3] — 2026-10-15 — harden: let + loop vars

### Fixed
- **`let x = ...` inside `for` / `for (i, x) in`**: desugar_methods now puts loop
  variables into the env before desugaring the body (expr_type can see them)
- Unblocks `let row = score_line(list_get(names, idx), val)` in loops

# Changelog

## [2.5.2] — 2026-10-15 — harden: diagnostics + ship-proof

### Improved
- Borrow errors mention **`clone(name)`** and why the move happened
- Uninitialized-variable errors include a short **help** line
- Runtime `undefined variable` includes **function name** from call stack when available

### Ship-proof example
- `examples/ship_scoreboard.snp` — indexed for, match, if-expr, pipeline, assert (pure S+N++)

# Changelog

## [2.5.1] — 2026-10-15 — **core harden** (not a feature parade)

### Fixed (real bugs)
- **Match-expression bindings**: `match o { Option.Some(n) => n + 1, ... }` now works
  (desugar via real helper function, not broken IIFE/lambda)
- **Ownership**: `list_rev` **moves** the list — reuse after rev is a hard error
- **Method sugar ownership**: `xs.len()` / `.get` / `.take` **borrow**, do not falsely move `xs`

### Tests
- `harden_match_expr_binding`, `harden_move_list_rejected`, `harden_copyable_int_ok`
- `stmt_match_option_binding`

This release prioritizes **correctness** over version theater.

# Changelog

## [2.5.0] — 2026-10-15

### Added — method sugar
- **List methods**: `.len()`, `.get(i)`, `.push`, `.contains`, `.rev`, `.sort`, `.sum`, `.min`, `.max`,
  `.is_empty()`, `.map`, `.filter`, `.fold`, `.slice`, `.unique`, `.find`, `.flatten`, `.chunk`,
  `.take(n)`, `.drop(n)`, `.any`, `.all`
- **String methods**: `.len()`, `.contains`, `.split`, `.replace`, `.starts_with`, `.ends_with`,
  `.is_empty()`, `.upper()`, `.lower()`, `.lines()`, `.index_of`, `.substring`, `.repeat`, `.trim()`
- **Map methods**: `.len()`, `.get`, `.set`, `.has`, `.keys()`, `.values()`, `.remove`
- **Tuple methods**: `.get(i)`, `.len()`
- **New builtins**: `list_take`, `list_drop`, `str_trim`, `str_lower`

# Changelog

## [2.4.0] — 2026-10-14

### Added
- **Indexed for**: `for (i, x) in xs { ... }` — `i` is Int index, `x` is element
- **`enumerate(xs)`** → list of `(index, value)` tuples

# Changelog

## [2.3.0] — 2026-10-14

### Added
- **Pipeline placeholder `_`**: `1 |> list_get(xs, _)` → `list_get(xs, 1)`
  - Without `_`, still first-arg style: `xs |> list_map(f)`
- **Tuples**: `(a, b)`, `tuple_get` / `tuple_len`, pattern `(a, b)` in match

# Changelog

## [2.2.0] — 2026-10-14

### Added
- **Match-expression**: `let x = match e { pat => expr, _ => expr };`
- **`list_sort_by(xs, key_fn)`** — sort by key function (Int/String keys)

### Note
- Statement `match` still uses `=> { stmts }`
- Expression `match` uses `=> expr` (no block)

# Changelog

## [2.1.1] — 2026-10-14

### Pipeline + integration upgrade
- Pipeline `|>` accepts **method calls** (left becomes first method arg)
- **`list_flat_map`** for nested list pipelines
- Expanded **integration tests**: rich pipelines, if-expr, maps, chunk/find

# Changelog

## [2.1.0] — 2026-10-14

### Added — real language feature
- **If-expression**: `let x = if cond { a } else { b };` (both branches expressions; `else` required)
- **`list_unique`**, **`list_chunk(xs, n)`**, **`str_lines`**

# Changelog

## [2.0.0] — 2026-10-14

### Major milestone
S+N++ **2.0** — production-oriented language surface.

### Added in 2.0
- **`list_find(xs, pred)`** → index or `-1`
- **`list_zip(a, b)`** → list of `[ai, bi]` pairs
- **`list_is_empty` / `str_is_empty`**
- **`map_merge(a, b)`** (b wins on key clash)

### Included from 1.x
- Control: `?`, `defer`, `if let`, `while let`, `loop`, `+=`, `&&`/`||`, `|>`, `%`
- Data: list/map pipelines, literals `[…]`, sort/sum/min/max/any/all/slice/flatten
- Types: generics, traits, Result/Option, modules, `use`, `const`
- Strings: split/replace/join/index/substring/repeat
- Maps: len/keys/values/remove/merge
- Tooling: LSP, registry, MIR pipeline, integration suite

# Changelog

## [1.5.0] — 2026-10-14

### Added
- **Map helpers**: `map_len`, `map_keys`, `map_values`, `map_remove`
- **`str_repeat(s, n)`**
- **`list_flatten`** (one level)

# Changelog

## [1.4.0] — 2026-10-14

### Added
- **`list_min` / `list_max`**
- **`list_any` / `list_all`** (predicate closures)
- **`str_index_of` / `str_substring`**
- **`clone(x)`** explicit copy helper

# Changelog

## [1.3.0] — 2026-10-14

### Added
- **Modulo** `%` and `%=`
- **`str_join(list, sep)`**
- **`list_sort`** (Int / String)
- **`list_sum`** (list of Int)

# Changelog

## [1.2.0] — 2026-10-14

### Added
- **Short-circuit `&&` and `||`**
- **`range(start, end)`** → list of Ints (exclusive end)
- **`list_rev` / `list_contains`**

# Changelog

## [1.1.0] — 2026-10-14

### Added
- **Compound assignment**: `+=`, `-=`, `*=`, `/=`
- **`loop { ... }`** (infinite loop, use `break`)
- **`while let PATTERN = expr`**
- **`list_slice(xs, start, end)`** half-open `[start, end)`

# Changelog

## [1.0.0] — 2026-10-14

### Stable milestone
- Full feature set from 0.1–0.9 considered **1.0 language surface**
- Expanded **integration tests** covering try, pipelines, generics, use, defer, const, ranges, if-let, Option, strings, assert

### Language surface (1.0)
- Core: functions, structs, enums, match, modules, traits
- Control: `?`, `defer`, `if let`, `for`/`..=`, pipelines `|>`
- Data: list/map, literals `[…]`, map/filter/fold
- Types: generics `[T]`, Result/Option prelude, `const`, `use`
- Tooling: LSP, registry, native plan, fmt/doc

# Changelog

## [0.9.0] — 2026-10-14

### Added — Impact Pack
- **Inclusive ranges**: `for i in 1..=5`
- **`if let PATTERN = expr`**
- **Option prelude**: `Option.Some` / `Option.None`
- **`panic("msg")` / `todo("msg")`**

# Changelog

## [0.8.0] — 2026-10-14

### Added — Impact Pack
- **Pipeline `|>`**: `xs |> list_map(f) |> list_fold(0, g)` (first-arg style)
- **List literals**: `[1, 2, 3]` → `list(1, 2, 3)`
- **`assert(cond, "msg")`** builtin

# Changelog

## [0.7.0] — 2026-10-14

### Added — Impact Pack
- **Generics**: `fn id[T](x: T) -> T` with call-site type inference
- **`use`**: `use mod.name;` and `use mod.{a, b};`
- **`defer { ... }`**: LIFO cleanup before `return` and at end of body
- **String ops**: `str_split`, `str_replace`, `str_starts_with`, `str_ends_with`

# Changelog

## [0.6.0] — 2026-10-14

### Added — Impact Pack
- **`?` operator** on `Result` — unwrap `Ok` or early-return `Err`
- **`list_map` / `list_filter` / `list_fold`** with closures
- **Exhaustive match** on enums (missing variants → error)
- **`const NAME = value`** top-level and local constants

# Changelog

## [0.5.3] — 2026-10-14

### Added
- **Nested module paths**: `a.b.v()`, `x.y.z.n()` resolve to dotted Call names
- Parser flattens `Field` chains into `a.b.c` for calls

# Changelog

## [0.5.2] — 2026-10-14

### Added
- Client ↔ **local registry** integration test (`publish` → one-shot HTTP → `fetch_index`)
- `backend::cranelift_research` stub (no extra deps; status string for tooling)
- Offline index cache markers use valid `sha256:` checksum field

# Changelog

## [0.5.1] — 2026-10-14

### Added
- **`snp registry-publish <pkg-dir> [registry-root]`** — copy package into local registry + write `index/<name>.json`
- Local index checksums (`sha256:local…`) so protocol validation passes
- `snp emit-native` covered by unit test

### Notes
- Local fingerprint is not cryptographic SHA-256; replace when hosting publicly

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
