//! S+N++ recursive-descent parser.

use crate::ast::{
    Expr, Function, InterpPart, MatchArm, Pattern, Program, Stmt, StructDef, Type, Visibility,
};
use crate::diagnostics::{offset_to_loc, SourceLoc};
use crate::lexer::{SpannedToken, Token};

pub fn parse(source: &str) -> Result<Vec<Function>, String> {
    Ok(parse_program(source)?.functions)
}

pub fn parse_program(source: &str) -> Result<Program, String> {
    let tokens = crate::lexer::lex_spanned(source)?;
    let mut parser = Parser::new(tokens, source);
    parser.program()
}

struct Parser<'a> {
    tokens: Vec<SpannedToken>,
    position: usize,
    source: &'a str,
}

impl<'a> Parser<'a> {
    fn new(tokens: Vec<SpannedToken>, source: &'a str) -> Self {
        Self {
            tokens,
            position: 0,
            source,
        }
    }

    fn current(&self) -> &Token {
        &self.tokens[self.position].token
    }

    fn current_span(&self) -> (usize, usize) {
        let t = &self.tokens[self.position];
        (t.start, t.end)
    }

    fn loc(&self) -> SourceLoc {
        let (start, _) = self.current_span();
        offset_to_loc(self.source, start)
    }

    fn err(&self, msg: impl Into<String>) -> String {
        let loc = self.loc();
        format!("{} (line {}, column {})", msg.into(), loc.line, loc.column)
    }

    fn bump(&mut self) -> Token {
        let t = self.tokens[self.position].token.clone();
        self.position += 1;
        t
    }

    fn eat(&mut self, wanted: Token) -> Result<(), String> {
        if *self.current() == wanted {
            self.bump();
            Ok(())
        } else {
            Err(self.err(format!("expected {:?}, got {:?}", wanted, self.current())))
        }
    }

    #[allow(dead_code)]
    fn peek_is(&self, t: &Token) -> bool {
        self.current() == t
    }

    fn peek_at(&self, offset: usize) -> Option<&Token> {
        self.tokens.get(self.position + offset).map(|s| &s.token)
    }

    const MAX_MOD_DEPTH: usize = 8;

    fn program(&mut self) -> Result<Program, String> {
        let mut imports = Vec::new();
        let mut structs = Vec::new();
        let mut enums = Vec::new();
        let mut functions = Vec::new();
        let mut externs = Vec::new();
        self.parse_items(
            &mut imports,
            &mut structs,
            &mut enums,
            &mut functions,
            &mut externs,
            &[],
            0,
            true, // root: stop at Eof
        )?;
        Ok(Program {
            imports,
            structs,
            enums,
            functions,
            externs,
        })
    }

    fn parse_items(
        &mut self,
        imports: &mut Vec<String>,
        structs: &mut Vec<StructDef>,
        enums: &mut Vec<crate::ast::EnumDef>,
        functions: &mut Vec<Function>,
        externs: &mut Vec<crate::ast::ExternFn>,
        module_path: &[String],
        depth: usize,
        until_eof: bool,
    ) -> Result<(), String> {
        while *self.current() != Token::Eof {
            if !until_eof && *self.current() == Token::RBrace {
                break;
            }
            // optional pub
            let is_pub = if *self.current() == Token::Pub {
                self.bump();
                true
            } else {
                false
            };
            let vis = if is_pub || module_path.is_empty() {
                // top-level items are public by default
                Visibility::Public
            } else {
                Visibility::Private
            };
            // if pub was consumed but next is not an item keyword, error
            match self.current() {
                Token::Import => {
                    if is_pub {
                        return Err(self.err("`pub` cannot apply to import"));
                    }
                    imports.push(self.import_path()?);
                }
                Token::Struct => {
                    let mut s = self.struct_def()?;
                    s.visibility = vis;
                    s.module_path = module_path.to_vec();
                    if !module_path.is_empty() {
                        s.name = format!("{}.{}", module_path.join("."), s.name);
                    }
                    structs.push(s);
                }
                Token::Enum => {
                    let mut e = self.enum_def()?;
                    e.visibility = vis;
                    e.module_path = module_path.to_vec();
                    if !module_path.is_empty() {
                        e.name = format!("{}.{}", module_path.join("."), e.name);
                    }
                    enums.push(e);
                }
                Token::Fn => {
                    let mut f = self.function()?;
                    f.visibility = vis;
                    f.module_path = module_path.to_vec();
                    if !module_path.is_empty() {
                        f.name = format!("{}.{}", module_path.join("."), f.name);
                    }
                    functions.push(f);
                }
                Token::Extern => {
                    if is_pub {
                        return Err(self.err("`pub` cannot apply to extern"));
                    }
                    externs.push(self.extern_fn()?);
                }
                Token::Impl => {
                    if is_pub {
                        return Err(self.err("`pub` cannot apply to impl"));
                    }
                    functions.extend(self.impl_block()?);
                }
                Token::Mod => {
                    self.bump();
                    let name = match self.bump() {
                        Token::Ident(n) => n,
                        _ => return Err(self.err("expected module name after `mod`")),
                    };
                    if *self.current() == Token::LBrace {
                        // nested inline module
                        if depth + 1 > Self::MAX_MOD_DEPTH {
                            return Err(self.err(format!(
                                "module nesting exceeds max depth {}",
                                Self::MAX_MOD_DEPTH
                            )));
                        }
                        self.bump();
                        let mut path = module_path.to_vec();
                        path.push(name);
                        self.parse_items(
                            imports,
                            structs,
                            enums,
                            functions,
                            externs,
                            &path,
                            depth + 1,
                            false,
                        )?;
                        self.eat(Token::RBrace)?;
                    } else {
                        // file module marker
                        if *self.current() == Token::Semi {
                            self.bump();
                        }
                        imports.push(format!("mod:{name}"));
                    }
                }
                other => {
                    return Err(self.err(format!(
                        "expected import, struct, enum, fn, extern, impl or mod, got {:?}",
                        other
                    )));
                }
            }
        }
        Ok(())
    }

    /// `extern "C" fn name(a: Int, b: Int) -> Int;`
    
    /// `impl Player { fn heal(self, n: Int) -> Player { ... } }`
    /// Lowers to functions named `Player.heal` with `self: Player` as first param.
    fn impl_block(&mut self) -> Result<Vec<Function>, String> {
        self.eat(Token::Impl)?;
        let type_name = match self.bump() {
            Token::Ident(n) => n,
            _ => return Err(self.err("expected type name after `impl`")),
        };
        self.eat(Token::LBrace)?;
        let mut methods = Vec::new();
        while *self.current() != Token::RBrace && *self.current() != Token::Eof {
            let mut f = self.function()?;
            // Ensure first param is self with struct type if named self
            if let Some(first) = f.params.first() {
                if first == "self" {
                    if f.param_types.is_empty() {
                        f.param_types.push(Type::Struct(type_name.clone()));
                    } else {
                        f.param_types[0] = Type::Struct(type_name.clone());
                    }
                }
            }
            f.name = format!("{}.{}", type_name, f.name);
            methods.push(f);
        }
        self.eat(Token::RBrace)?;
        Ok(methods)
    }

fn extern_fn(&mut self) -> Result<crate::ast::ExternFn, String> {
        self.eat(Token::Extern)?;
        let abi = match self.bump() {
            Token::Str(s) => s,
            _ => return Err(self.err("expected ABI string after `extern` (e.g. \"C\")")),
        };
        self.eat(Token::Fn)?;
        let name = match self.bump() {
            Token::Ident(n) => n,
            _ => return Err(self.err("expected function name in extern declaration")),
        };
        self.eat(Token::LParen)?;
        let mut params = Vec::new();
        let mut param_types = Vec::new();
        if *self.current() != Token::RParen {
            loop {
                match self.bump() {
                    Token::Ident(n) => params.push(n),
                    _ => return Err(self.err("expected parameter name in extern fn")),
                }
                self.eat(Token::Colon)?;
                param_types.push(self.parse_type()?);
                if *self.current() == Token::Comma {
                    self.bump();
                } else {
                    break;
                }
            }
        }
        self.eat(Token::RParen)?;
        let return_type = if *self.current() == Token::Arrow {
            self.bump();
            self.parse_type()?
        } else {
            Type::Unit
        };
        if *self.current() == Token::Semi {
            self.bump();
        }
        Ok(crate::ast::ExternFn {
            abi,
            name,
            params,
            param_types,
            return_type,
        })
    }

    fn import_path(&mut self) -> Result<String, String> {
        self.eat(Token::Import)?;
        let mut parts = Vec::new();
        match self.bump() {
            Token::Ident(name) => parts.push(name),
            token => return Err(format!("expected module name after import, got {:?}", token)),
        }
        while *self.current() == Token::Dot {
            self.bump();
            match self.bump() {
                Token::Ident(name) => parts.push(name),
                token => return Err(format!("expected name after `.`, got {:?}", token)),
            }
        }
        if *self.current() == Token::Semi { self.bump(); }
        Ok(parts.join("."))
    }


    fn enum_def(&mut self) -> Result<crate::ast::EnumDef, String> {
        self.eat(Token::Enum)?;
        let name = match self.bump() {
            Token::Ident(n) => n,
            _ => return Err(self.err("expected enum name")),
        };
        self.eat(Token::LBrace)?;
        let mut variants = Vec::new();
        while *self.current() != Token::RBrace && *self.current() != Token::Eof {
            let vname = match self.bump() {
                Token::Ident(n) => n,
                _ => return Err(self.err("expected variant name")),
            };
            let mut payload = Vec::new();
            if *self.current() == Token::LParen {
                self.bump();
                if *self.current() != Token::RParen {
                    loop {
                        payload.push(self.parse_type()?);
                        if *self.current() == Token::Comma {
                            self.bump();
                        } else {
                            break;
                        }
                    }
                }
                self.eat(Token::RParen)?;
            }
            variants.push((vname, payload));
            if *self.current() == Token::Comma {
                self.bump();
            }
        }
        self.eat(Token::RBrace)?;
        Ok(crate::ast::EnumDef { name, visibility: Visibility::Public, module_path: vec![], variants })
    }

    fn struct_def(&mut self) -> Result<StructDef, String> {
        self.eat(Token::Struct)?;
        let name = match self.bump() {
            Token::Ident(n) => n,
            _ => return Err("expected struct name".into()),
        };
        self.eat(Token::LBrace)?;
        let mut fields = Vec::new();
        while *self.current() != Token::RBrace && *self.current() != Token::Eof {
            let fname = match self.bump() {
                Token::Ident(n) => n,
                _ => return Err("expected field name".into()),
            };
            self.eat(Token::Colon)?;
            let fty = self.parse_type()?;
            fields.push((fname, fty));
            if *self.current() == Token::Comma { self.bump(); }
        }
        self.eat(Token::RBrace)?;
        Ok(StructDef { name, visibility: Visibility::Public, module_path: vec![], fields })
    }

    fn parse_type(&mut self) -> Result<Type, String> {
        if *self.current() == Token::Amp {
            self.bump();
            if *self.current() == Token::Mut {
                self.bump();
                return Ok(Type::MutRef(Box::new(self.parse_type()?)));
            }
            return Ok(Type::Ref(Box::new(self.parse_type()?)));
        }
        let base = match self.bump() {
            Token::Ident(name) => match name.as_str() {
                "Int" => Type::Int,
                "String" => Type::Str,
                "Bool" => Type::Bool,
                "Unit" => Type::Unit,
                "Channel" => Type::Channel,
                "Thread" => Type::Thread,
                "List" => Type::List,
                "Map" => Type::Map,
                other => Type::Struct(other.to_string()), // user type or generic
            },
            token => return Err(format!("expected type, got {:?}", token)),
        };
        // List[T] or Map[K, V]
        if *self.current() == Token::LBracket {
            self.bump();
            match base {
                Type::List => {
                    let inner = self.parse_type()?;
                    self.eat(Token::RBracket)?;
                    Ok(Type::ListOf(Box::new(inner)))
                }
                Type::Map => {
                    let k = self.parse_type()?;
                    self.eat(Token::Comma)?;
                    let v = self.parse_type()?;
                    self.eat(Token::RBracket)?;
                    Ok(Type::MapOf(Box::new(k), Box::new(v)))
                }
                Type::Struct(name) if name.len() == 1 || name.chars().all(|c| c.is_uppercase()) => {
                    // treat as generic application placeholder — keep Struct name
                    let _ = self.parse_type()?;
                    while *self.current() == Token::Comma {
                        self.bump();
                        let _ = self.parse_type()?;
                    }
                    self.eat(Token::RBracket)?;
                    Ok(Type::Struct(name))
                }
                other => {
                    // Generic type param style T
                    let _ = self.parse_type()?;
                    self.eat(Token::RBracket)?;
                    Ok(other)
                }
            }
        } else {
            Ok(base)
        }
    }

    fn function(&mut self) -> Result<Function, String> {
        self.eat(Token::Fn)?;
        let name = match self.bump() {
            Token::Ident(n) => n,
            _ => return Err("expected function name".into()),
        };
        let mut type_params = Vec::new();
        if *self.current() == Token::LBracket {
            self.bump();
            loop {
                match self.bump() {
                    Token::Ident(t) => type_params.push(t),
                    _ => return Err("expected type parameter".into()),
                }
                if *self.current() == Token::Comma { self.bump(); } else { break; }
            }
            self.eat(Token::RBracket)?;
        }
        self.eat(Token::LParen)?;
        let mut params = Vec::new();
        let mut param_types = Vec::new();
        if *self.current() != Token::RParen {
            loop {
                match self.bump() {
                    Token::Ident(n) => params.push(n),
                    _ => return Err("expected parameter name".into()),
                }
                if *self.current() == Token::Colon {
                    self.bump();
                    param_types.push(self.parse_type()?);
                } else if params.last().map(|p| p.as_str()) == Some("self") {
                    // `self` without explicit type — filled by `impl` block
                    param_types.push(Type::Unknown);
                } else {
                    return Err(self.err("expected `:` after parameter name"));
                }
                if *self.current() == Token::Comma { self.bump(); } else { break; }
            }
        }
        self.eat(Token::RParen)?;
        let return_type = if *self.current() == Token::Arrow {
            self.bump();
            self.parse_type()?
        } else {
            Type::Unit
        };
        self.eat(Token::LBrace)?;
        let body = self.block()?;
        Ok(Function { name, visibility: Visibility::Public, module_path: vec![], params, param_types, return_type, type_params, body })
    }

    fn block(&mut self) -> Result<Vec<Stmt>, String> {
        let mut statements = Vec::new();
        while *self.current() != Token::RBrace && *self.current() != Token::Eof {
            statements.push(self.statement()?);
            if *self.current() == Token::Semi { self.bump(); }
        }
        self.eat(Token::RBrace)?;
        Ok(statements)
    }

    fn statement(&mut self) -> Result<Stmt, String> {
        match self.current() {
            Token::Let => { self.bump(); self.let_statement() }
            Token::If => self.if_statement(),
            Token::While => self.while_statement(),
            Token::For => self.for_statement(),
            Token::Match => self.match_statement(),
            Token::Return => { self.bump(); Ok(Stmt::Return(self.expression()?)) }
            Token::Break => { self.bump(); Ok(Stmt::Break) }
            Token::Continue => { self.bump(); Ok(Stmt::Continue) }
            Token::Ident(name) => {
                // assignment or field assign or expr
                if self.peek_at(1) == Some(&Token::Eq) {
                    let name = name.clone();
                    self.bump(); self.bump();
                    return Ok(Stmt::Assign(name, self.expression()?));
                }
                // Field assign: Ident Dot Ident Eq
                if self.peek_at(1) == Some(&Token::Dot) {
                    if let Some(Token::Ident(_)) = self.peek_at(2) {
                        if self.peek_at(3) == Some(&Token::Eq) {
                            let obj_name = name.clone();
                            self.bump(); // ident
                            self.bump(); // dot
                            let field = match self.bump() {
                                Token::Ident(f) => f,
                                _ => return Err("expected field name".into()),
                            };
                            self.bump(); // =
                            let value = self.expression()?;
                            return Ok(Stmt::FieldAssign {
                                object: Expr::Var(obj_name),
                                field,
                                value,
                            });
                        }
                    }
                }
                Ok(Stmt::Expr(self.expression()?))
            }
            _ => Ok(Stmt::Expr(self.expression()?)),
        }
    }

    fn let_statement(&mut self) -> Result<Stmt, String> {
        let name = match self.bump() {
            Token::Ident(n) => n,
            _ => return Err("expected variable name after `let`".into()),
        };
        let declared = if *self.current() == Token::Colon {
            self.bump();
            Some(self.parse_type()?)
        } else {
            None
        };
        if *self.current() != Token::Eq {
            if let Some(ty) = declared {
                return Ok(Stmt::Decl(name, ty));
            }
            return Err("let requires an initializer or explicit type".into());
        }
        self.bump();
        Ok(Stmt::Let(name, self.expression()?))
    }

    fn if_statement(&mut self) -> Result<Stmt, String> {
        self.bump();
        let condition = self.expression()?;
        self.eat(Token::LBrace)?;
        let yes = self.block()?;
        let no = if *self.current() == Token::Else {
            self.bump();
            if *self.current() == Token::If {
                vec![self.if_statement()?]
            } else {
                self.eat(Token::LBrace)?;
                self.block()?
            }
        } else {
            Vec::new()
        };
        Ok(Stmt::If(condition, yes, no))
    }

    fn while_statement(&mut self) -> Result<Stmt, String> {
        self.bump();
        let condition = self.expression()?;
        self.eat(Token::LBrace)?;
        Ok(Stmt::While(condition, self.block()?))
    }

    fn for_statement(&mut self) -> Result<Stmt, String> {
        self.bump(); // for
        let var = match self.bump() {
            Token::Ident(n) => n,
            _ => return Err("expected loop variable after `for`".into()),
        };
        self.eat(Token::In)?;
        // Could be range `a..b` or collection expression
        let first = self.expression()?;
        if *self.current() == Token::DotDot {
            self.bump();
            let end = self.expression()?;
            self.eat(Token::LBrace)?;
            let body = self.block()?;
            Ok(Stmt::For { var, start: first, end, body })
        } else {
            self.eat(Token::LBrace)?;
            let body = self.block()?;
            Ok(Stmt::ForIn { var, iter: first, body })
        }
    }

    fn match_statement(&mut self) -> Result<Stmt, String> {
        self.bump(); // match
        let scrutinee = self.expression()?;
        self.eat(Token::LBrace)?;
        let mut arms = Vec::new();
        while *self.current() != Token::RBrace && *self.current() != Token::Eof {
            let pat = self.pattern()?;
            let guard = if *self.current() == Token::If {
                self.bump();
                Some(self.expression()?)
            } else {
                None
            };
            self.eat(Token::FatArrow)?;
            self.eat(Token::LBrace)?;
            let body = self.block()?;
            arms.push(MatchArm {
                pattern: pat,
                guard,
                body,
            });
            if *self.current() == Token::Comma { self.bump(); }
        }
        self.eat(Token::RBrace)?;
        Ok(Stmt::Match { scrutinee, arms })
    }

    fn pattern(&mut self) -> Result<Pattern, String> {
        let first = self.primary_pattern()?;
        if *self.current() != Token::Pipe {
            return Ok(first);
        }
        let mut alts = vec![first];
        while *self.current() == Token::Pipe {
            self.bump();
            alts.push(self.primary_pattern()?);
        }
        Ok(Pattern::Or(alts))
    }

    fn primary_pattern(&mut self) -> Result<Pattern, String> {
        match self.bump() {
            Token::Ident(n) if n == "_" => Ok(Pattern::Wildcard),
            Token::Ident(n) => {
                if *self.current() == Token::Dot {
                    self.bump();
                    let variant = match self.bump() {
                        Token::Ident(v) => v,
                        _ => return Err(self.err("expected variant after `.` in pattern")),
                    };
                    let mut bindings = Vec::new();
                    if *self.current() == Token::LParen {
                        self.bump();
                        if *self.current() != Token::RParen {
                            loop {
                                match self.bump() {
                                    Token::Ident(b) => bindings.push(b),
                                    _ => return Err(self.err("expected binding in variant pattern")),
                                }
                                if *self.current() == Token::Comma {
                                    self.bump();
                                } else {
                                    break;
                                }
                            }
                        }
                        self.eat(Token::RParen)?;
                    }
                    Ok(Pattern::Variant {
                        enum_name: Some(n),
                        variant,
                        bindings,
                    })
                } else if *self.current() == Token::LParen {
                    self.bump();
                    let mut bindings = Vec::new();
                    if *self.current() != Token::RParen {
                        loop {
                            match self.bump() {
                                Token::Ident(b) => bindings.push(b),
                                _ => return Err(self.err("expected binding in variant pattern")),
                            }
                            if *self.current() == Token::Comma {
                                self.bump();
                            } else {
                                break;
                            }
                        }
                    }
                    self.eat(Token::RParen)?;
                    Ok(Pattern::Variant {
                        enum_name: None,
                        variant: n,
                        bindings,
                    })
                } else if *self.current() == Token::LBrace {
                    self.bump();
                    let mut fields = Vec::new();
                    while *self.current() != Token::RBrace && *self.current() != Token::Eof {
                        let field = match self.bump() {
                            Token::Ident(f) => f,
                            _ => return Err(self.err("expected field name in struct pattern")),
                        };
                        let binding = if *self.current() == Token::Colon {
                            self.bump();
                            match self.bump() {
                                Token::Ident(b) => b,
                                _ => {
                                    return Err(
                                        self.err("expected binding after `:` in struct pattern")
                                    )
                                }
                            }
                        } else {
                            field.clone()
                        };
                        fields.push((field, binding));
                        if *self.current() == Token::Comma {
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    self.eat(Token::RBrace)?;
                    Ok(Pattern::Struct { name: n, fields })
                } else {
                    Ok(Pattern::Var(n))
                }
            }
            Token::Int(v) => Ok(Pattern::Int(v)),
            Token::True => Ok(Pattern::Bool(true)),
            Token::False => Ok(Pattern::Bool(false)),
            Token::Str(s) => Ok(Pattern::Str(s)),
            other => Err(format!("invalid match pattern {:?}", other)),
        }
    }


    fn expression(&mut self) -> Result<Expr, String> { self.compare() }

    fn compare(&mut self) -> Result<Expr, String> {
        let mut e = self.add()?;
        while matches!(self.current(), Token::EqEq | Token::BangEq | Token::Lt | Token::Lte | Token::Gt | Token::Gte) {
            let op = self.bump();
            e = Expr::Binary(Box::new(e), op, Box::new(self.add()?));
        }
        Ok(e)
    }

    fn add(&mut self) -> Result<Expr, String> {
        let mut e = self.multiply()?;
        while matches!(self.current(), Token::Plus | Token::Minus) {
            let op = self.bump();
            e = Expr::Binary(Box::new(e), op, Box::new(self.multiply()?));
        }
        Ok(e)
    }

    fn multiply(&mut self) -> Result<Expr, String> {
        let mut e = self.unary()?;
        while matches!(self.current(), Token::Star | Token::Slash) {
            let op = self.bump();
            e = Expr::Binary(Box::new(e), op, Box::new(self.unary()?));
        }
        Ok(e)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if matches!(self.current(), Token::Minus | Token::Bang) {
            let op = self.bump();
            return Ok(Expr::Unary(op, Box::new(self.unary()?)));
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr, String> {
        let mut e = self.atom()?;
        loop {
            if *self.current() == Token::LBracket {
                self.bump();
                let idx = self.expression()?;
                self.eat(Token::RBracket)?;
                e = Expr::Index(Box::new(e), Box::new(idx));
            } else if *self.current() == Token::Dot {
                self.bump();
                let field = match self.bump() {
                    Token::Ident(f) => f,
                    t => return Err(format!("expected field name after `.`, got {:?}", t)),
                };
                // `EnumName.Variant` (both capitalized) → unit variant lit when no call follows
                if matches!(&e, Expr::Var(n) if n.chars().next().map(|c| c.is_uppercase()).unwrap_or(false))
                    && field.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                    && *self.current() != Token::LParen
                {
                    if let Expr::Var(enum_name) = e {
                        e = Expr::VariantLit {
                            enum_name,
                            variant: field,
                            args: vec![],
                        };
                    }
                } else {
                    e = Expr::Field(Box::new(e), field);
                }
            } else if *self.current() == Token::LParen {
                if matches!(e, Expr::Var(_)) {
                    let name = if let Expr::Var(n) = &e { n.clone() } else { unreachable!() };
                    self.bump();
                    let mut args = Vec::new();
                    if *self.current() != Token::RParen {
                        loop {
                            args.push(self.expression()?);
                            if *self.current() == Token::Comma { self.bump(); } else { break; }
                        }
                    }
                    self.eat(Token::RParen)?;
                    e = Expr::Call(name, args);
                } else if let Expr::Field(obj, method) = e {
                    self.bump();
                    let mut args = Vec::new();
                    if *self.current() != Token::RParen {
                        loop {
                            args.push(self.expression()?);
                            if *self.current() == Token::Comma { self.bump(); } else { break; }
                        }
                    }
                    self.eat(Token::RParen)?;
                    // `module.fn(args)` or `Enum.Variant(args)` — resolve in typecheck:
                    // emit Call with dotted name; enum construction desugars when name is an enum.
                    if let Expr::Var(base) = obj.as_ref() {
                        e = Expr::Call(format!("{base}.{method}"), args);
                    } else {
                        e = Expr::MethodCall {
                            receiver: obj,
                            method,
                            args,
                        };
                    }
                } else {
                    self.bump();
                    let mut args = Vec::new();
                    if *self.current() != Token::RParen {
                        loop {
                            args.push(self.expression()?);
                            if *self.current() == Token::Comma { self.bump(); } else { break; }
                        }
                    }
                    self.eat(Token::RParen)?;
                    e = Expr::CallExpr(Box::new(e), args);
                }
            } else {
                break;
            }
        }
        Ok(e)
    }

    fn atom(&mut self) -> Result<Expr, String> {
        match self.bump() {
            Token::Int(v) => Ok(Expr::Int(v)),
            Token::Str(s) => Ok(self.maybe_interp(s)),
            Token::True => Ok(Expr::Bool(true)),
            Token::False => Ok(Expr::Bool(false)),
            Token::Fn => self.parse_lambda(),
            Token::Ident(name) => {
                // Struct literal: Name { field: expr, ... }
                if *self.current() == Token::LBrace {
                    // Struct lit if `Name { }` or `Name { field: ...`
                    let is_struct = matches!(self.peek_at(1), Some(Token::RBrace))
                        || (matches!(self.peek_at(1), Some(Token::Ident(_)))
                            && matches!(self.peek_at(2), Some(Token::Colon)));
                    if is_struct {
                        self.bump(); // {
                        let mut fields = Vec::new();
                        while *self.current() != Token::RBrace && *self.current() != Token::Eof {
                            let fname = match self.bump() {
                                Token::Ident(f) => f,
                                _ => return Err("expected field name in struct literal".into()),
                            };
                            self.eat(Token::Colon)?;
                            let fexpr = self.expression()?;
                            fields.push((fname, fexpr));
                            if *self.current() == Token::Comma { self.bump(); }
                        }
                        self.eat(Token::RBrace)?;
                        return Ok(Expr::StructLit { name, fields });
                    }
                }
                if *self.current() == Token::LParen {
                    self.bump();
                    let mut args = Vec::new();
                    if *self.current() != Token::RParen {
                        loop {
                            args.push(self.expression()?);
                            if *self.current() == Token::Comma { self.bump(); } else { break; }
                        }
                    }
                    self.eat(Token::RParen)?;
                    Ok(Expr::Call(name, args))
                } else {
                    Ok(Expr::Var(name))
                }
            }
            Token::LParen => {
                let e = self.expression()?;
                self.eat(Token::RParen)?;
                Ok(e)
            }
            token => Err(self.err(format!("unexpected token {:?}", token))),
        }
    }

    fn parse_lambda(&mut self) -> Result<Expr, String> {
        // fn already consumed
        let mut type_params = Vec::new();
        if *self.current() == Token::LBracket {
            self.bump();
            loop {
                match self.bump() {
                    Token::Ident(t) => type_params.push(t),
                    _ => return Err("expected type parameter in lambda".into()),
                }
                if *self.current() == Token::Comma { self.bump(); } else { break; }
            }
            self.eat(Token::RBracket)?;
        }
        self.eat(Token::LParen)?;
        let mut params = Vec::new();
        let mut param_types = Vec::new();
        if *self.current() != Token::RParen {
            loop {
                match self.bump() {
                    Token::Ident(n) => params.push(n),
                    _ => return Err("expected parameter in lambda".into()),
                }
                self.eat(Token::Colon)?;
                param_types.push(self.parse_type()?);
                if *self.current() == Token::Comma { self.bump(); } else { break; }
            }
        }
        self.eat(Token::RParen)?;
        let return_type = if *self.current() == Token::Arrow {
            self.bump();
            self.parse_type()?
        } else {
            Type::Unit
        };
        self.eat(Token::LBrace)?;
        let body = self.block()?;
        let _ = type_params;
        Ok(Expr::Lambda { params, param_types, return_type, body })
    }

        /// Parse `"hello {name}"` into Interp if `{` present, else plain Str.
    fn maybe_interp(&self, s: String) -> Expr {
        if !s.contains('{') {
            return Expr::Str(s);
        }
        let mut parts = Vec::new();
        let mut rest = s.as_str();
        while let Some(start) = rest.find('{') {
            if start > 0 {
                parts.push(InterpPart::Lit(rest[..start].to_string()));
            }
            rest = &rest[start + 1..];
            if let Some(end) = rest.find('}') {
                let name = rest[..end].trim();
                if !name.is_empty() {
                    parts.push(InterpPart::Expr(Expr::Var(name.to_string())));
                }
                rest = &rest[end + 1..];
            } else {
                parts.push(InterpPart::Lit(format!("{{{rest}")));
                rest = "";
                break;
            }
        }
        if !rest.is_empty() {
            parts.push(InterpPart::Lit(rest.to_string()));
        }
        if parts.is_empty() {
            Expr::Str(s)
        } else {
            Expr::Interp(parts)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_for_loop() {
        let prog = parse_program("fn main() { for i in 0..5 { print(i); } }").unwrap();
        assert!(matches!(prog.functions[0].body[0], Stmt::For { .. }));
    }

    #[test]
    fn parses_for_in() {
        let prog = parse_program("fn main() { for x in xs { print(x); } }").unwrap();
        assert!(matches!(prog.functions[0].body[0], Stmt::ForIn { .. }));
    }

    #[test]
    fn parses_struct() {
        let prog = parse_program("struct Player { hp: Int, name: String } fn main() {}").unwrap();
        assert_eq!(prog.structs.len(), 1);
        assert_eq!(prog.structs[0].name, "Player");
    }

    #[test]
    fn parses_enum_variant_pattern() {
        let src = r#"
            enum OptionInt { None, Some(Int) }
            fn main() {
                match OptionInt.Some(1) {
                    OptionInt.Some(n) => { print(n); }
                    OptionInt.None => {}
                }
            }
        "#;
        let prog = parse_program(src).expect("parse");
        let Stmt::Match { arms, .. } = &prog.functions[0].body[0] else {
            panic!("expected match");
        };
        match &arms[0].pattern {
            Pattern::Variant { variant, bindings, .. } => {
                assert_eq!(variant, "Some");
                assert_eq!(bindings, &vec!["n".to_string()]);
            }
            other => panic!("expected variant pattern, got {other:?}"),
        }
    }

    #[test]
    fn parses_struct_pattern() {
        let src = r#"
            struct Player { hp: Int, name: String }
            fn main() {
                match Player { hp: 1, name: "a" } {
                    Player { hp, name } => { print(hp); }
                    Player { hp: h, name: n } => { print(h); }
                }
            }
        "#;
        let prog = parse_program(src).unwrap();
        let Stmt::Match { arms, .. } = &prog.functions[0].body[0] else {
            panic!("match");
        };
        match &arms[0].pattern {
            Pattern::Struct { name, fields } => {
                assert_eq!(name, "Player");
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0], ("hp".into(), "hp".into()));
            }
            other => panic!("expected struct pattern, got {other:?}"),
        }
        match &arms[1].pattern {
            Pattern::Struct { fields, .. } => {
                assert_eq!(fields[0], ("hp".into(), "h".into()));
            }
            other => panic!("expected renamed fields, got {other:?}"),
        }
    }

    #[test]
    fn parses_match_guard() {
        let src = r#"
            fn main() {
                match 5 {
                    n if n > 3 => { print(n); }
                    _ => {}
                }
            }
        "#;
        let prog = parse_program(src).unwrap();
        let Stmt::Match { arms, .. } = &prog.functions[0].body[0] else {
            panic!("match");
        };
        assert!(arms[0].guard.is_some());
        assert!(arms[1].guard.is_none());
    }

    #[test]
    fn parses_match() {
        let prog = parse_program("fn main() { match x { 1 => { print(1); } _ => { print(0); } } }").unwrap();
        assert!(matches!(prog.functions[0].body[0], Stmt::Match { .. }));
    }

    #[test]
    fn parses_unary_and_index() {
        let prog = parse_program("fn main() { let x = -42; let v = xs[0]; }").unwrap();
        assert!(prog.functions[0].body.len() >= 2);
    }
}
