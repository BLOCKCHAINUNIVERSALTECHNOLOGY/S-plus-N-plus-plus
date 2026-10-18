//! S+N++ abstract syntax tree and language data models.

use crate::lexer::Token;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::fmt;

/// Byte offset span in source (for diagnostics).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// Static type of an S+N++ value or expression.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    Int,
    Float,
    Str,
    Bool,
    Unit,
    Channel,
    Thread,
    /// Untyped / legacy list
    List,
    /// Typed list: List[T]
    ListOf(Box<Type>),
    Map,
    /// Typed map: Map[K,V] (K usually String)
    MapOf(Box<Type>, Box<Type>),
    /// User-defined struct type by name
    Struct(String),
    /// Generic struct application: `Box[Int]`
    StructApp(String, Vec<Type>),
    /// User-defined enum type by name
    Enum(String),
    /// Generic enum application: `Option[Int]`
    EnumApp(String, Vec<Type>),
    /// Generic type parameter name (lightweight generics)
    Generic(String),
    /// Tuple type: (Int, String)
    Tuple(Vec<Type>),
    Ref(Box<Type>),
    MutRef(Box<Type>),
    Unknown,
}

impl Type {
    pub fn is_copyable(&self) -> bool {
        matches!(
            self,
            Type::Int
                | Type::Float
                | Type::Bool
                | Type::Str
                | Type::Unit
                | Type::Channel
                | Type::Thread
                | Type::Generic(_)
                | Type::Tuple(_)
        )
    }

    pub fn name(&self) -> String {
        match self {
            Type::Int => "Int".into(),
            Type::Float => "Float".into(),
            Type::Str => "String".into(),
            Type::Bool => "Bool".into(),
            Type::Unit => "Unit".into(),
            Type::Channel => "Channel".into(),
            Type::Thread => "Thread".into(),
            Type::List => "List".into(),
            Type::ListOf(t) => format!("List[{}]", t.name()),
            Type::Map => "Map".into(),
            Type::MapOf(k, v) => format!("Map[{}, {}]", k.name(), v.name()),
            Type::Struct(n) => n.clone(),
            Type::StructApp(n, args) => {
                let inner = args.iter().map(|t| t.name()).collect::<Vec<_>>().join(", ");
                format!("{n}[{inner}]")
            }
            Type::Enum(n) => n.clone(),
            Type::EnumApp(n, args) => {
                let inner = args.iter().map(|t| t.name()).collect::<Vec<_>>().join(", ");
                format!("{n}[{inner}]")
            }
            Type::Generic(n) => n.clone(),
            Type::Tuple(ts) => {
                let inner = ts.iter().map(|t| t.name()).collect::<Vec<_>>().join(", ");
                format!("({inner})")
            }
            Type::Ref(inner) => format!("&{}", inner.name()),
            Type::MutRef(inner) => format!("&mut {}", inner.name()),
            Type::Unknown => "<unknown>".into(),
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Expression nodes.
#[derive(Debug, Clone)]
pub enum Expr {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Var(String),
    Binary(Box<Expr>, Token, Box<Expr>),
    Unary(Token, Box<Expr>),
    Call(String, Vec<Expr>),
    Index(Box<Expr>, Box<Expr>),
    /// Field access: `player.hp`
    Field(Box<Expr>, String),
    /// Struct literal: `Player { hp: 100, name: "a" }`
    StructLit {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    /// String interpolation parts: "hello {name}" → [Str("hello "), Var(name)]
    Interp(Vec<InterpPart>),
    /// Closure / anonymous function: `fn(x: Int) -> Int { return x + 1; }`
    Lambda {
        params: Vec<String>,
        param_types: Vec<Type>,
        return_type: Type,
        body: Vec<Stmt>,
    },
    /// Indirect call: `f(1, 2)` where f is a closure value
    CallExpr(Box<Expr>, Vec<Expr>),
    /// Method call: `hero.heal(10)` — resolved using receiver's struct type
    MethodCall {
        receiver: Box<Expr>,
        method: String,
        args: Vec<Expr>,
    },
    /// Enum variant construction: `Option.Some(42)` or unit `Option.None`
    VariantLit {
        enum_name: String,
        variant: String,
        args: Vec<Expr>,
    },
    /// `expr?` — unwrap Result.Ok or early-return Result.Err
    Try(Box<Expr>),
    /// Expression if: `if cond { then } else { else_ }` (both branches are expressions)
    IfExpr {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Box<Expr>,
    },
    /// Expression match: `match e { pat => expr, ... }` (desugars to IIFE + stmt match)
    MatchExpr {
        scrutinee: Box<Expr>,
        arms: Vec<MatchExprArm>,
    },
    /// Pipeline hole `_` — filled by `|>` left value
    Hole,
    /// Tuple literal: `(a, b, c)`
    Tuple(Vec<Expr>),
}

#[derive(Debug, Clone)]
pub struct MatchExprArm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub value: Expr,
}


#[derive(Debug, Clone)]
pub enum InterpPart {
    Lit(String),
    Expr(Expr),
}

/// One arm of a `match`: pattern, optional `if` guard, body.
#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Vec<Stmt>,
}

/// Pattern for match arms.
#[derive(Debug, Clone)]
pub enum Pattern {
    Wildcard,
    Int(i64),
    Bool(bool),
    Str(String),
    Var(String),
    /// `Option.Some(n)` or `Some(n)` — bindings bind payload fields
    Variant {
        enum_name: Option<String>,
        variant: String,
        bindings: Vec<String>,
    },
    /// `Player { hp, name }` or `Player { hp: h, name: n }`
    /// fields: (struct_field, binding_name)
    Struct {
        name: String,
        fields: Vec<(String, String)>,
    },
    /// `1 | 2 | 3` or `Red | Blue` — alternatives (no bindings across arms for v0.3)
    Or(Vec<Pattern>),
    /// `(a, b)` tuple pattern
    Tuple(Vec<Pattern>),
    /// `1..10` or `1..=10` integer range pattern
    Range {
        start: i64,
        end: i64,
        inclusive: bool,
    },
}

/// Enum definition: `enum Option { None, Some(Int) }`
#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: String,
    pub visibility: Visibility,
    pub module_path: Vec<String>,
    /// `enum Option[T] { None, Some(T) }`
    pub type_params: Vec<String>,
    /// (variant name, payload field types)
    pub variants: Vec<(String, Vec<Type>)>,
}

/// Statement nodes.
#[derive(Debug, Clone)]
pub enum Stmt {
    Let(String, Expr),
    Decl(String, Type),
    Assign(String, Expr),
    /// Field assign: `player.hp = 50`
    FieldAssign {
        object: Expr,
        field: String,
        value: Expr,
    },
    Expr(Expr),
    Return(Expr),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    While(Expr, Vec<Stmt>),
    /// `for i in start..end`
    For {
        var: String,
        start: Expr,
        end: Expr,
        /// `true` for `start..=end` (inclusive end)
        inclusive: bool,
        body: Vec<Stmt>,
    },
    /// `for item in collection`
    ForIn {
        var: String,
        /// Optional index binding: `for (i, x) in xs`
        index: Option<String>,
        iter: Expr,
        body: Vec<Stmt>,
    },
    /// `match expr { pat if guard => {..}, ... }`
    Match {
        scrutinee: Expr,
        arms: Vec<MatchArm>,
    },
    /// `if let PATTERN = expr { yes } else { no }`
    IfLet {
        pattern: Pattern,
        value: Expr,
        yes: Vec<Stmt>,
        no: Vec<Stmt>,
    },
    /// `break` or `break label`
    Break(Option<String>),
    /// `continue` or `continue label`
    Continue(Option<String>),
    /// `label: while/for/loop ...`
    Labeled(String, Box<Stmt>),
    /// `const NAME = expr;` (compile-time literal preferred)
    Const(String, Expr),
    /// `defer { stmts }` — run LIFO at scope exit / before return
    Defer(Vec<Stmt>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Private,
}

/// Trait definition: `trait Drawable { fn draw(self) -> Int; }`
#[derive(Debug, Clone)]
pub struct TraitDef {
    pub name: String,
    pub visibility: Visibility,
    pub module_path: Vec<String>,
    pub methods: Vec<TraitMethod>,
}

#[derive(Debug, Clone)]
pub struct TraitMethod {
    pub name: String,
    pub params: Vec<String>,
    pub param_types: Vec<Type>,
    pub return_type: Type,
    /// Optional default body: `fn foo(self) -> Int { return 0; }` inside trait
    pub default_body: Option<Vec<Stmt>>,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub visibility: Visibility,
    /// Module path prefix, e.g. `["combat"]` for `mod combat { fn ... }`
    pub module_path: Vec<String>,
    pub params: Vec<String>,
    pub param_types: Vec<Type>,
    pub return_type: Type,
    /// Optional type parameters: `fn id[T](x: T) -> T`
    pub type_params: Vec<String>,
    /// `where Player: Healable` — type must implement trait
    pub trait_bounds: Vec<(String, String)>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub visibility: Visibility,
    pub module_path: Vec<String>,
    /// `struct Box[T] { ... }`
    pub type_params: Vec<String>,
    pub fields: Vec<(String, Type)>,
    /// Default field values: `hp: Int = 100`
    pub defaults: Vec<(String, Expr)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Unit,
    Thread(usize),
    Channel(usize),
    /// Shared until mutation (`Rc::make_mut`) — cheap clones.
    List(Arc<Vec<Value>>),
    /// Shared until mutation.
    Map(Arc<BTreeMap<String, Value>>),
    /// Struct instance: type name + field map
    Struct {
        name: String,
        fields: BTreeMap<String, Value>,
    },
    /// Enum instance: `Option.Some(42)`
    Enum {
        name: String,
        variant: String,
        fields: Vec<Value>,
    },
    /// Closure: compiled function name + captured outer values (by move/copy at creation).
    Closure {
        name: String,
        captures: BTreeMap<String, Value>,
    },
    /// Tuple value
    Tuple(Vec<Value>),
}

impl Value {
    pub fn type_of(&self) -> Type {
        match self {
            Value::Int(_) => Type::Int,
            Value::Float(_) => Type::Float,
            Value::Str(_) => Type::Str,
            Value::Bool(_) => Type::Bool,
            Value::Unit => Type::Unit,
            Value::Thread(_) => Type::Thread,
            Value::Channel(_) => Type::Channel,
            Value::List(_) => Type::List,
            Value::Map(_) => Type::Map,
            Value::Struct { name, .. } => Type::Struct(name.clone()),
            Value::Enum { name, .. } => Type::Enum(name.clone()),
            Value::Closure { .. } => Type::Unknown, // function type later
            Value::Tuple(items) => Type::Tuple(items.iter().map(|v| v.type_of()).collect()),
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Float(n) => *n != 0.0,
            Value::Unit => false,
            _ => true,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(x) => write!(f, "{x}"),
            Value::Float(x) => write!(f, "{x}"),
            Value::Str(s) => write!(f, "{s}"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Unit => write!(f, "unit"),
            Value::Thread(id) => write!(f, "thread({id})"),
            Value::Channel(id) => write!(f, "channel({id})"),
            Value::List(items) => write!(f, "list(len={})", items.len()),
            Value::Map(items) => write!(f, "map(len={})", items.len()),
            Value::Struct { name, fields } => {
                write!(f, "{}{{", name)?;
                let mut first = true;
                for (k, v) in fields {
                    if !first {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                    first = false;
                }
                write!(f, "}}")
            }
            Value::Closure { name, captures } => write!(f, "closure({name}, caps={})", captures.len()),
            Value::Tuple(items) => {
                write!(f, "(")?;
                for (i, it) in items.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{it}")?;
                }
                write!(f, ")")
            }
            Value::Enum { name, variant, fields } => {
                write!(f, "{name}.{variant}")?;
                if !fields.is_empty() {
                    write!(f, "(")?;
                    for (i, v) in fields.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{v}")?;
                    }
                    write!(f, ")")?;
                }
                Ok(())
            }
        }
    }
}

/// External (FFI) function declaration — body provided by the host.
#[derive(Debug, Clone)]
pub struct ExternFn {
    pub abi: String,
    pub name: String,
    pub params: Vec<String>,
    pub param_types: Vec<Type>,
    pub return_type: Type,
}

#[derive(Debug, Clone)]
pub struct TypeAlias {
    pub name: String,
    pub type_params: Vec<String>,
    pub target: Type,
    pub visibility: Visibility,
}

#[derive(Debug, Clone)]
pub struct Program {
    pub imports: Vec<String>,
    pub structs: Vec<StructDef>,
    pub enums: Vec<EnumDef>,
    pub traits: Vec<TraitDef>,
    pub functions: Vec<Function>,
    pub externs: Vec<ExternFn>,
    /// Top-level `const NAME = value;`
    pub consts: Vec<(String, Expr)>,
    /// `use path.to.name` or `use path.{a, b}` → local aliases
    pub uses: Vec<UseDecl>,
    /// `type Name[T] = Target`
    pub type_aliases: Vec<TypeAlias>,
}

#[derive(Debug, Clone)]
pub struct UseDecl {
    /// Full path segments, e.g. ["combat", "heal"] or ["combat"] with names
    pub path: Vec<String>,
    /// If non-empty, brace form: use combat.{heal, damage}
    pub names: Vec<String>,
}
