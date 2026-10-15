//! S+N++ — a readable, statically-oriented systems language.
//!
//! # Architecture
//!
//! ```text
//! source (.snp)
//!   → lexer        (tokens)
//!   → parser       (AST)
//!   → sema         (type check + ownership + definite assignment)
//!   → vm           (bytecode compile + stack VM)
//!   → runtime services (threads, channels)
//! ```
//!
//! Supporting modules: `stdlib`, `package`, `registry`, `cli`, `diagnostics`.

use std::collections::HashMap as Map;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;

pub mod ast;
pub mod cli;
pub mod diagnostics;
pub mod hir;
pub mod mir;
pub mod backend;
pub mod ffi;
pub mod fmtutil;
pub mod lsp;
pub mod lexer;
pub mod loader;
pub mod package;
pub mod parser;
pub mod registry;
pub mod sema;
pub mod stdlib;
pub mod vm;

// ── Global runtime tables for threads and channels ──────────────────────────

static THREADS: OnceLock<Mutex<Map<usize, JoinHandle<Value>>>> = OnceLock::new();
pub(crate) static NEXT_THREAD_ID: AtomicUsize = AtomicUsize::new(1);

static CHANNELS: OnceLock<Mutex<Map<usize, (Sender<Value>, Receiver<Value>)>>> = OnceLock::new();
pub(crate) static NEXT_CHANNEL_ID: AtomicUsize = AtomicUsize::new(1);

pub(crate) fn channel_table() -> &'static Mutex<Map<usize, (Sender<Value>, Receiver<Value>)>> {
    CHANNELS.get_or_init(|| Mutex::new(Map::new()))
}

pub(crate) fn thread_table() -> &'static Mutex<Map<usize, JoinHandle<Value>>> {
    THREADS.get_or_init(|| Mutex::new(Map::new()))
}

// ── Public re-exports ───────────────────────────────────────────────────────

pub use ast::{Expr, Function, Program, Stmt, Type, Value};
pub use lexer::{lex, Token};
pub use vm::{build_artifact, execute, BytecodeVm};
pub use ffi::{
    arg_bool, arg_int, arg_str, call_host, execute_with_hosts, expect_arity, has_host,
    list_hosts, register_host, reset_registry, resolve_externs, unregister_host, HostFn,
};

/// Language / toolchain version.
pub const VERSION: &str = "0.5.0";

