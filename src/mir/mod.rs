//! Mid-level Intermediate Representation (MIR) for S+N++.
//!
//! MIR is a control-flow graph of basic blocks with explicit jumps.
//! It is the natural bridge between HIR and:
//!   - the existing stack bytecode VM
//!   - a future LLVM / Cranelift backend
//!
//! Design goals:
//! - explicit CFG (no nested statements)
//! - SSA-light: named temps, not full SSA yet
//! - small instruction set that maps 1:1 to VM ops or LLVM IR

use crate::hir::{HirExpr, HirFunction, HirInterpPart, HirModule, HirStmt};
use crate::lexer::Token;
use std::collections::BTreeMap;

/// A whole module in MIR form.
#[derive(Debug, Clone)]
pub struct MirModule {
    pub functions: Vec<MirFunction>,
}

#[derive(Debug, Clone)]
pub struct MirFunction {
    pub name: String,
    pub params: Vec<String>,
    pub blocks: Vec<MirBlock>,
    /// entry block index (always 0 for now)
    pub entry: usize,
}

#[derive(Debug, Clone)]
pub struct MirBlock {
    pub id: usize,
    pub stmts: Vec<MirInstr>,
    pub terminator: MirTerminator,
}

#[derive(Debug, Clone)]
pub enum MirTerminator {
    Return(Option<MirOperand>),
    Jump(usize),
    Branch {
        cond: MirOperand,
        then_bb: usize,
        else_bb: usize,
    },
    /// Unreachable / fallthrough placeholder
    Unreachable,
}

#[derive(Debug, Clone)]
pub enum MirOperand {
    ConstInt(i64),
    ConstBool(bool),
    ConstStr(String),
    ConstUnit,
    /// Local / temp name
    Local(String),
}

#[derive(Debug, Clone)]
pub enum MirInstr {
    /// dest = operand
    Assign {
        dest: String,
        value: MirOperand,
    },
    /// dest = a op b
    Binary {
        dest: String,
        op: Token,
        left: MirOperand,
        right: MirOperand,
    },
    /// dest = op a
    Unary {
        dest: String,
        op: Token,
        value: MirOperand,
    },
    /// dest = value?  (Result unwrap / early return Err)
    Try {
        dest: String,
        value: MirOperand,
    },
    /// dest = call name(args)
    Call {
        dest: String,
        name: String,
        args: Vec<MirOperand>,
    },
    /// dest = call_indirect callee(args)
    CallIndirect {
        dest: String,
        callee: MirOperand,
        args: Vec<MirOperand>,
    },
    /// dest = base[index]
    Index {
        dest: String,
        base: MirOperand,
        index: MirOperand,
    },
    /// dest = object.field
    Field {
        dest: String,
        object: MirOperand,
        field: String,
    },
    /// object.field = value
    SetField {
        object: MirOperand,
        field: String,
        value: MirOperand,
    },
    /// dest = StructName { fields }
    MakeStruct {
        dest: String,
        name: String,
        fields: Vec<(String, MirOperand)>,
    },
    /// dest = make_closure(name, captures)
    MakeClosure {
        dest: String,
        name: String,
        captures: Vec<(String, MirOperand)>,
    },
    /// print(value)
    Print(MirOperand),
    /// no-op placeholder for future (e.g. drop)
    MakeEnum {
        dest: String,
        enum_name: String,
        variant: String,
        args: Vec<MirOperand>,
    },
    Nop,
}

/// Lower HIR → MIR.
pub fn lower(module: &HirModule) -> Result<MirModule, String> {
    let mut functions = Vec::new();
    let mut nested_all = Vec::new();
    for f in &module.functions {
        let (func, nested) = lower_function(f)?;
        functions.push(func);
        nested_all.extend(nested);
    }
    functions.extend(nested_all);
    Ok(MirModule { functions })
}

struct LoopFrame {
    continue_bb: usize,
    break_bb: usize,
}

struct Builder {
    blocks: Vec<MirBlock>,
    current: usize,
    temp: usize,
    loops: Vec<LoopFrame>,
    error: Option<String>,
    nested: Vec<MirFunction>,
    closure_id: usize,
}

impl Builder {
    fn new() -> Self {
        Self {
            blocks: vec![MirBlock {
                id: 0,
                stmts: Vec::new(),
                terminator: MirTerminator::Unreachable,
            }],
            current: 0,
            temp: 0,
            loops: Vec::new(),
            error: None,
            nested: Vec::new(),
            closure_id: 0,
        }
    }

    fn fresh(&mut self, prefix: &str) -> String {
        let id = self.temp;
        self.temp += 1;
        format!("_{prefix}{id}")
    }

    fn push(&mut self, instr: MirInstr) {
        self.blocks[self.current].stmts.push(instr);
    }

    fn new_block(&mut self) -> usize {
        let id = self.blocks.len();
        self.blocks.push(MirBlock {
            id,
            stmts: Vec::new(),
            terminator: MirTerminator::Unreachable,
        });
        id
    }

    fn set_term(&mut self, term: MirTerminator) {
        self.blocks[self.current].terminator = term;
    }

    fn switch(&mut self, bb: usize) {
        self.current = bb;
    }
}

fn lower_function(f: &HirFunction) -> Result<(MirFunction, Vec<MirFunction>), String> {
    let mut b = Builder::new();
    lower_stmts(&f.body, &mut b);
    if let Some(e) = b.error {
        return Err(e);
    }
    if matches!(
        b.blocks[b.current].terminator,
        MirTerminator::Unreachable
    ) {
        b.set_term(MirTerminator::Return(Some(MirOperand::ConstUnit)));
    }
    Ok((
        MirFunction {
            name: f.name.clone(),
            params: f.params.iter().map(|(n, _)| n.clone()).collect(),
            blocks: b.blocks,
            entry: 0,
        },
        b.nested,
    ))
}

fn lower_stmts(xs: &[HirStmt], b: &mut Builder) {
    for s in xs {
        lower_stmt(s, b);
    }
}

fn lower_stmt(s: &HirStmt, b: &mut Builder) {
    match s {
        HirStmt::Let { name, value } | HirStmt::Assign { name, value } => {
            let op = lower_expr(value, b);
            b.push(MirInstr::Assign {
                dest: name.clone(),
                value: op,
            });
        }
        HirStmt::FieldAssign {
            object,
            field,
            value,
        } => {
            let obj = lower_expr(object, b);
            let val = lower_expr(value, b);
            b.push(MirInstr::SetField {
                object: obj,
                field: field.clone(),
                value: val,
            });
        }
        HirStmt::Expr(e) => {
            let _ = lower_expr(e, b);
        }
        HirStmt::Return(e) => {
            let op = lower_expr(e, b);
            b.set_term(MirTerminator::Return(Some(op)));
            // subsequent code goes to a fresh dead block
            let dead = b.new_block();
            b.switch(dead);
        }
        HirStmt::If {
            cond,
            then_body,
            else_body,
        } => {
            let c = lower_expr(cond, b);
            let then_bb = b.new_block();
            let else_bb = b.new_block();
            let join_bb = b.new_block();
            b.set_term(MirTerminator::Branch {
                cond: c,
                then_bb,
                else_bb,
            });

            b.switch(then_bb);
            lower_stmts(then_body, b);
            if matches!(b.blocks[b.current].terminator, MirTerminator::Unreachable) {
                b.set_term(MirTerminator::Jump(join_bb));
            }

            b.switch(else_bb);
            lower_stmts(else_body, b);
            if matches!(b.blocks[b.current].terminator, MirTerminator::Unreachable) {
                b.set_term(MirTerminator::Jump(join_bb));
            }

            b.switch(join_bb);
        }
        HirStmt::While { cond, body } => {
            let header = b.new_block();
            let body_bb = b.new_block();
            let exit = b.new_block();
            b.set_term(MirTerminator::Jump(header));

            b.switch(header);
            let c = lower_expr(cond, b);
            b.set_term(MirTerminator::Branch {
                cond: c,
                then_bb: body_bb,
                else_bb: exit,
            });

            b.switch(body_bb);
            b.loops.push(LoopFrame {
                continue_bb: header,
                break_bb: exit,
            });
            lower_stmts(body, b);
            b.loops.pop();
            if matches!(b.blocks[b.current].terminator, MirTerminator::Unreachable) {
                b.set_term(MirTerminator::Jump(header));
            }

            b.switch(exit);
        }
        HirStmt::ForRange {
            var,
            start,
            end,
            inclusive,
            body,
        } => {
            // desugar: let var = start; while var < end (or <=) { body; var = var + 1 }
            let s = lower_expr(start, b);
            b.push(MirInstr::Assign {
                dest: var.clone(),
                value: s,
            });
            let end_tmp = b.fresh("end");
            let e = lower_expr(end, b);
            b.push(MirInstr::Assign {
                dest: end_tmp.clone(),
                value: e,
            });

            let header = b.new_block();
            let body_bb = b.new_block();
            let exit = b.new_block();
            b.set_term(MirTerminator::Jump(header));

            b.switch(header);
            let cmp = b.fresh("cmp");
            let cmp_op = if *inclusive { Token::Lte } else { Token::Lt };
            b.push(MirInstr::Binary {
                dest: cmp.clone(),
                op: cmp_op,
                left: MirOperand::Local(var.clone()),
                right: MirOperand::Local(end_tmp),
            });
            b.set_term(MirTerminator::Branch {
                cond: MirOperand::Local(cmp),
                then_bb: body_bb,
                else_bb: exit,
            });

            b.switch(body_bb);
            // continue should jump to increment — use a continue block
            let cont_bb = b.new_block();
            b.loops.push(LoopFrame {
                continue_bb: cont_bb,
                break_bb: exit,
            });
            lower_stmts(body, b);
            b.loops.pop();
            if matches!(b.blocks[b.current].terminator, MirTerminator::Unreachable) {
                b.set_term(MirTerminator::Jump(cont_bb));
            }
            b.switch(cont_bb);
            let next = b.fresh("next");
            b.push(MirInstr::Binary {
                dest: next.clone(),
                op: Token::Plus,
                left: MirOperand::Local(var.clone()),
                right: MirOperand::ConstInt(1),
            });
            b.push(MirInstr::Assign {
                dest: var.clone(),
                value: MirOperand::Local(next),
            });
            b.set_term(MirTerminator::Jump(header));

            b.switch(exit);
        }
        HirStmt::ForIn { var, index, iter, body } => {
            let iter_tmp = b.fresh("iter");
            let it = lower_expr(iter, b);
            b.push(MirInstr::Assign {
                dest: iter_tmp.clone(),
                value: it,
            });
            let idx = b.fresh("idx");
            b.push(MirInstr::Assign {
                dest: idx.clone(),
                value: MirOperand::ConstInt(0),
            });
            let len_tmp = b.fresh("len");
            b.push(MirInstr::Call {
                dest: len_tmp.clone(),
                name: "__iter_len".into(),
                args: vec![MirOperand::Local(iter_tmp.clone())],
            });

            let header = b.new_block();
            let body_bb = b.new_block();
            let exit = b.new_block();
            b.set_term(MirTerminator::Jump(header));

            b.switch(header);
            let cmp = b.fresh("cmp");
            b.push(MirInstr::Binary {
                dest: cmp.clone(),
                op: Token::Lt,
                left: MirOperand::Local(idx.clone()),
                right: MirOperand::Local(len_tmp),
            });
            b.set_term(MirTerminator::Branch {
                cond: MirOperand::Local(cmp),
                then_bb: body_bb,
                else_bb: exit,
            });

            b.switch(body_bb);
            if let Some(user_idx) = index {
                b.push(MirInstr::Assign {
                    dest: user_idx.clone(),
                    value: MirOperand::Local(idx.clone()),
                });
            }
            b.push(MirInstr::Index {
                dest: var.clone(),
                base: MirOperand::Local(iter_tmp),
                index: MirOperand::Local(idx.clone()),
            });
            let cont_bb = b.new_block();
            b.loops.push(LoopFrame {
                continue_bb: cont_bb,
                break_bb: exit,
            });
            lower_stmts(body, b);
            b.loops.pop();
            if matches!(b.blocks[b.current].terminator, MirTerminator::Unreachable) {
                b.set_term(MirTerminator::Jump(cont_bb));
            }
            b.switch(cont_bb);
            let next = b.fresh("next");
            b.push(MirInstr::Binary {
                dest: next.clone(),
                op: Token::Plus,
                left: MirOperand::Local(idx.clone()),
                right: MirOperand::ConstInt(1),
            });
            b.push(MirInstr::Assign {
                dest: idx,
                value: MirOperand::Local(next),
            });
            b.set_term(MirTerminator::Jump(header));

            b.switch(exit);
        }
        HirStmt::Match { scrutinee, arms } => {
            // Simple chain: if-else style on patterns
            let scr = lower_expr(scrutinee, b);
            let scr_tmp = b.fresh("scr");
            b.push(MirInstr::Assign {
                dest: scr_tmp.clone(),
                value: scr,
            });
            let join = b.new_block();
            for (i, (pat, guard, body)) in arms.iter().enumerate() {
                let arm_bb = b.new_block();
                let next_bb = if i + 1 == arms.len() {
                    join
                } else {
                    b.new_block()
                };
                // Pattern test is simplified: always take arm for now if Var, else compare
                match pat {
                    crate::ast::Pattern::Wildcard => {
                        b.set_term(MirTerminator::Jump(arm_bb));
                    }
                    crate::ast::Pattern::Var(n) => {
                        b.push(MirInstr::Assign {
                            dest: n.clone(),
                            value: MirOperand::Local(scr_tmp.clone()),
                        });
                        b.set_term(MirTerminator::Jump(arm_bb));
                    }
                    crate::ast::Pattern::Int(v) => {
                        let cmp = b.fresh("mcmp");
                        b.push(MirInstr::Binary {
                            dest: cmp.clone(),
                            op: Token::EqEq,
                            left: MirOperand::Local(scr_tmp.clone()),
                            right: MirOperand::ConstInt(*v),
                        });
                        b.set_term(MirTerminator::Branch {
                            cond: MirOperand::Local(cmp),
                            then_bb: arm_bb,
                            else_bb: next_bb,
                        });
                    }
                    crate::ast::Pattern::Bool(v) => {
                        let cmp = b.fresh("mcmp");
                        b.push(MirInstr::Binary {
                            dest: cmp.clone(),
                            op: Token::EqEq,
                            left: MirOperand::Local(scr_tmp.clone()),
                            right: MirOperand::ConstBool(*v),
                        });
                        b.set_term(MirTerminator::Branch {
                            cond: MirOperand::Local(cmp),
                            then_bb: arm_bb,
                            else_bb: next_bb,
                        });
                    }
                    crate::ast::Pattern::Str(s) => {
                        let cmp = b.fresh("mcmp");
                        b.push(MirInstr::Binary {
                            dest: cmp.clone(),
                            op: Token::EqEq,
                            left: MirOperand::Local(scr_tmp.clone()),
                            right: MirOperand::ConstStr(s.clone()),
                        });
                        b.set_term(MirTerminator::Branch {
                            cond: MirOperand::Local(cmp),
                            then_bb: arm_bb,
                            else_bb: next_bb,
                        });
                    }
                    crate::ast::Pattern::Variant {
                        variant,
                        ..
                    } => {
                        let cmp = b.fresh("mcmp");
                        b.push(MirInstr::Call {
                            dest: cmp.clone(),
                            name: "__enum_is".into(),
                            args: vec![
                                MirOperand::Local(scr_tmp.clone()),
                                MirOperand::ConstStr(variant.clone()),
                            ],
                        });
                        b.set_term(MirTerminator::Branch {
                            cond: MirOperand::Local(cmp),
                            then_bb: arm_bb,
                            else_bb: next_bb,
                        });
                    }
                    crate::ast::Pattern::Struct { name, .. } => {
                        let cmp = b.fresh("mcmp");
                        b.push(MirInstr::Call {
                            dest: cmp.clone(),
                            name: "__struct_is".into(),
                            args: vec![
                                MirOperand::Local(scr_tmp.clone()),
                                MirOperand::ConstStr(name.clone()),
                            ],
                        });
                        b.set_term(MirTerminator::Branch {
                            cond: MirOperand::Local(cmp),
                            then_bb: arm_bb,
                            else_bb: next_bb,
                        });
                    }
                    crate::ast::Pattern::Or(_) => {
                        b.set_term(MirTerminator::Jump(next_bb));
                    }
                    crate::ast::Pattern::Tuple(_) => {
                        // structural match deferred; accept and bind in arm
                        b.set_term(MirTerminator::Jump(arm_bb));
                    }
                }
                b.switch(arm_bb);
                if let crate::ast::Pattern::Variant { bindings, .. } = pat {
                    for (i, bind) in bindings.iter().enumerate() {
                        b.push(MirInstr::Call {
                            dest: bind.clone(),
                            name: "__enum_get".into(),
                            args: vec![
                                MirOperand::Local(scr_tmp.clone()),
                                MirOperand::ConstInt(i as i64),
                            ],
                        });
                    }
                }
                if let crate::ast::Pattern::Tuple(ps) = pat {
                    for (i, p) in ps.iter().enumerate() {
                        if let crate::ast::Pattern::Var(name) = p {
                            b.push(MirInstr::Call {
                                dest: name.clone(),
                                name: "tuple_get".into(),
                                args: vec![
                                    MirOperand::Local(scr_tmp.clone()),
                                    MirOperand::ConstInt(i as i64),
                                ],
                            });
                        }
                    }
                }
                if let crate::ast::Pattern::Struct { fields, .. } = pat {
                    for (fname, bname) in fields {
                        b.push(MirInstr::Field {
                            dest: bname.clone(),
                            object: MirOperand::Local(scr_tmp.clone()),
                            field: fname.clone(),
                        });
                    }
                }
                // Optional `if` guard — fail → try next arm
                if let Some(g) = guard {
                    let body_bb = b.new_block();
                    let gop = lower_expr(g, b);
                    b.set_term(MirTerminator::Branch {
                        cond: gop,
                        then_bb: body_bb,
                        else_bb: next_bb,
                    });
                    b.switch(body_bb);
                }
                lower_stmts(body, b);
                if matches!(b.blocks[b.current].terminator, MirTerminator::Unreachable) {
                    b.set_term(MirTerminator::Jump(join));
                }
                if next_bb != join {
                    b.switch(next_bb);
                }
            }
            b.switch(join);
        }
        HirStmt::Break => {
            if let Some(frame) = b.loops.last() {
                let target = frame.break_bb;
                b.set_term(MirTerminator::Jump(target));
                let dead = b.new_block();
                b.switch(dead);
            } else {
                b.error = Some("break outside of loop".into());
            }
        }
        HirStmt::Continue => {
            if let Some(frame) = b.loops.last() {
                let target = frame.continue_bb;
                b.set_term(MirTerminator::Jump(target));
                let dead = b.new_block();
                b.switch(dead);
            } else {
                b.error = Some("continue outside of loop".into());
            }
        }
    }
}


fn hir_free_vars_expr(e: &HirExpr, bound: &std::collections::HashSet<String>, out: &mut std::collections::HashSet<String>) {
    match e {
        HirExpr::Var(n) => {
            if !bound.contains(n) {
                out.insert(n.clone());
            }
        }
        HirExpr::Binary(a, _, b) => {
            hir_free_vars_expr(a, bound, out);
            hir_free_vars_expr(b, bound, out);
        }
        HirExpr::Tuple(items) => {
            for it in items {
                hir_free_vars_expr(it, bound, out);
            }
        }
        HirExpr::Hole => {}
        HirExpr::IfExpr {
            cond,
            then_branch,
            else_branch,
        } => {
            hir_free_vars_expr(cond, bound, out);
            hir_free_vars_expr(then_branch, bound, out);
            hir_free_vars_expr(else_branch, bound, out);
        }
        HirExpr::Unary(_, v) | HirExpr::Try(v) | HirExpr::Field { object: v, .. } => {
            hir_free_vars_expr(v, bound, out);
        }
        HirExpr::Call { args, .. } => {
            for a in args {
                hir_free_vars_expr(a, bound, out);
            }
        }
        HirExpr::CallExpr { callee, args } => {
            hir_free_vars_expr(callee, bound, out);
            for a in args {
                hir_free_vars_expr(a, bound, out);
            }
        }
        HirExpr::Index { base, index } => {
            hir_free_vars_expr(base, bound, out);
            hir_free_vars_expr(index, bound, out);
        }
        HirExpr::StructLit { fields, .. } => {
            for (_, e) in fields {
                hir_free_vars_expr(e, bound, out);
            }
        }
        HirExpr::Interp(parts) => {
            for p in parts {
                if let HirInterpPart::Expr(e) = p {
                    hir_free_vars_expr(e, bound, out);
                }
            }
        }
        HirExpr::Lambda { params, body, .. } => {
            let mut inner = bound.clone();
            for (n, _) in params {
                inner.insert(n.clone());
            }
            hir_free_vars_stmts(body, &inner, out);
        }
        HirExpr::VariantLit { args, .. } => {
            for a in args {
                hir_free_vars_expr(a, bound, out);
            }
        }
        HirExpr::Literal(_) => {}
    }
}

fn hir_free_vars_stmts(xs: &[HirStmt], bound: &std::collections::HashSet<String>, out: &mut std::collections::HashSet<String>) {
    let mut bound = bound.clone();
    for s in xs {
        match s {
            HirStmt::Let { name, value } => {
                hir_free_vars_expr(value, &bound, out);
                bound.insert(name.clone());
            }
            HirStmt::Assign { value, .. } | HirStmt::Expr(value) | HirStmt::Return(value) => {
                hir_free_vars_expr(value, &bound, out);
            }
            HirStmt::FieldAssign { object, value, .. } => {
                hir_free_vars_expr(object, &bound, out);
                hir_free_vars_expr(value, &bound, out);
            }
            HirStmt::If { cond, then_body, else_body } => {
                hir_free_vars_expr(cond, &bound, out);
                hir_free_vars_stmts(then_body, &bound, out);
                hir_free_vars_stmts(else_body, &bound, out);
            }
            HirStmt::While { cond, body } => {
                hir_free_vars_expr(cond, &bound, out);
                hir_free_vars_stmts(body, &bound, out);
            }
            HirStmt::ForRange { var, start, end, body, .. } => {
                hir_free_vars_expr(start, &bound, out);
                hir_free_vars_expr(end, &bound, out);
                let mut b2 = bound.clone();
                b2.insert(var.clone());
                hir_free_vars_stmts(body, &b2, out);
            }
            HirStmt::ForIn { var, index, iter, body } => {
                hir_free_vars_expr(iter, &bound, out);
                let mut b2 = bound.clone();
                b2.insert(var.clone());
                if let Some(i) = index {
                    b2.insert(i.clone());
                }
                hir_free_vars_stmts(body, &b2, out);
            }
            HirStmt::Match { scrutinee, arms } => {
                hir_free_vars_expr(scrutinee, &bound, out);
                for (pat, guard, body) in arms {
                    let mut b2 = bound.clone();
                    if let crate::ast::Pattern::Var(n) = pat {
                        b2.insert(n.clone());
                    }
                    if let crate::ast::Pattern::Variant { bindings, .. } = pat {
                        for bn in bindings {
                            b2.insert(bn.clone());
                        }
                    }
                    if let crate::ast::Pattern::Struct { fields, .. } = pat {
                        for (_, bn) in fields {
                            b2.insert(bn.clone());
                        }
                    }
                    if let Some(g) = guard {
                        hir_free_vars_expr(g, &b2, out);
                    }
                    hir_free_vars_stmts(body, &b2, out);
                }
            }
            HirStmt::Break | HirStmt::Continue => {}
        }
    }
}

fn lower_expr(e: &HirExpr, b: &mut Builder) -> MirOperand {
    match e {
        HirExpr::Literal(v) => match v {
            crate::ast::Value::Int(n) => MirOperand::ConstInt(*n),
            crate::ast::Value::Bool(x) => MirOperand::ConstBool(*x),
            crate::ast::Value::Str(s) => MirOperand::ConstStr(s.clone()),
            crate::ast::Value::Unit => MirOperand::ConstUnit,
            other => {
                // Complex literals: assign via temp after stringifying isn't needed;
                // store as local by printing path — use Unit fallback for exotic values.
                let dest = b.fresh("lit");
                let _ = other;
                b.push(MirInstr::Assign {
                    dest: dest.clone(),
                    value: MirOperand::ConstUnit,
                });
                MirOperand::Local(dest)
            }
        },
        HirExpr::Var(n) => MirOperand::Local(n.clone()),
        HirExpr::Binary(a, op, right) if matches!(op, Token::AmpAmp | Token::PipePipe) => {
            // Short-circuit: a && b / a || b
            let left = lower_expr(a, b);
            let dest = b.fresh("logic");
            let then_bb = b.new_block();
            let else_bb = b.new_block();
            let join = b.new_block();
            if matches!(op, Token::AmpAmp) {
                // if left then right else false
                b.set_term(MirTerminator::Branch {
                    cond: left,
                    then_bb,
                    else_bb,
                });
                b.switch(then_bb);
                let r = lower_expr(right, b);
                b.push(MirInstr::Assign {
                    dest: dest.clone(),
                    value: r,
                });
                b.set_term(MirTerminator::Jump(join));
                b.switch(else_bb);
                b.push(MirInstr::Assign {
                    dest: dest.clone(),
                    value: MirOperand::ConstBool(false),
                });
                b.set_term(MirTerminator::Jump(join));
            } else {
                // if left then true else right
                b.set_term(MirTerminator::Branch {
                    cond: left,
                    then_bb,
                    else_bb,
                });
                b.switch(then_bb);
                b.push(MirInstr::Assign {
                    dest: dest.clone(),
                    value: MirOperand::ConstBool(true),
                });
                b.set_term(MirTerminator::Jump(join));
                b.switch(else_bb);
                let r = lower_expr(right, b);
                b.push(MirInstr::Assign {
                    dest: dest.clone(),
                    value: r,
                });
                b.set_term(MirTerminator::Jump(join));
            }
            b.switch(join);
            MirOperand::Local(dest)
        }
        HirExpr::Binary(a, op, right) => {
            let left = lower_expr(a, b);
            let right = lower_expr(right, b);
            let dest = b.fresh("bin");
            b.push(MirInstr::Binary {
                dest: dest.clone(),
                op: op.clone(),
                left,
                right,
            });
            MirOperand::Local(dest)
        }
        HirExpr::Tuple(items) => {
            let mut parts = Vec::new();
            for it in items {
                parts.push(lower_expr(it, b));
            }
            let dest = b.fresh("tup");
            // encode as MakeStruct-like via Call __tuple_make
            b.push(MirInstr::Call {
                dest: dest.clone(),
                name: "__tuple_make".into(),
                args: parts,
            });
            MirOperand::Local(dest)
        }
        HirExpr::Hole => MirOperand::ConstUnit,
        HirExpr::IfExpr {
            cond,
            then_branch,
            else_branch,
        } => {
            let c = lower_expr(cond, b);
            let dest = b.fresh("ifx");
            let then_bb = b.new_block();
            let else_bb = b.new_block();
            let join = b.new_block();
            b.set_term(MirTerminator::Branch {
                cond: c,
                then_bb,
                else_bb,
            });
            b.switch(then_bb);
            let t = lower_expr(then_branch, b);
            b.push(MirInstr::Assign {
                dest: dest.clone(),
                value: t,
            });
            b.set_term(MirTerminator::Jump(join));
            b.switch(else_bb);
            let e = lower_expr(else_branch, b);
            b.push(MirInstr::Assign {
                dest: dest.clone(),
                value: e,
            });
            b.set_term(MirTerminator::Jump(join));
            b.switch(join);
            MirOperand::Local(dest)
        }
        HirExpr::Tuple(items) => {
            let mut parts = Vec::new();
            for it in items {
                parts.push(lower_expr(it, b));
            }
            let dest = b.fresh("tup");
            b.push(MirInstr::Call {
                dest: dest.clone(),
                name: "__tuple_make".into(),
                args: parts,
            });
            MirOperand::Local(dest)
        }
        HirExpr::Hole => MirOperand::ConstUnit,
        HirExpr::Try(v) => {
            let src = lower_expr(v, b);
            let dest = b.fresh("try");
            b.push(MirInstr::Try {
                dest: dest.clone(),
                value: src,
            });
            MirOperand::Local(dest)
        }
        HirExpr::Unary(op, v) => {
            let value = lower_expr(v, b);
            let dest = b.fresh("un");
            b.push(MirInstr::Unary {
                dest: dest.clone(),
                op: op.clone(),
                value,
            });
            MirOperand::Local(dest)
        }
        HirExpr::Call { name, args } => {
            let args: Vec<_> = args.iter().map(|a| lower_expr(a, b)).collect();
            // print is special: no dest
            if name == "print" && args.len() == 1 {
                b.push(MirInstr::Print(args[0].clone()));
                return MirOperand::ConstUnit;
            }
            let dest = b.fresh("call");
            b.push(MirInstr::Call {
                dest: dest.clone(),
                name: name.clone(),
                args,
            });
            MirOperand::Local(dest)
        }
        HirExpr::CallExpr { callee, args } => {
            let callee = lower_expr(callee, b);
            let args: Vec<_> = args.iter().map(|a| lower_expr(a, b)).collect();
            let dest = b.fresh("icall");
            b.push(MirInstr::CallIndirect {
                dest: dest.clone(),
                callee,
                args,
            });
            MirOperand::Local(dest)
        }
        HirExpr::Index { base, index } => {
            let base = lower_expr(base, b);
            let index = lower_expr(index, b);
            let dest = b.fresh("idx");
            b.push(MirInstr::Index {
                dest: dest.clone(),
                base,
                index,
            });
            MirOperand::Local(dest)
        }
        HirExpr::Field { object, field } => {
            let object = lower_expr(object, b);
            let dest = b.fresh("fld");
            b.push(MirInstr::Field {
                dest: dest.clone(),
                object,
                field: field.clone(),
            });
            MirOperand::Local(dest)
        }
        HirExpr::VariantLit {
            enum_name,
            variant,
            args,
        } => {
            let arg_ops: Vec<MirOperand> = args.iter().map(|a| lower_expr(a, b)).collect();
            let dest = b.fresh("enum");
            b.push(MirInstr::MakeEnum {
                dest: dest.clone(),
                enum_name: enum_name.clone(),
                variant: variant.clone(),
                args: arg_ops,
            });
            MirOperand::Local(dest)
        }
        HirExpr::StructLit { name, fields } => {
            let fields: Vec<_> = fields
                .iter()
                .map(|(n, e)| (n.clone(), lower_expr(e, b)))
                .collect();
            let dest = b.fresh("st");
            b.push(MirInstr::MakeStruct {
                dest: dest.clone(),
                name: name.clone(),
                fields,
            });
            MirOperand::Local(dest)
        }
        HirExpr::Interp(parts) => {
            // Lower to successive concatenations
            let mut acc: Option<MirOperand> = None;
            for p in parts {
                let piece = match p {
                    HirInterpPart::Lit(s) => MirOperand::ConstStr(s.clone()),
                    HirInterpPart::Expr(e) => {
                        let v = lower_expr(e, b);
                        let dest = b.fresh("itoa");
                        b.push(MirInstr::Call {
                            dest: dest.clone(),
                            name: "__to_string".into(),
                            args: vec![v],
                        });
                        MirOperand::Local(dest)
                    }
                };
                acc = Some(match acc {
                    None => piece,
                    Some(prev) => {
                        let dest = b.fresh("cat");
                        b.push(MirInstr::Binary {
                            dest: dest.clone(),
                            op: Token::Plus,
                            left: prev,
                            right: piece,
                        });
                        MirOperand::Local(dest)
                    }
                });
            }
            acc.unwrap_or(MirOperand::ConstStr(String::new()))
        }
        HirExpr::Lambda {
            params, body, ..
        } => {
            // Free-variable analysis: used in body but not parameters
            let mut bound = std::collections::HashSet::new();
            for (n, _) in params {
                bound.insert(n.clone());
            }
            let mut free = std::collections::HashSet::new();
            hir_free_vars_stmts(body, &bound, &mut free);
            let mut caps: Vec<String> = free.into_iter().collect();
            caps.sort();

            let id = b.closure_id;
            b.closure_id += 1;
            let cname = format!("__closure_{id}");

            // Lower closure body as its own MirFunction
            let mut inner = Builder::new();
            inner.closure_id = b.closure_id;
            lower_stmts(body, &mut inner);
            if let Some(e) = inner.error.take() {
                b.error = Some(e);
            }
            if matches!(
                inner.blocks[inner.current].terminator,
                MirTerminator::Unreachable
            ) {
                inner.set_term(MirTerminator::Return(Some(MirOperand::ConstUnit)));
            }
            // Nested closures discovered inside this lambda
            b.closure_id = inner.closure_id;
            b.nested.extend(inner.nested);
            b.nested.push(MirFunction {
                name: cname.clone(),
                params: params.iter().map(|(n, _)| n.clone()).collect(),
                blocks: inner.blocks,
                entry: 0,
            });

            let dest = b.fresh("lam");
            let capture_ops: Vec<(String, MirOperand)> = caps
                .iter()
                .map(|n| (n.clone(), MirOperand::Local(n.clone())))
                .collect();
            b.push(MirInstr::MakeClosure {
                dest: dest.clone(),
                name: cname,
                captures: capture_ops,
            });
            MirOperand::Local(dest)
        }
    }
}

/// Module statistics for tooling.
pub fn module_stats(m: &MirModule) -> BTreeMap<&'static str, usize> {
    let mut map = BTreeMap::new();
    map.insert("functions", m.functions.len());
    let blocks: usize = m.functions.iter().map(|f| f.blocks.len()).sum();
    map.insert("blocks", blocks);
    let instrs: usize = m
        .functions
        .iter()
        .flat_map(|f| f.blocks.iter())
        .map(|bb| bb.stmts.len())
        .sum();
    map.insert("instructions", instrs);
    map
}


/// Light MIR optimizations (safe, local).
pub fn optimize(module: &mut MirModule) {
    for f in &mut module.functions {
        const_fold_function(f);
        jump_thread_function(f);
        mark_dead_blocks(f);
    }
}

/// Jump threading: Jump(B) where B is empty and ends with Jump(T) → Jump(T).
fn jump_thread_function(f: &mut MirFunction) {
    // Build map: block id → terminator target if block is a pure jump
    for _pass in 0..4 {
        let mut changed = false;
        let pure_jump: Vec<Option<usize>> = f
            .blocks
            .iter()
            .map(|bb| {
                if bb.stmts.is_empty() {
                    if let MirTerminator::Jump(t) = bb.terminator {
                        return Some(t);
                    }
                }
                None
            })
            .collect();
        for bb in &mut f.blocks {
            match &mut bb.terminator {
                MirTerminator::Jump(t) => {
                    if let Some(Some(t2)) = pure_jump.get(*t) {
                        if *t2 != *t {
                            *t = *t2;
                            changed = true;
                        }
                    }
                }
                MirTerminator::Branch {
                    then_bb, else_bb, ..
                } => {
                    if let Some(Some(t2)) = pure_jump.get(*then_bb) {
                        if *t2 != *then_bb {
                            *then_bb = *t2;
                            changed = true;
                        }
                    }
                    if let Some(Some(t2)) = pure_jump.get(*else_bb) {
                        if *t2 != *else_bb {
                            *else_bb = *t2;
                            changed = true;
                        }
                    }
                }
                _ => {}
            }
        }
        if !changed {
            break;
        }
    }
}

fn const_fold_function(f: &mut MirFunction) {
    for bb in &mut f.blocks {
        let mut i = 0;
        while i < bb.stmts.len() {
            let folded = match &bb.stmts[i] {
                MirInstr::Binary {
                    dest,
                    op,
                    left,
                    right,
                } => fold_binary(dest, op, left, right),
                MirInstr::Unary { dest, op, value } => fold_unary(dest, op, value),
                _ => None,
            };
            if let Some(instr) = folded {
                bb.stmts[i] = instr;
            }
            i += 1;
        }
    }
}

fn fold_binary(
    dest: &str,
    op: &Token,
    left: &MirOperand,
    right: &MirOperand,
) -> Option<MirInstr> {
    match (left, right) {
        (MirOperand::ConstInt(a), MirOperand::ConstInt(b)) => {
            let v = match op {
                Token::Plus => a.wrapping_add(*b),
                Token::Minus => a.wrapping_sub(*b),
                Token::Star => a.wrapping_mul(*b),
                Token::Slash if *b != 0 => a / b,
                Token::Slash => return None,
                Token::EqEq => {
                    return Some(MirInstr::Assign {
                        dest: dest.to_string(),
                        value: MirOperand::ConstBool(a == b),
                    });
                }
                Token::BangEq => {
                    return Some(MirInstr::Assign {
                        dest: dest.to_string(),
                        value: MirOperand::ConstBool(a != b),
                    });
                }
                Token::Lt => {
                    return Some(MirInstr::Assign {
                        dest: dest.to_string(),
                        value: MirOperand::ConstBool(a < b),
                    });
                }
                Token::Lte => {
                    return Some(MirInstr::Assign {
                        dest: dest.to_string(),
                        value: MirOperand::ConstBool(a <= b),
                    });
                }
                Token::Gt => {
                    return Some(MirInstr::Assign {
                        dest: dest.to_string(),
                        value: MirOperand::ConstBool(a > b),
                    });
                }
                Token::Gte => {
                    return Some(MirInstr::Assign {
                        dest: dest.to_string(),
                        value: MirOperand::ConstBool(a >= b),
                    });
                }
                _ => return None,
            };
            Some(MirInstr::Assign {
                dest: dest.to_string(),
                value: MirOperand::ConstInt(v),
            })
        }
        (MirOperand::ConstBool(a), MirOperand::ConstBool(b)) => match op {
            Token::EqEq => Some(MirInstr::Assign {
                dest: dest.to_string(),
                value: MirOperand::ConstBool(a == b),
            }),
            Token::BangEq => Some(MirInstr::Assign {
                dest: dest.to_string(),
                value: MirOperand::ConstBool(a != b),
            }),
            _ => None,
        },
        _ => None,
    }
}

fn fold_unary(dest: &str, op: &Token, value: &MirOperand) -> Option<MirInstr> {
    match (op, value) {
        (Token::Minus, MirOperand::ConstInt(n)) => Some(MirInstr::Assign {
            dest: dest.to_string(),
            value: MirOperand::ConstInt(n.wrapping_neg()),
        }),
        (Token::Bang, MirOperand::ConstBool(b)) => Some(MirInstr::Assign {
            dest: dest.to_string(),
            value: MirOperand::ConstBool(!b),
        }),
        (Token::Bang, MirOperand::ConstInt(n)) => Some(MirInstr::Assign {
            dest: dest.to_string(),
            value: MirOperand::ConstBool(*n == 0),
        }),
        _ => None,
    }
}

/// Clear unreachable blocks (keep ids stable for jump targets).
fn mark_dead_blocks(f: &mut MirFunction) {
    let mut reach = vec![false; f.blocks.len()];
    if f.entry < reach.len() {
        let mut stack = vec![f.entry];
        while let Some(id) = stack.pop() {
            if id >= reach.len() || reach[id] {
                continue;
            }
            reach[id] = true;
            match &f.blocks[id].terminator {
                MirTerminator::Jump(t) => stack.push(*t),
                MirTerminator::Branch {
                    then_bb, else_bb, ..
                } => {
                    stack.push(*then_bb);
                    stack.push(*else_bb);
                }
                MirTerminator::Return(_) | MirTerminator::Unreachable => {}
            }
        }
    }
    for (i, bb) in f.blocks.iter_mut().enumerate() {
        if !reach[i] {
            bb.stmts.clear();
            bb.terminator = MirTerminator::Unreachable;
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir;
    use crate::parser;

    #[test]
    fn jump_thread_collapses() {
        // smoke: optimize does not panic on simple program
        let prog = crate::parser::parse_program("fn main() -> Int { if true { return 1; } else { return 0; } }").unwrap();
        let h = crate::hir::lower(&prog);
        let mut m = lower(&h).unwrap();
        optimize(&mut m);
        assert!(!m.functions.is_empty());
    }

    #[test]
    fn optimizes_const_fold() {
        let prog = parser::parse_program("fn main() -> Int { return 20 + 22; }").unwrap();
        let h = hir::lower(&prog);
        let mut m = lower(&h).unwrap();
        optimize(&mut m);
        // After fold, binary becomes assign of const 42 somewhere in entry
        let has_const = m.functions[0].blocks.iter().any(|bb| {
            bb.stmts.iter().any(|s| matches!(
                s,
                MirInstr::Assign { value: MirOperand::ConstInt(42), .. }
            ))
        });
        assert!(has_const, "expected const-fold of 20+22 to 42");
    }

    #[test]
    fn lowers_if_to_cfg() {
        let prog = parser::parse_program(
            "fn main() -> Int { if 1 < 2 { return 1; } else { return 0; } }",
        )
        .unwrap();
        let h = hir::lower(&prog);
        let m = lower(&h).unwrap();
        assert_eq!(m.functions.len(), 1);
        assert!(m.functions[0].blocks.len() >= 3);
        let stats = module_stats(&m);
        assert!(stats["blocks"] >= 3);
    }

    #[test]
    fn lowers_while_loop() {
        let prog = parser::parse_program(
            "fn main() -> Int { let i = 0; while i < 3 { i = i + 1; } return i; }",
        )
        .unwrap();
        let h = hir::lower(&prog);
        let m = lower(&h).unwrap();
        assert!(m.functions[0].blocks.len() >= 3);
    }
}
