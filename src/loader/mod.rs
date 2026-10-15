//! Multi-file module loader for S+N++.
//!
//! `mod utils;` in `main.snp` loads `./utils.snp` or `./utils/mod.snp`
//! relative to the entry file's directory, merges structs/functions,
//! and rejects import cycles.

use crate::ast::{Function, Program, StructDef};
use crate::parser;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LoadedUnit {
    pub path: PathBuf,
    pub source: String,
    pub program: Program,
}

/// Load entry file and all `mod name;` dependencies recursively.
pub fn load_entry(entry: &Path) -> Result<(Program, Vec<LoadedUnit>), String> {
    let entry = entry
        .canonicalize()
        .map_err(|e| format!("cannot open `{}`: {e}", entry.display()))?;
    let mut visited = HashSet::new();
    let mut stack = Vec::new();
    let mut units = Vec::new();
    let mut all_structs: Vec<StructDef> = Vec::new();
    let mut all_enums: Vec<crate::ast::EnumDef> = Vec::new();
    let mut all_traits = Vec::new();
    let mut all_functions: Vec<Function> = Vec::new();
    let mut all_externs: Vec<crate::ast::ExternFn> = Vec::new();
    let mut all_imports: Vec<String> = Vec::new();
    let mut struct_names = HashSet::new();
    let mut fn_names = HashSet::new();

    load_recursive(
        &entry,
        &mut visited,
        &mut stack,
        &mut units,
        &mut all_structs,
        &mut all_enums,
        &mut all_traits,
        &mut all_functions,
        &mut all_externs,
        &mut all_imports,
        &mut struct_names,
        &mut fn_names,
    )?;

    // Keep only real stdlib imports (drop mod: markers)
    let imports: Vec<String> = all_imports
        .into_iter()
        .filter(|i| !i.starts_with("mod:"))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    Ok((
        Program {
            imports,
            structs: all_structs,
            enums: all_enums,
            traits: all_traits,
            functions: all_functions,
            externs: all_externs,
        },
        units,
    ))
}

fn load_recursive(
    path: &Path,
    visited: &mut HashSet<PathBuf>,
    stack: &mut Vec<PathBuf>,
    units: &mut Vec<LoadedUnit>,
    all_structs: &mut Vec<StructDef>,
    all_enums: &mut Vec<crate::ast::EnumDef>,
    all_traits: &mut Vec<crate::ast::TraitDef>,
    all_functions: &mut Vec<Function>,
    all_externs: &mut Vec<crate::ast::ExternFn>,
    all_imports: &mut Vec<String>,
    struct_names: &mut HashSet<String>,
    fn_names: &mut HashSet<String>,
) -> Result<(), String> {
    if stack.contains(&path.to_path_buf()) {
        let cycle = stack
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(" -> ");
        return Err(format!(
            "module cycle detected: {cycle} -> {}",
            path.display()
        ));
    }
    if visited.contains(path) {
        return Ok(());
    }
    visited.insert(path.to_path_buf());
    stack.push(path.to_path_buf());

    let source = fs::read_to_string(path)
        .map_err(|e| format!("cannot read `{}`: {e}", path.display()))?;
    let program = parser::parse_program(&source)
        .map_err(|e| format!("{}: {e}", path.display()))?;

    let dir = path.parent().unwrap_or(Path::new("."));
    for imp in &program.imports {
        if let Some(name) = imp.strip_prefix("mod:") {
            let child = resolve_mod_path(dir, name)?;
            load_recursive(
                &child,
                visited,
                stack,
                units,
                all_structs,
                all_enums,
                all_traits,
                all_functions,
                all_externs,
                all_imports,
                struct_names,
                fn_names,
            )?;
        } else {
            all_imports.push(imp.clone());
        }
    }

    for s in program.structs {
        if !struct_names.insert(s.name.clone()) {
            return Err(format!(
                "duplicate struct `{}` while loading `{}`",
                s.name,
                path.display()
            ));
        }
        all_structs.push(s);
    }
    for e in program.enums {
        all_enums.push(e);
    }
    for t in program.traits {
        all_traits.push(t);
    }
    for f in program.functions {
        if !fn_names.insert(f.name.clone()) {
            return Err(format!(
                "duplicate function `{}` while loading `{}`",
                f.name,
                path.display()
            ));
        }
        all_functions.push(f);
    }
    for e in program.externs {
        if !fn_names.insert(e.name.clone()) {
            return Err(format!(
                "duplicate extern/function `{}` while loading `{}`",
                e.name,
                path.display()
            ));
        }
        all_externs.push(e);
    }

    units.push(LoadedUnit {
        path: path.to_path_buf(),
        source,
        program: Program {
            imports: program.imports,
            structs: vec![],
            enums: vec![],
            traits: vec![],
            functions: vec![],
            externs: vec![],
        },
    });

    stack.pop();
    Ok(())
}

fn resolve_mod_path(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let candidates = [
        dir.join(format!("{name}.snp")),
        dir.join(name).join("mod.snp"),
        dir.join(name).join(format!("{name}.snp")),
    ];
    for c in &candidates {
        if c.is_file() {
            return c
                .canonicalize()
                .map_err(|e| format!("cannot resolve module `{name}`: {e}"));
        }
    }
    Err(format!(
        "module `{name}` not found (looked for {}, {}, {})",
        candidates[0].display(),
        candidates[1].display(),
        candidates[2].display()
    ))
}

/// Execute a program loaded from a filesystem entry path (supports `mod`).
pub fn execute_path(entry: &Path) -> Result<crate::ast::Value, String> {
    let (mut program, _units) = load_entry(entry)?;
    crate::stdlib::resolve(&program.imports)?;
    crate::sema::expand_trait_defaults(&mut program)?;
    crate::sema::check_program(&program)?;
    execute_program(&program)
}

pub fn check_path(entry: &Path) -> Result<(), String> {
    let (mut program, _) = load_entry(entry)?;
    crate::stdlib::resolve(&program.imports)?;
    crate::sema::expand_trait_defaults(&mut program)?;
    crate::sema::check_program(&program)
}

pub fn execute_program(program: &Program) -> Result<crate::ast::Value, String> {
    // Build a synthetic source is not needed — call into vm internals via public API.
    // We expose a thin helper on vm.
    crate::vm::execute_program(program)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_sibling_mod_file() {
        let dir = std::env::temp_dir().join("snp_mod_test_load");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("utils.snp"),
            "fn helper() -> Int { return 7; }\n",
        )
        .unwrap();
        fs::write(
            dir.join("main.snp"),
            "mod utils;\nfn main() -> Int { return helper(); }\n",
        )
        .unwrap();
        let (prog, units) = load_entry(&dir.join("main.snp")).unwrap();
        assert!(units.len() >= 2);
        assert!(prog.functions.iter().any(|f| f.name == "helper"));
        assert!(prog.functions.iter().any(|f| f.name == "main"));
        let _ = fs::remove_dir_all(&dir);
    }
}
