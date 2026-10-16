# S+N++ 2.0.0 delivery notes

## Version
2.0.0

## New in this ZIP (2.0)
- list_find, list_zip, list_is_empty
- str_is_empty
- map_merge

## How to use
```bash
cargo build --release
./target/release/snp run examples/v20_impact.snp
cargo test
```

## Layout
- src/ — compiler + VM
- examples/ — v06…v20 demos
- tests/ — unit + integration
- docs/ — ROADMAP, CHANGELOG, ARCHITECTURE
