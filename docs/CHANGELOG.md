# Changelog

## [2.24.0] — 2026-10-18 — Phase A start (sandbox + SNPBC2)

### Production Phase A
- **Sandbox**: `allow_fs` / `allow_net` via `set_sandbox`, `--sandbox`, `SNP_SANDBOX=1`
- FS builtins blocked when sandbox disables FS
- **SNPBC2** artifact header (`version`, `funcs`, fn list) + `inspect_artifact`
- `build_artifact` uses full desugar/mono pipeline
- `docs/PHASE_A.md`

# Changelog

## [2.23.0] — 2026-10-18 — JSON objects + FS expand

### Added / improved
- `json_parse` supports objects `{...}` → Map
- `json_get(map, key)`
- Float in JSON stringify/parse
- `fs_read_lines` / `fs_append_text` / `fs_remove`

### Example
- `examples/v43_json_fs.snp`

# Changelog

## [2.22.0] — 2026-10-18 — Result + ? polish

### Improved
- `?` accepts `Result` and monomorphized `Result__…`
- Clearer type / runtime errors for invalid `?` use
- Err propagates out of the current function; Ok unwraps payload

### Example
- `examples/v42_result_try.snp`

# Changelog

## [2.21.0] — 2026-10-18 — loop + break labels

### Added
- `loop { ... }` (while true)
- Labeled loops: `outer: loop { ... }`
- `break outer` / `continue outer` jump to labeled loop
- Nested loop stack for correct label resolution

### Example
- `examples/v41_loop_labels.snp`

# Changelog

## [2.20.0] — 2026-10-18 — range patterns in match

### Added
- `1..=10` inclusive and `1..10` exclusive patterns in `match`
- Runtime `__in_range` helper + MIR/VM support

### Example
- `examples/v40_range_patterns.snp`

# Changelog

## [2.19.0] — 2026-10-18 — struct destructure, enum impl, exhaustiveness

### Added
- `let Player { hp, atk: a } = p` struct destructuring
- `impl Option-like[T] { ... }` method specialization for monomorphized enums
- Stronger match exhaustiveness (prelude enums included)

### Example
- `examples/v39_destructure_enum_impl.snp`

# Changelog

## [2.18.0] — 2026-10-18 — defaults + stdlib polish

### Added
- Struct field defaults: `hp: Int = 100`
- Partial struct lits fill missing fields from defaults
- `list_first` / `list_last` / `clamp(x, lo, hi)`

### Example
- `examples/v38_defaults_stdlib.snp`

# Changelog

## [2.17.0] — 2026-10-18 — type aliases + tuple let

### Added
- `type Name = Target` and `type Id[T] = T`
- `let (a, b) = pair` desugars to `tuple_get`
- Clearer generic type-parameter mismatch help text

### Example
- `examples/v37_alias_tuple.snp`

# Changelog

## [2.16.0] — 2026-10-18 — nested generics + prelude Option[T]

### Added
- Prelude **`Option[T]`** / **`Result[T, E]`** (generic)
- Nested struct inference: `Wrap { inner: Box { value: 1 } }` → `Wrap__Box_Int`
- Stable nested type mangling (`List_Int`, `Box_Int`, …)
- Prelude enums injected for monomorphization when missing

### Example
- `examples/v36_nested_prelude.snp`

# Changelog

## [2.15.0] — 2026-10-18 — generic impl methods

### Added
- `impl Box[T] { fn get(self) -> T { ... } }`
- Impl type params merge into methods
- On struct monomorphize, methods specialized to `Box__Int.get`
- `Self` in generic impl still rewrites to impl type

### Example
- `examples/v35_generic_impl.snp`

# Changelog

## [2.14.0] — 2026-10-18 — generic enum monomorphization

### Compiler
- **`monomorphize_enums`**: `Option2.Some(40)` → specialized `Option2__Int`
- VariantLit + match pattern enum names rewritten
- `EnumApp` in signatures rewritten to concrete `Enum`
- Wired after struct monomorphization

### Example
- `examples/v34_enum_mono.snp`

# Changelog

## [2.13.0] — 2026-10-18 — generic struct monomorphization

### Compiler
- **`monomorphize_structs`**: for each concrete `Box { value: 42 }` / `Box[Int]`,
  emits specialized `Box__Int` with concrete field types
- Struct literals rewritten to specialized names
- `StructApp` in signatures rewritten to specialized `Struct`
- Runs after function monomorphization inside `sema::monomorphize`

### Example
- `examples/v33_struct_mono.snp`

# Changelog

## [2.12.0] — 2026-10-18 — generic enums

### Added
- `enum Option2[T] { None, Some(T) }` with type parameters
- `Type::EnumApp(name, args)` for specialized enum types
- Variant construction infers type args from payloads
- Same type parameter must be consistent across variant fields
- `Enum` / `EnumApp` compatible in type checks

### Example
- `examples/v32_generic_enum.snp`

# Changelog

## [2.11.0] — 2026-10-18 — generic structs

### Added
- `struct Box[T] { value: T }` with type parameters
- `Type::StructApp(name, args)` for `Box[Int]` in annotations
- Struct literal infers type args from fields
- Same type parameter must be consistent across fields
- Field access substitutes type parameters

### Example
- `examples/v31_generic_struct.snp`

# Changelog

## [2.10.0] — 2026-10-16 — monomorphization

### Compiler
- **`sema::monomorphize`**: for each concrete call to `fn f[T](...)`, emits
  specialized `f__Type` with concrete param/return types and rewritten body
- Call sites rewritten to specialized names before HIR/MIR/VM
- Fixed-point: nested generic calls inside specialized bodies also specialized
- Wired into `execute` and `build_artifact`

### Example
- `examples/v30_mono.snp`

# Changelog

## [2.9.0] — 2026-10-16 — generics + traits hardened

### Generics
- `fn id[T](x: T) -> T` — type params normalized to `Type::Generic`
- Same `T` must infer to one concrete type (consistency error otherwise)
- Return type substitutes inferred `T`

### Traits
- `trait` + `impl Trait for Type` + default method bodies (existing)
- `where Type: Trait` at definition (concrete) and **call-site** for type params
- `FN_BOUNDS` / `TRAIT_IMPLS` drive call-site bound checks

### Examples
- `examples/v29_generics.snp`
- `examples/v29_traits.snp`

# Changelog

## [2.8.0] — 2026-10-16 — error spans + struct methods

### Diagnostics
- **`locate_error`**: line/column from message, or from `` `name` `` in source
- **`annotate(source, err)`**: always adds `--> path:line:col` + caret snippet when possible
- CLI `snp check` / `snp run` already route through `format_error_report`

### Struct methods
- **`Self`** type in `impl` methods (params + return)
- Clearer errors: unknown method lists available `Type.method` names
- Help text when calling method on non-struct

### Examples
- `examples/v28_methods.snp`
- `examples/v28_error_span.snp` (intentional fail for span demo)

# Changelog

## [2.7.0] — 2026-10-16 — stdlib depth

### Added
- **`list_insert`**, **`list_remove`**, **`list_pop`** (immutable-style new lists)
- **`str_char_at(s, i)`**
- **`map_get_or(map, key, default)`**
- **`abs` / `min` / `max` / `pow` / `sqrt`** work on **Float** (and Int)
- **`while let`** confirmed (parser desugar → while + match)

### Examples
- `examples/v27_stdlib.snp`

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
