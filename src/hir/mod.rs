//! High-level Intermediate Representation (HIR) for S+N++.
//!
//! HIR sits between the typed AST and bytecode lowering. It is intentionally
//! simpler than the surface language so future passes (borrow checker
//! refinements, MIR, LLVM) have a stable core IR.
//!
//! Current scope: faithful lowering of the typed program into explicit
//! control-flow and value operations — no optimisations yet.

use crate::ast::{
    Expr, Function, InterpPart, Pattern, Program, Stmt, StructDef, Type, Value,
};
use crate::lexer::Token;
use std::collections::BTreeMap;

/// A complete HIR module after lowering.
#[derive(Debug, Clone)]
pub struct HirModule {
    pub structs: Vec<StructDef>,
    pub functions: Vec<HirFunction>,
}

#[derive(Debug, Clone)]
pub struct HirFunction {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub body: Vec<HirStmt>,
}

#[derive(Debug, Clone)]
pub enum HirStmt {
    Let { name: String, value: HirExpr },
    Assign { name: String, value: HirExpr },
    FieldAssign {
        object: HirExpr,
        field: String,
        value: HirExpr,
    },
    Expr(HirExpr),
    Return(HirExpr),
    If {
        cond: HirExpr,
        then_body: Vec<HirStmt>,
        else_body: Vec<HirStmt>,
    },
    While {
        cond: HirExpr,
        body: Vec<HirStmt>,
    },
    /// Lowered range / collection loops still explicit for VM mapping
    ForRange {
        var: String,
        start: HirExpr,
        end: HirExpr,
        body: Vec<HirStmt>,
    },
    ForIn {
        var: String,
        iter: HirExpr,
        body: Vec<HirStmt>,
    },
    Match {
        scrutinee: HirExpr,
        arms: Vec<(Pattern, Option<HirExpr>, Vec<HirStmt>)>,
    },
    Break,
    Continue,
}

#[derive(Debug, Clone)]
pub enum HirExpr {
    Literal(Value),
    Var(String),
    Binary(Box<HirExpr>, Token, Box<HirExpr>),
    Unary(Token, Box<HirExpr>),
    Call { name: String, args: Vec<HirExpr> },
    CallExpr {
        callee: Box<HirExpr>,
        args: Vec<HirExpr>,
    },
    Index {
        base: Box<HirExpr>,
        index: Box<HirExpr>,
    },
    Field {
        object: Box<HirExpr>,
        field: String,
    },
    StructLit {
        name: String,
        fields: Vec<(String, HirExpr)>,
    },
    VariantLit {
        enum_name: String,
        variant: String,
        args: Vec<HirExpr>,
    },
    Interp(Vec<HirInterpPart>),
    Lambda {
        params: Vec<(String, Type)>,
        return_type: Type,
        body: Vec<HirStmt>,
    },
}

#[derive(Debug, Clone)]
pub enum HirInterpPart {
    Lit(String),
    Expr(HirExpr),
}

/// Lower a surface `Program` into HIR.
pub fn lower(program: &Program) -> HirModule {
    HirModule {
        structs: program.structs.clone(),
        functions: program
            .functions
            .iter()
            .map(lower_function)
            .collect(),
    }
}

fn lower_function(f: &Function) -> HirFunction {
    HirFunction {
        name: f.name.clone(),
        params: f
            .params
            .iter()
            .zip(f.param_types.iter())
            .map(|(n, t)| (n.clone(), t.clone()))
            .collect(),
        return_type: f.return_type.clone(),
        body: lower_stmts(&f.body),
    }
}

fn lower_stmts(xs: &[Stmt]) -> Vec<HirStmt> {
    xs.iter().map(lower_stmt).collect()
}

fn lower_stmt(s: &Stmt) -> HirStmt {
    match s {
        Stmt::Let(n, e) => HirStmt::Let {
            name: n.clone(),
            value: lower_expr(e),
        },
        Stmt::Decl(n, _) => HirStmt::Let {
            name: n.clone(),
            value: HirExpr::Literal(Value::Unit),
        },
        Stmt::Assign(n, e) => HirStmt::Assign {
            name: n.clone(),
            value: lower_expr(e),
        },
        Stmt::FieldAssign {
            object,
            field,
            value,
        } => HirStmt::FieldAssign {
            object: lower_expr(object),
            field: field.clone(),
            value: lower_expr(value),
        },
        Stmt::Expr(e) => HirStmt::Expr(lower_expr(e)),
        Stmt::Return(e) => HirStmt::Return(lower_expr(e)),
        Stmt::If(c, a, b) => HirStmt::If {
            cond: lower_expr(c),
            then_body: lower_stmts(a),
            else_body: lower_stmts(b),
        },
        Stmt::While(c, body) => HirStmt::While {
            cond: lower_expr(c),
            body: lower_stmts(body),
        },
        Stmt::For {
            var,
            start,
            end,
            body,
        } => HirStmt::ForRange {
            var: var.clone(),
            start: lower_expr(start),
            end: lower_expr(end),
            body: lower_stmts(body),
        },
        Stmt::ForIn { var, iter, body } => HirStmt::ForIn {
            var: var.clone(),
            iter: lower_expr(iter),
            body: lower_stmts(body),
        },
        Stmt::Match { scrutinee, arms } => {
            // Expand or-patterns into separate arms (same guard/body)
            let mut expanded = Vec::new();
            for arm in arms {
                let alts = match &arm.pattern {
                    Pattern::Or(xs) => xs.clone(),
                    other => vec![other.clone()],
                };
                for pat in alts {
                    expanded.push((
                        pat,
                        arm.guard.as_ref().map(lower_expr),
                        lower_stmts(&arm.body),
                    ));
                }
            }
            HirStmt::Match {
                scrutinee: lower_expr(scrutinee),
                arms: expanded,
            }
        }
        Stmt::Break => HirStmt::Break,
        Stmt::Continue => HirStmt::Continue,
    }
}

fn lower_expr(e: &Expr) -> HirExpr {
    match e {
        Expr::Int(v) => HirExpr::Literal(Value::Int(*v)),
        Expr::Str(v) => HirExpr::Literal(Value::Str(v.clone())),
        Expr::Bool(v) => HirExpr::Literal(Value::Bool(*v)),
        Expr::Var(n) => HirExpr::Var(n.clone()),
        Expr::Binary(a, op, b) => HirExpr::Binary(
            Box::new(lower_expr(a)),
            op.clone(),
            Box::new(lower_expr(b)),
        ),
        Expr::Unary(op, e) => HirExpr::Unary(op.clone(), Box::new(lower_expr(e))),
        Expr::Call(n, args) => HirExpr::Call {
            name: n.clone(),
            args: args.iter().map(lower_expr).collect(),
        },
        Expr::CallExpr(callee, args) => HirExpr::CallExpr {
            callee: Box::new(lower_expr(callee)),
            args: args.iter().map(lower_expr).collect(),
        },
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            // Prefer desugar before HIR; keep a Call-shaped fallback
            let mut call_args = vec![lower_expr(receiver)];
            call_args.extend(args.iter().map(lower_expr));
            HirExpr::Call {
                name: format!("?.{method}"),
                args: call_args,
            }
        },
        Expr::Index(base, idx) => HirExpr::Index {
            base: Box::new(lower_expr(base)),
            index: Box::new(lower_expr(idx)),
        },
        Expr::Field(obj, f) => HirExpr::Field {
            object: Box::new(lower_expr(obj)),
            field: f.clone(),
        },
        Expr::VariantLit {
            enum_name,
            variant,
            args,
        } => HirExpr::VariantLit {
            enum_name: enum_name.clone(),
            variant: variant.clone(),
            args: args.iter().map(lower_expr).collect(),
        },
        Expr::StructLit { name, fields } => HirExpr::StructLit {
            name: name.clone(),
            fields: fields
                .iter()
                .map(|(n, e)| (n.clone(), lower_expr(e)))
                .collect(),
        },
        Expr::Interp(parts) => HirExpr::Interp(
            parts
                .iter()
                .map(|p| match p {
                    InterpPart::Lit(s) => HirInterpPart::Lit(s.clone()),
                    InterpPart::Expr(e) => HirInterpPart::Expr(lower_expr(e)),
                })
                .collect(),
        ),
        Expr::Lambda {
            params,
            param_types,
            return_type,
            body,
        } => HirExpr::Lambda {
            params: params
                .iter()
                .zip(param_types.iter())
                .map(|(n, t)| (n.clone(), t.clone()))
                .collect(),
            return_type: return_type.clone(),
            body: lower_stmts(body),
        },
    }
}

/// Summary stats — useful for tooling / future passes.
pub fn module_stats(m: &HirModule) -> BTreeMap<&'static str, usize> {
    let mut map = BTreeMap::new();
    map.insert("structs", m.structs.len());
    map.insert("functions", m.functions.len());
    let stmts: usize = m.functions.iter().map(|f| f.body.len()).sum();
    map.insert("top_level_stmts", stmts);
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    #[test]
    fn lowers_simple_program() {
        let prog = parser::parse_program(
            "fn add(a: Int, b: Int) -> Int { return a + b; } fn main() -> Int { return add(1, 2); }",
        )
        .unwrap();
        let hir = lower(&prog);
        assert_eq!(hir.functions.len(), 2);
        assert!(hir.functions.iter().any(|f| f.name == "main"));
        let stats = module_stats(&hir);
        assert_eq!(stats["functions"], 2);
    }
}
