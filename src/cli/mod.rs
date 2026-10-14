//! S+N++ command-line interface.

use std::env;
use std::fs;
use std::io::{self, Write};

pub fn run() {
    let args: Vec<String> = env::args().collect();
    if args.len() >= 2 && (args[1] == "--version" || args[1] == "-V" || args[1] == "version") {
        println!("snp {}", crate::VERSION);
        return;
    }
    if args.len() < 2 {
        eprintln!("S+N++ — usage: snp init <dir> [name] | snp package-check [snp.toml] | snp lock [snp.toml] | snp run <file.snp> | snp check <file.snp> | snp build <file.snp> | snp test [dir|file] | snp bench [file.snp] [iters] | snp version | snp fmt <file> | snp doc <file> | snp fetch <pkg> [constraint] | snp emit-llvm <file.snp> | snp emit-wasm <file.snp> | snp repl");
        return;
    }
    if args[1] == "repl" {
        repl();
        return;
    }
    if args[1] == "init" {
        let root = args.get(2).cloned().unwrap_or_else(|| ".".into());
        let name = args.get(3).cloned().unwrap_or_else(|| std::path::Path::new(&root).file_name().and_then(|x| x.to_str()).unwrap_or("splusnpp-app").into());
        match crate::package::init_project(&root, &name) {
            Ok(()) => println!("initialized package `{name}` at {root}"),
            Err(error) => eprintln!("error: {error}"),
        }
        return;
    }
    if args[1] == "lock" {
        let manifest_path = args.get(2).cloned().unwrap_or_else(|| "snp.toml".into());
        let path = std::path::Path::new(&manifest_path);
        match crate::package::Manifest::load(path).and_then(|manifest| {
            let root = path.parent().unwrap_or(std::path::Path::new("."));
            let registry = std::env::var_os("SNP_REGISTRY").map(crate::package::Registry::open);
            let lock = crate::package::generate_lock_with_registry(root, &manifest, registry.as_ref())?;
            crate::package::write_lock(root.join("snp.lock"), &lock)?;
            Ok(lock.packages.len())
        }) {
            Ok(count) => println!("locked {count} local dependenc{} into snp.lock", if count == 1 { "y" } else { "ies" }),
            Err(error) => eprintln!("error: {error}"),
        }
        return;
    }
    if args[1] == "package-check" {
        let manifest_path = args.get(2).cloned().unwrap_or_else(|| "snp.toml".into());
        let path = std::path::Path::new(&manifest_path);
        match crate::package::Manifest::load(path).and_then(|manifest| {
            let root = path.parent().unwrap_or(std::path::Path::new("."));
            let deps = if manifest.dependencies.values().any(|dependency| dependency.source.starts_with("registry:")) {
                let registry_root = std::env::var_os("SNP_REGISTRY").ok_or("registry dependencies require SNP_REGISTRY")?;
                let registry = crate::package::Registry::open(registry_root);
                crate::package::resolve_registry_graph(&registry, &manifest)?.packages.into_values().map(|package| std::path::PathBuf::from(package.source)).collect()
            } else { crate::package::resolve_local(root, &manifest)? };
            let lock_path = root.join("snp.lock");
            if lock_path.is_file() {
                let lock = crate::package::load_lock(&lock_path)?;
                let registry = std::env::var_os("SNP_REGISTRY").map(crate::package::Registry::open);
                crate::package::validate_lock_with_registry(root, &manifest, &lock, registry.as_ref())?;
            }
            let entry = root.join(&manifest.entry);
            let source = fs::read_to_string(&entry).map_err(|e| format!("cannot read package entry {}: {e}", entry.display()))?;
            let program = crate::parser::parse_program(&source)?;
            crate::package::validate_imports(&program.imports, &manifest)?;
            Ok((manifest, deps))
        }) {
            Ok((manifest, deps)) => println!("ok: package `{}` v{} with {} local dependenc{}", manifest.name, manifest.version, deps.len(), if deps.len() == 1 { "y" } else { "ies" }),
            Err(error) => eprintln!("error: {error}"),
        }
        return;
    }
    if args[1] == "fetch" {
        let package = match args.get(2) {
            Some(p) => p.as_str(),
            None => {
                eprintln!("usage: snp fetch <package> [constraint]");
                return;
            }
        };
        let constraint = args.get(3).map(|s| s.as_str()).unwrap_or("*");
        match fetch_package_cli(package, constraint) {
            Ok(msg) => println!("{msg}"),
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if args[1] == "bench" {
        let file = args.get(2).map(|s| s.as_str()).unwrap_or("examples/hello.snp");
        let iters: u32 = args
            .get(3)
            .and_then(|s| s.parse().ok())
            .unwrap_or(1000);
        match run_bench(file, iters) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if args[1] == "test" {
        let target = args.get(2).map(|s| s.as_str()).unwrap_or("tests/snp");
        match run_tests(target) {
            Ok((passed, failed)) => {
                if failed == 0 {
                    println!("ok: {passed} passed");
                } else {
                    eprintln!("test result: {passed} passed, {failed} failed");
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if args[1] == "fmt" {
        if args.len() < 3 {
            eprintln!("usage: snp fmt <file.snp>");
            return;
        }
        let path = &args[2];
        match fs::read_to_string(path) {
            Ok(src) => {
                let formatted = crate::fmtutil::format_source(&src);
                match fs::write(path, &formatted) {
                    Ok(()) => println!("formatted {path}"),
                    Err(e) => eprintln!("fmt write error: {e}"),
                }
            }
            Err(e) => eprintln!("fmt: {e}"),
        }
        return;
    }
    if args[1] == "doc" {
        if args.len() < 3 {
            eprintln!("usage: snp doc <file.snp>");
            return;
        }
        let path = &args[2];
        match fs::read_to_string(path) {
            Ok(src) => {
                let docs = crate::fmtutil::extract_docs(&src);
                if docs.is_empty() {
                    println!("(no /// doc comments found in {path})");
                } else {
                    for (name, text) in docs {
                        println!("{name}: {text}");
                    }
                }
            }
            Err(e) => eprintln!("doc: {e}"),
        }
        return;
    }
    if args.len() < 3 { eprintln!("missing source file"); return; }

    let source_path = std::path::Path::new(&args[2]);
    let source = match fs::read_to_string(source_path) {
        Ok(source) => source,
        Err(error) => { eprintln!("cannot read {}: {error}", source_path.display()); return; }
    };
    match args[1].as_str() {
        "emit-llvm" => {
            match crate::backend::emit_from_source(&source, crate::backend::Backend::LlvmIr) {
                Ok(ir) => {
                    let out = format!("{}.ll", source_path.display());
                    match fs::write(&out, &ir) {
                        Ok(()) => {
                            println!("wrote {out} ({} bytes)", ir.len());
                        }
                        Err(e) => eprintln!("cannot write {out}: {e}"),
                    }
                }
                Err(error) => eprintln!("{}", diagnostic(&source, source_path, &error)),
            }
        }
        "emit-wasm" => {
            match crate::backend::emit_from_source(&source, crate::backend::Backend::WasmText) {
                Ok(wat) => {
                    let out = format!("{}.wat", source_path.display());
                    match fs::write(&out, &wat) {
                        Ok(()) => println!("wrote {out} ({} bytes)", wat.len()),
                        Err(e) => eprintln!("cannot write {out}: {e}"),
                    }
                }
                Err(error) => eprintln!("{}", diagnostic(&source, source_path, &error)),
            }
        }
        "check" => {
            // Prefer multi-file loader when path exists on disk
            let result = crate::loader::check_path(source_path).or_else(|e| {
                // fallback single-file (e.g. if path quirks)
                if e.contains("cannot open") || e.contains("cannot read") {
                    crate::parser::parse_program(&source).and_then(|program| {
                        crate::stdlib::resolve(&program.imports)?;
                        crate::sema::check_program(&program)
                    })
                } else {
                    Err(e)
                }
            });
            match result {
                Ok(()) => println!("ok: valid S+N++ program"),
                Err(error) => eprintln!("{}", diagnostic(&source, source_path, &error)),
            }
        }
        "build" => match crate::loader::load_entry(source_path).and_then(|(program, _)| {
            crate::stdlib::resolve(&program.imports)?;
            crate::sema::check_program(&program)?;
            crate::build_artifact(&source) // artifact from entry source for now
        }) {
            Ok(bytes) => {
                let artifact = format!("{}.snpbc", source_path.display());
                match fs::write(&artifact, bytes) {
                    Ok(()) => println!("built: {artifact}"),
                    Err(error) => eprintln!("cannot write {artifact}: {error}"),
                }
            }
            Err(error) => eprintln!("{}", diagnostic(&source, source_path, &error)),
        },
        "run" => {
            let result = crate::loader::execute_path(source_path).or_else(|e| {
                if e.contains("cannot open") || e.contains("cannot read") {
                    crate::execute(&source)
                } else {
                    Err(e)
                }
            });
            match result {
                Ok(value) if value != crate::Value::Unit => {
                    println!("program returned {}", show(&value))
                }
                Ok(_) => {}
                Err(error) => eprintln!("{}", diagnostic(&source, source_path, &error)),
            }
        }
        command => eprintln!(
            "unknown command `{command}`; use init, package-check, lock, run, check, build, test, bench, fetch, emit-llvm, emit-wasm or repl"
        ),
    }
}

fn repl() {
    let mut line = String::new();
    loop {
        print!("snp> "); let _ = io::stdout().flush(); line.clear();
        if io::stdin().read_line(&mut line).unwrap_or(0) == 0 { break; }
        let source = format!("fn main() {{{line}}}");
        match crate::execute(&source) {
            Ok(value) if value != crate::Value::Unit => println!("{}", show(&value)),
            Ok(_) => {},
            Err(error) => eprintln!("{}", diagnostic(&source, std::path::Path::new("<repl>"), &error)),
        }
    }
}

fn show(value: &crate::Value) -> String {
    match value {
        crate::Value::Int(value) => value.to_string(),
        crate::Value::Str(value) => value.clone(),
        crate::Value::Bool(value) => value.to_string(),
        crate::Value::Unit => "unit".into(),
        crate::Value::Thread(id) => format!("thread({id})"),
        crate::Value::Channel(id) => format!("channel({id})"),
        crate::Value::List(items) => format!("list(len={})", items.len()),
        crate::Value::Map(items) => format!("map(len={})", items.len()),
        crate::Value::Struct { name, fields: _ } => format!("{}{{..}}", name),
        crate::Value::Closure { name, captures } => format!("closure({name}, caps={})", captures.len()),
        crate::Value::Enum { name, variant, fields } => format!("{name}.{variant}(fields={})", fields.len()),
    }
}




fn fetch_package_cli(package: &str, constraint: &str) -> Result<String, String> {
    use crate::package::Constraint;
    use crate::registry::{fetch_package, HttpRegistryClient, PackageCache, RegistryClientConfig};
    let constraint = Constraint::parse(constraint)?;
    let base = std::env::var("SNP_REGISTRY_URL")
        .unwrap_or_else(|_| "https://registry.splusnpp.dev".into());
    let cache_dir = std::env::var("SNP_CACHE")
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            format!("{home}/.cache/snp")
        });
    let client = HttpRegistryClient::new(RegistryClientConfig::new(&base, &cache_dir));
    let cache = PackageCache::open(&cache_dir);
    let (ver, path) = fetch_package(&client, &cache, package, &constraint, true)
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "fetched {package}@{ver} into {}",
        path.display()
    ))
}

fn run_bench(file: &str, iters: u32) -> Result<(), String> {
    let path = std::path::Path::new(file);
    // Warmup
    let _ = crate::loader::execute_path(path)?;
    let start = std::time::Instant::now();
    for _ in 0..iters {
        crate::loader::execute_path(path)?;
    }
    let elapsed = start.elapsed();
    let total_ms = elapsed.as_secs_f64() * 1000.0;
    let per = total_ms / iters as f64;
    println!("bench {file}");
    println!("  iters:  {iters}");
    println!("  total:  {total_ms:.3} ms");
    println!("  per:    {per:.4} ms/op");
    println!("  ops/s:  {:.0}", 1000.0 / per.max(1e-12));
    Ok(())
}

fn run_tests(target: &str) -> Result<(usize, usize), String> {
    let path = std::path::Path::new(target);
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    if path.is_file() {
        files.push(path.to_path_buf());
    } else if path.is_dir() {
        for entry in std::fs::read_dir(path).map_err(|e| format!("cannot read {target}: {e}"))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let p = entry.path();
            if p.extension().and_then(|x| x.to_str()) == Some("snp") {
                files.push(p);
            }
        }
        files.sort();
    } else {
        return Err(format!("test path not found: {target}"));
    }
    if files.is_empty() {
        return Err(format!("no .snp test files in {target}"));
    }
    let mut passed = 0usize;
    let mut failed = 0usize;
    for file in files {
        let name = file.display().to_string();
        print!("test {name} ... ");
        let _ = io::stdout().flush();
        match crate::loader::execute_path(&file) {
            Ok(_) => {
                println!("ok");
                passed += 1;
            }
            Err(e) => {
                println!("FAILED");
                eprintln!("{e}");
                failed += 1;
            }
        }
    }
    Ok((passed, failed))
}

fn diagnostic(source: &str, path: &std::path::Path, error: &str) -> String {
    crate::diagnostics::format_error_report(&path.display().to_string(), source, error)
}
