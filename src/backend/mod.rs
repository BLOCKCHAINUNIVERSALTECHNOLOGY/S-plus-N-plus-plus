//! Code-generation backends for S+N++.
//!
//! Current backends:
//! - **Bytecode** — default, via `vm::compile_from_mir`
//! - **LLVM IR text** — emits a `.ll` module from MIR (no libllvm required)
//! - **WASM text** — CFG dispatch loop for the integer subset
//!
//! Keeping backends dependency-free preserves the "light + fast" design.
//! A future `llvm-sys` / Cranelift feature flag can replace the text emitters
//! with real object-file generation.

use crate::mir::{
    MirBlock, MirFunction, MirInstr, MirModule, MirOperand, MirTerminator,
};
use crate::lexer::Token;
use std::fmt::Write as _;

/// Selected code-generation target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Stack bytecode VM (default).
    Bytecode,
    /// Textual LLVM IR (`.ll`).
    LlvmIr,
    /// Textual WebAssembly (`.wat`) skeleton.
    WasmText,
    /// Research: human-readable native lowering plan (not executable).
    NativePlan,
}

impl Backend {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "bytecode" | "vm" => Some(Self::Bytecode),
            "llvm" | "llvm-ir" | "ll" => Some(Self::LlvmIr),
            "wasm" | "wat" => Some(Self::WasmText),
            "native" | "native-plan" | "jit-plan" => Some(Self::NativePlan),
            _ => None,
        }
    }
}

/// Emit LLVM IR text for a MIR module.
pub fn emit_llvm_ir(module: &MirModule) -> Result<String, String> {
    let mut out = String::new();
    writeln!(out, "; S+N++ → LLVM IR (text backend)").unwrap();
    writeln!(out, "; Auto-generated — suitable for `llc` / `clang -emit-obj` experiments").unwrap();
    writeln!(out, "target datalayout = \"e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128\"").unwrap();
    writeln!(out, "target triple = \"x86_64-unknown-linux-gnu\"").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "declare i32 @printf(i8*, ...)").unwrap();
    writeln!(out, "declare i8* @malloc(i64)").unwrap();
    writeln!(out).unwrap();
    // simple string constant pool for print
    writeln!(out, "@.fmt_int = private unnamed_addr constant [4 x i8] c\"%d\\0A\\00\"").unwrap();
    writeln!(out, "@.fmt_str = private unnamed_addr constant [4 x i8] c\"%s\\0A\\00\"").unwrap();
    writeln!(out).unwrap();

    for f in &module.functions {
        emit_llvm_function(&mut out, f)?;
        writeln!(out).unwrap();
    }
    Ok(out)
}

fn emit_llvm_function(out: &mut String, f: &MirFunction) -> Result<(), String> {
    // All values are i64 for this first-cut integer-oriented IR.
    let ret_ty = "i64";
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| format!("i64 %{}", sanitize(p)))
        .collect();
    let param_list = params.join(", ");
    writeln!(
        out,
        "define {ret_ty} @{}({param_list}) {{",
        sanitize(&f.name)
    )
    .unwrap();

    // Allocate stack slots for locals encountered
    let mut slots: Vec<String> = f.params.clone();
    collect_locals(f, &mut slots);
    slots.sort();
    slots.dedup();

    writeln!(out, "entry_alloca:").unwrap();
    for s in &slots {
        writeln!(out, "  %{0}.addr = alloca i64, align 8", sanitize(s)).unwrap();
    }
    for p in &f.params {
        writeln!(
            out,
            "  store i64 %{}, i64* %{}.addr, align 8",
            sanitize(p),
            sanitize(p)
        )
        .unwrap();
    }
    if f.entry < f.blocks.len() {
        writeln!(out, "  br label %{}", block_label(f.entry)).unwrap();
    } else {
        writeln!(out, "  ret i64 0").unwrap();
    }

    for bb in &f.blocks {
        emit_llvm_block(out, bb)?;
    }
    writeln!(out, "}}").unwrap();
    Ok(())
}

fn collect_locals(f: &MirFunction, slots: &mut Vec<String>) {
    for bb in &f.blocks {
        for instr in &bb.stmts {
            match instr {
                MirInstr::Assign { dest, .. }
                | MirInstr::Binary { dest, .. }
                | MirInstr::Unary { dest, .. }
                | MirInstr::Call { dest, .. }
                | MirInstr::CallIndirect { dest, .. }
                | MirInstr::Index { dest, .. }
                | MirInstr::Field { dest, .. }
                | MirInstr::MakeStruct { dest, .. }
                | MirInstr::MakeClosure { dest, .. } => {
                    slots.push(dest.clone());
                }
                _ => {}
            }
            if let MirInstr::MakeClosure { captures, .. } = instr {
                for (n, _) in captures {
                    slots.push(n.clone());
                }
            }
        }
    }
}

fn emit_llvm_block(out: &mut String, bb: &MirBlock) -> Result<(), String> {
    writeln!(out, "{}:", block_label(bb.id)).unwrap();
    for (i, instr) in bb.stmts.iter().enumerate() {
        emit_llvm_instr(out, instr, bb.id, i)?;
    }
    match &bb.terminator {
        MirTerminator::Return(None) => {
            writeln!(out, "  ret i64 0").unwrap();
        }
        MirTerminator::Return(Some(op)) => {
            let v = operand_llvm(out, op, bb.id, 9000)?;
            writeln!(out, "  ret i64 {v}").unwrap();
        }
        MirTerminator::Jump(t) => {
            writeln!(out, "  br label %{}", block_label(*t)).unwrap();
        }
        MirTerminator::Branch {
            cond,
            then_bb,
            else_bb,
        } => {
            let c = operand_llvm(out, cond, bb.id, 9001)?;
            let cmp = format!("%bc{}_{}", bb.id, 9001);
            writeln!(out, "  {cmp} = icmp ne i64 {c}, 0").unwrap();
            writeln!(
                out,
                "  br i1 {cmp}, label %{}, label %{}",
                block_label(*then_bb),
                block_label(*else_bb)
            )
            .unwrap();
        }
        MirTerminator::Unreachable => {
            writeln!(out, "  unreachable").unwrap();
        }
    }
    Ok(())
}

fn emit_llvm_instr(
    out: &mut String,
    instr: &MirInstr,
    bb: usize,
    idx: usize,
) -> Result<(), String> {
    match instr {
        MirInstr::Try { dest, value } => {
            // research: try is VM-only; emit load
            let v = operand_llvm(out, value, bb, idx)?;
            writeln!(out, "  store i64 {v}, i64* %{}.addr, align 8", sanitize(dest)).unwrap();
        }
        MirInstr::Assign { dest, value } => {
            let v = operand_llvm(out, value, bb, idx)?;
            writeln!(
                out,
                "  store i64 {v}, i64* %{}.addr, align 8",
                sanitize(dest)
            )
            .unwrap();
        }
        MirInstr::Binary {
            dest,
            op,
            left,
            right,
        } => {
            let l = operand_llvm(out, left, bb, idx * 2)?;
            let r = operand_llvm(out, right, bb, idx * 2 + 1)?;
            let tmp = format!("%t{}_{}", bb, idx);
            match op {
                Token::Plus => writeln!(out, "  {tmp} = add i64 {l}, {r}").unwrap(),
                Token::Minus => writeln!(out, "  {tmp} = sub i64 {l}, {r}").unwrap(),
                Token::Star => writeln!(out, "  {tmp} = mul i64 {l}, {r}").unwrap(),
                Token::Slash => writeln!(out, "  {tmp} = sdiv i64 {l}, {r}").unwrap(),
                Token::EqEq => {
                    let c = format!("%c{}_{}", bb, idx);
                    writeln!(out, "  {c} = icmp eq i64 {l}, {r}").unwrap();
                    writeln!(out, "  {tmp} = zext i1 {c} to i64").unwrap();
                }
                Token::BangEq => {
                    let c = format!("%c{}_{}", bb, idx);
                    writeln!(out, "  {c} = icmp ne i64 {l}, {r}").unwrap();
                    writeln!(out, "  {tmp} = zext i1 {c} to i64").unwrap();
                }
                Token::Lt => {
                    let c = format!("%c{}_{}", bb, idx);
                    writeln!(out, "  {c} = icmp slt i64 {l}, {r}").unwrap();
                    writeln!(out, "  {tmp} = zext i1 {c} to i64").unwrap();
                }
                Token::Lte => {
                    let c = format!("%c{}_{}", bb, idx);
                    writeln!(out, "  {c} = icmp sle i64 {l}, {r}").unwrap();
                    writeln!(out, "  {tmp} = zext i1 {c} to i64").unwrap();
                }
                Token::Gt => {
                    let c = format!("%c{}_{}", bb, idx);
                    writeln!(out, "  {c} = icmp sgt i64 {l}, {r}").unwrap();
                    writeln!(out, "  {tmp} = zext i1 {c} to i64").unwrap();
                }
                Token::Gte => {
                    let c = format!("%c{}_{}", bb, idx);
                    writeln!(out, "  {c} = icmp sge i64 {l}, {r}").unwrap();
                    writeln!(out, "  {tmp} = zext i1 {c} to i64").unwrap();
                }
                other => {
                    return Err(format!("LLVM backend: unsupported binop {:?}", other));
                }
            }
            writeln!(
                out,
                "  store i64 {tmp}, i64* %{}.addr, align 8",
                sanitize(dest)
            )
            .unwrap();
        }
        MirInstr::Unary { dest, op, value } => {
            let v = operand_llvm(out, value, bb, idx)?;
            let tmp = format!("%t{}_{}", bb, idx);
            match op {
                Token::Minus => writeln!(out, "  {tmp} = sub i64 0, {v}").unwrap(),
                Token::Bang => {
                    let c = format!("%c{}_{}", bb, idx);
                    writeln!(out, "  {c} = icmp eq i64 {v}, 0").unwrap();
                    writeln!(out, "  {tmp} = zext i1 {c} to i64").unwrap();
                }
                other => return Err(format!("LLVM backend: unsupported unop {:?}", other)),
            }
            writeln!(
                out,
                "  store i64 {tmp}, i64* %{}.addr, align 8",
                sanitize(dest)
            )
            .unwrap();
        }
        MirInstr::Call { dest, name, args } => {
            if name == "print" && args.len() == 1 {
                let v = operand_llvm(out, &args[0], bb, idx)?;
                writeln!(
                    out,
                    "  call i32 (i8*, ...) @printf(i8* getelementptr inbounds ([4 x i8], [4 x i8]* @.fmt_int, i64 0, i64 0), i64 {v})"
                )
                .unwrap();
                writeln!(
                    out,
                    "  store i64 0, i64* %{}.addr, align 8",
                    sanitize(dest)
                )
                .unwrap();
            } else {
                let mut arg_strs = Vec::new();
                for (i, a) in args.iter().enumerate() {
                    arg_strs.push(format!("i64 {}", operand_llvm(out, a, bb, idx * 10 + i)?));
                }
                let tmp = format!("%t{}_{}", bb, idx);
                writeln!(
                    out,
                    "  {tmp} = call i64 @{}({})",
                    sanitize(name),
                    arg_strs.join(", ")
                )
                .unwrap();
                writeln!(
                    out,
                    "  store i64 {tmp}, i64* %{}.addr, align 8",
                    sanitize(dest)
                )
                .unwrap();
            }
        }
        MirInstr::Print(v) => {
            let val = operand_llvm(out, v, bb, idx)?;
            writeln!(
                out,
                "  call i32 (i8*, ...) @printf(i8* getelementptr inbounds ([4 x i8], [4 x i8]* @.fmt_int, i64 0, i64 0), i64 {val})"
            )
            .unwrap();
        }
        MirInstr::CallIndirect { dest, .. }
        | MirInstr::Index { dest, .. }
        | MirInstr::Field { dest, .. }
        | MirInstr::MakeStruct { dest, .. }
        | MirInstr::MakeClosure { dest, .. } => {
            // First-cut: lower complex ops to zero (runtime still handles via VM).
            writeln!(
                out,
                "  store i64 0, i64* %{}.addr, align 8  ; complex op stub",
                sanitize(dest)
            )
            .unwrap();
        }
        MirInstr::SetField { .. } | MirInstr::MakeEnum { .. } | MirInstr::Nop => {}
    }
    Ok(())
}

fn operand_llvm(
    out: &mut String,
    op: &MirOperand,
    bb: usize,
    idx: usize,
) -> Result<String, String> {
    match op {
        MirOperand::ConstInt(n) => Ok(n.to_string()),
        MirOperand::ConstFloat(n) => Ok(format!("{}", *n as i64)), // stub: truncate
        MirOperand::ConstBool(true) => Ok("1".into()),
        MirOperand::ConstBool(false) => Ok("0".into()),
        MirOperand::ConstUnit => Ok("0".into()),
        MirOperand::ConstStr(_) => Ok("0".into()), // string ptr later
        MirOperand::Local(n) => {
            let tmp = format!("%ld{}_{}_{}", bb, idx, sanitize(n));
            writeln!(
                out,
                "  {tmp} = load i64, i64* %{}.addr, align 8",
                sanitize(n)
            )
            .unwrap();
            Ok(tmp)
        }
    }
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

fn block_label(id: usize) -> String {
    format!("bb{id}")
}

/// Emit WebAssembly text for the integer subset of MIR.
///
/// Control flow uses a block-index dispatch loop so arbitrary MIR CFGs lower
/// without full structured-control recovery.
pub fn emit_wasm_text(module: &MirModule) -> Result<String, String> {
    let mut out = String::new();
    writeln!(out, ";; S+N++ → WASM text (integer CFG backend)").unwrap();
    writeln!(out, "(module").unwrap();
    writeln!(
        out,
        "  (import \"env\" \"print_i64\" (func $print_i64 (param i64)))"
    )
    .unwrap();

    for f in &module.functions {
        emit_wasm_function(&mut out, f)?;
    }

    for f in &module.functions {
        if f.name == "main" {
            writeln!(out, "  (export \"main\" (func $main))").unwrap();
        }
    }
    writeln!(out, ")").unwrap();
    Ok(out)
}

fn collect_wasm_locals(f: &MirFunction) -> Vec<String> {
    let mut locals = f.params.clone();
    for bb in &f.blocks {
        for instr in &bb.stmts {
            let dest = match instr {
                MirInstr::Assign { dest, .. }
                | MirInstr::Binary { dest, .. }
                | MirInstr::Unary { dest, .. }
                | MirInstr::Call { dest, .. }
                | MirInstr::CallIndirect { dest, .. }
                | MirInstr::Index { dest, .. }
                | MirInstr::Field { dest, .. }
                | MirInstr::MakeStruct { dest, .. }
                | MirInstr::MakeClosure { dest, .. } => Some(dest.clone()),
                _ => None,
            };
            if let Some(d) = dest {
                if !locals.contains(&d) {
                    locals.push(d);
                }
            }
        }
    }
    locals
}

fn emit_wasm_function(out: &mut String, f: &MirFunction) -> Result<(), String> {
    let params = f
        .params
        .iter()
        .map(|p| format!("(param ${} i64)", sanitize(p)))
        .collect::<Vec<_>>()
        .join(" ");
    writeln!(
        out,
        "  (func ${} {params} (result i64)",
        sanitize(&f.name)
    )
    .unwrap();

    let locals = collect_wasm_locals(f);
    for loc in &locals {
        if !f.params.iter().any(|p| p == loc) {
            writeln!(out, "    (local ${} i64)", sanitize(loc)).unwrap();
        }
    }
    writeln!(out, "    (local $bb i32)").unwrap();
    writeln!(out, "    (local.set $bb (i32.const {}))", f.entry).unwrap();
    writeln!(out, "    (loop $dispatch").unwrap();

    let n = f.blocks.len().max(1);
    // Nested blocks: (block $b0 (block $b1 ... (block $default
    for i in 0..n {
        writeln!(out, "      (block $exit{i}").unwrap();
    }
    writeln!(out, "      (block $default").unwrap();

    // br_table targets: exit0, exit1, ... then default
    write!(out, "        (br_table").unwrap();
    for i in 0..n {
        write!(out, " $exit{i}").unwrap();
    }
    writeln!(out, " $default (local.get $bb))").unwrap();
    writeln!(out, "      ) ;; default").unwrap();
    writeln!(out, "      (i64.const 0) (return)").unwrap();


    for i in 0..n {
        if i < f.blocks.len() {
            emit_wasm_block_body(out, &f.blocks[i], &f.name)?;
        } else {
            writeln!(out, "      (i64.const 0) (return)").unwrap();
        }
        writeln!(out, "      ) ;; end exit{i}").unwrap();
    }

    writeln!(out, "      (br $dispatch)").unwrap();
    writeln!(out, "    ) ;; loop").unwrap();
    writeln!(out, "    (i64.const 0)").unwrap();
    writeln!(out, "  )").unwrap();
    Ok(())
}

fn emit_wasm_block_body(
    out: &mut String,
    bb: &MirBlock,
    _fname: &str,
) -> Result<(), String> {
    for (idx, instr) in bb.stmts.iter().enumerate() {
        match instr {
            MirInstr::Try { dest, value } => {
                emit_wasm_operand(out, value)?;
                writeln!(out, "        (local.set ${})", sanitize(dest)).unwrap();
            }
            MirInstr::Assign { dest, value } => {
                emit_wasm_operand(out, value)?;
                writeln!(out, "        (local.set ${})", sanitize(dest)).unwrap();
            }
            MirInstr::Binary {
                dest,
                op,
                left,
                right,
            } => {
                emit_wasm_operand(out, left)?;
                emit_wasm_operand(out, right)?;
                match op {
                    Token::Plus => writeln!(out, "        (i64.add)").unwrap(),
                    Token::Minus => writeln!(out, "        (i64.sub)").unwrap(),
                    Token::Star => writeln!(out, "        (i64.mul)").unwrap(),
                    Token::Slash => writeln!(out, "        (i64.div_s)").unwrap(),
                    Token::EqEq => {
                        writeln!(out, "        (i64.eq)").unwrap();
                        writeln!(out, "        (i64.extend_i32_u)").unwrap();
                    }
                    Token::BangEq => {
                        writeln!(out, "        (i64.ne)").unwrap();
                        writeln!(out, "        (i64.extend_i32_u)").unwrap();
                    }
                    Token::Lt => {
                        writeln!(out, "        (i64.lt_s)").unwrap();
                        writeln!(out, "        (i64.extend_i32_u)").unwrap();
                    }
                    Token::Lte => {
                        writeln!(out, "        (i64.le_s)").unwrap();
                        writeln!(out, "        (i64.extend_i32_u)").unwrap();
                    }
                    Token::Gt => {
                        writeln!(out, "        (i64.gt_s)").unwrap();
                        writeln!(out, "        (i64.extend_i32_u)").unwrap();
                    }
                    Token::Gte => {
                        writeln!(out, "        (i64.ge_s)").unwrap();
                        writeln!(out, "        (i64.extend_i32_u)").unwrap();
                    }
                    other => {
                        return Err(format!("wasm: unsupported binop {:?}", other));
                    }
                }
                writeln!(out, "        (local.set ${})", sanitize(dest)).unwrap();
            }
            MirInstr::Unary { dest, op, value } => {
                match op {
                    Token::Minus => {
                        writeln!(out, "        (i64.const 0)").unwrap();
                        emit_wasm_operand(out, value)?;
                        writeln!(out, "        (i64.sub)").unwrap();
                    }
                    Token::Bang => {
                        emit_wasm_operand(out, value)?;
                        writeln!(out, "        (i64.eqz)").unwrap();
                        writeln!(out, "        (i64.extend_i32_u)").unwrap();
                    }
                    other => return Err(format!("wasm: unsupported unop {:?}", other)),
                }
                writeln!(out, "        (local.set ${})", sanitize(dest)).unwrap();
            }
            MirInstr::Call { dest, name, args } => {
                if name == "print" && args.len() == 1 {
                    emit_wasm_operand(out, &args[0])?;
                    writeln!(out, "        (call $print_i64)").unwrap();
                    writeln!(out, "        (i64.const 0)").unwrap();
                    writeln!(out, "        (local.set ${})", sanitize(dest)).unwrap();
                } else {
                    for a in args {
                        emit_wasm_operand(out, a)?;
                    }
                    writeln!(out, "        (call ${})", sanitize(name)).unwrap();
                    writeln!(out, "        (local.set ${})", sanitize(dest)).unwrap();
                }
            }
            MirInstr::Print(v) => {
                emit_wasm_operand(out, v)?;
                writeln!(out, "        (call $print_i64)").unwrap();
            }
            MirInstr::CallIndirect { dest, .. }
            | MirInstr::Index { dest, .. }
            | MirInstr::Field { dest, .. }
            | MirInstr::MakeStruct { dest, .. }
            | MirInstr::MakeClosure { dest, .. } => {
                let _ = idx;
                writeln!(
                    out,
                    "        (i64.const 0) (local.set ${}) ;; complex op stub",
                    sanitize(dest)
                )
                .unwrap();
            }
            MirInstr::SetField { .. } | MirInstr::MakeEnum { .. } | MirInstr::Nop => {}
        }
    }

    match &bb.terminator {
        MirTerminator::Return(None) => {
            writeln!(out, "        (i64.const 0) (return)").unwrap();
        }
        MirTerminator::Return(Some(op)) => {
            emit_wasm_operand(out, op)?;
            writeln!(out, "        (return)").unwrap();
        }
        MirTerminator::Jump(t) => {
            writeln!(out, "        (local.set $bb (i32.const {t}))").unwrap();
            writeln!(out, "        (br $dispatch)").unwrap();
        }
        MirTerminator::Branch {
            cond,
            then_bb,
            else_bb,
        } => {
            emit_wasm_operand(out, cond)?;
            writeln!(out, "        (i64.eqz)").unwrap();
            writeln!(out, "        (if (result i32)").unwrap();
            writeln!(out, "          (then (i32.const {else_bb}))").unwrap();
            writeln!(out, "          (else (i32.const {then_bb}))").unwrap();
            writeln!(out, "        )").unwrap();
            writeln!(out, "        (local.set $bb)").unwrap();
            writeln!(out, "        (br $dispatch)").unwrap();
        }
        MirTerminator::Unreachable => {
            writeln!(out, "        (unreachable)").unwrap();
        }
    }
    Ok(())
}

fn emit_wasm_operand(out: &mut String, op: &MirOperand) -> Result<(), String> {
    match op {
        MirOperand::ConstInt(n) => {
            writeln!(out, "        (i64.const {n})").unwrap();
        }
        MirOperand::ConstFloat(n) => {
            writeln!(out, "        (f64.const {n})").unwrap();
        }
        MirOperand::ConstBool(true) => {
            writeln!(out, "        (i64.const 1)").unwrap();
        }
        MirOperand::ConstBool(false) | MirOperand::ConstUnit => {
            writeln!(out, "        (i64.const 0)").unwrap();
        }
        MirOperand::ConstStr(_) => {
            writeln!(out, "        (i64.const 0) ;; string ptr stub").unwrap();
        }
        MirOperand::Local(n) => {
            writeln!(out, "        (local.get ${})", sanitize(n)).unwrap();
        }
    }
    Ok(())
}



/// High-level entry: lower program through HIR/MIR and emit chosen backend artifact.
pub fn emit_from_source(source: &str, backend: Backend) -> Result<String, String> {
    let mut program = crate::parser::parse_program(source)?;
    crate::stdlib::resolve(&program.imports)?;
    let _ = crate::sema::expand_trait_defaults(&mut program);
    crate::sema::check_program(&program)?;
    let hir = crate::hir::lower(&program);
    let mut mir = crate::mir::lower(&hir)?;
    crate::mir::optimize(&mut mir);
    match backend {
        Backend::Bytecode => {
            let funcs = crate::vm::compile_from_mir(&mir)?;
            Ok(format!(
                "; bytecode module: {} functions\n",
                funcs.len()
            ))
        }
        Backend::LlvmIr => emit_llvm_ir(&mir),
        Backend::WasmText => emit_wasm_text(&mir),
        Backend::NativePlan => emit_native_plan(&mir),
    }
}

/// Research output: how MIR would lower to native-ish ops (not executable).
pub fn emit_native_plan(module: &MirModule) -> Result<String, String> {
    let mut out = String::new();
    out.push_str("; S+N++ native lowering plan (research, non-executable)\n");
    out.push_str("; See docs/NATIVE.md — prefer Cranelift for a future real JIT.\n\n");
    for f in &module.functions {
        let _ = writeln!(out, "fn {} (params: {:?}) {{", f.name, f.params);
        for (bi, bb) in f.blocks.iter().enumerate() {
            let _ = writeln!(out, "  bb{}:", bi);
            for inst in &bb.stmts {
                let _ = writeln!(out, "    ; MIR  {:?}", inst);
                let _ = writeln!(out, "    ; NATIVE ≈ {}", native_hint(inst));
            }
            let _ = writeln!(out, "    ; TERM {:?}", bb.terminator);
            let _ = writeln!(out, "    ; NATIVE ≈ {}", native_term_hint(&bb.terminator));
        }
        out.push_str("}\n\n");
    }
    out.push_str("; register allocation: linear scan over locals (TODO)\n");
    out.push_str("; ABI: system-v / win64 TBD for cranelift feature\n");
    Ok(out)
}

fn native_hint(inst: &MirInstr) -> String {
    match inst {
        MirInstr::Assign { dest, value } => format!("mov %{dest}, {value:?}"),
        MirInstr::Binary { dest, op, left, right } => {
            format!("{:?} %{dest}, {left:?}, {right:?}", op)
        }
        MirInstr::Unary { dest, op, value } => format!("{:?} %{dest}, {value:?}", op),
        MirInstr::Call { dest, name, args } => {
            format!("call {name} -> %{dest}  args={args:?}")
        }
        MirInstr::Field { dest, object, field } => {
            format!("load %{dest}, {object:?}.{field}")
        }
        MirInstr::Index { dest, base, index } => {
            format!("load %{dest}, {base:?}[{index:?}]")
        }
        _ => format!("op {inst:?}"),
    }
}

fn native_term_hint(t: &MirTerminator) -> String {
    match t {
        MirTerminator::Jump(b) => format!("jmp bb{b}"),
        MirTerminator::Branch {
            cond,
            then_bb,
            else_bb,
        } => format!("cmp {cond:?}; jc bb{then_bb}; jmp bb{else_bb}"),
        MirTerminator::Return(v) => format!("ret {v:?}"),
        MirTerminator::Unreachable => "ud2".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_native_plan() {
        let src = "fn main() -> Int { return 1 + 2; }";
        let plan = emit_from_source(src, Backend::NativePlan).unwrap();
        assert!(plan.contains("native lowering plan"));
        assert!(plan.contains("fn main") || plan.contains("main"));
        assert!(plan.contains("NATIVE") || plan.contains("MIR"));
    }

    #[test]
    fn emits_llvm_for_arithmetic() {
        let src = "fn main() -> Int { return 20 + 22; }";
        let ir = emit_from_source(src, Backend::LlvmIr).unwrap();
        assert!(ir.contains("define i64 @main"));
        assert!(ir.contains("add i64") || ir.contains("i64 42") || ir.contains("ret i64"));
        assert!(ir.contains("ret i64"));
    }

    #[test]
    fn emits_wasm_cfg() {
        let src = "fn main() -> Int { return 1; }";
        let wat = emit_from_source(src, Backend::WasmText).unwrap();
        assert!(wat.contains("(module"));
        assert!(wat.contains("func $main"));
        assert!(wat.contains("br_table") || wat.contains("i64.const"));
        assert!(wat.contains("loop $dispatch") || wat.contains("$dispatch"));
    }

    #[test]
    fn emits_wasm_arithmetic() {
        let src = "fn main() -> Int { return 20 + 22; }";
        let wat = emit_from_source(src, Backend::WasmText).unwrap();
        assert!(wat.contains("i64.add") || wat.contains("i64.const 42"));
    }

    #[test]
    fn emits_llvm_if_branch() {
        let src = "fn main() -> Int { if 1 < 2 { return 1; } else { return 0; } }";
        let ir = emit_from_source(src, Backend::LlvmIr).unwrap();
        assert!(ir.contains("br i1") || ir.contains("icmp"));
    }

    #[test]
    fn emits_llvm_call() {
        let src = "fn add(a: Int, b: Int) -> Int { return a + b; } fn main() -> Int { return add(2, 3); }";
        let ir = emit_from_source(src, Backend::LlvmIr).unwrap();
        assert!(ir.contains("call i64 @add") || ir.contains("@add"));
        assert!(ir.contains("define i64 @add"));
    }
}


/// Research notes for a future optional `cranelift` feature (0.5.x+).
///
/// Not linked: keep the default binary dependency-free.
pub mod cranelift_research {
    //! Planned mapping (when feature is enabled):
    //!
    //! - `MirInstr::Binary { Add, .. }` → `cranelift_codegen::ir::InstBuilder::iadd`
    //! - `MirTerminator::Branch` → `brif` / `jump`
    //! - Function ABI: system-v or win64 via cranelift-native
    //!
    //! Enable sketch:
    //! ```toml
    //! [features]
    //! cranelift = ["dep:cranelift-codegen", "dep:cranelift-frontend", "dep:cranelift-native"]
    //! ```

    /// Returns a short status string for tooling / `snp emit-native`.
    pub fn status() -> &'static str {
        "cranelift: not linked (research only — see docs/NATIVE.md)"
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn status_mentions_research() {
            assert!(super::status().contains("research") || super::status().contains("not linked"));
        }
    }
}
