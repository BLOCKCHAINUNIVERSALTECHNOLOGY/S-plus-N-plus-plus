//! S+N++ static type, ownership and definite-assignment checker.

use crate::ast::{EnumDef, Expr, Function, InterpPart, Pattern, Program, Stmt, StructDef, Type, TraitDef, Visibility};
use crate::lexer::Token;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

thread_local! {
    static ENUMS: RefCell<HashMap<String, EnumDef>> = RefCell::new(HashMap::new());
    static CURRENT_MODULE: RefCell<Vec<String>> = RefCell::new(Vec::new());
    static FN_META: RefCell<HashMap<String, (crate::ast::Visibility, Vec<String>)>> =
        RefCell::new(HashMap::new());
}

fn with_enums<R>(f: impl FnOnce(&HashMap<String, EnumDef>) -> R) -> R {
    ENUMS.with(|e| f(&e.borrow()))
}

fn type_name(t: &Type) -> String {
    t.name()
}

fn expr_type(
    x: &Expr,
    env: &HashMap<String, Type>,
    sigs: &HashMap<String, (Vec<Type>, Type)>,
    structs: &HashMap<String, StructDef>,
) -> Result<Type, String> {
    match x {
        Expr::Int(_) => Ok(Type::Int),
        Expr::Str(_) => Ok(Type::Str),
        Expr::Bool(_) => Ok(Type::Bool),
        Expr::Var(n) => env
            .get(n)
            .cloned()
            .ok_or_else(|| format!("undefined variable `{n}`")),
        Expr::Call(n, args) => {
            let builtins = [
                "print", "spawn", "join", "channel", "send", "receive",
                "str_len", "str_contains", "str_upper", "str_lower", "str_trim", "str_concat",
                "list", "list_push", "list_len", "list_get", "list_set", "list_concat",
                "map", "map_set", "map_get", "map_has",
                "abs", "min", "max", "pow", "sqrt_approx",
                "now_ms", "rand", "rand_range",
                "fs_read_text", "fs_write_text", "fs_exists",
                "json_stringify", "json_parse",
                "assert_eq", "assert_true", "assert_false",
            ];
            if builtins.contains(&n.as_str()) {
                for arg in args {
                    expr_type(arg, env, sigs, structs)?;
                }
                return Ok(match n.as_str() {
                    "spawn" => Type::Thread,
                    "channel" => Type::Channel,
                    "receive" | "list_get" | "map_get" | "rand" | "rand_range" | "now_ms" => Type::Unknown,
                    "str_len" | "list_len" | "abs" | "min" | "max" | "pow" | "sqrt_approx" | "host_strlen" => Type::Int,
                    "str_contains" | "map_has" | "fs_exists" => Type::Bool,
                    "str_upper" | "str_lower" | "str_trim" | "str_concat" | "fs_read_text" | "json_stringify" => Type::Str,
                    "list" | "list_push" | "list_set" | "list_concat" => Type::List,
                    "map" | "map_set" => Type::Map,
                    "json_parse" => Type::Unknown,
                    "fs_write_text" | "assert_true" | "assert_false" | "assert_eq" => Type::Unit,
                    _ => Type::Unit,
                });
            }
            let (params, ret) = if let Some(s) = sigs.get(n) {
                // Visibility: private items only callable from same module path
                FN_META.with(|meta| {
                    if let Some((vis, fpath)) = meta.borrow().get(n) {
                        if *vis == crate::ast::Visibility::Private {
                            let cur = CURRENT_MODULE.with(|c| c.borrow().clone());
                            if &cur != fpath {
                                return Err(format!(
                                    "function `{n}` is private to its module"
                                ));
                            }
                        }
                    }
                    Ok(())
                })?;
                s
            } else if env.contains_key(n) {
                // likely a closure local — accept any args, return Unknown
                for arg in args {
                    expr_type(arg, env, sigs, structs)?;
                }
                return Ok(Type::Unknown);
            } else if let Some((base, variant)) = n.split_once('.') {
                // Enum.Variant(args) sugar before desugar
                let is_enum = with_enums(|enums| enums.contains_key(base));
                if is_enum {
                    let lit = Expr::VariantLit {
                        enum_name: base.to_string(),
                        variant: variant.to_string(),
                        args: args.clone(),
                    };
                    return expr_type(&lit, env, sigs, structs);
                }
                if let Some(Type::Struct(type_name)) = env.get(base) {
                    let mut method_args = vec![Expr::Var(base.to_string())];
                    method_args.extend(args.iter().cloned());
                    return expr_type(
                        &Expr::Call(format!("{type_name}.{variant}"), method_args),
                        env,
                        sigs,
                        structs,
                    );
                }
                return Err(format!("unknown function `{n}`"));
            } else {
                return Err(format!("unknown function `{n}`"));
            };
            if args.len() != params.len() {
                return Err(format!(
                    "function `{n}` expects {} argument(s), got {}",
                    params.len(),
                    args.len()
                ));
            }
            for (i, arg) in args.iter().enumerate() {
                let got = expr_type(arg, env, sigs, structs)?;
                let compatible = match &params[i] {
                    Type::Ref(inner) | Type::MutRef(inner) => got == **inner,
                    Type::Generic(_) => true,
                    _ => got == params[i] || got == Type::Unknown || params[i] == Type::Unknown,
                };
                if !compatible {
                    return Err(format!(
                        "function `{n}` argument {} expects {}, got {}",
                        i + 1,
                        type_name(&params[i]),
                        type_name(&got)
                    ));
                }
            }
            Ok(ret.clone())
        }
        Expr::Unary(op, e) => {
            let t = expr_type(e, env, sigs, structs)?;
            match op {
                Token::Minus => {
                    if t == Type::Int || t == Type::Unknown {
                        Ok(Type::Int)
                    } else {
                        Err(format!("unary - expects Int, got {}", type_name(&t)))
                    }
                }
                Token::Bang => {
                    if t == Type::Bool || t == Type::Unknown {
                        Ok(Type::Bool)
                    } else {
                        Err(format!("unary ! expects Bool, got {}", type_name(&t)))
                    }
                }
                _ => Err("invalid unary operator".into()),
            }
        }
        Expr::Index(base, idx) => {
            let bt = expr_type(base, env, sigs, structs)?;
            let it = expr_type(idx, env, sigs, structs)?;
            match bt {
                Type::List | Type::ListOf(_) => {
                    if it == Type::Int || it == Type::Unknown {
                        Ok(Type::Unknown)
                    } else {
                        Err(format!("list index expects Int, got {}", type_name(&it)))
                    }
                }
                Type::Map | Type::MapOf(_, _) => {
                    if it == Type::Str || it == Type::Unknown {
                        Ok(Type::Unknown)
                    } else {
                        Err(format!("map key expects String, got {}", type_name(&it)))
                    }
                }
                _ => Err(format!(
                    "indexing requires List or Map, got {}",
                    type_name(&bt)
                )),
            }
        }
        Expr::Field(obj, field) => {
            // Unit enum variant: `Option.None` as Field(Var("Option"), "None")
            if let Expr::Var(ename) = obj.as_ref() {
                let unit = with_enums(|enums| {
                    if let Some(def) = enums.get(ename) {
                        if let Some((_, payload)) =
                            def.variants.iter().find(|(n, _)| n == field)
                        {
                            if payload.is_empty() {
                                return Some(Type::Enum(ename.clone()));
                            }
                        }
                    }
                    None
                });
                if let Some(t) = unit {
                    return Ok(t);
                }
            }
            let ot = expr_type(obj, env, sigs, structs)?;
            match ot {
                Type::Struct(name) => {
                    let def = structs
                        .get(&name)
                        .ok_or_else(|| format!("unknown struct type `{name}`"))?;
                    def.fields
                        .iter()
                        .find(|(f, _)| f == field)
                        .map(|(_, t)| t.clone())
                        .ok_or_else(|| format!("struct `{name}` has no field `{field}`"))
                }
                Type::Unknown => Ok(Type::Unknown),
                other => Err(format!(
                    "field access requires struct, got {}",
                    type_name(&other)
                )),
            }
        }
        Expr::StructLit { name, fields } => {
            if let Some(def) = structs.get(name) {
                for (fname, fexpr) in fields {
                    let got = expr_type(fexpr, env, sigs, structs)?;
                    if let Some((_, expected)) = def.fields.iter().find(|(f, _)| f == fname) {
                        if got != *expected && got != Type::Unknown && *expected != Type::Unknown {
                            return Err(format!(
                                "field `{fname}` expects {}, got {}",
                                type_name(expected),
                                type_name(&got)
                            ));
                        }
                    } else {
                        return Err(format!("struct `{name}` has no field `{fname}`"));
                    }
                }
            }
            Ok(Type::Struct(name.clone()))
        }
        Expr::Interp(parts) => {
            for p in parts {
                if let InterpPart::Expr(e) = p {
                    expr_type(e, env, sigs, structs)?;
                }
            }
            Ok(Type::Str)
        }
        Expr::Lambda { params, param_types, return_type, body } => {
            let mut lenv = env.clone();
            for (p, t) in params.iter().zip(param_types.iter()) {
                lenv.insert(p.clone(), t.clone());
            }
            // shallow check body
            for s in body {
                if let Stmt::Return(e) = s {
                    let got = expr_type(e, &lenv, sigs, structs)?;
                    if got != *return_type && got != Type::Unknown && *return_type != Type::Unknown {
                        return Err(format!(
                            "lambda return type mismatch: expected {}, got {}",
                            type_name(return_type),
                            type_name(&got)
                        ));
                    }
                }
            }
            Ok(Type::Unknown) // function type
        }
        Expr::CallExpr(callee, args) => {
            expr_type(callee, env, sigs, structs)?;
            for a in args {
                expr_type(a, env, sigs, structs)?;
            }
            Ok(Type::Unknown)
        }
        Expr::VariantLit {
            enum_name,
            variant,
            args,
        } => with_enums(|enums| {
            let def = enums
                .get(enum_name)
                .ok_or_else(|| format!("unknown enum `{enum_name}`"))?;
            let payload = def
                .variants
                .iter()
                .find(|(n, _)| n == variant)
                .ok_or_else(|| format!("enum `{enum_name}` has no variant `{variant}`"))?;
            if args.len() != payload.1.len() {
                return Err(format!(
                    "variant `{enum_name}.{variant}` expects {} field(s), got {}",
                    payload.1.len(),
                    args.len()
                ));
            }
            for (a, t) in args.iter().zip(payload.1.iter()) {
                let got = expr_type(a, env, sigs, structs)?;
                if got != *t && got != Type::Unknown && *t != Type::Unknown {
                    return Err(format!(
                        "variant field type mismatch: expected {}, got {}",
                        type_name(t),
                        type_name(&got)
                    ));
                }
            }
            Ok(Type::Enum(enum_name.clone()))
        }),
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let recv_ty = expr_type(receiver, env, sigs, structs)?;
            let type_name_s = match &recv_ty {
                Type::Struct(n) => n.clone(),
                other => {
                    return Err(format!(
                        "method `{method}` requires struct receiver, got {}",
                        type_name(other)
                    ));
                }
            };
            let full = format!("{type_name_s}.{method}");
            let mut full_args = vec![(*receiver.as_ref()).clone()];
            full_args.extend(args.iter().cloned());
            expr_type(&Expr::Call(full, full_args), env, sigs, structs)
        }

        Expr::Binary(a, op, b) => {
            let left = expr_type(a, env, sigs, structs)?;
            let right = expr_type(b, env, sigs, structs)?;
            match op {
                Token::Plus => {
                    if left == right && (left == Type::Int || left == Type::Str) {
                        Ok(left)
                    } else if left == Type::Unknown || right == Type::Unknown {
                        Ok(Type::Unknown)
                    } else {
                        Err(format!(
                            "operator + expects two Int or two String, got {} and {}",
                            type_name(&left),
                            type_name(&right)
                        ))
                    }
                }
                Token::Minus | Token::Star | Token::Slash => {
                    if (left == Type::Int || left == Type::Unknown)
                        && (right == Type::Int || right == Type::Unknown)
                    {
                        Ok(Type::Int)
                    } else {
                        Err(format!(
                            "arithmetic expects Int, got {} and {}",
                            type_name(&left),
                            type_name(&right)
                        ))
                    }
                }
                Token::EqEq | Token::BangEq => Ok(Type::Bool),
                Token::Lt | Token::Lte | Token::Gt | Token::Gte => {
                    if (left == Type::Int || left == Type::Unknown)
                        && (right == Type::Int || right == Type::Unknown)
                    {
                        Ok(Type::Bool)
                    } else {
                        Err(format!(
                            "comparison expects Int, got {} and {}",
                            type_name(&left),
                            type_name(&right)
                        ))
                    }
                }
                _ => Err("invalid binary operator".into()),
            }
        }
    }
}

fn check_typed_stmts(
    xs: &[Stmt],
    env: &mut HashMap<String, Type>,
    sigs: &HashMap<String, (Vec<Type>, Type)>,
    ret: &Type,
    structs: &HashMap<String, StructDef>,
) -> Result<(), String> {
    for statement in xs {
        match statement {
            Stmt::Let(name, expr) => {
                let ty = expr_type(expr, env, sigs, structs)?;
                env.insert(name.clone(), ty);
            }
            Stmt::Decl(name, ty) => {
                env.insert(name.clone(), ty.clone());
            }
            Stmt::Assign(name, expr) => {
                let got = expr_type(expr, env, sigs, structs)?;
                let old = env
                    .get(name)
                    .ok_or_else(|| format!("assignment to undefined variable `{name}`"))?;
                if *old != got && got != Type::Unknown && *old != Type::Unknown {
                    return Err(format!(
                        "cannot assign {} to `{name}` of type {}",
                        type_name(&got),
                        type_name(old)
                    ));
                }
            }
            Stmt::FieldAssign { object, field, value } => {
                let _ = expr_type(object, env, sigs, structs)?;
                let _ = expr_type(value, env, sigs, structs)?;
                let _ = field;
            }
            Stmt::Expr(expr) => {
                expr_type(expr, env, sigs, structs)?;
            }
            Stmt::Return(expr) => {
                let got = expr_type(expr, env, sigs, structs)?;
                if *ret != Type::Unit && got != *ret && got != Type::Unknown && *ret != Type::Unknown
                {
                    return Err(format!(
                        "return type mismatch: expected {}, got {}",
                        type_name(ret),
                        type_name(&got)
                    ));
                }
            }
            Stmt::If(condition, yes, no) => {
                if expr_type(condition, env, sigs, structs)? != Type::Bool
                    && expr_type(condition, env, sigs, structs)? != Type::Unknown
                {
                    // re-check once
                }
                let ct = expr_type(condition, env, sigs, structs)?;
                if ct != Type::Bool && ct != Type::Unknown {
                    return Err("if condition must be Bool".into());
                }
                check_typed_stmts(yes, &mut env.clone(), sigs, ret, structs)?;
                check_typed_stmts(no, &mut env.clone(), sigs, ret, structs)?;
            }
            Stmt::While(condition, body) => {
                let ct = expr_type(condition, env, sigs, structs)?;
                if ct != Type::Bool && ct != Type::Unknown {
                    return Err("while condition must be Bool".into());
                }
                check_typed_stmts(body, &mut env.clone(), sigs, ret, structs)?;
            }
            Stmt::For { var, start, end, body } => {
                if expr_type(start, env, sigs, structs)? != Type::Int
                    && expr_type(start, env, sigs, structs)? != Type::Unknown
                {
                    return Err("for range start must be Int".into());
                }
                let _ = expr_type(end, env, sigs, structs)?;
                let mut loop_env = env.clone();
                loop_env.insert(var.clone(), Type::Int);
                check_typed_stmts(body, &mut loop_env, sigs, ret, structs)?;
            }
            Stmt::ForIn { var, iter, body } => {
                let it = expr_type(iter, env, sigs, structs)?;
                if !matches!(it, Type::List | Type::ListOf(_) | Type::Unknown) {
                    return Err(format!(
                        "for-in expects List, got {}",
                        type_name(&it)
                    ));
                }
                let mut loop_env = env.clone();
                loop_env.insert(var.clone(), Type::Unknown);
                check_typed_stmts(body, &mut loop_env, sigs, ret, structs)?;
            }
            Stmt::Match { scrutinee, arms } => {
                let _ = expr_type(scrutinee, env, sigs, structs)?;
                for arm in arms {
                    let mut arm_env = env.clone();
                    match &arm.pattern {
                        Pattern::Var(n) => {
                            arm_env.insert(n.clone(), Type::Unknown);
                        }
                        Pattern::Variant { bindings, .. } => {
                            for b in bindings {
                                arm_env.insert(b.clone(), Type::Unknown);
                            }
                        }
                        Pattern::Struct { name, fields } => {
                            if !structs.contains_key(name) {
                                return Err(format!("unknown struct `{name}` in pattern"));
                            }
                            let def = &structs[name];
                            for (fname, bname) in fields {
                                let fty = def
                                    .fields
                                    .iter()
                                    .find(|(f, _)| f == fname)
                                    .map(|(_, t)| t.clone())
                                    .unwrap_or(Type::Unknown);
                                arm_env.insert(bname.clone(), fty);
                            }
                        }
                        Pattern::Or(alts) => {
                            // Or-patterns: no bindings across alternatives in 0.3
                            for alt in alts {
                                if matches!(
                                    alt,
                                    Pattern::Var(_)
                                        | Pattern::Variant { .. }
                                        | Pattern::Struct { .. }
                                        | Pattern::Or(_)
                                ) {
                                    return Err(
                                        "or-patterns cannot bind variables in 0.3 (use separate arms)"
                                            .into(),
                                    );
                                }
                            }
                        }
                        _ => {}
                    }
                    if let Some(g) = &arm.guard {
                        let gt = expr_type(g, &arm_env, sigs, structs)?;
                        if gt != Type::Bool && gt != Type::Unknown {
                            return Err(format!(
                                "match guard must be Bool, got {}",
                                type_name(&gt)
                            ));
                        }
                    }
                    check_typed_stmts(&arm.body, &mut arm_env, sigs, ret, structs)?;
                }
            }
            Stmt::Break | Stmt::Continue => {}
        }
    }
    Ok(())
}

// Simplified ownership / definite assignment — keep previous spirit
fn copyable(ty: &Type) -> bool {
    ty.is_copyable()
}

fn moved_var(expr: &Expr) -> Option<String> {
    if let Expr::Var(name) = expr {
        Some(name.clone())
    } else {
        None
    }
}

fn ownership_expr(
    expr: &Expr,
    env: &HashMap<String, Type>,
    moved: &mut HashMap<String, bool>,
    move_value: bool,
    sigs: &HashMap<String, (Vec<Type>, Type)>,
) -> Result<(), String> {
    match expr {
        Expr::Var(name) => {
            if *moved.get(name).unwrap_or(&false) {
                let ty = env.get(name).map(|t| t.name()).unwrap_or_else(|| "unknown".into());
                return Err(format!(
                    "borrow error: use of moved value `{name}` (type {ty})
                     	help: this value was moved earlier and cannot be used again
                     	help: copyable types (Int, Bool, String) are fine; for structs/lists use a fresh binding"
                ));
            }
            if move_value && !copyable(env.get(name).unwrap_or(&Type::Unknown)) {
                moved.insert(name.clone(), true);
            }
        }
        Expr::Binary(a, _, b) => {
            ownership_expr(a, env, moved, false, sigs)?;
            ownership_expr(b, env, moved, false, sigs)?;
        }
        Expr::Unary(_, e) => ownership_expr(e, env, moved, false, sigs)?,
        Expr::Field(e, field) => {
            // Accessing a field of a moved struct is still a use of moved value
            if let Expr::Var(name) = e.as_ref() {
                if *moved.get(name).unwrap_or(&false) {
                    return Err(format!(
                        "borrow error: field `{field}` access on moved value `{name}`
                         	help: move happened earlier; bind fields before the move if you need them"
                    ));
                }
            }
            ownership_expr(e, env, moved, false, sigs)?;
        }
        Expr::Index(base, idx) => {
            ownership_expr(base, env, moved, false, sigs)?;
            ownership_expr(idx, env, moved, false, sigs)?;
        }
        Expr::Call(name, args) => {
            for arg in args {
                let borrow = matches!(
                    name.as_str(),
                    "print"
                        | "spawn"
                        | "str_len"
                        | "str_contains"
                        | "str_upper"
                        | "str_lower"
                        | "str_trim"
                        | "str_concat"
                        | "list"
                        | "list_push"
                        | "list_len"
                        | "list_get"
                        | "list_set"
                        | "list_concat"
                        | "map"
                        | "map_set"
                        | "map_get"
                        | "map_has"
                        | "abs"
                        | "min"
                        | "max"
                        | "pow"
                        | "sqrt_approx"
                        | "now_ms"
                        | "rand"
                        | "rand_range"
                        | "fs_read_text"
                        | "fs_write_text"
                        | "fs_exists"
                        | "json_stringify"
                        | "json_parse"
                        | "assert_eq"
                        | "assert_true"
                        | "assert_false"
                );
                ownership_expr(arg, env, moved, !borrow, sigs)?;
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, e) in fields {
                ownership_expr(e, env, moved, false, sigs)?;
            }
        }
        Expr::Interp(parts) => {
            for p in parts {
                if let InterpPart::Expr(e) = p {
                    ownership_expr(e, env, moved, false, sigs)?;
                }
            }
        }
        Expr::Int(_) | Expr::Str(_) | Expr::Bool(_) => {}
        Expr::MethodCall { receiver, args, .. } => {
            // Method receiver: move non-copyable self into the call
            ownership_expr(receiver, env, moved, moved_var(receiver).is_some(), sigs)?;
            for a in args {
                ownership_expr(a, env, moved, moved_var(a).is_some(), sigs)?;
            }
        }
        Expr::VariantLit { args, .. } => {
            for a in args {
                ownership_expr(a, env, moved, false, sigs)?;
            }
        }
        Expr::Lambda { body, .. } => {
            for s in body {
                match s {
                    Stmt::Return(e) | Stmt::Expr(e) => ownership_expr(e, env, moved, false, sigs)?,
                    _ => {}
                }
            }
        }
        Expr::CallExpr(callee, args) => {
            ownership_expr(callee, env, moved, false, sigs)?;
            for a in args {
                ownership_expr(a, env, moved, false, sigs)?;
            }
        }
    }
    Ok(())
}

fn ownership_stmts(
    xs: &[Stmt],
    env: &mut HashMap<String, Type>,
    moved: &mut HashMap<String, bool>,
    sigs: &HashMap<String, (Vec<Type>, Type)>,
) -> Result<(), String> {
    for s in xs {
        match s {
            Stmt::Let(name, expr) => {
                ownership_expr(expr, env, moved, moved_var(expr).is_some(), sigs)?;
                // Prefer a real type so Int/Bool stay copyable under moves
                let ty = expr_type(expr, env, sigs, &HashMap::new()).unwrap_or(Type::Unknown);
                env.insert(name.clone(), ty);
                moved.insert(name.clone(), false);
            }
            Stmt::Assign(name, expr) => {
                ownership_expr(expr, env, moved, moved_var(expr).is_some(), sigs)?;
                if !env.contains_key(name) {
                    return Err(format!("assignment to undefined variable `{name}`"));
                }
                moved.insert(name.clone(), false);
            }
            Stmt::Decl(name, ty) => {
                env.insert(name.clone(), ty.clone());
                moved.insert(name.clone(), false);
            }
            Stmt::Expr(expr) | Stmt::Return(expr) => {
                ownership_expr(expr, env, moved, false, sigs)?
            }
            Stmt::FieldAssign { object, value, .. } => {
                ownership_expr(object, env, moved, false, sigs)?;
                ownership_expr(value, env, moved, false, sigs)?;
            }
            Stmt::If(condition, yes, no) => {
                ownership_expr(condition, env, moved, false, sigs)?;
                let before = moved.clone();
                let mut a = before.clone();
                let mut b = before;
                ownership_stmts(yes, env, &mut a, sigs)?;
                ownership_stmts(no, env, &mut b, sigs)?;
                for (name, value) in a.iter().chain(b.iter()) {
                    if *value {
                        moved.insert(name.clone(), true);
                    }
                }
            }
            Stmt::While(condition, body)
            | Stmt::For {
                body,
                start: condition,
                ..
            } => {
                // approximate
                let _ = condition;
                let mut loop_state = moved.clone();
                ownership_stmts(body, env, &mut loop_state, sigs)?;
            }
            Stmt::ForIn { body, iter, var } => {
                ownership_expr(iter, env, moved, false, sigs)?;
                env.insert(var.clone(), Type::Unknown);
                moved.insert(var.clone(), false);
                let mut loop_state = moved.clone();
                ownership_stmts(body, env, &mut loop_state, sigs)?;
            }
            Stmt::Match { scrutinee, arms } => {
                ownership_expr(scrutinee, env, moved, false, sigs)?;
                for arm in arms {
                    let mut m = moved.clone();
                    let mut e = env.clone();
                    if let Pattern::Variant { bindings, .. } = &arm.pattern {
                        for b in bindings {
                            e.insert(b.clone(), Type::Unknown);
                        }
                    }
                    if let Pattern::Var(n) = &arm.pattern {
                        e.insert(n.clone(), Type::Unknown);
                    }
                    if let Pattern::Struct { fields, .. } = &arm.pattern {
                        for (_, b) in fields {
                            e.insert(b.clone(), Type::Unknown);
                        }
                    }
                    if let Some(g) = &arm.guard {
                        ownership_expr(g, &mut e, &mut m, false, sigs)?;
                    }
                    ownership_stmts(&arm.body, &mut e, &mut m, sigs)?;
                }
            }
            Stmt::Break | Stmt::Continue => {}
        }
    }
    Ok(())
}

fn definite_expr(
    expr: &Expr,
    types: &HashMap<String, Type>,
    assigned: &HashMap<String, ()>,
) -> Result<(), String> {
    match expr {
        Expr::Var(name) => {
            if !types.contains_key(name) {
                return Err(format!("undefined variable `{name}`"));
            }
            if !assigned.contains_key(name) {
                return Err(format!("use of uninitialized variable `{name}`"));
            }
        }
        Expr::Binary(a, _, b) | Expr::Index(a, b) => {
            definite_expr(a, types, assigned)?;
            definite_expr(b, types, assigned)?;
        }
        Expr::Unary(_, e) | Expr::Field(e, _) => definite_expr(e, types, assigned)?,
        Expr::Call(_, args) => {
            for arg in args {
                definite_expr(arg, types, assigned)?;
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, e) in fields {
                definite_expr(e, types, assigned)?;
            }
        }
        Expr::Interp(parts) => {
            for p in parts {
                if let InterpPart::Expr(e) = p {
                    definite_expr(e, types, assigned)?;
                }
            }
        }
        Expr::Int(_) | Expr::Str(_) | Expr::Bool(_) => {}
        Expr::Lambda { .. } => {}
        Expr::MethodCall { receiver, args, .. } => {
            definite_expr(receiver, types, assigned)?;
            for a in args {
                definite_expr(a, types, assigned)?;
            }
        }
        Expr::VariantLit { args, .. } => {
            for a in args {
                definite_expr(a, types, assigned)?;
            }
        }
        Expr::CallExpr(callee, args) => {
            definite_expr(callee, types, assigned)?;
            for a in args {
                definite_expr(a, types, assigned)?;
            }
        }
    }
    Ok(())
}

fn intersect(a: &HashMap<String, ()>, b: &HashMap<String, ()>) -> HashMap<String, ()> {
    a.iter()
        .filter(|(k, _)| b.contains_key(*k))
        .map(|(k, v)| (k.clone(), *v))
        .collect()
}

fn definite_stmts(
    xs: &[Stmt],
    types: &mut HashMap<String, Type>,
    assigned: &mut HashMap<String, ()>,
) -> Result<(), String> {
    for s in xs {
        match s {
            Stmt::Decl(name, ty) => {
                types.insert(name.clone(), ty.clone());
                assigned.remove(name);
            }
            Stmt::Let(name, expr) => {
                definite_expr(expr, types, assigned)?;
                types.insert(name.clone(), Type::Unknown);
                assigned.insert(name.clone(), ());
            }
            Stmt::Assign(name, expr) => {
                if !types.contains_key(name) {
                    return Err(format!("assignment to undefined variable `{name}`"));
                }
                definite_expr(expr, types, assigned)?;
                assigned.insert(name.clone(), ());
            }
            Stmt::Expr(expr) | Stmt::Return(expr) => definite_expr(expr, types, assigned)?,
            Stmt::FieldAssign { object, value, .. } => {
                definite_expr(object, types, assigned)?;
                definite_expr(value, types, assigned)?;
            }
            Stmt::If(condition, yes, no) => {
                definite_expr(condition, types, assigned)?;
                let mut a = assigned.clone();
                let mut b = assigned.clone();
                definite_stmts(yes, types, &mut a)?;
                definite_stmts(no, types, &mut b)?;
                *assigned = intersect(&a, &b);
            }
            Stmt::While(condition, body) => {
                definite_expr(condition, types, assigned)?;
                let mut body_state = assigned.clone();
                definite_stmts(body, types, &mut body_state)?;
            }
            Stmt::For { var, start, end, body } => {
                definite_expr(start, types, assigned)?;
                definite_expr(end, types, assigned)?;
                types.insert(var.clone(), Type::Int);
                assigned.insert(var.clone(), ());
                let mut body_state = assigned.clone();
                definite_stmts(body, types, &mut body_state)?;
            }
            Stmt::ForIn { var, iter, body } => {
                definite_expr(iter, types, assigned)?;
                types.insert(var.clone(), Type::Unknown);
                assigned.insert(var.clone(), ());
                let mut body_state = assigned.clone();
                definite_stmts(body, types, &mut body_state)?;
            }
            Stmt::Match { scrutinee, arms } => {
                definite_expr(scrutinee, types, assigned)?;
                for arm in arms {
                    let mut a = assigned.clone();
                    let mut t = types.clone();
                    match &arm.pattern {
                        Pattern::Var(n) => {
                            t.insert(n.clone(), Type::Unknown);
                            a.insert(n.clone(), ());
                        }
                        Pattern::Variant { bindings, .. } => {
                            for b in bindings {
                                t.insert(b.clone(), Type::Unknown);
                                a.insert(b.clone(), ());
                            }
                        }
                        Pattern::Struct { fields, .. } => {
                            for (_, b) in fields {
                                t.insert(b.clone(), Type::Unknown);
                                a.insert(b.clone(), ());
                            }
                        }
                        Pattern::Or(_) => {}
                        _ => {}
                    }
                    if let Some(g) = &arm.guard {
                        definite_expr(g, &t, &a)?;
                    }
                    definite_stmts(&arm.body, &mut t, &mut a)?;
                }
            }
            Stmt::Break | Stmt::Continue => {}
        }
    }
    Ok(())
}

pub fn check(fs: &[Function]) -> Result<(), String> {
    check_program(&Program {
        imports: vec![],
        structs: vec![],
        enums: vec![],
        traits: vec![],
        functions: fs.to_vec(),
        externs: vec![],
    })
}


/// Rewrite `MethodCall` nodes into `Call("Type.method", [recv, ...args])`.
pub fn desugar_methods(program: &mut Program) -> Result<(), String> {
    let mut structs = HashMap::new();
    for s in &program.structs {
        structs.insert(s.name.clone(), s.clone());
    }
    let mut enums = HashMap::new();
    for e in &program.enums {
        enums.insert(e.name.clone(), e.clone());
    }
    // Prelude: Result convention (Ok(Int) / Err(String)) if user did not define Result
    if !enums.contains_key("Result") {
        let result_enum = crate::ast::EnumDef {
            name: "Result".into(),
            visibility: crate::ast::Visibility::Public,
            module_path: vec![],
            variants: vec![
                ("Ok".into(), vec![Type::Int]),
                ("Err".into(), vec![Type::Str]),
            ],
        };
        enums.insert("Result".into(), result_enum);
    }
    ENUMS.with(|cell| {
        *cell.borrow_mut() = enums.clone();
    });
    let mut sigs = HashMap::new();
    let mut fn_meta: HashMap<String, (crate::ast::Visibility, Vec<String>)> = HashMap::new();
    for f in &program.functions {
        sigs.insert(
            f.name.clone(),
            (f.param_types.clone(), f.return_type.clone()),
        );
        fn_meta.insert(
            f.name.clone(),
            (f.visibility, f.module_path.clone()),
        );
    }
    FN_META.with(|cell| {
        *cell.borrow_mut() = fn_meta;
    });
    for e in &program.externs {
        sigs.insert(
            e.name.clone(),
            (e.param_types.clone(), e.return_type.clone()),
        );
    }
    for f in &mut program.functions {
        let mut env = HashMap::new();
        for (p, t) in f.params.iter().zip(f.param_types.iter()) {
            env.insert(p.clone(), t.clone());
        }
        desugar_stmts(&mut f.body, &mut env, &sigs, &structs)?;
    }
    Ok(())
}

fn desugar_stmts(
    xs: &mut [Stmt],
    env: &mut HashMap<String, Type>,
    sigs: &HashMap<String, (Vec<Type>, Type)>,
    structs: &HashMap<String, StructDef>,
) -> Result<(), String> {
    for s in xs.iter_mut() {
        match s {
            Stmt::Let(n, e) => {
                *e = desugar_expr(e, env, sigs, structs)?;
                let ty = expr_type(e, env, sigs, structs)?;
                env.insert(n.clone(), ty);
            }
            Stmt::Assign(_, e) | Stmt::Expr(e) | Stmt::Return(e) => {
                *e = desugar_expr(e, env, sigs, structs)?;
            }
            Stmt::Decl(n, t) => {
                env.insert(n.clone(), t.clone());
            }
            Stmt::FieldAssign {
                object,
                value,
                ..
            } => {
                *object = desugar_expr(object, env, sigs, structs)?;
                *value = desugar_expr(value, env, sigs, structs)?;
            }
            Stmt::If(c, a, b) => {
                *c = desugar_expr(c, env, sigs, structs)?;
                desugar_stmts(a, env, sigs, structs)?;
                desugar_stmts(b, env, sigs, structs)?;
            }
            Stmt::While(c, body) => {
                *c = desugar_expr(c, env, sigs, structs)?;
                desugar_stmts(body, env, sigs, structs)?;
            }
            Stmt::For {
                start, end, body, ..
            } => {
                *start = desugar_expr(start, env, sigs, structs)?;
                *end = desugar_expr(end, env, sigs, structs)?;
                desugar_stmts(body, env, sigs, structs)?;
            }
            Stmt::ForIn { iter, body, .. } => {
                *iter = desugar_expr(iter, env, sigs, structs)?;
                desugar_stmts(body, env, sigs, structs)?;
            }
            Stmt::Match { scrutinee, arms } => {
                *scrutinee = desugar_expr(scrutinee, env, sigs, structs)?;
                for arm in arms {
                    let mut arm_env = env.clone();
                    if let Pattern::Variant { bindings, .. } = &arm.pattern {
                        for b in bindings {
                            arm_env.insert(b.clone(), Type::Unknown);
                        }
                    }
                    if let Pattern::Var(n) = &arm.pattern {
                        arm_env.insert(n.clone(), Type::Unknown);
                    }
                    if let Pattern::Struct { fields, .. } = &arm.pattern {
                        for (_, b) in fields {
                            arm_env.insert(b.clone(), Type::Unknown);
                        }
                    }
                    if let Some(g) = &mut arm.guard {
                        *g = desugar_expr(g, &arm_env, sigs, structs)?;
                    }
                    desugar_stmts(&mut arm.body, &mut arm_env, sigs, structs)?;
                }
            }
            Stmt::Break | Stmt::Continue => {}
        }
    }
    Ok(())
}

fn desugar_expr(
    e: &Expr,
    env: &HashMap<String, Type>,
    sigs: &HashMap<String, (Vec<Type>, Type)>,
    structs: &HashMap<String, StructDef>,
) -> Result<Expr, String> {
    match e {
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let recv = desugar_expr(receiver, env, sigs, structs)?;
            let mut new_args = Vec::new();
            for a in args {
                new_args.push(desugar_expr(a, env, sigs, structs)?);
            }
            let recv_ty = expr_type(&recv, env, sigs, structs)?;
            let type_name = match &recv_ty {
                Type::Struct(n) => n.clone(),
                other => {
                    return Err(format!(
                        "method `{method}` requires struct receiver, got {}",
                        type_name(other)
                    ));
                }
            };
            let full = format!("{type_name}.{method}");
            let mut call_args = vec![recv];
            call_args.extend(new_args);
            Ok(Expr::Call(full, call_args))
        }
        Expr::Binary(a, op, b) => Ok(Expr::Binary(
            Box::new(desugar_expr(a, env, sigs, structs)?),
            op.clone(),
            Box::new(desugar_expr(b, env, sigs, structs)?),
        )),
        Expr::Unary(op, a) => Ok(Expr::Unary(
            op.clone(),
            Box::new(desugar_expr(a, env, sigs, structs)?),
        )),
        Expr::Call(n, args) => {
            let mut na = Vec::new();
            for a in args {
                na.push(desugar_expr(a, env, sigs, structs)?);
            }
            if let Some((base, variant)) = n.split_once('.') {
                let is_enum = with_enums(|enums| enums.contains_key(base));
                if is_enum {
                    return Ok(Expr::VariantLit {
                        enum_name: base.to_string(),
                        variant: variant.to_string(),
                        args: na,
                    });
                }
                // The parser intentionally keeps `value.method(...)` as a
                // dotted call until semantic analysis can distinguish it
                // from `module.function(...)`. A local struct variable is a
                // method receiver; rewrite it to the canonical Type.method
                // function name and prepend the receiver argument.
                if let Some(Type::Struct(type_name)) = env.get(base) {
                    let mut method_args = vec![Expr::Var(base.to_string())];
                    method_args.extend(na);
                    return Ok(Expr::Call(
                        format!("{type_name}.{variant}"),
                        method_args,
                    ));
                }
            }
            Ok(Expr::Call(n.clone(), na))
        }
        Expr::CallExpr(callee, args) => Ok(Expr::CallExpr(
            Box::new(desugar_expr(callee, env, sigs, structs)?),
            args.iter()
                .map(|a| desugar_expr(a, env, sigs, structs))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Expr::Index(a, b) => Ok(Expr::Index(
            Box::new(desugar_expr(a, env, sigs, structs)?),
            Box::new(desugar_expr(b, env, sigs, structs)?),
        )),
        Expr::Field(a, f) => {
            let a2 = desugar_expr(a, env, sigs, structs)?;
            if let Expr::Var(ename) = &a2 {
                let unit = with_enums(|enums| {
                    enums.get(ename).and_then(|def| {
                        def.variants.iter().find(|(n, p)| n == f && p.is_empty())
                    }).is_some()
                });
                // Only if `ename` is not a local variable
                if unit && !env.contains_key(ename) {
                    return Ok(Expr::VariantLit {
                        enum_name: ename.clone(),
                        variant: f.clone(),
                        args: vec![],
                    });
                }
            }
            Ok(Expr::Field(Box::new(a2), f.clone()))
        }
        Expr::StructLit { name, fields } => Ok(Expr::StructLit {
            name: name.clone(),
            fields: fields
                .iter()
                .map(|(n, e)| Ok((n.clone(), desugar_expr(e, env, sigs, structs)?)))
                .collect::<Result<Vec<_>, String>>()?,
        }),
        Expr::Interp(parts) => {
            let mut np = Vec::new();
            for p in parts {
                match p {
                    InterpPart::Lit(s) => np.push(InterpPart::Lit(s.clone())),
                    InterpPart::Expr(e) => {
                        np.push(InterpPart::Expr(desugar_expr(e, env, sigs, structs)?))
                    }
                }
            }
            Ok(Expr::Interp(np))
        }
        Expr::Lambda {
            params,
            param_types,
            return_type,
            body,
        } => {
            let mut body = body.clone();
            let mut lenv = env.clone();
            for (p, t) in params.iter().zip(param_types.iter()) {
                lenv.insert(p.clone(), t.clone());
            }
            desugar_stmts(&mut body, &mut lenv, sigs, structs)?;
            Ok(Expr::Lambda {
                params: params.clone(),
                param_types: param_types.clone(),
                return_type: return_type.clone(),
                body,
            })
        }
        other => Ok(other.clone()),
    }
}


/// For each `impl Trait for Type`, copy missing methods that have a default body
/// into `program.functions` as `Type.method`.
pub fn expand_trait_defaults(program: &mut Program) -> Result<(), String> {
    let traits: HashMap<String, TraitDef> =
        program.traits.iter().map(|t| (t.name.clone(), t.clone())).collect();
    // Which (trait, type) pairs are implemented?
    let mut impl_pairs: Vec<(String, String)> = Vec::new();
    for f in &program.functions {
        if f.module_path.len() == 2 && f.module_path[0] == "__trait__" {
            let trait_name = f.module_path[1].clone();
            if let Some((ty, _)) = f.name.rsplit_once('.') {
                let key = (trait_name, ty.to_string());
                if !impl_pairs.contains(&key) {
                    impl_pairs.push(key);
                }
            }
        }
    }
    let existing: HashSet<String> = program.functions.iter().map(|f| f.name.clone()).collect();
    for (trait_name, type_name) in impl_pairs {
        let Some(tdef) = traits.get(&trait_name) else { continue };
        for tm in &tdef.methods {
            let fname = format!("{type_name}.{}", tm.name);
            if existing.contains(&fname) {
                continue;
            }
            let Some(body) = &tm.default_body else { continue };
            let mut param_types = tm.param_types.clone();
            // Fix bare self type to the implementing struct
            if let Some(p0) = tm.params.first() {
                if p0 == "self" {
                    if param_types.is_empty() {
                        param_types.push(Type::Struct(type_name.clone()));
                    } else {
                        param_types[0] = Type::Struct(type_name.clone());
                    }
                }
            }
            program.functions.push(Function {
                name: fname.clone(),
                visibility: Visibility::Public,
                module_path: vec!["__trait__".into(), trait_name.clone()],
                params: tm.params.clone(),
                param_types,
                return_type: tm.return_type.clone(),
                type_params: vec![],
                trait_bounds: vec![],
                body: body.clone(),
            });
        }
    }
    Ok(())
}

/// `where Type: Trait` — ensure Type has an `impl Trait for Type`.
fn check_trait_bounds(program: &Program) -> Result<(), String> {
    let trait_names: HashSet<String> = program.traits.iter().map(|t| t.name.clone()).collect();
    // Types that implement each trait
    let mut impl_map: HashMap<String, HashSet<String>> = HashMap::new();
    for f in &program.functions {
        if f.module_path.len() == 2 && f.module_path[0] == "__trait__" {
            let tr = f.module_path[1].clone();
            if let Some((ty, _)) = f.name.rsplit_once('.') {
                impl_map.entry(tr).or_default().insert(ty.to_string());
            }
        }
    }
    for f in &program.functions {
        for (ty, tr) in &f.trait_bounds {
            if !trait_names.contains(tr) {
                return Err(format!(
                    "in function `{}`: unknown trait `{tr}` in where-clause",
                    f.name
                ));
            }
            let ok = impl_map.get(tr).map(|s| s.contains(ty)).unwrap_or(false);
            if !ok {
                return Err(format!(
                    "in function `{}`: where-clause `{ty}: {tr}` not satisfied — no `impl {tr} for {ty}`",
                    f.name
                ));
            }
        }
    }
    Ok(())
}

/// Verify every `impl Trait for Type` covers all trait methods.
fn check_trait_impls(program: &Program) -> Result<(), String> {
    let traits: std::collections::HashMap<String, &crate::ast::TraitDef> =
        program.traits.iter().map(|t| (t.name.clone(), t)).collect();
    // Collect methods implemented for (trait, type) from functions with module_path sentinel
    // f.name = "Type.method", f.module_path = ["__trait__", "TraitName"]
    let mut impls: std::collections::HashMap<(String, String), Vec<String>> =
        std::collections::HashMap::new();
    for f in &program.functions {
        if f.module_path.len() == 2 && f.module_path[0] == "__trait__" {
            let trait_name = f.module_path[1].clone();
            // Type is prefix of f.name before last dot
            if let Some((ty, method)) = f.name.rsplit_once('.') {
                impls
                    .entry((trait_name, ty.to_string()))
                    .or_default()
                    .push(method.to_string());
            }
        }
    }
    for ((trait_name, type_name), methods) in &impls {
        let tdef = traits.get(trait_name).ok_or_else(|| {
            format!("impl of unknown trait `{trait_name}` for `{type_name}`")
        })?;
        for tm in &tdef.methods {
            if !methods.iter().any(|m| m == &tm.name) {
                if tm.default_body.is_some() {
                    continue; // will be filled by expand_trait_defaults
                }
                return Err(format!(
                    "trait `{trait_name}` not fully implemented for `{type_name}`: missing method `{}`",
                    tm.name
                ));
            }
        }
        // arity / return type soft check
        for tm in &tdef.methods {
            let fname = format!("{type_name}.{}", tm.name);
            if let Some(f) = program.functions.iter().find(|f| f.name == fname) {
                // self + extra params
                let expected_params = tm.params.len();
                if f.params.len() != expected_params {
                    return Err(format!(
                        "method `{fname}` for trait `{trait_name}` expects {} param(s), got {}",
                        expected_params,
                        f.params.len()
                    ));
                }
                if f.return_type != tm.return_type
                    && f.return_type != crate::ast::Type::Unknown
                    && tm.return_type != crate::ast::Type::Unknown
                {
                    return Err(format!(
                        "method `{fname}` return type mismatch for trait `{trait_name}`"
                    ));
                }
            }
        }
    }
    // Also: if someone wrote `impl Trait for T` but trait has methods and none registered — caught above
    Ok(())
}

pub fn check_program(program: &Program) -> Result<(), String> {
    let mut structs = HashMap::new();
    for s in &program.structs {
        structs.insert(s.name.clone(), s.clone());
    }
    let mut enums = HashMap::new();
    for e in &program.enums {
        enums.insert(e.name.clone(), e.clone());
    }
    // Prelude: Result convention (Ok(Int) / Err(String)) if user did not define Result
    if !enums.contains_key("Result") {
        let result_enum = crate::ast::EnumDef {
            name: "Result".into(),
            visibility: crate::ast::Visibility::Public,
            module_path: vec![],
            variants: vec![
                ("Ok".into(), vec![Type::Int]),
                ("Err".into(), vec![Type::Str]),
            ],
        };
        enums.insert("Result".into(), result_enum);
    }
    ENUMS.with(|cell| {
        *cell.borrow_mut() = enums.clone();
    });
    let mut sigs = HashMap::new();
    let mut fn_meta: HashMap<String, (crate::ast::Visibility, Vec<String>)> = HashMap::new();
    for f in &program.functions {
        sigs.insert(
            f.name.clone(),
            (f.param_types.clone(), f.return_type.clone()),
        );
        fn_meta.insert(
            f.name.clone(),
            (f.visibility, f.module_path.clone()),
        );
    }
    FN_META.with(|cell| {
        *cell.borrow_mut() = fn_meta;
    });
    for e in &program.externs {
        if sigs.contains_key(&e.name) {
            return Err(format!("extern `{}` conflicts with existing function", e.name));
        }
        if e.abi != "C" && e.abi != "c" {
            return Err(format!(
                "unsupported ABI `{}` for extern `{}` (supported: \"C\")",
                e.abi, e.name
            ));
        }
        sigs.insert(
            e.name.clone(),
            (e.param_types.clone(), e.return_type.clone()),
        );
    }
    check_trait_impls(program)?;
    check_trait_bounds(program)?;

    for f in &program.functions {
        CURRENT_MODULE.with(|cell| {
            *cell.borrow_mut() = f.module_path.clone();
        });
        let mut env = HashMap::new();
        for (p, t) in f.params.iter().zip(f.param_types.iter()) {
            env.insert(p.clone(), t.clone());
        }
        if let Err(e) = check_typed_stmts(&f.body, &mut env, &sigs, &f.return_type, &structs) {
            return Err(format!("in function `{}`: {e}", f.name));
        }
        let mut moved = HashMap::new();
        let mut oenv = env.clone();
        ownership_stmts(&f.body, &mut oenv, &mut moved, &sigs)?;
        let mut types = HashMap::new();
        let mut assigned = HashMap::new();
        for (p, t) in f.params.iter().zip(f.param_types.iter()) {
            types.insert(p.clone(), t.clone());
            assigned.insert(p.clone(), ());
        }
        definite_stmts(&f.body, &mut types, &mut assigned)?;
    }
    Ok(())
}
