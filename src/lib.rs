//! S+N++ — a readable, statically-oriented systems language.
//!
//! Architecture: lexer → parser → AST → sema → HIR → MIR → bytecode VM
//! (+ optional LLVM/WASM backends, LSP, registry).

pub mod ast;
pub mod backend;
pub mod cli;
pub mod diagnostics;
pub mod ffi;
pub mod fmtutil;
pub mod hir;
pub mod lexer;
pub mod loader;
pub mod lsp;
pub mod mir;
pub mod package;
pub mod parser;
pub mod registry;
pub mod sema;
pub mod stdlib;
pub mod vm;

pub const VERSION: &str = "2.6.0";

pub use ast::{Expr, Function, Program, Stmt, Type, Value};
pub use lexer::{lex, Token};
pub use vm::{execute, execute_program, BytecodeVm};
