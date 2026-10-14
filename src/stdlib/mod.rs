//! Built-in S+N++ standard-library module registry.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleSpec {
    pub name: &'static str,
    pub functions: &'static [&'static str],
}

pub fn registry() -> BTreeMap<&'static str, ModuleSpec> {
    BTreeMap::from([
        ("std.io", ModuleSpec { name: "std.io", functions: &["print"] }),
        ("std.string", ModuleSpec {
            name: "std.string",
            functions: &["str_len", "str_contains", "str_upper", "str_lower", "str_trim", "str_concat"],
        }),
        ("std.collections", ModuleSpec {
            name: "std.collections",
            functions: &["list", "list_push", "list_len", "list_get", "list_set", "list_concat", "map", "map_set", "map_get", "map_has"],
        }),
        ("std.concurrency", ModuleSpec {
            name: "std.concurrency",
            functions: &["channel", "send", "receive", "spawn", "join"],
        }),
        ("std.math", ModuleSpec {
            name: "std.math",
            functions: &["abs", "min", "max", "pow", "sqrt_approx"],
        }),
        ("std.time", ModuleSpec {
            name: "std.time",
            functions: &["now_ms"],
        }),
        ("std.random", ModuleSpec {
            name: "std.random",
            functions: &["rand", "rand_range"],
        }),
        ("std.fs", ModuleSpec {
            name: "std.fs",
            functions: &["fs_read_text", "fs_write_text", "fs_exists"],
        }),
        ("std.json", ModuleSpec {
            name: "std.json",
            functions: &["json_stringify", "json_parse"],
        }),
        ("std.test", ModuleSpec {
            name: "std.test",
            functions: &["assert_eq", "assert_true", "assert_false"],
        }),
    ])
}

pub fn resolve(imports: &[String]) -> Result<(), String> {
    let modules = registry();
    for import in imports {
        if import.starts_with("mod:") {
            // multi-file module declaration — accepted; file loading is a future step
            continue;
        }
        if !modules.contains_key(import.as_str()) {
            return Err(format!(
                "unknown standard-library module `{import}`; available: std.io, std.string, std.collections, std.concurrency, std.math, std.time, std.random, std.fs, std.json, std.test"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_core_modules() {
        assert!(resolve(&["std.io".into(), "std.math".into()]).is_ok());
    }

    #[test]
    fn rejects_unknown_modules() {
        let error = resolve(&["std.network".into()]).unwrap_err();
        assert!(error.contains("unknown standard-library module"));
    }
}
