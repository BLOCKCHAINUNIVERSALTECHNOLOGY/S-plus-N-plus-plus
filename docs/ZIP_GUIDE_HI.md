# S+N++ 2.13.0 — ZIP गाइड (हिंदी)

## यह ZIP क्या है?
पूरा **S+N++** कंपाइलर (Rust) — lexer → parser → sema (generics + monomorphize) → MIR → VM।

**वर्ज़न:** 2.13.0

## GitHub पर कैसे डालें
1. ZIP अनज़िप करें  
2. फाइलें अपने repo (`BLOCKCHAINUNIVERSALTECHNOLOGY/S-N-`) की root पर पेस्ट/पुश करें  
3. मशीन पर:
```bash
cargo test
cargo build --release
./target/release/snp run examples/v33_struct_mono.snp
```

## नए बड़े फीचर्स (2.10–2.13)
- **Generic functions** + monomorphize (`identity__Int`)
- **Generic structs** + monomorphize (`Box__Int`)
- **Generic enums** (`enum Opt[T] { Some(T) }`)
- Traits, where-bounds, float, list helpers, error spans

## ज़रूरी फोल्डर
```
src/          → पूरा कंपाइलर
examples/     → .snp डेमो
tests/        → integration + snp tests
docs/         → FILEMAP, CHANGELOG, ROADMAP
Cargo.toml    → वर्ज़न 2.13.0
```

## कमांड
```bash
snp run file.snp
snp check file.snp
snp test
snp build file.snp
```

## अगर `cargo test` फेल हो
- Rust toolchain अपडेट रखें  
- `CARGO_TARGET_DIR` permission issues पर अलग target dir इस्तेमाल करें  
- पहले `cargo clean` फिर `cargo test`
