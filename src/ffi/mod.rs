//! Stable embedder FFI API for S+N++.
//!
//! ## Language side
//!
//! ```text
//! extern "C" fn damage(base: Int, mult: Int) -> Int;
//!
//! fn main() -> Int {
//!     return damage(10, 3);
//! }
//! ```
//!
//! ## Host side (Rust embedder)
//!
//! ```rust,ignore
//! use splusnpp::ffi::{self, HostFn};
//! use splusnpp::{execute, Value};
//!
//! fn damage(args: &[Value]) -> Result<Value, String> {
//!     let base = ffi::arg_int(args, 0, "damage")?;
//!     let mult = ffi::arg_int(args, 1, "damage")?;
//!     Ok(Value::Int(base * mult))
//! }
//!
//! fn main() {
//!     ffi::register_host("damage", damage);
//!     let out = execute(r#"
//!         extern "C" fn damage(base: Int, mult: Int) -> Int;
//!         fn main() -> Int { return damage(10, 3); }
//!     "#).unwrap();
//!     assert_eq!(out, Value::Int(30));
//! }
//! ```
//!
//! ## ABI rules (v0.2)
//!
//! Supported `extern "C"` parameter / return types:
//! - `Int`  ↔ `Value::Int(i64)`
//! - `Bool` ↔ `Value::Bool(bool)`
//! - `String` ↔ `Value::Str(String)`
//! - `Unit` ↔ `Value::Unit` (return only)
//!
//! Lists, maps, structs, channels, and closures are **not** valid across the
//! extern boundary in 0.2 (use stdlib inside the VM instead).
//!
//! Built-in host symbols (always registered):
//! - `host_abs(x: Int) -> Int`
//! - `host_clamp(x: Int, lo: Int, hi: Int) -> Int`
//! - `host_strlen(s: String) -> Int`

use crate::ast::Value;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Serializes registry swap + execute (tests / multi-engine embedders).
fn exec_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Host callback signature — safe Rust, no raw pointers required.
pub type HostFn = fn(&[Value]) -> Result<Value, String>;

fn host_abs(args: &[Value]) -> Result<Value, String> {
    let x = arg_int(args, 0, "host_abs")?;
    Ok(Value::Int(x.abs()))
}

fn host_clamp(args: &[Value]) -> Result<Value, String> {
    let x = arg_int(args, 0, "host_clamp")?;
    let lo = arg_int(args, 1, "host_clamp")?;
    let hi = arg_int(args, 2, "host_clamp")?;
    Ok(Value::Int(x.clamp(lo, hi)))
}

fn host_strlen(args: &[Value]) -> Result<Value, String> {
    let s = arg_str(args, 0, "host_strlen")?;
    Ok(Value::Int(s.len() as i64))
}

fn builtin_table() -> HashMap<String, HostFn> {
    let mut m: HashMap<String, HostFn> = HashMap::new();
    m.insert("host_abs".into(), host_abs);
    m.insert("host_clamp".into(), host_clamp);
    m.insert("host_strlen".into(), host_strlen);
    m
}

fn with_registry<R>(f: impl FnOnce(&mut HashMap<String, HostFn>) -> R) -> R {
    static REG: OnceLock<Mutex<HashMap<String, HostFn>>> = OnceLock::new();
    let mutex = REG.get_or_init(|| Mutex::new(builtin_table()));
    let mut guard = mutex.lock().expect("ffi registry poisoned");
    f(&mut guard)
}

// ---------------------------------------------------------------------------
// Argument helpers (embedder convenience)
// ---------------------------------------------------------------------------

/// Extract `Int` argument at `index`.
pub fn arg_int(args: &[Value], index: usize, fname: &str) -> Result<i64, String> {
    match args.get(index) {
        Some(Value::Int(n)) => Ok(*n),
        Some(other) => Err(format!(
            "{fname}: argument {} expects Int, got {}",
            index + 1,
            other.type_of()
        )),
        None => Err(format!(
            "{fname}: missing argument {} (expected Int)",
            index + 1
        )),
    }
}

/// Extract `Bool` argument at `index`.
pub fn arg_bool(args: &[Value], index: usize, fname: &str) -> Result<bool, String> {
    match args.get(index) {
        Some(Value::Bool(b)) => Ok(*b),
        Some(other) => Err(format!(
            "{fname}: argument {} expects Bool, got {}",
            index + 1,
            other.type_of()
        )),
        None => Err(format!(
            "{fname}: missing argument {} (expected Bool)",
            index + 1
        )),
    }
}

/// Extract `String` argument at `index` (cloned).
pub fn arg_str(args: &[Value], index: usize, fname: &str) -> Result<String, String> {
    match args.get(index) {
        Some(Value::Str(s)) => Ok(s.clone()),
        Some(other) => Err(format!(
            "{fname}: argument {} expects String, got {}",
            index + 1,
            other.type_of()
        )),
        None => Err(format!(
            "{fname}: missing argument {} (expected String)",
            index + 1
        )),
    }
}

/// Require exact arity.
pub fn expect_arity(args: &[Value], n: usize, fname: &str) -> Result<(), String> {
    if args.len() != n {
        Err(format!(
            "{fname}: expected {n} argument(s), got {}",
            args.len()
        ))
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Registry API
// ---------------------------------------------------------------------------

/// Register or replace a host function visible to `extern "C"` declarations.
pub fn register_host(name: impl Into<String>, f: HostFn) {
    with_registry(|reg| {
        reg.insert(name.into(), f);
    });
}

/// Remove a host binding. Built-ins can be removed (tests); re-init via
/// [`reset_registry`] if needed.
pub fn unregister_host(name: &str) -> bool {
    with_registry(|reg| reg.remove(name).is_some())
}

/// Names of all currently registered host functions (sorted).
pub fn list_hosts() -> Vec<String> {
    with_registry(|reg| {
        let mut names: Vec<String> = reg.keys().cloned().collect();
        names.sort();
        names
    })
}

/// True if `name` has a host binding.
pub fn has_host(name: &str) -> bool {
    with_registry(|reg| reg.contains_key(name))
}

/// Restore the default built-in table (clears custom registrations).
pub fn reset_registry() {
    with_registry(|reg| {
        *reg = builtin_table();
    });
}

/// Call a registered host function by name.
pub fn call_host(name: &str, args: Vec<Value>) -> Result<Value, String> {
    with_registry(|reg| {
        let f = reg
            .get(name)
            .ok_or_else(|| format!("unresolved extern `{name}` (no host binding)"))?;
        f(&args)
    })
}

/// Ensure every declared extern name has a host binding.
pub fn resolve_externs(names: &[String]) -> Result<(), String> {
    with_registry(|reg| {
        for n in names {
            if !reg.contains_key(n) {
                return Err(format!(
                    "extern `{n}` has no host binding (register via ffi::register_host or use a built-in host_* symbol)"
                ));
            }
        }
        Ok(())
    })
}

/// Register several hosts, run source, then leave registry as-is.
/// Prefer this in embedders that own the process lifetime.
pub fn execute_with_hosts(
    source: &str,
    hosts: &[(&str, HostFn)],
) -> Result<Value, String> {
    let _guard = exec_lock().lock().expect("ffi exec lock");
    for (name, f) in hosts {
        register_host(*name, *f);
    }
    crate::execute(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn damage(args: &[Value]) -> Result<Value, String> {
        expect_arity(args, 2, "damage")?;
        let base = arg_int(args, 0, "damage")?;
        let mult = arg_int(args, 1, "damage")?;
        Ok(Value::Int(base * mult))
    }

    #[test]
    fn host_abs_works() {
        assert_eq!(
            call_host("host_abs", vec![Value::Int(-7)]).unwrap(),
            Value::Int(7)
        );
    }

    #[test]
    fn resolve_unknown_fails() {
        assert!(resolve_externs(&["no_such_host_fn".into()]).is_err());
    }

    #[test]
    fn embedder_register_and_call() {
        let out = execute_with_hosts(
            r#"
                extern "C" fn damage(base: Int, mult: Int) -> Int;
                fn main() -> Int { return damage(10, 3); }
            "#,
            &[("damage", damage)],
        )
        .unwrap();
        assert_eq!(out, Value::Int(30));
    }

    #[test]
    fn arg_helpers_reject_wrong_type() {
        let err = arg_int(&[Value::Str("x".into())], 0, "f").unwrap_err();
        assert!(err.contains("expects Int"));
    }
}

// ---------------------------------------------------------------------------
// Engine isolate (v0.3.1) — per-embedder host registry, not only process-global
// ---------------------------------------------------------------------------

/// Isolated execution environment for embedders.
///
/// Each `Engine` has its own host-function table. Global `register_host` still
/// works for simple scripts; prefer `Engine` when hosting multiple plugins.
pub struct Engine {
    hosts: HashMap<String, HostFn>,
    /// Max call depth (soft recursion guard).
    pub max_call_depth: usize,
    /// Max value stack depth per call frame.
    pub max_stack_depth: usize,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        let mut hosts = builtin_table();
        // Copy any process-global registrations into the isolate
        with_registry(|reg| {
            for (k, v) in reg.iter() {
                hosts.insert(k.clone(), *v);
            }
        });
        Self {
            hosts,
            max_call_depth: 256,
            max_stack_depth: 10_000,
        }
    }

    pub fn register(&mut self, name: impl Into<String>, f: HostFn) {
        self.hosts.insert(name.into(), f);
    }

    pub fn unregister(&mut self, name: &str) -> bool {
        self.hosts.remove(name).is_some()
    }

    pub fn call_host(&self, name: &str, args: &[Value]) -> Result<Value, String> {
        match self.hosts.get(name) {
            Some(f) => f(args),
            None => Err(format!("host function `{name}` not registered in this Engine")),
        }
    }

    /// Run source with this engine's hosts temporarily installed.
    pub fn execute(&self, src: &str) -> Result<Value, String> {
        let _guard = exec_lock().lock().expect("ffi exec lock");
        crate::vm::set_runtime_limits(self.max_call_depth, self.max_stack_depth);
        reset_registry();
        for (name, f) in &self.hosts {
            register_host(name.clone(), *f);
        }
        let result = crate::execute(src);
        reset_registry();
        crate::vm::reset_runtime_limits();
        result
    }
}

#[cfg(test)]
mod engine_tests {
    use super::*;
    use crate::ast::Value;

    fn times2(args: &[Value]) -> Result<Value, String> {
        let x = arg_int(args, 0, "times2")?;
        Ok(Value::Int(x * 2))
    }

    #[test]
    fn engine_isolate_registers() {
        let mut eng = Engine::new();
        eng.register("times2", times2);
        let out = eng
            .execute(
                r#"
                extern "C" fn times2(x: Int) -> Int;
                fn main() -> Int { return times2(21); }
            "#,
            )
            .unwrap();
        assert_eq!(out, Value::Int(42));
    }
}
