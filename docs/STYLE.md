# S+N++ Style Guide (v0.3.1)

## Indentation
- 4 spaces (no tabs)
- Brace on same line: `fn main() -> Int {`

## Naming
- Types / enums / structs: `PascalCase` (`Player`, `OptionInt`)
- Functions / variables: `snake_case` (`take_damage`, `hp`)
- Modules: `snake_case` (`combat`, `math`)

## Modules
- Prefer `pub` only on the API surface of a module
- Max nesting depth: 8

## Match
- Prefer guards over nested ifs when matching values
- Put `_` last

## Comments
- `//` line comments
- `///` doc comments immediately above `fn` / `struct` / `enum`

## Formatting
Run `snp fmt <file>` for a light normalize (trim trailing space, ensure final newline).
Full prettier-style formatting is not required for 0.3.1.
