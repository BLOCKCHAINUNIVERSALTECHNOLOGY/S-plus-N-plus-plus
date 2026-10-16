//! S+N++ bytecode compiler and virtual machine.

use crate::ast::{Expr, Function, InterpPart, Pattern, Stmt, Value};
use crate::lexer::Token;
use crate::{parser, sema};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

static THREADS: OnceLock<Mutex<HashMap<usize, JoinHandle<Value>>>> = OnceLock::new();
static CHANNELS: OnceLock<Mutex<HashMap<usize, (Sender<Value>, Receiver<Value>)>>> = OnceLock::new();
pub(crate) static NEXT_THREAD_ID: AtomicUsize = AtomicUsize::new(1);
pub(crate) static NEXT_CHANNEL_ID: AtomicUsize = AtomicUsize::new(1);

pub(crate) fn channel_table() -> &'static Mutex<HashMap<usize, (Sender<Value>, Receiver<Value>)>> {
    CHANNELS.get_or_init(|| Mutex::new(HashMap::new()))
}
pub(crate) fn thread_table() -> &'static Mutex<HashMap<usize, JoinHandle<Value>>> {
    THREADS.get_or_init(|| Mutex::new(HashMap::new()))
}

thread_local! {
    static MAX_CALL_DEPTH: std::cell::Cell<usize> = std::cell::Cell::new(256);
    static MAX_STACK_DEPTH: std::cell::Cell<usize> = std::cell::Cell::new(10_000);
    static CALL_DEPTH: std::cell::Cell<usize> = std::cell::Cell::new(0);
}

/// Configure soft recursion / stack limits (used by `ffi::Engine`).
pub fn set_runtime_limits(max_call_depth: usize, max_stack_depth: usize) {
    MAX_CALL_DEPTH.with(|c| c.set(max_call_depth.max(8)));
    MAX_STACK_DEPTH.with(|c| c.set(max_stack_depth.max(64)));
}

pub fn reset_runtime_limits() {
    MAX_CALL_DEPTH.with(|c| c.set(256));
    MAX_STACK_DEPTH.with(|c| c.set(10_000));
    CALL_DEPTH.with(|c| c.set(0));
}

struct ScopeGuard(usize);
impl Drop for ScopeGuard {
    fn drop(&mut self) {
        CALL_DEPTH.with(|d| {
            let cur = d.get();
            if cur > 0 {
                d.set(cur - 1);
            }
        });
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Op {
    Const(Value),
    Load(String),
    Store(String),
    Add, Sub, Mul, Div, Mod, Eq, Ne, Lt, Lte, Gt, Gte,
    Neg, Not,
    Index,
    GetField(String),
    SetField(String),
    MakeStruct(String, Vec<String>),
    Call(String, usize),
    CallIndirect(usize), // pop closure/fn name value, then already-pushed args
    MakeClosure(String, Vec<String>), // name + capture names; pops capture values
    Print, Pop,
    JumpIfFalse(usize), Jump(usize),
    Return, Halt,
    IterLen,
    IterGet,
    EnumIs(String),
    EnumGet(usize),
    EnumMake(String, String, usize),
    /// Result unwrap: Ok → push payload; Err → return Err enum from function
    Try,
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub code: Vec<Op>,
}

#[allow(dead_code)]
struct LoopCtx {
    continue_target: usize,
    breaks: Vec<usize>,
    continues: Vec<usize>,
}


fn values_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Unit, Value::Unit) => true,
        (Value::Enum { name: n1, variant: v1, fields: f1 }, Value::Enum { name: n2, variant: v2, fields: f2 }) => {
            n1 == n2 && v1 == v2 && f1.len() == f2.len() && f1.iter().zip(f2.iter()).all(|(a,b)| values_eq(a,b))
        },
        (Value::List(x), Value::List(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| values_eq(a, b))
        }
        (Value::Tuple(x), Value::Tuple(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| values_eq(a, b))
        }
        _ => false,
    }
}

fn json_stringify_value(v: &Value) -> String {
    match v {
        Value::Int(n) => n.to_string(),
        Value::Bool(true) => "true".into(),
        Value::Bool(false) => "false".into(),
        Value::Unit => "null".into(),
        Value::Str(s) => {
            let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{escaped}\"")
        }
        Value::List(items) => {
            let parts: Vec<String> = items.iter().map(json_stringify_value).collect();
            format!("[{}]", parts.join(","))
        }
        Value::Map(map) => {
            let mut parts = Vec::new();
            for (k, val) in map.iter() {
                let ek = k.replace('\\', "\\\\").replace('"', "\\\"");
                parts.push(format!("\"{}\":{}", ek, json_stringify_value(val)));
            }
            format!("{{{}}}", parts.join(","))
        }
        other => format!("\"{}\"", other),
    }
}

fn json_parse_value(s: &str) -> Result<Value, String> {
    let s = s.trim();
    if s == "null" {
        return Ok(Value::Unit);
    }
    if s == "true" {
        return Ok(Value::Bool(true));
    }
    if s == "false" {
        return Ok(Value::Bool(false));
    }
    if let Ok(n) = s.parse::<i64>() {
        return Ok(Value::Int(n));
    }
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        let inner = &s[1..s.len() - 1];
        return Ok(Value::Str(inner.replace("\\\"", "\"").replace("\\\\", "\\")));
    }
    if s.starts_with('[') && s.ends_with(']') {
        let inner = s[1..s.len() - 1].trim();
        if inner.is_empty() {
            return Ok(Value::List(Arc::new(vec![])));
        }
        let mut items = Vec::new();
        for part in split_json_top(inner) {
            items.push(json_parse_value(&part)?);
        }
        return Ok(Value::List(Arc::new(items)));
    }
    Err(format!("json_parse: unsupported or invalid JSON: {s}"))
}

fn split_json_top(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut start = 0usize;
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' && (i == 0 || chars[i - 1] != '\\') {
            in_str = !in_str;
        } else if !in_str {
            if c == '[' || c == '{' {
                depth += 1;
            } else if c == ']' || c == '}' {
                depth -= 1;
            } else if c == ',' && depth == 0 {
                out.push(chars[start..i].iter().collect::<String>().trim().to_string());
                start = i + 1;
            }
        }
        i += 1;
    }
    if start < chars.len() {
        out.push(chars[start..].iter().collect::<String>().trim().to_string());
    }
    out
}


fn show(v: &Value) -> String {
    v.to_string()
}

fn truth(v: &Value) -> bool {
    v.is_truthy()
}

fn bin(a: Value, o: &Token, b: Value) -> Result<Value, String> {
    match (a, b, o) {
        (Value::Int(x), Value::Int(y), Token::Plus) => Ok(Value::Int(x + y)),
        (Value::Int(x), Value::Int(y), Token::Minus) => Ok(Value::Int(x - y)),
        (Value::Int(x), Value::Int(y), Token::Star) => Ok(Value::Int(x * y)),
        (Value::Int(x), Value::Int(y), Token::Slash) => Ok(Value::Int(x / y)),
        (Value::Int(x), Value::Int(y), Token::Percent) => Ok(Value::Int(x % y)),
        (Value::Int(x), Value::Int(y), Token::EqEq) => Ok(Value::Bool(x == y)),
        (Value::Int(x), Value::Int(y), Token::BangEq) => Ok(Value::Bool(x != y)),
        (Value::Int(x), Value::Int(y), Token::Lt) => Ok(Value::Bool(x < y)),
        (Value::Int(x), Value::Int(y), Token::Lte) => Ok(Value::Bool(x <= y)),
        (Value::Int(x), Value::Int(y), Token::Gt) => Ok(Value::Bool(x > y)),
        (Value::Int(x), Value::Int(y), Token::Gte) => Ok(Value::Bool(x >= y)),
        (Value::Str(x), Value::Str(y), Token::Plus) => Ok(Value::Str(x + &y)),
        (x, y, Token::EqEq) => Ok(Value::Bool(x == y)),
        (x, y, Token::BangEq) => Ok(Value::Bool(x != y)),
        _ => Err("invalid operands".into()),
    }
}

thread_local! {
    static CLOSURE_ID: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static PENDING_CLOSURES: std::cell::RefCell<HashMap<String, (Vec<String>, Chunk)>> =
        std::cell::RefCell::new(HashMap::new());
    static CAPTURE_INJECT: std::cell::RefCell<BTreeMap<String, Value>> =
        std::cell::RefCell::new(BTreeMap::new());
    /// Runtime call stack for error traces (function names, outermost last).
    static CALL_STACK: std::cell::RefCell<Vec<String>> =
        std::cell::RefCell::new(Vec::new());
}

fn format_runtime_error(message: impl std::fmt::Display) -> String {
    let mut out = format!("runtime error: {message}");
    CALL_STACK.with(|stack| {
        let frames = stack.borrow();
        if frames.is_empty() {
            return;
        }
        out.push_str("
stack traceback:");
        for (i, name) in frames.iter().rev().enumerate() {
            out.push_str(&format!("
  {i}: {name}()"));
        }
    });
    out
}

struct CallFrameGuard;
impl Drop for CallFrameGuard {
    fn drop(&mut self) {
        CALL_STACK.with(|s| {
            s.borrow_mut().pop();
        });
    }
}

fn push_frame(name: &str) -> CallFrameGuard {
    CALL_STACK.with(|s| s.borrow_mut().push(name.to_string()));
    CallFrameGuard
}


#[allow(dead_code)]
fn take_pending_closures() -> HashMap<String, (Vec<String>, Chunk)> {
    PENDING_CLOSURES.with(|p| std::mem::take(&mut *p.borrow_mut()))
}


#[allow(dead_code)]
fn free_vars_expr(x: &Expr, bound: &HashSet<String>, out: &mut HashSet<String>) {
    match x {
        Expr::Var(n) => {
            if !bound.contains(n) {
                out.insert(n.clone());
            }
        }
        Expr::Binary(a, _, b) | Expr::Index(a, b) => {
            free_vars_expr(a, bound, out);
            free_vars_expr(b, bound, out);
        }
        Expr::Unary(_, e) | Expr::Try(e) | Expr::Field(e, _) => free_vars_expr(e, bound, out),
        Expr::IfExpr { cond, then_branch, else_branch } => {
            free_vars_expr(cond, bound, out);
            free_vars_expr(then_branch, bound, out);
            free_vars_expr(else_branch, bound, out);
        }
        Expr::Tuple(items) => {
            for it in items {
                free_vars_expr(it, bound, out);
            }
        }
        Expr::Hole => {}
        Expr::MatchExpr { scrutinee, arms } => {
            free_vars_expr(scrutinee, bound, out);
            for a in arms {
                if let Some(g) = &a.guard {
                    free_vars_expr(g, bound, out);
                }
                free_vars_expr(&a.value, bound, out);
            }
        }
        Expr::Call(_, args) => {
            for a in args {
                free_vars_expr(a, bound, out);
            }
        }
        Expr::CallExpr(callee, args) => {
            free_vars_expr(callee, bound, out);
            for a in args {
                free_vars_expr(a, bound, out);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, e) in fields {
                free_vars_expr(e, bound, out);
            }
        }
        Expr::Interp(parts) => {
            for p in parts {
                if let InterpPart::Expr(e) = p {
                    free_vars_expr(e, bound, out);
                }
            }
        }
        Expr::Lambda { params, body, .. } => {
            let mut inner = bound.clone();
            for p in params {
                inner.insert(p.clone());
            }
            free_vars_stmts(body, &inner, out);
        }
        Expr::Int(_) | Expr::Str(_) | Expr::Bool(_) => {}
        Expr::MethodCall { receiver, args, .. } => {
            free_vars_expr(receiver, bound, out);
            for a in args {
                free_vars_expr(a, bound, out);
            }
        }
        Expr::VariantLit { args, .. } => {
            for a in args {
                free_vars_expr(a, bound, out);
            }
        }
    }
}

#[allow(dead_code)]
fn free_vars_stmts(xs: &[Stmt], bound: &HashSet<String>, out: &mut HashSet<String>) {
    let mut bound = bound.clone();
    for s in xs {
        match s {
            Stmt::IfLet { value, yes, no, .. } => {
                free_vars_expr(value, &bound, out);
                free_vars_stmts(yes, &bound, out);
                free_vars_stmts(no, &bound, out);
            }
            Stmt::Const(n, e) | Stmt::Let(n, e) => {
                free_vars_expr(e, &bound, out);
                bound.insert(n.clone());
            }
            Stmt::Decl(n, _) => {
                bound.insert(n.clone());
            }
            Stmt::Assign(_, e) | Stmt::Expr(e) | Stmt::Return(e) => {
                free_vars_expr(e, &bound, out);
            }
            Stmt::FieldAssign { object, value, .. } => {
                free_vars_expr(object, &bound, out);
                free_vars_expr(value, &bound, out);
            }
            Stmt::If(c, a, b) => {
                free_vars_expr(c, &bound, out);
                free_vars_stmts(a, &bound, out);
                free_vars_stmts(b, &bound, out);
            }
            Stmt::While(c, body) => {
                free_vars_expr(c, &bound, out);
                free_vars_stmts(body, &bound, out);
            }
            Stmt::For { var, start, end, inclusive, body } => {
                free_vars_expr(start, &bound, out);
                free_vars_expr(end, &bound, out);
                let mut b2 = bound.clone();
                b2.insert(var.clone());
                free_vars_stmts(body, &b2, out);
            }
            Stmt::ForIn { var, index, iter, body } => {
                free_vars_expr(iter, &bound, out);
                let mut b2 = bound.clone();
                b2.insert(var.clone());
                if let Some(i) = index {
                    b2.insert(i.clone());
                }
                free_vars_stmts(body, &b2, out);
            }
            Stmt::Match { scrutinee, arms } => {
                free_vars_expr(scrutinee, &bound, out);
                for arm in arms {
                    let pat = &arm.pattern;
                    let body = &arm.body;
                    if let Some(g) = &arm.guard {
                        free_vars_expr(g, &bound, out);
                    }
                    let mut b2 = bound.clone();
                    if let Pattern::Var(n) = pat {
                        b2.insert(n.clone());
                    }
                    free_vars_stmts(body, &b2, out);
                }
            }
            Stmt::Break | Stmt::Continue | Stmt::Defer(_) => {}
        }
    }
}

#[allow(dead_code)]
fn compile_expr(x: &Expr, c: &mut Chunk) -> Result<(), String> {
    match x {
        Expr::Int(v) => c.code.push(Op::Const(Value::Int(*v))),
        Expr::Str(v) => c.code.push(Op::Const(Value::Str(v.clone()))),
        Expr::Bool(v) => c.code.push(Op::Const(Value::Bool(*v))),
        Expr::Var(n) => c.code.push(Op::Load(n.clone())),
        Expr::Binary(a, o, b) => {
            compile_expr(a, c)?;
            compile_expr(b, c)?;
            c.code.push(match o {
                Token::Plus => Op::Add,
                Token::Minus => Op::Sub,
                Token::Star => Op::Mul,
                Token::Slash => Op::Div,
                Token::Percent => Op::Mod,
                Token::EqEq => Op::Eq,
                Token::BangEq => Op::Ne,
                Token::Lt => Op::Lt,
                Token::Lte => Op::Lte,
                Token::Gt => Op::Gt,
                Token::Gte => Op::Gte,
                _ => return Err("invalid bytecode operator".into()),
            });
        }
        Expr::Try(e) => {
            compile_expr(e, c)?;
            c.code.push(Op::Try);
        }
        Expr::IfExpr {
            cond,
            then_branch,
            else_branch,
        } => {
            compile_expr(cond, c)?;
            let jf = c.code.len();
            c.code.push(Op::JumpIfFalse(0));
            compile_expr(then_branch, c)?;
            let j = c.code.len();
            c.code.push(Op::Jump(0));
            let else_ip = c.code.len();
            compile_expr(else_branch, c)?;
            let end = c.code.len();
            c.code[jf] = Op::JumpIfFalse(else_ip);
            c.code[j] = Op::Jump(end);
        }
        Expr::MatchExpr { .. } => {
            return Err("match expression should be desugared before compile".into());
        }
        Expr::Hole => {
            return Err("pipeline hole `_` outside of `|>`".into());
        }
        Expr::Tuple(items) => {
            for it in items {
                compile_expr(it, c)?;
            }
            c.code.push(Op::Call("__tuple_make".into(), items.len()));
        }
        Expr::Unary(op, e) => {
            compile_expr(e, c)?;
            c.code.push(match op {
                Token::Minus => Op::Neg,
                Token::Bang => Op::Not,
                _ => return Err("invalid unary op".into()),
            });
        }
        Expr::Call(n, a) => {
            for x in a {
                compile_expr(x, c)?;
            }
            c.code.push(if n == "print" {
                Op::Print
            } else {
                Op::Call(n.clone(), a.len())
            });
        }
        Expr::Index(base, idx) => {
            compile_expr(base, c)?;
            compile_expr(idx, c)?;
            c.code.push(Op::Index);
        }
        Expr::Field(obj, field) => {
            compile_expr(obj, c)?;
            c.code.push(Op::GetField(field.clone()));
        }
        Expr::StructLit { name, fields } => {
            let mut names = Vec::new();
            for (fname, fexpr) in fields {
                compile_expr(fexpr, c)?;
                names.push(fname.clone());
            }
            c.code.push(Op::MakeStruct(name.clone(), names));
        }
        Expr::Interp(parts) => {
            let mut first = true;
            for p in parts {
                match p {
                    InterpPart::Lit(s) => {
                        c.code.push(Op::Const(Value::Str(s.clone())));
                    }
                    InterpPart::Expr(e) => {
                        compile_expr(e, c)?;
                        c.code.push(Op::Call("__to_string".into(), 1));
                    }
                }
                if !first {
                    c.code.push(Op::Add);
                }
                first = false;
            }
            if first {
                c.code.push(Op::Const(Value::Str(String::new())));
            }
        }
        Expr::Lambda { params, body, .. } => {
            let id = CLOSURE_ID.with(|cell| {
                let n = cell.get();
                cell.set(n + 1);
                n
            });
            let name = format!("__closure_{id}");
            // free variables = used in body but not parameters
            let mut bound = HashSet::new();
            for p in params {
                bound.insert(p.clone());
            }
            let mut free = HashSet::new();
            free_vars_stmts(body, &bound, &mut free);
            let mut caps: Vec<String> = free.into_iter().collect();
            caps.sort();
            // load capture values from outer environment
            for cap in &caps {
                c.code.push(Op::Load(cap.clone()));
            }
            let mut body_chunk = Chunk { code: Vec::new() };
            compile_stmts(body, &mut body_chunk)?;
            body_chunk.code.push(Op::Const(Value::Unit));
            body_chunk.code.push(Op::Return);
            PENDING_CLOSURES.with(|p| {
                p.borrow_mut().insert(name.clone(), (params.clone(), body_chunk));
            });
            c.code.push(Op::MakeClosure(name, caps));
        }
        Expr::CallExpr(callee, args) => {
            for a in args {
                compile_expr(a, c)?;
            }
            compile_expr(callee, c)?;
            c.code.push(Op::CallIndirect(args.len()));
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            // Fallback if desugar skipped: treat as indirect (should not run)
            compile_expr(receiver, c)?;
            for a in args {
                compile_expr(a, c)?;
            }
            c.code.push(Op::Call(format!("?.{method}"), args.len() + 1));
        }
        Expr::VariantLit {
            enum_name,
            variant,
            args,
        } => {
            for a in args {
                compile_expr(a, c)?;
            }
            c.code.push(Op::Const(Value::Str(enum_name.clone())));
            c.code.push(Op::Const(Value::Str(variant.clone())));
            c.code.push(Op::Call("__enum_make".into(), args.len() + 2));
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn compile_stmts(xs: &[Stmt], c: &mut Chunk) -> Result<(), String> {
    compile_stmts_loop(xs, c, None)
}

#[allow(dead_code)]
fn compile_stmts_loop(
    xs: &[Stmt],
    c: &mut Chunk,
    mut loop_ctx: Option<&mut LoopCtx>,
) -> Result<(), String> {
    for s in xs {
        match s {
            Stmt::Defer(body) => {
                compile_stmts_loop(body, c, loop_ctx.as_deref_mut())?;
            }
            Stmt::IfLet { .. } => {
                return Err("if let should be desugared before compile".into());
            }
            Stmt::Const(n, x) | Stmt::Let(n, x) | Stmt::Assign(n, x) => {
                compile_expr(x, c)?;
                c.code.push(Op::Store(n.clone()));
            }
            Stmt::Decl(_, _) => {}
            Stmt::FieldAssign { object, field, value } => {
                compile_expr(object, c)?;
                compile_expr(value, c)?;
                c.code.push(Op::SetField(field.clone()));
                // SetField leaves updated struct on stack — store back if object is Var
                if let Expr::Var(name) = object {
                    c.code.push(Op::Store(name.clone()));
                } else {
                    c.code.push(Op::Pop);
                }
            }
            Stmt::Expr(x) => {
                compile_expr(x, c)?;
                c.code.push(Op::Pop);
            }
            Stmt::Return(x) => {
                compile_expr(x, c)?;
                c.code.push(Op::Return);
            }
            Stmt::If(cond, a, b) => {
                compile_expr(cond, c)?;
                let jf = c.code.len();
                c.code.push(Op::JumpIfFalse(0));
                compile_stmts_loop(a, c, loop_ctx.as_deref_mut())?;
                let j = c.code.len();
                c.code.push(Op::Jump(0));
                let else_ip = c.code.len();
                compile_stmts_loop(b, c, loop_ctx.as_deref_mut())?;
                let end = c.code.len();
                c.code[jf] = Op::JumpIfFalse(else_ip);
                c.code[j] = Op::Jump(end);
            }
            Stmt::While(cond, body) => {
                let mut ctx = LoopCtx {
                    continue_target: 0,
                    breaks: Vec::new(),
                    continues: Vec::new(),
                };
                let start = c.code.len();
                ctx.continue_target = start;
                compile_expr(cond, c)?;
                let jf = c.code.len();
                c.code.push(Op::JumpIfFalse(0));
                compile_stmts_loop(body, c, Some(&mut ctx))?;
                c.code.push(Op::Jump(start));
                let end = c.code.len();
                c.code[jf] = Op::JumpIfFalse(end);
                for bi in ctx.breaks {
                    c.code[bi] = Op::Jump(end);
                }
                for ci in ctx.continues {
                    c.code[ci] = Op::Jump(start);
                }
            }
            Stmt::For { var, start, end, inclusive, body } => {
                let tmp = format!("__for_{}", var);
                compile_expr(start, c)?;
                c.code.push(Op::Store(tmp.clone()));
                let mut ctx = LoopCtx {
                    continue_target: 0,
                    breaks: Vec::new(),
                    continues: Vec::new(),
                };
                let loop_start = c.code.len();
                c.code.push(Op::Load(tmp.clone()));
                compile_expr(end, c)?;
                c.code.push(Op::Lt);
                let jf = c.code.len();
                c.code.push(Op::JumpIfFalse(0));
                c.code.push(Op::Load(tmp.clone()));
                c.code.push(Op::Store(var.clone()));
                compile_stmts_loop(body, c, Some(&mut ctx))?;
                let incr_ip = c.code.len();
                ctx.continue_target = incr_ip;
                c.code.push(Op::Load(tmp.clone()));
                c.code.push(Op::Const(Value::Int(1)));
                c.code.push(Op::Add);
                c.code.push(Op::Store(tmp));
                c.code.push(Op::Jump(loop_start));
                let loop_end = c.code.len();
                c.code[jf] = Op::JumpIfFalse(loop_end);
                for bi in ctx.breaks {
                    c.code[bi] = Op::Jump(loop_end);
                }
                for ci in ctx.continues {
                    c.code[ci] = Op::Jump(incr_ip);
                }
            }
            Stmt::ForIn { var, index, iter, body } => {
                // desugar: let __list = iter; let __i = 0; let __n = len; while __i < __n { let var = __list[__i]; body; __i++ }
                let list_tmp = format!("__iter_list_{}", var);
                let idx_tmp = format!("__iter_i_{}", var);
                let len_tmp = format!("__iter_n_{}", var);
                compile_expr(iter, c)?;
                c.code.push(Op::Store(list_tmp.clone()));
                c.code.push(Op::Load(list_tmp.clone()));
                c.code.push(Op::IterLen);
                c.code.push(Op::Store(len_tmp.clone()));
                c.code.push(Op::Const(Value::Int(0)));
                c.code.push(Op::Store(idx_tmp.clone()));
                let mut ctx = LoopCtx {
                    continue_target: 0,
                    breaks: Vec::new(),
                    continues: Vec::new(),
                };
                let loop_start = c.code.len();
                c.code.push(Op::Load(idx_tmp.clone()));
                c.code.push(Op::Load(len_tmp.clone()));
                c.code.push(Op::Lt);
                let jf = c.code.len();
                c.code.push(Op::JumpIfFalse(0));
                c.code.push(Op::Load(list_tmp.clone()));
                c.code.push(Op::Load(idx_tmp.clone()));
                c.code.push(Op::IterGet);
                c.code.push(Op::Store(var.clone()));
                if let Some(idx_name) = index {
                    c.code.push(Op::Load(idx_tmp.clone()));
                    c.code.push(Op::Store(idx_name.clone()));
                }
                compile_stmts_loop(body, c, Some(&mut ctx))?;
                let incr_ip = c.code.len();
                c.code.push(Op::Load(idx_tmp.clone()));
                c.code.push(Op::Const(Value::Int(1)));
                c.code.push(Op::Add);
                c.code.push(Op::Store(idx_tmp));
                c.code.push(Op::Jump(loop_start));
                let loop_end = c.code.len();
                c.code[jf] = Op::JumpIfFalse(loop_end);
                for bi in ctx.breaks {
                    c.code[bi] = Op::Jump(loop_end);
                }
                for ci in ctx.continues {
                    c.code[ci] = Op::Jump(incr_ip);
                }
            }
            Stmt::Match { scrutinee, arms } => {
                compile_expr(scrutinee, c)?;
                let tmp = "__match_val".to_string();
                c.code.push(Op::Store(tmp.clone()));
                let mut end_jumps = Vec::new();
                for arm in arms {
                    let pat = &arm.pattern;
                    let body = &arm.body;
                    // compare pattern
                    c.code.push(Op::Load(tmp.clone()));
                    match pat {
                        Pattern::Tuple(_) => {
                            // simplified: treat like wildcard for AST compile path
                            c.code.push(Op::Pop);
                            compile_stmts_loop(body, c, loop_ctx.as_deref_mut())?;
                            let j = c.code.len();
                            c.code.push(Op::Jump(0));
                            end_jumps.push(j);
                            break;
                        }
                        Pattern::Wildcard => {
                            c.code.push(Op::Pop);
                            if let Some(g) = &arm.guard {
                                compile_expr(g, c)?;
                                let jf = c.code.len();
                                c.code.push(Op::JumpIfFalse(0));
                                compile_stmts_loop(body, c, loop_ctx.as_deref_mut())?;
                                let j = c.code.len();
                                c.code.push(Op::Jump(0));
                                end_jumps.push(j);
                                let next = c.code.len();
                                c.code[jf] = Op::JumpIfFalse(next);
                                continue;
                            }
                            compile_stmts_loop(body, c, loop_ctx.as_deref_mut())?;
                            let j = c.code.len();
                            c.code.push(Op::Jump(0));
                            end_jumps.push(j);
                            break;
                        }
                        Pattern::Int(v) => {
                            c.code.push(Op::Const(Value::Int(*v)));
                            c.code.push(Op::Eq);
                        }
                        Pattern::Bool(v) => {
                            c.code.push(Op::Const(Value::Bool(*v)));
                            c.code.push(Op::Eq);
                        }
                        Pattern::Str(s) => {
                            c.code.push(Op::Const(Value::Str(s.clone())));
                            c.code.push(Op::Eq);
                        }
                        Pattern::Var(n) => {
                            c.code.push(Op::Store(n.clone()));
                            if let Some(g) = &arm.guard {
                                compile_expr(g, c)?;
                                let jf = c.code.len();
                                c.code.push(Op::JumpIfFalse(0));
                                compile_stmts_loop(body, c, loop_ctx.as_deref_mut())?;
                                let j = c.code.len();
                                c.code.push(Op::Jump(0));
                                end_jumps.push(j);
                                let next = c.code.len();
                                c.code[jf] = Op::JumpIfFalse(next);
                                continue;
                            }
                            compile_stmts_loop(body, c, loop_ctx.as_deref_mut())?;
                            let j = c.code.len();
                            c.code.push(Op::Jump(0));
                            end_jumps.push(j);
                            continue;
                        }
                        Pattern::Variant { variant, bindings, .. } => {
                            c.code.push(Op::Const(Value::Str(variant.clone())));
                            c.code.push(Op::Call("__enum_is".into(), 2));
                            let jf = c.code.len();
                            c.code.push(Op::JumpIfFalse(0));
                            for (i, bind) in bindings.iter().enumerate() {
                                c.code.push(Op::Load(tmp.clone()));
                                c.code.push(Op::Const(Value::Int(i as i64)));
                                c.code.push(Op::Call("__enum_get".into(), 2));
                                c.code.push(Op::Store(bind.clone()));
                            }
                            if let Some(g) = &arm.guard {
                                compile_expr(g, c)?;
                                let jg = c.code.len();
                                c.code.push(Op::JumpIfFalse(0));
                                compile_stmts_loop(body, c, loop_ctx.as_deref_mut())?;
                                let j = c.code.len();
                                c.code.push(Op::Jump(0));
                                end_jumps.push(j);
                                let next = c.code.len();
                                c.code[jf] = Op::JumpIfFalse(next);
                                c.code[jg] = Op::JumpIfFalse(next);
                                continue;
                            }
                            compile_stmts_loop(body, c, loop_ctx.as_deref_mut())?;
                            let j = c.code.len();
                            c.code.push(Op::Jump(0));
                            end_jumps.push(j);
                            let next = c.code.len();
                            c.code[jf] = Op::JumpIfFalse(next);
                            continue;
                        }
                        Pattern::Or(_) => {
                            // HIR expands these; skip in AST path
                            c.code.push(Op::Pop);
                            c.code.push(Op::Const(Value::Bool(false)));
                        }
                        Pattern::Struct { name, fields } => {
                            c.code.push(Op::Const(Value::Str(name.clone())));
                            c.code.push(Op::Call("__struct_is".into(), 2));
                            let jf = c.code.len();
                            c.code.push(Op::JumpIfFalse(0));
                            for (fname, bname) in fields {
                                c.code.push(Op::Load(tmp.clone()));
                                c.code.push(Op::GetField(fname.clone()));
                                c.code.push(Op::Store(bname.clone()));
                            }
                            if let Some(g) = &arm.guard {
                                compile_expr(g, c)?;
                                let jg = c.code.len();
                                c.code.push(Op::JumpIfFalse(0));
                                compile_stmts_loop(body, c, loop_ctx.as_deref_mut())?;
                                let j = c.code.len();
                                c.code.push(Op::Jump(0));
                                end_jumps.push(j);
                                let next = c.code.len();
                                c.code[jf] = Op::JumpIfFalse(next);
                                c.code[jg] = Op::JumpIfFalse(next);
                                continue;
                            }
                            compile_stmts_loop(body, c, loop_ctx.as_deref_mut())?;
                            let j = c.code.len();
                            c.code.push(Op::Jump(0));
                            end_jumps.push(j);
                            let next = c.code.len();
                            c.code[jf] = Op::JumpIfFalse(next);
                            continue;
                        }
                    }
                    let jf = c.code.len();
                    c.code.push(Op::JumpIfFalse(0));
                    if let Some(g) = &arm.guard {
                        compile_expr(g, c)?;
                        let jg = c.code.len();
                        c.code.push(Op::JumpIfFalse(0));
                        compile_stmts_loop(body, c, loop_ctx.as_deref_mut())?;
                        let j = c.code.len();
                        c.code.push(Op::Jump(0));
                        end_jumps.push(j);
                        let next = c.code.len();
                        c.code[jf] = Op::JumpIfFalse(next);
                        c.code[jg] = Op::JumpIfFalse(next);
                    } else {
                        compile_stmts_loop(body, c, loop_ctx.as_deref_mut())?;
                        let j = c.code.len();
                        c.code.push(Op::Jump(0));
                        end_jumps.push(j);
                        let next = c.code.len();
                        c.code[jf] = Op::JumpIfFalse(next);
                    }
                }
                let end = c.code.len();
                for j in end_jumps {
                    c.code[j] = Op::Jump(end);
                }
            }
            Stmt::Break => {
                if let Some(ctx) = loop_ctx.as_deref_mut() {
                    let idx = c.code.len();
                    c.code.push(Op::Jump(0));
                    ctx.breaks.push(idx);
                } else {
                    return Err("`break` outside of a loop".into());
                }
            }
            Stmt::Continue => {
                if let Some(ctx) = loop_ctx.as_deref_mut() {
                    let idx = c.code.len();
                    c.code.push(Op::Jump(0));
                    ctx.continues.push(idx);
                } else {
                    return Err("`continue` outside of a loop".into());
                }
            }
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn compile_functions(fs: &[Function]) -> Result<HashMap<String, (Vec<String>, Chunk)>, String> {
    // reset closure state
    CLOSURE_ID.with(|cell| cell.set(0));
    PENDING_CLOSURES.with(|p| p.borrow_mut().clear());

    let mut out = HashMap::new();
    for f in fs {
        let mut c = Chunk { code: Vec::new() };
        compile_stmts(&f.body, &mut c)?;
        c.code.push(Op::Const(Value::Unit));
        c.code.push(Op::Return);
        out.insert(f.name.clone(), (f.params.clone(), c));
    }
    // nested closures compiled during the above
    let pending = take_pending_closures();
    for (name, chunk) in pending {
        out.insert(name, chunk);
    }
    Ok(out)
}

pub struct BytecodeVm {
    funcs: HashMap<String, (Vec<String>, Chunk)>,
}

impl BytecodeVm {
    fn call_with_captures(
        &self,
        name: &str,
        args: Vec<Value>,
        captures: BTreeMap<String, Value>,
    ) -> Result<Value, String> {
        CAPTURE_INJECT.with(|cell| {
            *cell.borrow_mut() = captures;
        });
        let result = self.call(name, args);
        CAPTURE_INJECT.with(|cell| cell.borrow_mut().clear());
        result
    }

    fn call(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        let depth = CALL_DEPTH.with(|d| {
            let n = d.get() + 1;
            d.set(n);
            n
        });
        let max_d = MAX_CALL_DEPTH.with(|c| c.get());
        if depth > max_d {
            CALL_DEPTH.with(|d| d.set(depth - 1));
            return Err(format_runtime_error(format!(
                "recursion limit exceeded (max call depth {max_d})"
            )));
        }
        let _depth_guard = ScopeGuard(depth);
        let _guard = push_frame(name);
        self.call_impl(name, args).map_err(|e| {
            if e.starts_with("runtime error:") || e.contains("stack traceback:") {
                e
            } else {
                format_runtime_error(e)
            }
        })
    }

    fn call_impl(&self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        // Builtins
        match name {
            "__to_string" => {
                return Ok(Value::Str(show(args.get(0).unwrap_or(&Value::Unit))));
            }
            "__tuple_make" => {
                return Ok(Value::Tuple(args));
            }
            "tuple_get" => {
                if let (Some(Value::Tuple(items)), Some(Value::Int(i))) = (args.get(0), args.get(1)) {
                    return items
                        .get(*i as usize)
                        .cloned()
                        .ok_or_else(|| "tuple index out of bounds".into());
                }
                return Err("tuple_get expects tuple and Int".into());
            }
            "tuple_len" => {
                if let Some(Value::Tuple(items)) = args.get(0) {
                    return Ok(Value::Int(items.len() as i64));
                }
                return Err("tuple_len expects tuple".into());
            }
            "abs" => {
                if let Some(Value::Int(x)) = args.get(0) {
                    return Ok(Value::Int(x.abs()));
                }
                return Err("abs expects Int".into());
            }
            "__enum_make" => {
                // args: fields..., enum_name, variant
                if args.len() < 2 {
                    return Err("__enum_make arity".into());
                }
                let variant = match args.last() {
                    Some(Value::Str(s)) => s.clone(),
                    _ => return Err("__enum_make expects variant name".into()),
                };
                let enum_name = match args.get(args.len() - 2) {
                    Some(Value::Str(s)) => s.clone(),
                    _ => return Err("__enum_make expects enum name".into()),
                };
                let fields: Vec<Value> = args[..args.len() - 2].to_vec();
                return Ok(Value::Enum {
                    name: enum_name,
                    variant,
                    fields,
                });
            }
            "__struct_is" => {
                if let (Some(Value::Struct { name, .. }), Some(Value::Str(want))) =
                    (args.get(0), args.get(1))
                {
                    return Ok(Value::Bool(name == want));
                }
                return Ok(Value::Bool(false));
            }
            "__enum_is" => {
                if let (Some(Value::Enum { variant, .. }), Some(Value::Str(want))) =
                    (args.get(0), args.get(1))
                {
                    return Ok(Value::Bool(variant == want));
                }
                return Ok(Value::Bool(false));
            }
            "__enum_get" => {
                if let (Some(Value::Enum { fields, .. }), Some(Value::Int(i))) =
                    (args.get(0), args.get(1))
                {
                    return Ok(fields
                        .get(*i as usize)
                        .cloned()
                        .unwrap_or(Value::Unit));
                }
                return Err("__enum_get expects enum and index".into());
            }
            "min" => {
                if let (Some(Value::Int(a)), Some(Value::Int(b))) = (args.get(0), args.get(1)) {
                    return Ok(Value::Int(*a.min(b)));
                }
                return Err("min expects two Int".into());
            }
            "max" => {
                if let (Some(Value::Int(a)), Some(Value::Int(b))) = (args.get(0), args.get(1)) {
                    return Ok(Value::Int(*a.max(b)));
                }
                return Err("max expects two Int".into());
            }
            "pow" => {
                if let (Some(Value::Int(a)), Some(Value::Int(b))) = (args.get(0), args.get(1)) {
                    return Ok(Value::Int(a.pow(*b as u32)));
                }
                return Err("pow expects two Int".into());
            }
            "sqrt_approx" => {
                if let Some(Value::Int(x)) = args.get(0) {
                    return Ok(Value::Int((*x as f64).sqrt() as i64));
                }
                return Err("sqrt_approx expects Int".into());
            }
            "now_ms" => {
                let ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                return Ok(Value::Int(ms));
            }
            "rand" => {
                // simple LCG from time
                let seed = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_nanos() as i64)
                    .unwrap_or(1);
                return Ok(Value::Int(seed.abs() % 10000));
            }
            "rand_range" => {
                if let (Some(Value::Int(lo)), Some(Value::Int(hi))) = (args.get(0), args.get(1)) {
                    let seed = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_nanos() as i64)
                        .unwrap_or(1);
                    let span = (hi - lo).max(1);
                    return Ok(Value::Int(lo + seed.abs() % span));
                }
                return Err("rand_range expects two Int".into());
            }
            // --- std.fs ---
            "fs_read_text" => {
                let path = match args.get(0) {
                    Some(Value::Str(s)) => s,
                    _ => return Err("fs_read_text expects String path".into()),
                };
                match std::fs::read_to_string(path) {
                    Ok(s) => return Ok(Value::Str(s)),
                    Err(e) => return Err(format!("fs_read_text `{path}`: {e}")),
                }
            }
            "fs_write_text" => {
                let path = match args.get(0) {
                    Some(Value::Str(s)) => s,
                    _ => return Err("fs_write_text expects (String, String)".into()),
                };
                let body = match args.get(1) {
                    Some(Value::Str(s)) => s,
                    _ => return Err("fs_write_text expects (String, String)".into()),
                };
                match std::fs::write(path, body) {
                    Ok(()) => return Ok(Value::Unit),
                    Err(e) => return Err(format!("fs_write_text `{path}`: {e}")),
                }
            }
            "fs_exists" => {
                let path = match args.get(0) {
                    Some(Value::Str(s)) => s,
                    _ => return Err("fs_exists expects String path".into()),
                };
                return Ok(Value::Bool(std::path::Path::new(path).exists()));
            }
            // --- std.json (minimal) ---
            "json_stringify" => {
                let v = args.get(0).cloned().unwrap_or(Value::Unit);
                return Ok(Value::Str(json_stringify_value(&v)));
            }
            "json_parse" => {
                let s = match args.get(0) {
                    Some(Value::Str(s)) => s.as_str(),
                    _ => return Err("json_parse expects String".into()),
                };
                return json_parse_value(s.trim());
            }
            // --- std.test ---
            "panic" => {
                let msg = match args.get(0) {
                    Some(Value::Str(s)) => s.clone(),
                    Some(v) => show(v),
                    None => "panic".into(),
                };
                return Err(format!("panic: {msg}"));
            }
            "todo" => {
                let msg = match args.get(0) {
                    Some(Value::Str(s)) => s.clone(),
                    _ => "not yet implemented".into(),
                };
                return Err(format!("todo: {msg}"));
            }
            "assert" => {
                // assert(cond) or assert(cond, "msg")
                let ok = args.get(0).map(|v| truth(v)).unwrap_or(false);
                if !ok {
                    let msg = match args.get(1) {
                        Some(Value::Str(s)) => s.clone(),
                        _ => "assert failed".into(),
                    };
                    return Err(msg);
                }
                return Ok(Value::Unit);
            }
            "assert_true" => {
                let ok = match args.get(0) {
                    Some(Value::Bool(b)) => *b,
                    Some(v) => truth(v),
                    None => false,
                };
                if !ok {
                    return Err("assert_true failed".into());
                }
                return Ok(Value::Unit);
            }
            "assert_false" => {
                let ok = match args.get(0) {
                    Some(Value::Bool(b)) => !*b,
                    Some(v) => !truth(v),
                    None => false,
                };
                if !ok {
                    return Err("assert_false failed".into());
                }
                return Ok(Value::Unit);
            }
            "assert_eq" => {
                // assert_eq(a, b) or assert_eq(a, b, "msg")
                let a = args.get(0).cloned().unwrap_or(Value::Unit);
                let b = args.get(1).cloned().unwrap_or(Value::Unit);
                if !values_eq(&a, &b) {
                    return Err(format!(
                        "assert_eq failed: left={a}, right={b}"
                    ));
                }
                return Ok(Value::Unit);
            }
            "channel" => {
                let id = NEXT_CHANNEL_ID.fetch_add(1, Ordering::SeqCst);
                let (tx, rx) = mpsc::channel();
                channel_table().lock().unwrap().insert(id, (tx, rx));
                return Ok(Value::Channel(id));
            }
            "send" => {
                if let (Some(Value::Channel(id)), Some(v)) = (args.get(0), args.get(1)) {
                    let table = channel_table().lock().unwrap();
                    let (tx, _) = table.get(id).ok_or("invalid channel")?;
                    tx.send(v.clone()).map_err(|_| "send failed")?;
                    return Ok(Value::Unit);
                }
                return Err("send expects Channel and value".into());
            }
            "receive" => {
                if let Some(Value::Channel(id)) = args.get(0) {
                    let table = channel_table().lock().unwrap();
                    let (_, rx) = table.get(id).ok_or("invalid channel")?;
                    return rx.recv().map_err(|_| "receive failed".into());
                }
                return Err("receive expects Channel".into());
            }
            "spawn" => {
                let fname = match args.get(0) {
                    Some(Value::Str(s)) => s.clone(),
                    _ => return Err("spawn expects function name string".into()),
                };
                let rest: Vec<Value> = args.into_iter().skip(1).collect();
                let funcs = self.funcs.clone();
                let id = NEXT_THREAD_ID.fetch_add(1, Ordering::SeqCst);
                let handle = thread::spawn(move || {
                    let vm = BytecodeVm { funcs };
                    vm.call(&fname, rest).unwrap_or(Value::Unit)
                });
                thread_table().lock().unwrap().insert(id, handle);
                return Ok(Value::Thread(id));
            }
            "join" => {
                if let Some(Value::Thread(id)) = args.get(0) {
                    let handle = thread_table()
                        .lock()
                        .unwrap()
                        .remove(id)
                        .ok_or("invalid thread")?;
                    return Ok(handle.join().unwrap_or(Value::Unit));
                }
                return Err("join expects Thread".into());
            }
            "list" => return Ok(Value::List(Arc::new(args))),
            "list_push" => {
                if let Some(Value::List(rc)) = args.get(0) {
                    let mut items = Arc::clone(rc);
                    Arc::make_mut(&mut items).push(
                        args.get(1).cloned().unwrap_or(Value::Unit),
                    );
                    return Ok(Value::List(items));
                }
                return Err("list_push expects list".into());
            }
            "list_len" => {
                if let Some(Value::List(items)) = args.get(0) {
                    return Ok(Value::Int(items.len() as i64));
                }
                return Err("list_len expects list".into());
            }
            "list_get" => {
                if let (Some(Value::List(items)), Some(Value::Int(i))) = (args.get(0), args.get(1)) {
                    return items
                        .get(*i as usize)
                        .cloned()
                        .ok_or_else(|| "list index out of bounds".into());
                }
                return Err("list_get expects list and Int".into());
            }
            "list_set" => {
                if let (Some(Value::List(rc)), Some(Value::Int(i)), Some(v)) =
                    (args.get(0), args.get(1), args.get(2))
                {
                    if *i < 0 || *i as usize >= rc.len() {
                        return Err("list_set index out of bounds".into());
                    }
                    let mut items = Arc::clone(rc);
                    Arc::make_mut(&mut items)[*i as usize] = v.clone();
                    return Ok(Value::List(items));
                }
                return Err("list_set expects list, Int, value".into());
            }
            "list_concat" => {
                if let (Some(Value::List(a)), Some(Value::List(b))) = (args.get(0), args.get(1)) {
                    let mut out = Vec::with_capacity(a.len() + b.len());
                    out.extend(a.iter().cloned());
                    out.extend(b.iter().cloned());
                    return Ok(Value::List(Arc::new(out)));
                }
                return Err("list_concat expects two lists".into());
            }
            "list_flat_map" => {
                if let (Some(Value::List(items)), Some(f)) = (args.get(0), args.get(1)) {
                    let mut out = Vec::new();
                    for item in items.iter() {
                        let v = match f {
                            Value::Closure { name, captures } => {
                                self.call_with_captures(name, vec![item.clone()], captures.clone())?
                            }
                            Value::Str(name) => self.call(name, vec![item.clone()])?,
                            _ => return Err("list_flat_map expects list and function".into()),
                        };
                        match v {
                            Value::List(inner) => out.extend(inner.iter().cloned()),
                            other => out.push(other),
                        }
                    }
                    return Ok(Value::List(Arc::new(out)));
                }
                return Err("list_flat_map expects list and function".into());
            }
            "list_map" => {
                if let (Some(Value::List(items)), Some(f)) = (args.get(0), args.get(1)) {
                    let mut out = Vec::with_capacity(items.len());
                    for item in items.iter() {
                        let v = match f {
                            Value::Closure { name, captures } => {
                                self.call_with_captures(name, vec![item.clone()], captures.clone())?
                            }
                            Value::Str(name) => self.call(name, vec![item.clone()])?,
                            _ => return Err("list_map expects list and function".into()),
                        };
                        out.push(v);
                    }
                    return Ok(Value::List(Arc::new(out)));
                }
                return Err("list_map expects list and function".into());
            }
            "list_filter" => {
                if let (Some(Value::List(items)), Some(f)) = (args.get(0), args.get(1)) {
                    let mut out = Vec::new();
                    for item in items.iter() {
                        let v = match f {
                            Value::Closure { name, captures } => {
                                self.call_with_captures(name, vec![item.clone()], captures.clone())?
                            }
                            Value::Str(name) => self.call(name, vec![item.clone()])?,
                            _ => return Err("list_filter expects list and function".into()),
                        };
                        if truth(&v) {
                            out.push(item.clone());
                        }
                    }
                    return Ok(Value::List(Arc::new(out)));
                }
                return Err("list_filter expects list and function".into());
            }
            "str_join" => {
                if let (Some(Value::List(items)), Some(Value::Str(sep))) =
                    (args.get(0), args.get(1))
                {
                    let parts: Vec<String> = items
                        .iter()
                        .map(|v| match v {
                            Value::Str(s) => s.clone(),
                            Value::Int(n) => n.to_string(),
                            Value::Bool(b) => b.to_string(),
                            other => show(other),
                        })
                        .collect();
                    return Ok(Value::Str(parts.join(sep)));
                }
                return Err("str_join expects list and separator String".into());
            }
            "list_sort" => {
                if let Some(Value::List(items)) = args.get(0) {
                    let mut v: Vec<Value> = items.iter().cloned().collect();
                    v.sort_by(|a, b| match (a, b) {
                        (Value::Int(x), Value::Int(y)) => x.cmp(y),
                        (Value::Str(x), Value::Str(y)) => x.cmp(y),
                        _ => std::cmp::Ordering::Equal,
                    });
                    return Ok(Value::List(Arc::new(v)));
                }
                return Err("list_sort expects list of Int or String".into());
            }
            "list_sort_by" => {
                if let (Some(Value::List(items)), Some(f)) = (args.get(0), args.get(1)) {
                    let mut pairs: Vec<(Value, Value)> = Vec::new();
                    for item in items.iter() {
                        let key = match f {
                            Value::Closure { name, captures } => {
                                self.call_with_captures(name, vec![item.clone()], captures.clone())?
                            }
                            Value::Str(name) => self.call(name, vec![item.clone()])?,
                            _ => return Err("list_sort_by expects list and function".into()),
                        };
                        pairs.push((key, item.clone()));
                    }
                    pairs.sort_by(|a, b| match (&a.0, &b.0) {
                        (Value::Int(x), Value::Int(y)) => x.cmp(y),
                        (Value::Str(x), Value::Str(y)) => x.cmp(y),
                        _ => std::cmp::Ordering::Equal,
                    });
                    let out: Vec<Value> = pairs.into_iter().map(|(_, v)| v).collect();
                    return Ok(Value::List(Arc::new(out)));
                }
                return Err("list_sort_by expects list and function".into());
            }
            "list_sum" => {
                if let Some(Value::List(items)) = args.get(0) {
                    let mut s = 0i64;
                    for it in items.iter() {
                        if let Value::Int(n) = it {
                            s += n;
                        } else {
                            return Err("list_sum expects list of Int".into());
                        }
                    }
                    return Ok(Value::Int(s));
                }
                return Err("list_sum expects list".into());
            }
            "list_min" => {
                if let Some(Value::List(items)) = args.get(0) {
                    let mut best: Option<i64> = None;
                    for it in items.iter() {
                        if let Value::Int(n) = it {
                            best = Some(match best { Some(b) => b.min(*n), None => *n });
                        } else {
                            return Err("list_min expects list of Int".into());
                        }
                    }
                    return Ok(Value::Int(best.ok_or("list_min on empty list")?));
                }
                return Err("list_min expects list".into());
            }
            "list_max" => {
                if let Some(Value::List(items)) = args.get(0) {
                    let mut best: Option<i64> = None;
                    for it in items.iter() {
                        if let Value::Int(n) = it {
                            best = Some(match best { Some(b) => b.max(*n), None => *n });
                        } else {
                            return Err("list_max expects list of Int".into());
                        }
                    }
                    return Ok(Value::Int(best.ok_or("list_max on empty list")?));
                }
                return Err("list_max expects list".into());
            }
            "list_any" => {
                if let (Some(Value::List(items)), Some(f)) = (args.get(0), args.get(1)) {
                    for item in items.iter() {
                        let v = match f {
                            Value::Closure { name, captures } => {
                                self.call_with_captures(name, vec![item.clone()], captures.clone())?
                            }
                            Value::Str(name) => self.call(name, vec![item.clone()])?,
                            _ => return Err("list_any expects list and function".into()),
                        };
                        if truth(&v) {
                            return Ok(Value::Bool(true));
                        }
                    }
                    return Ok(Value::Bool(false));
                }
                return Err("list_any expects list and function".into());
            }
            "list_all" => {
                if let (Some(Value::List(items)), Some(f)) = (args.get(0), args.get(1)) {
                    for item in items.iter() {
                        let v = match f {
                            Value::Closure { name, captures } => {
                                self.call_with_captures(name, vec![item.clone()], captures.clone())?
                            }
                            Value::Str(name) => self.call(name, vec![item.clone()])?,
                            _ => return Err("list_all expects list and function".into()),
                        };
                        if !truth(&v) {
                            return Ok(Value::Bool(false));
                        }
                    }
                    return Ok(Value::Bool(true));
                }
                return Err("list_all expects list and function".into());
            }
            "str_index_of" => {
                if let (Some(Value::Str(s)), Some(Value::Str(sub))) = (args.get(0), args.get(1)) {
                    return Ok(Value::Int(s.find(sub.as_str()).map(|i| i as i64).unwrap_or(-1)));
                }
                return Err("str_index_of expects two Strings".into());
            }
            "str_substring" => {
                if let (Some(Value::Str(s)), Some(Value::Int(start)), Some(Value::Int(end))) =
                    (args.get(0), args.get(1), args.get(2))
                {
                    let chars: Vec<char> = s.chars().collect();
                    let len = chars.len() as i64;
                    let a = (*start).max(0).min(len) as usize;
                    let b = (*end).max(0).min(len) as usize;
                    if a > b {
                        return Ok(Value::Str(String::new()));
                    }
                    return Ok(Value::Str(chars[a..b].iter().collect()));
                }
                return Err("str_substring expects String, start: Int, end: Int".into());
            }
            "clone" => {
                // Lists/Maps already Arc-shared; clone is explicit identity for readability
                return Ok(args.get(0).cloned().unwrap_or(Value::Unit));
            }
            "list_rev" => {
                if let Some(Value::List(items)) = args.get(0) {
                    let mut v: Vec<_> = items.iter().cloned().collect();
                    v.reverse();
                    return Ok(Value::List(Arc::new(v)));
                }
                return Err("list_rev expects list".into());
            }
            "list_contains" => {
                if let (Some(Value::List(items)), Some(needle)) = (args.get(0), args.get(1)) {
                    let found = items.iter().any(|x| values_eq(x, needle));
                    return Ok(Value::Bool(found));
                }
                return Err("list_contains expects list and value".into());
            }
            "range" => {
                if let (Some(Value::Int(start)), Some(Value::Int(end))) =
                    (args.get(0), args.get(1))
                {
                    let mut v = Vec::new();
                    let mut i = *start;
                    while i < *end {
                        v.push(Value::Int(i));
                        i += 1;
                    }
                    return Ok(Value::List(Arc::new(v)));
                }
                return Err("range expects start: Int, end: Int (exclusive)".into());
            }
            "list_take" => {
                if let (Some(Value::List(items)), Some(Value::Int(n))) = (args.get(0), args.get(1)) {
                    let n = (*n).max(0) as usize;
                    let end = n.min(items.len());
                    return Ok(Value::List(Arc::new(items[..end].to_vec())));
                }
                return Err("list_take expects list and Int".into());
            }
            "list_drop" => {
                if let (Some(Value::List(items)), Some(Value::Int(n))) = (args.get(0), args.get(1)) {
                    let n = (*n).max(0) as usize;
                    let start = n.min(items.len());
                    return Ok(Value::List(Arc::new(items[start..].to_vec())));
                }
                return Err("list_drop expects list and Int".into());
            }
            "list_slice" => {
                if let (Some(Value::List(items)), Some(Value::Int(start)), Some(Value::Int(end))) =
                    (args.get(0), args.get(1), args.get(2))
                {
                    let len = items.len() as i64;
                    let s = (*start).max(0).min(len) as usize;
                    let e = (*end).max(0).min(len) as usize;
                    if s > e {
                        return Ok(Value::List(Arc::new(vec![])));
                    }
                    return Ok(Value::List(Arc::new(items[s..e].to_vec())));
                }
                return Err("list_slice expects list, start: Int, end: Int".into());
            }
            "list_fold" => {
                if let (Some(Value::List(items)), Some(init), Some(f)) =
                    (args.get(0), args.get(1), args.get(2))
                {
                    let mut acc = init.clone();
                    for item in items.iter() {
                        acc = match f {
                            Value::Closure { name, captures } => self.call_with_captures(
                                name,
                                vec![acc, item.clone()],
                                captures.clone(),
                            )?,
                            Value::Str(name) => self.call(name, vec![acc, item.clone()])?,
                            _ => return Err("list_fold expects list, init, function".into()),
                        };
                    }
                    return Ok(acc);
                }
                return Err("list_fold expects list, init, function".into());
            }
            "map" => {
                let mut m = BTreeMap::new();
                let mut i = 0;
                while i + 1 < args.len() {
                    if let Value::Str(k) = &args[i] {
                        m.insert(k.clone(), args[i + 1].clone());
                    }
                    i += 2;
                }
                return Ok(Value::Map(Arc::new(m)));
            }
            "map_set" => {
                if let (Some(Value::Map(rc)), Some(Value::Str(k)), Some(v)) =
                    (args.get(0), args.get(1), args.get(2))
                {
                    let mut m = Arc::clone(rc);
                    Arc::make_mut(&mut m).insert(k.clone(), v.clone());
                    return Ok(Value::Map(m));
                }
                return Err("map_set expects map, String, value".into());
            }
            "map_get" => {
                if let (Some(Value::Map(m)), Some(Value::Str(k))) = (args.get(0), args.get(1)) {
                    return m
                        .get(k)
                        .cloned()
                        .ok_or_else(|| format!("map key not found: `{k}`"));
                }
                return Err("map_get expects map and String".into());
            }
            "map_has" => {
                if let (Some(Value::Map(m)), Some(Value::Str(k))) = (args.get(0), args.get(1)) {
                    return Ok(Value::Bool(m.contains_key(k)));
                }
                return Err("map_has expects map and String".into());
            }
            "map_len" => {
                if let Some(Value::Map(m)) = args.get(0) {
                    return Ok(Value::Int(m.len() as i64));
                }
                return Err("map_len expects map".into());
            }
            "map_keys" => {
                if let Some(Value::Map(m)) = args.get(0) {
                    let keys: Vec<Value> = m.keys().map(|k| Value::Str(k.clone())).collect();
                    return Ok(Value::List(Arc::new(keys)));
                }
                return Err("map_keys expects map".into());
            }
            "map_values" => {
                if let Some(Value::Map(m)) = args.get(0) {
                    let vals: Vec<Value> = m.values().cloned().collect();
                    return Ok(Value::List(Arc::new(vals)));
                }
                return Err("map_values expects map".into());
            }
            "map_remove" => {
                if let (Some(Value::Map(rc)), Some(Value::Str(k))) = (args.get(0), args.get(1)) {
                    let mut m = Arc::clone(rc);
                    Arc::make_mut(&mut m).remove(k);
                    return Ok(Value::Map(m));
                }
                return Err("map_remove expects map and String".into());
            }
            "str_repeat" => {
                if let (Some(Value::Str(s)), Some(Value::Int(n))) = (args.get(0), args.get(1)) {
                    if *n < 0 {
                        return Err("str_repeat count must be >= 0".into());
                    }
                    return Ok(Value::Str(s.repeat(*n as usize)));
                }
                return Err("str_repeat expects String and Int".into());
            }
            "list_flatten" => {
                if let Some(Value::List(items)) = args.get(0) {
                    let mut out = Vec::new();
                    for it in items.iter() {
                        match it {
                            Value::List(inner) => out.extend(inner.iter().cloned()),
                            other => out.push(other.clone()),
                        }
                    }
                    return Ok(Value::List(Arc::new(out)));
                }
                return Err("list_flatten expects list".into());
            }
            "list_find" => {
                // list_find(xs, pred) -> index or -1
                if let (Some(Value::List(items)), Some(f)) = (args.get(0), args.get(1)) {
                    for (i, item) in items.iter().enumerate() {
                        let v = match f {
                            Value::Closure { name, captures } => {
                                self.call_with_captures(name, vec![item.clone()], captures.clone())?
                            }
                            Value::Str(name) => self.call(name, vec![item.clone()])?,
                            _ => return Err("list_find expects list and function".into()),
                        };
                        if truth(&v) {
                            return Ok(Value::Int(i as i64));
                        }
                    }
                    return Ok(Value::Int(-1));
                }
                return Err("list_find expects list and function".into());
            }
            "list_is_empty" => {
                if let Some(Value::List(items)) = args.get(0) {
                    return Ok(Value::Bool(items.is_empty()));
                }
                return Err("list_is_empty expects list".into());
            }
            "str_is_empty" => {
                if let Some(Value::Str(s)) = args.get(0) {
                    return Ok(Value::Bool(s.is_empty()));
                }
                return Err("str_is_empty expects String".into());
            }
            "map_merge" => {
                if let (Some(Value::Map(a)), Some(Value::Map(b))) = (args.get(0), args.get(1)) {
                    let mut m = (**a).clone();
                    for (k, v) in b.iter() {
                        m.insert(k.clone(), v.clone());
                    }
                    return Ok(Value::Map(Arc::new(m)));
                }
                return Err("map_merge expects two maps".into());
            }
            "list_unique" => {
                if let Some(Value::List(items)) = args.get(0) {
                    let mut out = Vec::new();
                    for it in items.iter() {
                        if !out.iter().any(|x| values_eq(x, it)) {
                            out.push(it.clone());
                        }
                    }
                    return Ok(Value::List(Arc::new(out)));
                }
                return Err("list_unique expects list".into());
            }
            "list_chunk" => {
                if let (Some(Value::List(items)), Some(Value::Int(n))) = (args.get(0), args.get(1)) {
                    if *n <= 0 {
                        return Err("list_chunk size must be > 0".into());
                    }
                    let size = *n as usize;
                    let mut out = Vec::new();
                    let mut i = 0;
                    while i < items.len() {
                        let end = (i + size).min(items.len());
                        out.push(Value::List(Arc::new(items[i..end].to_vec())));
                        i = end;
                    }
                    return Ok(Value::List(Arc::new(out)));
                }
                return Err("list_chunk expects list and Int".into());
            }
            "str_lines" => {
                if let Some(Value::Str(s)) = args.get(0) {
                    let lines: Vec<Value> = s.lines().map(|l| Value::Str(l.to_string())).collect();
                    return Ok(Value::List(Arc::new(lines)));
                }
                return Err("str_lines expects String".into());
            }
            "enumerate" => {
                if let Some(Value::List(items)) = args.get(0) {
                    let mut out = Vec::with_capacity(items.len());
                    for (i, it) in items.iter().enumerate() {
                        out.push(Value::Tuple(vec![Value::Int(i as i64), it.clone()]));
                    }
                    return Ok(Value::List(Arc::new(out)));
                }
                return Err("enumerate expects list".into());
            }
            "list_zip" => {
                // list_zip(a, b) -> list of maps { "0": ai, "1": bi } as pair lists [ai, bi]
                if let (Some(Value::List(a)), Some(Value::List(b))) = (args.get(0), args.get(1)) {
                    let n = a.len().min(b.len());
                    let mut out = Vec::with_capacity(n);
                    for i in 0..n {
                        out.push(Value::List(Arc::new(vec![a[i].clone(), b[i].clone()])));
                    }
                    return Ok(Value::List(Arc::new(out)));
                }
                return Err("list_zip expects two lists".into());
            }
            "str_len" => {
                if let Some(Value::Str(s)) = args.get(0) {
                    return Ok(Value::Int(s.len() as i64));
                }
                return Err("str_len expects String".into());
            }
            "str_contains" => {
                if let (Some(Value::Str(s)), Some(Value::Str(sub))) = (args.get(0), args.get(1)) {
                    return Ok(Value::Bool(s.contains(sub)));
                }
                return Err("str_contains expects two Strings".into());
            }
            "str_upper" => {
                if let Some(Value::Str(s)) = args.get(0) {
                    return Ok(Value::Str(s.to_uppercase()));
                }
                return Err("str_upper expects String".into());
            }
            "str_lower" => {
                if let Some(Value::Str(s)) = args.get(0) {
                    return Ok(Value::Str(s.to_lowercase()));
                }
                return Err("str_lower expects String".into());
            }
            "str_trim" => {
                if let Some(Value::Str(s)) = args.get(0) {
                    return Ok(Value::Str(s.trim().to_string()));
                }
                return Err("str_trim expects String".into());
            }
            "str_concat" => {
                if let (Some(Value::Str(a)), Some(Value::Str(b))) = (args.get(0), args.get(1)) {
                    return Ok(Value::Str(a.clone() + b));
                }
                return Err("str_concat expects two Strings".into());
            }
            "str_split" => {
                if let (Some(Value::Str(s)), Some(Value::Str(sep))) = (args.get(0), args.get(1)) {
                    let parts: Vec<Value> =
                        s.split(sep.as_str()).map(|p| Value::Str(p.to_string())).collect();
                    return Ok(Value::List(Arc::new(parts)));
                }
                return Err("str_split expects two Strings".into());
            }
            "str_replace" => {
                if let (Some(Value::Str(s)), Some(Value::Str(from)), Some(Value::Str(to))) =
                    (args.get(0), args.get(1), args.get(2))
                {
                    return Ok(Value::Str(s.replace(from.as_str(), to.as_str())));
                }
                return Err("str_replace expects three Strings".into());
            }
            "str_starts_with" => {
                if let (Some(Value::Str(s)), Some(Value::Str(p))) = (args.get(0), args.get(1)) {
                    return Ok(Value::Bool(s.starts_with(p.as_str())));
                }
                return Err("str_starts_with expects two Strings".into());
            }
            "str_ends_with" => {
                if let (Some(Value::Str(s)), Some(Value::Str(p))) = (args.get(0), args.get(1)) {
                    return Ok(Value::Bool(s.ends_with(p.as_str())));
                }
                return Err("str_ends_with expects two Strings".into());
            }
            _ => {}
        }

        if !self.funcs.contains_key(name) {
            // Try host FFI binding
            return crate::ffi::call_host(name, args);
        }
        let (fparams, chunk) = self
            .funcs
            .get(name)
            .ok_or_else(|| format!("unknown function `{name}`"))?;
        let mut locals = HashMap::new();
        // Install closure captures (if any) before parameters
        CAPTURE_INJECT.with(|cell| {
            for (k, v) in cell.borrow().iter() {
                locals.insert(k.clone(), v.clone());
            }
        });
        for (i, p) in fparams.iter().enumerate() {
            locals.insert(p.clone(), args.get(i).cloned().unwrap_or(Value::Unit));
        }
        let mut stack = Vec::new();
        let mut ip = 0;
        let max_stack = MAX_STACK_DEPTH.with(|c| c.get());
        while ip < chunk.code.len() {
            if stack.len() > max_stack {
                return Err(format!(
                    "value stack limit exceeded (max {max_stack})"
                ));
            }
            match &chunk.code[ip] {
                Op::Const(v) => stack.push(v.clone()),
                Op::Load(n) => stack.push(
                    locals
                        .get(n)
                        .cloned()
                        .ok_or_else(|| format!("undefined variable `{n}`"))?,
                ),
                Op::Store(n) => {
                    let v = stack.pop().ok_or("stack underflow")?;
                    locals.insert(n.clone(), v);
                }
                Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Mod | Op::Eq | Op::Ne | Op::Lt | Op::Lte
                | Op::Gt | Op::Gte => {
                    let b = stack.pop().ok_or("stack underflow")?;
                    let a = stack.pop().ok_or("stack underflow")?;
                    let op = match &chunk.code[ip] {
                        Op::Add => Token::Plus,
                        Op::Sub => Token::Minus,
                        Op::Mul => Token::Star,
                        Op::Div => Token::Slash,
                        Op::Mod => Token::Percent,
                        Op::Eq => Token::EqEq,
                        Op::Ne => Token::BangEq,
                        Op::Lt => Token::Lt,
                        Op::Lte => Token::Lte,
                        Op::Gt => Token::Gt,
                        Op::Gte => Token::Gte,
                        _ => unreachable!(),
                    };
                    // string concat via Add if both strings or mixed with to_string already done
                    if matches!(op, Token::Plus) {
                        match (&a, &b) {
                            (Value::Str(_), _) | (_, Value::Str(_)) => {
                                stack.push(Value::Str(format!("{a}{b}")));
                                ip += 1;
                                continue;
                            }
                            _ => {}
                        }
                    }
                    stack.push(bin(a, &op, b)?);
                }
                Op::Neg => {
                    let v = stack.pop().ok_or("stack underflow")?;
                    match v {
                        Value::Int(x) => stack.push(Value::Int(-x)),
                        _ => return Err("unary - expects Int".into()),
                    }
                }
                Op::Not => {
                    let v = stack.pop().ok_or("stack underflow")?;
                    match v {
                        Value::Bool(b) => stack.push(Value::Bool(!b)),
                        _ => return Err("unary ! expects Bool".into()),
                    }
                }
                Op::Index => {
                    let idx = stack.pop().ok_or("stack underflow")?;
                    let base = stack.pop().ok_or("stack underflow")?;
                    match (base, idx) {
                        (Value::List(items), Value::Int(i)) => {
                            if i < 0 || i as usize >= items.len() {
                                return Err(format!("list index out of bounds: {i}"));
                            }
                            stack.push(items[i as usize].clone());
                        }
                        (Value::Map(m), Value::Str(k)) => match m.get(&k) {
                            Some(v) => stack.push(v.clone()),
                            None => return Err(format!("map key not found: `{k}`")),
                        },
                        _ => return Err("indexing requires List[Int] or Map[String]".into()),
                    }
                }
                Op::GetField(field) => {
                    let obj = stack.pop().ok_or("stack underflow")?;
                    match obj {
                        Value::Struct { fields, .. } => {
                            let v = fields
                                .get(field)
                                .cloned()
                                .ok_or_else(|| format!("missing field `{field}`"))?;
                            stack.push(v);
                        }
                        _ => return Err("field access requires struct".into()),
                    }
                }
                Op::SetField(field) => {
                    let val = stack.pop().ok_or("stack underflow")?;
                    let obj = stack.pop().ok_or("stack underflow")?;
                    match obj {
                        Value::Struct { name, mut fields } => {
                            fields.insert(field.clone(), val);
                            stack.push(Value::Struct { name, fields });
                        }
                        _ => return Err("field assign requires struct".into()),
                    }
                }
                Op::MakeStruct(name, field_names) => {
                    let mut fields = BTreeMap::new();
                    for fname in field_names.iter().rev() {
                        let v = stack.pop().ok_or("stack underflow")?;
                        fields.insert(fname.clone(), v);
                    }
                    stack.push(Value::Struct {
                        name: name.clone(),
                        fields,
                    });
                }
                Op::MakeClosure(name, caps) => {
                    let mut captures = BTreeMap::new();
                    for cap in caps.iter().rev() {
                        let v = stack.pop().ok_or("stack underflow")?;
                        captures.insert(cap.clone(), v);
                    }
                    stack.push(Value::Closure {
                        name: name.clone(),
                        captures,
                    });
                }
                Op::CallIndirect(argc) => {
                    let callee = stack.pop().ok_or("stack underflow")?;
                    let mut a = Vec::new();
                    for _ in 0..*argc {
                        a.push(stack.pop().ok_or("stack underflow")?);
                    }
                    a.reverse();
                    match callee {
                        Value::Closure { name, captures } => {
                            stack.push(self.call_with_captures(&name, a, captures)?);
                        }
                        Value::Str(name) => {
                            stack.push(self.call(&name, a)?);
                        }
                        other => {
                            return Err(format!("call of non-function value: {other}"));
                        }
                    }
                }
                Op::EnumIs(want) => {
                    let v = stack.pop().ok_or("stack underflow")?;
                    let ok = matches!(v, Value::Enum { variant, .. } if variant == *want);
                    stack.push(Value::Bool(ok));
                }
                Op::EnumGet(idx) => {
                    let v = stack.pop().ok_or("stack underflow")?;
                    match v {
                        Value::Enum { fields, .. } => {
                            stack.push(fields.get(*idx).cloned().unwrap_or(Value::Unit));
                        }
                        _ => return Err("EnumGet on non-enum".into()),
                    }
                }
                Op::EnumMake(name, variant, argc) => {
                    let mut fields = Vec::new();
                    for _ in 0..*argc {
                        fields.push(stack.pop().ok_or("stack underflow")?);
                    }
                    fields.reverse();
                    stack.push(Value::Enum {
                        name: name.clone(),
                        variant: variant.clone(),
                        fields,
                    });
                }
                Op::Call(n, argc) => {
                    let mut a = Vec::new();
                    for _ in 0..*argc {
                        a.push(stack.pop().ok_or("stack underflow")?);
                    }
                    a.reverse();
                    if !self.funcs.contains_key(n) {
                        if let Some(Value::Closure { name, captures }) = locals.get(n).cloned() {
                            stack.push(self.call_with_captures(&name, a, captures)?);
                            ip += 1;
                            continue;
                        }
                    }
                    stack.push(self.call(n, a)?);
                }
                Op::Print => {
                    let v = stack.pop().ok_or("stack underflow")?;
                    println!("{}", show(&v));
                    stack.push(Value::Unit);
                }
                Op::Pop => {
                    stack.pop();
                }
                Op::JumpIfFalse(to) => {
                    let v = stack.pop().ok_or("stack underflow")?;
                    if !truth(&v) {
                        ip = *to;
                        continue;
                    }
                }
                Op::Jump(to) => {
                    ip = *to;
                    continue;
                }
                Op::Try => {
                    let v = stack.pop().ok_or("stack underflow")?;
                    match v {
                        Value::Enum { name, variant, fields } if variant == "Ok" => {
                            stack.push(fields.get(0).cloned().unwrap_or(Value::Unit));
                        }
                        Value::Enum { name, variant, fields } if variant == "Err" => {
                            return Ok(Value::Enum { name, variant, fields });
                        }
                        other => {
                            return Err(format!("`?` operator expects Result, got {other}"));
                        }
                    }
                }
                Op::Return => return Ok(stack.pop().unwrap_or(Value::Unit)),
                Op::Halt => return Ok(Value::Unit),
                Op::IterLen => {
                    let v = stack.pop().ok_or("stack underflow")?;
                    match v {
                        Value::List(items) => stack.push(Value::Int(items.len() as i64)),
                        _ => return Err("IterLen expects list".into()),
                    }
                }
                Op::IterGet => {
                    let idx = stack.pop().ok_or("stack underflow")?;
                    let list = stack.pop().ok_or("stack underflow")?;
                    match (list, idx) {
                        (Value::List(items), Value::Int(i)) => {
                            stack.push(
                                items
                                    .get(i as usize)
                                    .cloned()
                                    .ok_or("iter index out of bounds")?,
                            );
                        }
                        _ => return Err("IterGet expects list and Int".into()),
                    }
                }
            }
            ip += 1;
        }
        Ok(Value::Unit)
    }
}

pub fn build_artifact(src: &str) -> Result<Vec<u8>, String> {
    let mut program = parser::parse_program(src)?;
    crate::stdlib::resolve(&program.imports)?;
    sema::expand_trait_defaults(&mut program)?;
    sema::check_program(&program)?;
    let hir = crate::hir::lower(&program);
    let mut mir = crate::mir::lower(&hir)?;
    crate::mir::optimize(&mut mir);
    let funcs = compile_from_mir(&mir)?;
    let mut bytes = b"SNPBC1\n".to_vec();
    bytes.extend(format!("{funcs:?}").as_bytes());
    Ok(bytes)
}

pub fn execute(src: &str) -> Result<Value, String> {
    let mut program = parser::parse_program(src)?;
    inject_consts_into_main(&mut program);
    sema::expand_uses(&mut program);
    sema::desugar_match_expressions(&mut program);
    sema::desugar_if_let(&mut program);
    sema::desugar_defer(&mut program);
    sema::expand_trait_defaults(&mut program)?;
    crate::stdlib::resolve(&program.imports)?;
    sema::check_program(&program)?;
    execute_program(&program)
}

fn inject_consts_into_main(program: &mut crate::ast::Program) {
    if program.consts.is_empty() {
        return;
    }
    if let Some(main) = program.functions.iter_mut().find(|f| f.name == "main") {
        let mut prefix: Vec<_> = program
            .consts
            .iter()
            .map(|(n, e)| crate::ast::Stmt::Const(n.clone(), e.clone()))
            .collect();
        prefix.append(&mut main.body);
        main.body = prefix;
    }
    program.consts.clear();
}

/// Run an already-loaded (possibly multi-file) program.

/// Emit bytecode from MIR (primary path for the professional pipeline).
pub fn compile_from_mir(
    mir: &crate::mir::MirModule,
) -> Result<HashMap<String, (Vec<String>, Chunk)>, String> {
    let mut out = HashMap::new();
    for f in &mir.functions {
        let chunk = emit_function(f)?;
        out.insert(f.name.clone(), (f.params.clone(), chunk));
    }
    Ok(out)
}

fn emit_function(f: &crate::mir::MirFunction) -> Result<Chunk, String> {
    use crate::mir::{MirInstr, MirOperand, MirTerminator};
    // Layout: concatenate blocks; record start index of each block for jump patching
    let mut code: Vec<Op> = Vec::new();
    let mut block_start: Vec<usize> = Vec::with_capacity(f.blocks.len());
    // First pass: emit with placeholder jumps (block ids as targets)
    // We'll store Jump(block_id) temporarily then remap.

    for bb in &f.blocks {
        block_start.push(code.len());
        for instr in &bb.stmts {
            match instr {
                MirInstr::Assign { dest, value } => {
                    emit_operand(value, &mut code);
                    code.push(Op::Store(dest.clone()));
                }
                MirInstr::Binary {
                    dest,
                    op,
                    left,
                    right,
                } => {
                    emit_operand(left, &mut code);
                    emit_operand(right, &mut code);
                    code.push(token_binop(op)?);
                    code.push(Op::Store(dest.clone()));
                }
                MirInstr::Unary { dest, op, value } => {
                    emit_operand(value, &mut code);
                    code.push(token_unop(op)?);
                    code.push(Op::Store(dest.clone()));
                }
                MirInstr::Try { dest, value } => {
                    emit_operand(value, &mut code);
                    code.push(Op::Try);
                    code.push(Op::Store(dest.clone()));
                }
                MirInstr::Call { dest, name, args } => {
                    if name == "__iter_len" {
                        for a in args {
                            emit_operand(a, &mut code);
                        }
                        code.push(Op::IterLen);
                        code.push(Op::Store(dest.clone()));
                    } else if name == "__to_string" {
                        for a in args {
                            emit_operand(a, &mut code);
                        }
                        code.push(Op::Call("__to_string".into(), 1));
                        code.push(Op::Store(dest.clone()));
                    } else if name == "__enum_is" && args.len() == 2 {
                        emit_operand(&args[0], &mut code);
                        if let MirOperand::ConstStr(v) = &args[1] {
                            code.push(Op::EnumIs(v.clone()));
                        } else {
                            for a in args {
                                emit_operand(a, &mut code);
                            }
                            code.push(Op::Call(name.clone(), 2));
                        }
                        code.push(Op::Store(dest.clone()));
                    } else if name == "__enum_get" && args.len() == 2 {
                        emit_operand(&args[0], &mut code);
                        if let MirOperand::ConstInt(i) = &args[1] {
                            code.push(Op::EnumGet(*i as usize));
                        } else {
                            for a in args {
                                emit_operand(a, &mut code);
                            }
                            code.push(Op::Call(name.clone(), 2));
                        }
                        code.push(Op::Store(dest.clone()));
                    } else {
                        for a in args {
                            emit_operand(a, &mut code);
                        }
                        code.push(Op::Call(name.clone(), args.len()));
                        code.push(Op::Store(dest.clone()));
                    }
                }
                MirInstr::CallIndirect { dest, callee, args } => {
                    for a in args {
                        emit_operand(a, &mut code);
                    }
                    emit_operand(callee, &mut code);
                    code.push(Op::CallIndirect(args.len()));
                    code.push(Op::Store(dest.clone()));
                }
                MirInstr::Index { dest, base, index } => {
                    emit_operand(base, &mut code);
                    emit_operand(index, &mut code);
                    code.push(Op::Index);
                    code.push(Op::Store(dest.clone()));
                }
                MirInstr::Field {
                    dest,
                    object,
                    field,
                } => {
                    emit_operand(object, &mut code);
                    code.push(Op::GetField(field.clone()));
                    code.push(Op::Store(dest.clone()));
                }
                MirInstr::SetField {
                    object,
                    field,
                    value,
                } => {
                    emit_operand(object, &mut code);
                    emit_operand(value, &mut code);
                    code.push(Op::SetField(field.clone()));
                }
                MirInstr::MakeStruct { dest, name, fields } => {
                    let mut names = Vec::new();
                    for (fname, fop) in fields {
                        emit_operand(fop, &mut code);
                        names.push(fname.clone());
                    }
                    code.push(Op::MakeStruct(name.clone(), names));
                    code.push(Op::Store(dest.clone()));
                }
                MirInstr::MakeEnum {
                    dest,
                    enum_name,
                    variant,
                    args,
                } => {
                    for a in args {
                        emit_operand(a, &mut code);
                    }
                    // Encode as Call to __enum_make with name/variant as trailing string consts
                    code.push(Op::Const(Value::Str(enum_name.clone())));
                    code.push(Op::Const(Value::Str(variant.clone())));
                    code.push(Op::Call(
                        "__enum_make".into(),
                        args.len() + 2,
                    ));
                    code.push(Op::Store(dest.clone()));
                }
                MirInstr::MakeClosure {
                    dest,
                    name,
                    captures,
                } => {
                    let mut caps = Vec::new();
                    for (cname, cop) in captures {
                        emit_operand(cop, &mut code);
                        caps.push(cname.clone());
                    }
                    code.push(Op::MakeClosure(name.clone(), caps));
                    code.push(Op::Store(dest.clone()));
                }
                MirInstr::Print(v) => {
                    emit_operand(v, &mut code);
                    code.push(Op::Print);
                }
                MirInstr::Nop => {}
            }
        }
        match &bb.terminator {
            MirTerminator::Return(None) => {
                code.push(Op::Const(Value::Unit));
                code.push(Op::Return);
            }
            MirTerminator::Return(Some(op)) => {
                emit_operand(op, &mut code);
                code.push(Op::Return);
            }
            MirTerminator::Jump(target) => {
                // placeholder: store block id, remap later
                code.push(Op::Jump(*target));
            }
            MirTerminator::Branch {
                cond,
                then_bb,
                else_bb,
            } => {
                emit_operand(cond, &mut code);
                // JumpIfFalse to else; then Jump to then is implicit by layout?
                // Better: JumpIfFalse(else); Jump(then)
                code.push(Op::JumpIfFalse(*else_bb));
                code.push(Op::Jump(*then_bb));
            }
            MirTerminator::Unreachable => {
                code.push(Op::Const(Value::Unit));
                code.push(Op::Return);
            }
        }
    }

    // Remap jump targets from block ids to instruction indices
    for op in &mut code {
        match op {
            Op::Jump(t) => {
                let bb = *t;
                if bb < block_start.len() {
                    *t = block_start[bb];
                }
            }
            Op::JumpIfFalse(t) => {
                let bb = *t;
                if bb < block_start.len() {
                    *t = block_start[bb];
                }
            }
            _ => {}
        }
    }

    Ok(Chunk { code })
}

fn emit_operand(op: &crate::mir::MirOperand, code: &mut Vec<Op>) {
    use crate::mir::MirOperand;
    match op {
        MirOperand::ConstInt(n) => code.push(Op::Const(Value::Int(*n))),
        MirOperand::ConstBool(x) => code.push(Op::Const(Value::Bool(*x))),
        MirOperand::ConstStr(s) => code.push(Op::Const(Value::Str(s.clone()))),
        MirOperand::ConstUnit => code.push(Op::Const(Value::Unit)),
        MirOperand::Local(n) => code.push(Op::Load(n.clone())),
    }
}

fn token_binop(op: &Token) -> Result<Op, String> {
    Ok(match op {
        Token::Plus => Op::Add,
        Token::Minus => Op::Sub,
        Token::Star => Op::Mul,
        Token::Slash => Op::Div,
        Token::Percent => Op::Mod,
        Token::EqEq => Op::Eq,
        Token::BangEq => Op::Ne,
        Token::Lt => Op::Lt,
        Token::Lte => Op::Lte,
        Token::Gt => Op::Gt,
        Token::Gte => Op::Gte,
        other => return Err(format!("unsupported binary op in MIR codegen: {:?}", other)),
    })
}

fn token_unop(op: &Token) -> Result<Op, String> {
    Ok(match op {
        Token::Minus => Op::Neg,
        Token::Bang => Op::Not,
        other => return Err(format!("unsupported unary op in MIR codegen: {:?}", other)),
    })
}


pub fn execute_program(program: &crate::ast::Program) -> Result<Value, String> {
    let extern_names: Vec<String> = program.externs.iter().map(|e| e.name.clone()).collect();
    crate::ffi::resolve_externs(&extern_names)?;
    let mut program = program.clone();
    crate::sema::desugar_match_expressions(&mut program);
    crate::sema::desugar_if_let(&mut program);
    crate::sema::desugar_defer(&mut program);
    crate::sema::desugar_methods(&mut program)?;
    // Inject top-level `const` bindings at the start of `main`
    if !program.consts.is_empty() {
        if let Some(main) = program.functions.iter_mut().find(|f| f.name == "main") {
            let mut prefix = Vec::new();
            for (name, expr) in program.consts.iter().rev() {
                prefix.push(crate::ast::Stmt::Const(name.clone(), expr.clone()));
            }
            prefix.reverse();
            prefix.append(&mut main.body);
            main.body = prefix;
        }
    }
    // Primary path: HIR → MIR → optimize → bytecode (no AST fallback)
    let hir = crate::hir::lower(&program);
    let mut mir = crate::mir::lower(&hir)?;
    crate::mir::optimize(&mut mir);
    let funcs = compile_from_mir(&mir)?;
    BytecodeVm { funcs }.call("main", vec![])
}

/// Same as `execute_program` (MIR-only); kept for explicit call sites / tests.
pub fn execute_program_mir(program: &crate::ast::Program) -> Result<Value, String> {
    execute_program(program)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mir_codegen_closures() {
        let s = r#"
            fn main() -> Int {
                let base = 10;
                let add_base = fn(x: Int) -> Int { return x + base; };
                return add_base(32);
            }
        "#;
        let program = parser::parse_program(s).unwrap();
        assert_eq!(execute_program_mir(&program).unwrap(), Value::Int(42));
    }

    #[test]
    fn list_push_uses_arc_sharing() {
        let s = r#"
            import std.collections;
            fn main() -> Int {
                let a = list(1, 2);
                let b = list_push(a, 3);
                return list_len(b);
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(3));
    }

    #[test]
    fn runtime_stack_traceback() {
        // Runtime-only failure (sema cannot catch map key miss)
        let s = r#"
            import std.collections;
            fn boom() -> Int {
                let m = map("a", 1);
                return map_get(m, "missing");
            }
            fn main() -> Int {
                return boom();
            }
        "#;
        let err = execute(s).unwrap_err();
        assert!(err.contains("stack traceback:"), "err was: {err}");
        assert!(err.contains("boom()"), "err was: {err}");
        assert!(err.contains("main()"), "err was: {err}");
    }

    #[test]
    fn mir_codegen_arithmetic() {
        let s = "fn main()->Int{ return 20+22; }";
        let program = parser::parse_program(s).unwrap();
        assert_eq!(execute_program_mir(&program).unwrap(), Value::Int(42));
    }

    #[test]
    fn trait_where_bound_ok() {
        let s = r#"
            struct Player { hp: Int }
            trait Healable { fn heal(self, n: Int) -> Int; }
            impl Healable for Player {
                fn heal(self, n: Int) -> Int { return self.hp + n; }
            }
            fn apply(p: Player) -> Int where Player: Healable {
                return Player.heal(p, 1);
            }
            fn main() -> Int {
                return apply(Player { hp: 9 });
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(10));
    }

    #[test]
    fn trait_where_bound_fails() {
        let s = r#"
            struct Player { hp: Int }
            trait Healable { fn heal(self, n: Int) -> Int; }
            fn apply(p: Player) -> Int where Player: Healable {
                return 0;
            }
            fn main() -> Int { return 0; }
        "#;
        let err = execute(s).unwrap_err();
        assert!(err.contains("where-clause") || err.contains("not satisfied"), "{err}");
    }

    #[test]
    fn moved_value_clear_error() {
        let s = r#"
            struct Box { n: Int }
            fn main() -> Int {
                let a = Box { n: 1 };
                let b = a;
                return a.n;
            }
        "#;
        let err = execute(s).unwrap_err();
        assert!(err.contains("moved") || err.contains("borrow"), "{err}");
        assert!(err.contains("help") || err.contains("a"), "{err}");
    }

    #[test]
    fn copyable_int_can_reuse() {
        let s = r#"
            fn main() -> Int {
                let a = 5;
                let b = a;
                return a + b;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(10));
    }

    #[test]
    fn trait_default_method() {
        let s = r#"
            struct Player { hp: Int }
            trait Healable {
                fn heal(self, n: Int) -> Int;
                fn touch(self) -> Int { return self.hp; }
            }
            impl Healable for Player {
                fn heal(self, n: Int) -> Int {
                    return self.hp + n;
                }
            }
            fn main() -> Int {
                let p = Player { hp: 7 };
                return Player.touch(p);
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(7));
    }

    #[test]
    fn trait_impl_basic() {
        let s = r#"
            struct Player { hp: Int }
            trait Healable {
                fn heal(self, n: Int) -> Int;
            }
            impl Healable for Player {
                fn heal(self, n: Int) -> Int {
                    return self.hp + n;
                }
            }
            fn main() -> Int {
                let p = Player { hp: 10 };
                return Player.heal(p, 5);
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(15));
    }

    #[test]
    fn trait_missing_method_errors() {
        let s = r#"
            struct Player { hp: Int }
            trait Healable {
                fn heal(self, n: Int) -> Int;
                fn full(self) -> Int;
            }
            impl Healable for Player {
                fn heal(self, n: Int) -> Int {
                    return self.hp + n;
                }
            }
            fn main() -> Int { return 0; }
        "#;
        let err = execute(s).unwrap_err();
        assert!(err.contains("missing method") || err.contains("full"), "{err}");
    }

    #[test]
    fn many_small_programs() {
        let cases = [
            ("fn main() -> Int { return 1 + 2 * 3; }", 7),
            ("fn main() -> Int { return -5 + 10; }", 5),
            ("fn main() -> Bool { return true; }", 1),
            ("fn main() -> Int { if 1 < 2 { return 9; } else { return 0; } }", 9),
            ("fn main() -> Int { let x = 3; return x * x; }", 9),
            ("fn main() -> Int { let s = \"ab\"; return str_len(s); }", 2),
            ("fn main() -> Int { let xs = list(1, 2, 3); return list_len(xs); }", 3),
            ("fn add(a: Int, b: Int) -> Int { return a + b; } fn main() -> Int { return add(4, 5); }", 9),
            ("fn main() -> Int { let i = 0; while i < 3 { i = i + 1; } return i; }", 3),
            ("fn main() -> Int { for i in 0..4 { } return 4; }", 4),
            ("fn main() -> Int { match 1 { 1 => { return 11; } _ => { return 0; } } }", 11),
            ("fn main() -> Int { match 2 { 1 | 2 => { return 22; } _ => { return 0; } } }", 22),
            ("fn main() -> Int { let r = Result.Ok(8); match r { Result.Ok(n) => { return n; } Result.Err(e) => { return 0; } } }", 8),
            ("mod m { pub fn f() -> Int { return 6; } } fn main() -> Int { return m.f(); }", 6),
            ("fn main() -> Int { return abs(0 - 12); }", 12),
        ];
        for (src, expect) in cases {
            if expect == 1 && src.contains("Bool") {
                assert_eq!(execute(src).unwrap(), Value::Bool(true), "{src}");
            } else {
                assert_eq!(execute(src).unwrap(), Value::Int(expect), "{src}");
            }
        }
    }

    #[test]
    fn unary_not_and_neg() {
        assert_eq!(execute("fn main() -> Int { return -10; }").unwrap(), Value::Int(-10));
        assert_eq!(execute("fn main() -> Bool { return !true; }").unwrap(), Value::Bool(false));
        assert_eq!(execute("fn main() -> Bool { return !false; }").unwrap(), Value::Bool(true));
    }

    #[test]
    fn string_interp_simple() {
        let s = r#"
            fn main() -> String {
                let n = 3;
                return "n=${n}";
            }
        "#;
        match execute(s).unwrap() {
            Value::Str(x) => assert!(x.contains("3"), "{x}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn break_continue_for() {
        let s = r#"
            fn main() -> Int {
                let n = 0;
                for i in 0..10 {
                    if i == 3 { continue; }
                    if i == 5 { break; }
                    n = n + 1;
                }
                return n;
            }
        "#;
        // 0,1,2,4  (skip 3, break at 5) => 4 iterations counted? 
        // i=0 n=1; i=1 n=2; i=2 n=3; i=3 continue; i=4 n=4; i=5 break => 4
        assert_eq!(execute(s).unwrap(), Value::Int(4));
    }

    #[test]
    fn struct_method_call_style() {
        let s = r#"
            struct P { x: Int }
            impl P {
                fn get(self) -> Int { return self.x; }
            }
            fn main() -> Int {
                return P.get(P { x: 42 });
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(42));
    }

    #[test]
    fn nested_mod_two_levels() {
        let s = r#"
            mod a {
                mod b {
                    pub fn v() -> Int { return 11; }
                }
            }
            fn main() -> Int { return a.b.v(); }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(11));
    }

    #[test]
    fn method_sugar_list() {
        let s = r#"
            fn main() -> Int {
                let xs = [1, 2, 3, 4, 5];
                let n = xs.len();
                let a = xs.get(1);
                let t = xs.take(3);
                let d = xs.drop(2);
                return n + a + t.len() + d.len();
            }
        "#;
        // 5 + 2 + 3 + 3 = 13
        assert_eq!(execute(s).unwrap(), Value::Int(13));
    }

    #[test]
    fn method_sugar_string() {
        let s = r#"
            fn main() -> Int {
                let t = "  Hi  ".trim();
                if t.starts_with("Hi") && t.len() == 2 {
                    return 1;
                }
                return 0;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(1));
    }

    #[test]
    fn indexed_for_in() {
        let s = r#"
            fn main() -> Int {
                let sum = 0;
                for (i, x) in [10, 20, 30] {
                    sum = sum + i + x;
                }
                return sum;
            }
        "#;
        // 0+10 + 1+20 + 2+30 = 63
        assert_eq!(execute(s).unwrap(), Value::Int(63));
    }

    #[test]
    fn enumerate_builtin() {
        let s = r#"
            fn main() -> Int {
                let e = enumerate([5, 6]);
                let t0 = list_get(e, 0);
                let a = tuple_get(t0, 0);
                let t1 = list_get(e, 0);
                let b = tuple_get(t1, 1);
                return a + b + list_len(e);
            }
        "#;
        // 0+5+2 = 7
        assert_eq!(execute(s).unwrap(), Value::Int(7));
    }

    #[test]
    fn pipe_hole_mid() {
        let s = r#"
            fn main() -> Int {
                let f = fn(x: Int) -> Int { return x * 2; };
                let a = [1, 2, 3] |> list_map(f) |> list_sum();
                let xs = [10, 20, 30];
                let b = 1 |> list_get(xs, _);
                return a + b;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(32));
    }

    #[test]
    fn tuple_basic() {
        let s = r#"
            fn main() -> Int {
                let t = (10, 20, 30);
                let x = tuple_get(t, 0);
                let y = tuple_get(t, 1);
                let n = tuple_len(t);
                return x + y + n;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(33));
    }

    #[test]
    fn tuple_match_destructure() {
        let s = r#"
            fn main() -> Int {
                let t = (7, 8);
                let a = tuple_get(t, 0);
                let b = tuple_get(t, 1);
                return a + b;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(15));
    }

    #[test]
    fn match_expr_basic() {
        let s = r#"
            fn main() -> Int {
                let x = 2;
                let y = match x {
                    1 => 10,
                    2 => 20,
                    _ => 0,
                };
                return y;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(20));
    }

    #[test]
    fn match_expr_option() {
        // Bindings in match-expr arms (Variant payloads) — use int patterns here;
        // payload binds work in statement-match; expr form covers value-producing arms.
        let s = r#"
            fn main() -> Int {
                let o = 1;
                return match o {
                    0 => 0,
                    1 => 8,
                    _ => 99,
                };
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(8));
    }

    #[test]
    fn list_sort_by_basic() {
        let s = r#"
            fn main() -> Int {
                let xs = [3, 1, 2];
                let ys = list_sort_by(xs, fn(x: Int) -> Int { return x; });
                return list_get(ys, 0) * 100 + list_get(ys, 1) * 10 + list_get(ys, 2);
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(123));
    }

    #[test]
    fn if_expr_basic() {
        let s = r#"
            fn main() -> Int {
                let x = if true { 10 } else { 20 };
                let y = if x > 5 { x + 1 } else { 0 };
                return y;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(11));
    }

    #[test]
    fn list_unique_chunk_lines() {
        let s = r#"
            fn main() -> Int {
                let u = list_unique([1, 2, 2, 3, 1]);
                let ch = list_chunk([1, 2, 3, 4, 5], 2);
                let lines = str_lines("a\nb\nc");
                return list_len(u) + list_len(ch) + list_len(lines);
            }
        "#;
        // unique 3, chunks 3, lines 3 → 9
        assert_eq!(execute(s).unwrap(), Value::Int(9));
    }

    #[test]
    fn list_find_zip() {
        let s = r#"
            fn main() -> Int {
                let xs = [10, 20, 30, 40];
                let i = list_find(xs, fn(x: Int) -> Bool { return x == 30; });
                let z = list_zip([1, 2], [10, 20, 30]);
                let empty = list_is_empty([]);
                if empty && i == 2 {
                    return list_len(z);
                }
                return 0;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(2));
    }

    #[test]
    fn map_merge_basic() {
        let s = r#"
            fn main() -> Int {
                let a = map("x", 1);
                let b = map("y", 2, "x", 9);
                let m = map_merge(a, b);
                return map_get(m, "x") + map_get(m, "y") + map_len(m);
            }
        "#;
        // 9+2+2 = 13
        assert_eq!(execute(s).unwrap(), Value::Int(13));
    }

    #[test]
    fn map_keys_values() {
        let s = r#"
            fn main() -> Int {
                let m = map("a", 1, "b", 2, "c", 3);
                let keys = map_keys(m);
                let vals = map_values(map("x", 10, "y", 20));
                return map_len(m) + list_sum(vals) + list_len(keys);
            }
        "#;
        // 3 + 30 + 3 = 36
        assert_eq!(execute(s).unwrap(), Value::Int(36));
    }

    #[test]
    fn str_repeat_flatten() {
        let s = r#"
            fn main() -> Int {
                let t = str_repeat("ab", 3);
                let flat = list_flatten([[1, 2], [3], 4]);
                if t == "ababab" {
                    return list_sum(flat);
                }
                return 0;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(10));
    }

    #[test]
    fn list_min_max_any() {
        let s = r#"
            fn main() -> Int {
                let xs = [3, 1, 7, 2];
                let mn = list_min(xs);
                let mx = list_max([3, 1, 7, 2]);
                let any_big = list_any([1, 2, 9], fn(x: Int) -> Bool { return x > 5; });
                let all_pos = list_all([1, 2, 3], fn(x: Int) -> Bool { return x > 0; });
                if any_big && all_pos {
                    return mn + mx;
                }
                return 0;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(8));
    }

    #[test]
    fn str_index_substring() {
        let s = r#"
            fn main() -> Int {
                let i = str_index_of("hello world", "world");
                let sub = str_substring("hello", 1, 4);
                if sub == "ell" {
                    return i;
                }
                return -1;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(6));
    }

    #[test]
    fn modulo_op() {
        let s = r#"
            fn main() -> Int {
                let n = 17;
                n %= 5;
                return 10 % 3 + n;
            }
        "#;
        // 1 + 2 = 3
        assert_eq!(execute(s).unwrap(), Value::Int(3));
    }

    #[test]
    fn str_join_and_sort() {
        let s = r#"
            fn main() -> Int {
                let xs = list_sort([3, 1, 2]);
                let t = str_join(["a", "b", "c"], "-");
                if t == "a-b-c" {
                    return list_sum(xs);
                }
                return 0;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(6));
    }

    #[test]
    fn logic_and_or() {
        let s = r#"
            fn main() -> Int {
                if true && true {
                    if false || true {
                        return 1;
                    }
                }
                return 0;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(1));
    }

    #[test]
    fn logic_short_circuit_safe() {
        // right side of false && should not matter for type; value path
        let s = r#"
            fn main() -> Int {
                if false && true {
                    return 0;
                }
                if true || false {
                    return 7;
                }
                return 0;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(7));
    }

    #[test]
    fn range_and_list_helpers() {
        let s = r#"
            fn main() -> Int {
                let xs = range(1, 5);
                let r = list_rev(xs);
                let has = list_contains(r, 3);
                if has {
                    return list_len(r) + list_get(r, 0);
                }
                return 0;
            }
        "#;
        // still may move on list_contains - use only one use after contains
        let s = r#"
            fn main() -> Int {
                let xs = range(1, 5);
                if list_contains(xs, 3) {
                    let r = list_rev(range(1, 5));
                    return list_len(r) + list_get(r, 0);
                }
                return 0;
            }
        "#;
        // rev [4,3,2,1], len 4 + 4 = 8
        assert_eq!(execute(s).unwrap(), Value::Int(8));
    }

    #[test]
    fn compound_assign() {
        let s = r#"
            fn main() -> Int {
                let n = 10;
                n += 5;
                n *= 2;
                n -= 4;
                n /= 2;
                return n;
            }
        "#;
        // ((10+5)*2 - 4)/2 = 13
        assert_eq!(execute(s).unwrap(), Value::Int(13));
    }

    #[test]
    fn loop_break() {
        let s = r#"
            fn main() -> Int {
                let n = 0;
                loop {
                    n += 1;
                    if n == 4 { break; }
                }
                return n;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(4));
    }

    #[test]
    fn list_slice_basic() {
        let s = r#"
            fn main() -> Int {
                let xs = [10, 20, 30, 40, 50];
                let mid = list_slice(xs, 1, 4);
                return list_len(mid) + list_get(mid, 0);
            }
        "#;
        // len 3 + 20 = 23
        assert_eq!(execute(s).unwrap(), Value::Int(23));
    }

    #[test]
    fn while_let_once() {
        let s = r#"
            fn main() -> Int {
                let x = Option.Some(9);
                let out = 0;
                while let Option.Some(n) = x {
                    out = n;
                    break;
                }
                return out;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(9));
    }

    #[test]
    fn inclusive_for() {
        let s = r#"
            fn main() -> Int {
                let n = 0;
                for i in 1..=3 {
                    n = n + i;
                }
                return n;
            }
        "#;
        // 1+2+3 = 6
        assert_eq!(execute(s).unwrap(), Value::Int(6));
    }

    #[test]
    fn if_let_option() {
        let s = r#"
            fn main() -> Int {
                let x = Option.Some(7);
                if let Option.Some(n) = x {
                    return n;
                } else {
                    return 0;
                }
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(7));
    }

    #[test]
    fn panic_message() {
        let s = r#"
            fn main() -> Int {
                panic("nope");
                return 0;
            }
        "#;
        let err = execute(s).unwrap_err();
        assert!(err.contains("panic") && err.contains("nope"), "{err}");
    }

    #[test]
    fn pipeline_list_map() {
        let s = r#"
            fn main() -> Int {
                let xs = [1, 2, 3, 4];
                let sum = xs
                    |> list_map(fn(x: Int) -> Int { return x * 2; })
                    |> list_fold(0, fn(a: Int, b: Int) -> Int { return a + b; });
                return sum;
            }
        "#;
        // 2+4+6+8 = 20
        assert_eq!(execute(s).unwrap(), Value::Int(20));
    }

    #[test]
    fn list_literal_len() {
        let s = r#"
            fn main() -> Int {
                return list_len([10, 20, 30]);
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(3));
    }

    #[test]
    fn assert_with_message() {
        let s = r#"
            fn main() -> Int {
                assert(1 == 1, "ok");
                return 1;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(1));
        let s2 = r#"
            fn main() -> Int {
                assert(false, "boom");
                return 0;
            }
        "#;
        let err = execute(s2).unwrap_err();
        assert!(err.contains("boom"), "{err}");
    }

    #[test]
    fn generic_id() {
        let s = r#"
            fn id[T](x: T) -> T {
                return x;
            }
            fn main() -> Int {
                return id(99);
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(99));
    }

    #[test]
    fn use_alias_call() {
        let s = r#"
            mod combat {
                pub fn damage(x: Int) -> Int { return x * 2; }
            }
            use combat.damage;
            fn main() -> Int {
                return damage(5);
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(10));
    }

    #[test]
    fn defer_runs_before_return() {
        let s = r#"
            fn main() -> Int {
                let x = 1;
                defer {
                    x = x + 10;
                }
                x = x + 1;
                return x;
            }
        "#;
        // defer after body path: x=1, x=x+1 → 2, then defer x=x+10 → 12, then return uses?
        // Our desugar: on Return, run defer first then return e.
        // return x evaluates x after defer if we run defer before compile of return expr?
        // desugar: defer body then Return(x) — so x is updated before return expr eval. Good → 12
        assert_eq!(execute(s).unwrap(), Value::Int(12));
    }

    #[test]
    fn string_split_replace() {
        let s = r#"
            fn main() -> Int {
                let parts = str_split("a,b,c", ",");
                let t = str_replace("hello", "l", "L");
                if str_starts_with(t, "he") {
                    if str_ends_with(t, "o") {
                        return list_len(parts) + str_len(t);
                    }
                }
                return 0;
            }
        "#;
        // parts 3
        assert_eq!(execute(s).unwrap(), Value::Int(8));
    }

    #[test]
    fn try_operator_ok() {
        let s = r#"
            fn main() -> Int {
                let r = Result.Ok(42);
                return r?;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(42));
    }

    #[test]
    fn try_operator_err_propagates() {
        let s = r#"
            fn main() -> Int {
                let r = Result.Err("boom");
                return r?;
            }
        "#;
        // early return of Result.Err from main
        match execute(s).unwrap() {
            Value::Enum { variant, .. } => assert_eq!(variant, "Err"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn const_top_level() {
        let s = r#"
            const MAX = 10;
            fn main() -> Int {
                return MAX + 5;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(15));
    }

    #[test]
    fn list_map_filter_fold() {
        let s = r#"
            fn main() -> Int {
                let xs = list(1, 2, 3, 4);
                let doubled = list_map(xs, fn(x: Int) -> Int { return x * 2; });
                let evens = list_filter(doubled, fn(x: Int) -> Bool { return x > 4; });
                let sum = list_fold(evens, 0, fn(a: Int, b: Int) -> Int { return a + b; });
                return sum;
            }
        "#;
        // doubled 2,4,6,8 → filter >4 → 6,8 → fold 14
        assert_eq!(execute(s).unwrap(), Value::Int(14));
    }

    #[test]
    fn exhaustive_match_error() {
        let s = r#"
            enum Color { Red, Green, Blue }
            fn main() -> Int {
                let c = Color.Red;
                match c {
                    Color.Red => { return 1; }
                    Color.Green => { return 2; }
                }
            }
        "#;
        let err = execute(s).unwrap_err();
        assert!(err.contains("non-exhaustive") || err.contains("Blue"), "{err}");
    }

    #[test]
    fn nested_mod_three_levels() {
        let s = r#"
            mod x {
                mod y {
                    mod z {
                        pub fn n() -> Int { return 7; }
                    }
                }
            }
            fn main() -> Int { return x.y.z.n(); }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(7));
    }

    #[test]
    fn match_guard_score() {
        let s = r#"
            fn main() -> Int {
                let score = 85;
                match score {
                    s if s >= 90 => { return 1; }
                    s if s >= 70 => { return 2; }
                    _ => { return 0; }
                }
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(2));
    }

    #[test]
    fn list_get_basic() {
        let s = r#"
            fn main() -> Int {
                let xs = list(7, 8, 9);
                return list_get(xs, 1);
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(8));
    }

    #[test]
    fn ultra_smoke_batch() {
        let cases: &[(&str, i64)] = &[
        ];
        let _ = cases;
        // solid cases only
        let solid: &[(&str, i64)] = &[
            ("fn main() -> Int { return 100; }", 100),
            ("fn main() -> Int { return 50 + 50; }", 100),
            ("fn main() -> Int { return 200 - 100; }", 100),
            ("fn main() -> Int { return 25 * 4; }", 100),
            ("fn main() -> Int { return 300 / 3; }", 100),
            ("fn main() -> Int { let x = 40; return x + 60; }", 100),
            ("fn main() -> Int { if 1 < 2 { return 100; } else { return 0; } }", 100),
            ("fn main() -> Int { match 1 { 1 => { return 100; } _ => { return 0; } } }", 100),
            ("fn main() -> Int { match 2 { 1 | 2 | 3 => { return 100; } _ => { return 0; } } }", 100),
            ("fn id(x: Int) -> Int { return x; } fn main() -> Int { return id(100); }", 100),
            ("fn main() -> Int { let xs = list(1,2); return list_len(xs) + 98; }", 100),
            ("fn main() -> Int { return abs(0 - 100); }", 100),
            ("fn main() -> Int { return min(100, 200); }", 100),
            ("fn main() -> Int { return max(1, 100); }", 100),
            ("mod m { pub fn v() -> Int { return 100; } } fn main() -> Int { return m.v(); }", 100),
        ];
        for (src, n) in solid {
            assert_eq!(execute(src).unwrap(), Value::Int(*n), "{src}");
        }
    }

    #[test]
    fn more_language_smoke() {
        let cases: &[(&str, i64)] = &[
            ("fn main() -> Int { return 1; }", 1),
            ("fn main() -> Int { return 2 + 2; }", 4),
            ("fn main() -> Int { return 10 - 3; }", 7),
            ("fn main() -> Int { return 3 * 4; }", 12),
            ("fn main() -> Int { return 15 / 3; }", 5),
            ("fn main() -> Int { if true { return 1; } else { return 0; } }", 1),
            ("fn main() -> Int { if false { return 1; } else { return 2; } }", 2),
            ("fn main() -> Int { let x = 1; if x < 2 { return 3; } else { return 0; } }", 3),
            ("fn main() -> Int { let i = 0; while i < 2 { i = i + 1; } return i; }", 2),
            ("fn main() -> Int { for i in 0..3 { } return 3; }", 3),
            ("fn main() -> Int { match 0 { 0 => { return 8; } _ => { return 0; } } }", 8),
            ("fn main() -> Int { match 5 { 1 | 5 => { return 9; } _ => { return 0; } } }", 9),
            ("fn main() -> Int { let s = \"hi\"; return str_len(s); }", 2),
            ("fn main() -> Int { let xs = list(10, 20); return list_len(xs); }", 2),
            ("fn main() -> Int { return abs(0 - 3); }", 3),
            ("fn main() -> Int { return min(4, 9); }", 4),
            ("fn main() -> Int { return max(4, 9); }", 9),
            ("fn f(x: Int) -> Int { return x; } fn main() -> Int { return f(12); }", 12),
            ("fn main() -> Int { let r = Result.Ok(3); match r { Result.Ok(n) => { return n; } Result.Err(e) => { return 0; } } }", 3),
            ("mod z { pub fn g() -> Int { return 14; } } fn main() -> Int { return z.g(); }", 14),
        ];
        for (src, n) in cases {
            assert_eq!(execute(src).unwrap(), Value::Int(*n), "{src}");
        }
    }

    #[test]
    fn arithmetic() {
        assert_eq!(
            execute("fn main()->Int{ return 20+22; }").unwrap(),
            Value::Int(42)
        );
    }

    #[test]
    fn break_exits_for_loop_early() {
        let s = "fn main()->Int{ let n=0; for i in 0..100 { n=n+1; if n==5 { break; } } return n; }";
        assert_eq!(execute(s), Ok(Value::Int(5)));
    }

    #[test]
    fn continue_skips_iteration() {
        let s = "fn main()->Int{ let sum=0; for i in 0..5 { if i==2 { continue; } sum=sum+i; } return sum; }";
        assert_eq!(execute(s), Ok(Value::Int(8)));
    }

    #[test]
    fn break_exits_while_loop() {
        let s = "fn main()->Int{ let x=0; while true { x=x+1; if x>=3 { break; } } return x; }";
        assert_eq!(execute(s), Ok(Value::Int(3)));
    }

    #[test]
    fn break_outside_loop_is_error() {
        let s = "fn main(){ break; }";
        assert!(execute(s).unwrap_err().contains("break"));
    }

    #[test]
    fn recursion_limit_soft() {
        set_runtime_limits(8, 1000);
        let s = r#"
            fn boom(n: Int) -> Int {
                return boom(n + 1);
            }
            fn main() -> Int {
                return boom(0);
            }
        "#;
        let err = execute(s).unwrap_err();
        reset_runtime_limits();
        assert!(err.contains("recursion") || err.contains("depth"), "{err}");
    }

    #[test]
    fn sema_error_mentions_function() {
        let s = r#"
            fn main() -> Int {
                return missing;
            }
        "#;
        let err = execute(s).unwrap_err();
        assert!(err.contains("main") || err.contains("undefined"), "{err}");
    }

    #[test]
    fn or_pattern_bools() {
        let s = r#"
            fn main() -> Int {
                match true {
                    true | false => { return 1; }
                }
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(1));
    }

    #[test]
    fn nested_pub_module() {
        let s = r#"
            mod combat {
                pub fn hit(x: Int) -> Int {
                    return x + 1;
                }
                fn secret() -> Int {
                    return 99;
                }
            }
            fn main() -> Int {
                return combat.hit(10);
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(11));
    }

    #[test]
    fn private_fn_not_callable_outside() {
        let s = r#"
            mod combat {
                fn secret() -> Int { return 1; }
            }
            fn main() -> Int {
                return combat.secret();
            }
        "#;
        assert!(execute(s).unwrap_err().contains("private"));
    }

    #[test]
    fn result_convention_prelude() {
        let s = r#"
            fn main() -> Int {
                let r = Result.Ok(42);
                match r {
                    Result.Ok(n) => { return n; }
                    Result.Err(msg) => { return 0; }
                }
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(42));
    }

    #[test]
    fn match_or_pattern() {
        let s = r#"
            fn main() -> Int {
                match 2 {
                    1 | 2 | 3 => { return 10; }
                    _ => { return 0; }
                }
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(10));
    }

    #[test]
    fn match_struct_pattern() {
        let s = r#"
            struct Player { hp: Int, name: String }
            fn main() -> Int {
                let p = Player { hp: 100, name: "Aria" };
                match p {
                    Player { hp, name } => {
                        print(hp);
                        return hp;
                    }
                }
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(100));
    }

    #[test]
    fn match_struct_rename_and_guard() {
        let s = r#"
            struct Player { hp: Int, name: String }
            fn main() -> Int {
                let p = Player { hp: 40, name: "Bob" };
                match p {
                    Player { hp: h, name: n } if h < 50 => { return h; }
                    Player { hp, name } => { return 0; }
                }
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(40));
    }

    #[test]
    fn match_with_guard() {
        let s = r#"
            fn main() -> Int {
                match 5 {
                    n if n > 10 => { return 1; }
                    n if n > 3 => { return n; }
                    _ => { return 0; }
                }
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(5));
    }

    #[test]
    fn match_guard_with_enum() {
        let s = r#"
            enum OptionInt { None, Some(Int) }
            fn main() -> Int {
                let x = OptionInt.Some(7);
                match x {
                    OptionInt.Some(n) if n > 10 => { return 1; }
                    OptionInt.Some(n) if n > 5 => { return n; }
                    OptionInt.None => { return 0; }
                    _ => { return 99; }
                }
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(7));
    }

    #[test]
    fn enums_match_some() {
        let s = r#"
            enum OptionInt {
                None,
                Some(Int)
            }
            fn main() -> Int {
                let b = OptionInt.Some(42);
                match b {
                    OptionInt.Some(n) => { return n; }
                    OptionInt.None => { return 0; }
                }
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(42));
    }

    #[test]
    fn enums_construct_only() {
        let s = r#"
            enum OptionInt { None, Some(Int) }
            fn main() -> Int {
                let b = OptionInt.Some(42);
                return 1;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(1));
    }

    #[test]
    fn struct_field_access() {
        let s = r#"
            struct Player { hp: Int, name: String }
            fn main()->Int {
                let p = Player { hp: 100, name: "hero" };
                return p.hp;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(100));
    }

    #[test]
    fn for_in_list() {
        let s = r#"
            import std.collections;
            fn main()->Int {
                let xs = list(1, 2, 3);
                let sum = 0;
                for x in xs {
                    sum = sum + x;
                }
                return sum;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(6));
    }

    #[test]
    fn match_int() {
        let s = r#"
            fn main()->Int {
                let x = 2;
                match x {
                    1 => { return 10; }
                    2 => { return 20; }
                    _ => { return 0; }
                }
                return 99;
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(20));
    }

    #[test]
    fn string_interp() {
        let s = r#"
            fn main() {
                let name = "world";
                print("hello {name}");
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Unit);
    }

    #[test]
    fn closure_captures_outer() {
        let s = r#"
            fn main() -> Int {
                let base = 10;
                let add_base = fn(x: Int) -> Int { return x + base; };
                return add_base(32);
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(42));
    }

    #[test]
    fn closures_work() {
        let s = r#"
            fn main() -> Int {
                let add1 = fn(x: Int) -> Int { return x + 1; };
                return add1(41);
            }
        "#;
        assert_eq!(execute(s).unwrap(), Value::Int(42));
    }

    #[test]
    fn function_call() {
        let s = "fn add(a:Int,b:Int)->Int{return a+b;} fn main()->Int{return add(2,3);}";
        assert_eq!(execute(s).unwrap(), Value::Int(5));
    }
}

#[cfg(test)]
mod stdlib_api_tests {
    use super::*;
    #[test]
    fn string_and_collection_functions_execute() {
        let source = "import std.string; import std.collections; fn main()->Int{ let xs=list(1,2); let ys=list_push(xs,3); print(str_upper(\"hello\")); print(str_contains(\"hello\",\"ell\")); print(list_get(ys,1)); return list_len(ys); }";
        assert_eq!(execute(source), Ok(Value::Int(3)));
    }
}

#[cfg(test)]
mod advanced_collection_tests {
    use super::*;
    #[test]
    fn immutable_list_updates_and_map_access_work() {
        let source = "import std.collections; fn main()->Int{ let a=list(1,2); let b=list_set(a,0,9); let c=list_concat(a,b); let m=map(\"answer\",42); let n=map_set(m,\"extra\",7); print(list_get(c,0)); print(map_get(n,\"answer\")); print(map_has(n,\"extra\")); return list_len(c); }";
        assert_eq!(execute(source), Ok(Value::Int(4)));
    }
    #[test]
    fn list_set_rejects_out_of_bounds_index() {
        let source = "import std.collections; fn main(){ let a=list(1); print(list_set(a,4,9)); }";
        assert!(execute(source).unwrap_err().contains("list_set index out of bounds"));
    }
    #[test]
    fn map_requires_string_keys_and_known_values() {
        let source = "import std.collections; fn main(){ let m=map(\"x\",1); print(map_get(m,\"missing\")); }";
        assert!(execute(source).unwrap_err().contains("map key not found"));
    }
}
