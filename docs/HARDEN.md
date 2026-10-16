# Core harden notes (2.5.1)

Goal: progress you can feel — not a bigger version number.

## What was wrong
1. Match-expression with pattern bindings (`Option.Some(n)`) reported `undefined variable n`
2. Ownership was soft: some ops claimed to borrow but method path always moved receiver
3. `list_rev` allowed use-after-"transform" which hid move bugs

## What we fixed
1. Match-expr → synthetic `__match_expr_N(__scr)` function (same path as statement match)
2. Intrinsic methods borrow; user struct methods still move
3. `list_rev` is a **move**; use `clone(xs)` if you need both

## How to verify
```bash
cargo test harden_
cargo test match_expr
```
