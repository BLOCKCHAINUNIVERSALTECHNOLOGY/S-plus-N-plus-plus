# S+N++ v2.5.0 — ZIP Guide (आसान समझ)

यह ZIP पूरा **S+N++** भाषा project है।  
Version: **2.5.0**

---

## जल्दी शुरू (3 steps)

```bash
unzip S-N-v2.5.0-complete.zip
cd S-N-   # या extract हुई folder
cargo test
cargo run -- run examples/v25_impact.snp
```

CLI binary नाम: **`snp`**

```bash
cargo build --release
./target/release/snp run examples/hello.snp
./target/release/snp --version   # → 2.5.0
```

---

## Folder structure (कहाँ क्या है)

| Path | मतलब |
|------|------|
| `src/` | पूरा compiler + VM source (Rust) |
| `src/lexer/` | tokens |
| `src/parser/` | AST बनाता है |
| `src/ast/` | language nodes (Expr, Stmt, Type…) |
| `src/sema/` | typecheck, ownership, desugar |
| `src/hir/` `src/mir/` | intermediate representations |
| `src/vm/` | bytecode VM + builtins |
| `src/cli/` | `snp` command line |
| `examples/` | demo `.snp` files (v06…v25) |
| `tests/` | unit + integration tests |
| `docs/` | ROADMAP, CHANGELOG, ARCHITECTURE |
| `Cargo.toml` | Rust package (version 2.5.0) |

---

## Version trail (क्या-क्या आया)

| Ver | मुख्य features |
|-----|----------------|
| 1.x–2.0 | core language, Result `?`, pipelines, maps/lists |
| 2.1 | if-expression, list_unique/chunk, str_lines |
| 2.2 | **match-expression**, list_sort_by |
| 2.3 | pipeline **`_`**, **tuples** `(a,b)` |
| 2.4 | **`for (i,x) in xs`**, **enumerate** |
| **2.5** | **method sugar** (`xs.len()`, `"hi".trim()`), take/drop/trim |

---

## महत्वपूर्ण examples

| File | दिखाता है |
|------|-----------|
| `examples/v25_impact.snp` | method sugar (नवीनतम) |
| `examples/v24_impact.snp` | indexed for + enumerate |
| `examples/v23_impact.snp` | pipeline `_` + tuples |
| `examples/v22_impact.snp` | match-expression |
| `examples/game_loop.snp` | gaming-style loop |
| `examples/traits.snp` | traits |
| `examples/modules.snp` | modules |

---

## भाषा की एक झलक

```snp
fn main() -> Int {
    let xs = [10, 20, 30, 40];
    let head = xs.take(2);
    let name = "  snp  ".trim();

    for (i, x) in xs {
        print(i);
        print(x);
    }

    let y = match 2 {
        1 => 100,
        2 => 200,
        _ => 0,
    };

    let t = (y, head.sum());
    return tuple_get(t, 0) + name.len();
}
```

---

## Docs और पढ़ो

1. `docs/CHANGELOG.md` — हर version क्या बदला  
2. `docs/ROADMAP.md` — आगे का plan  
3. `docs/ARCHITECTURE.md` — compiler pipeline  
4. `docs/FILEMAP.md` — file-by-file inventory  
5. `docs/NEXT_VERSIONS.md` — अगले packs  

---

## GitHub पर लगाने का तरीका

1. ZIP extract करो  
2. अपने repo folder में files copy/paste  
3. `git add . && git commit -m "S+N++ 2.5.0" && git push`

---

## Tests

```bash
cargo test
# ~149 unit + 18 integration ≈ 167 tests
```

सब green होने चाहिए।

---

**Contact / vision:** यह language coding, gaming logic, और solid tooling के लिए lightweight + fast रखी गई है।
