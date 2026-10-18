//! S+N++ recursive-descent parser.

use crate::ast::{
    Expr, Function, InterpPart, MatchArm, MatchExprArm, Pattern, Program, Stmt, StructDef, Type, Visibility, TraitDef, TraitMethod,
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
        let mut traits = Vec::new();
        let mut functions = Vec::new();
        let mut externs = Vec::new();
        let mut consts = Vec::new();
        let mut uses = Vec::new();
        self.parse_items(
            &mut imports,
            &mut structs,
            &mut enums,
            &mut traits,
            &mut functions,
            &mut externs,
            &mut consts,
            &mut uses,
            &[],
            0,
            true, // root: stop at Eof
        )?;
        Ok(Program {
            imports,
            structs,
            enums,
            traits,
            functions,
            externs,
            consts,
            uses,
        })
    }

    fn parse_items(
        &mut self,
        imports: &mut Vec<String>,
        structs: &mut Vec<StructDef>,
        enums: &mut Vec<crate::ast::EnumDef>,
        traits: &mut Vec<TraitDef>,
        functions: &mut Vec<Function>,
        externs: &mut Vec<crate::ast::ExternFn>,
        consts: &mut Vec<(String, Expr)>,
        uses: &mut Vec<crate::ast::UseDecl>,
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
                Token::Trait => {
                    let mut t = self.trait_def()?;
                    t.visibility = vis;
                    t.module_path = module_path.to_vec();
                    if !module_path.is_empty() {
                        t.name = format!("{}.{}", module_path.join("."), t.name);
                    }
                    traits.push(t);
                }
                Token::Const => {
                    self.bump();
                    let name = match self.bump() {
                        Token::Ident(n) => n,
                        _ => return Err(self.err("expected const name")),
                    };
                    self.eat(Token::Eq)?;
                    let expr = self.expression()?;
                    if *self.current() == Token::Semi {
                        self.bump();
                    }
                    consts.push((name, expr));
                }
                Token::Use => {
                    uses.push(self.parse_use()?);
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
                            traits,
                            functions,
                            externs,
                            consts,
                            uses,
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
                        "expected import, struct, enum, trait, fn, extern, impl or mod, got {:?}",
                        other
                    )));
                }
            }
        }
        Ok(())
    }

    /// `extern "C" fn name(a: Int, b: Int) -> Int;`
    
    /// `impl Player { ... }` or `impl Drawable for Player { ... }`
    /// Methods lower to `Player.method` with self: Player.
    fn impl_block(&mut self) -> Result<Vec<Function>, String> {
        self.eat(Token::Impl)?;
        let first = match self.bump() {
            Token::Ident(n) => n,
            _ => return Err(self.err("expected type or trait name after `impl`")),
        };
        let (type_name, trait_name) = if *self.current() == Token::For {
            self.bump();
            let ty = match self.bump() {
                Token::Ident(n) => n,
                _ => return Err(self.err("expected type name after `impl Trait for`")),
            };
            (ty, Some(first))
        } else {
            (first, None)
        };
        self.eat(Token::LBrace)?;
        let mut methods = Vec::new();
        while *self.current() != Token::RBrace && *self.current() != Token::Eof {
            let mut f = self.function()?;
            if let Some(first_p) = f.params.first() {
                if first_p == "self" {
                    if f.param_types.is_empty() {
                        f.param_types.push(Type::Struct(type_name.clone()));
                    } else {
                        f.param_types[0] = Type::Struct(type_name.clone());
                    }
                }
            }
            // Rewrite `Self` type placeholders to the impl type
            for t in f.param_types.iter_mut() {
                if matches!(t, Type::Struct(n) if n == "__Self__") {
                    *t = Type::Struct(type_name.clone());
                }
            }
            if matches!(&f.return_type, Type::Struct(n) if n == "__Self__") {
                f.return_type = Type::Struct(type_name.clone());
            }
            // Name: Type.method (dispatch stays the same; trait checked in sema)
            f.name = format!("{}.{}", type_name, f.name);
            // Stash trait name in module_path sentinel if present: ["__trait__", TraitName]
            if let Some(tn) = &trait_name {
                f.module_path = vec!["__trait__".into(), tn.clone()];
            }
            methods.push(f);
        }
        self.eat(Token::RBrace)?;
        Ok(methods)
    }

    /// `trait Drawable { fn draw(self) -> Int; }` — signatures only (semicolon body).
    fn trait_def(&mut self) -> Result<TraitDef, String> {
        self.eat(Token::Trait)?;
        let name = match self.bump() {
            Token::Ident(n) => n,
            _ => return Err(self.err("expected trait name")),
        };
        self.eat(Token::LBrace)?;
        let mut methods = Vec::new();
        while *self.current() != Token::RBrace && *self.current() != Token::Eof {
            // optional pub on method (ignored for now — all trait methods public)
            if *self.current() == Token::Pub {
                self.bump();
            }
            self.eat(Token::Fn)?;
            let mname = match self.bump() {
                Token::Ident(n) => n,
                _ => return Err(self.err("expected method name in trait")),
            };
            self.eat(Token::LParen)?;
            let mut params = Vec::new();
            let mut param_types = Vec::new();
            if *self.current() != Token::RParen {
                loop {
                    let pname = match self.bump() {
                        Token::Ident(n) => n,
                        _ => return Err(self.err("expected parameter name in trait method")),
                    };
                    params.push(pname);
                    if *self.current() == Token::Colon {
                        self.bump();
                        param_types.push(self.parse_type()?);
                    } else {
                        // bare `self` → Unknown, filled later
                        param_types.push(Type::Unknown);
                    }
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
            // `;` = required abstract method; `{ ... }` = default implementation
            let default_body = if *self.current() == Token::LBrace {
                // Match `function()`: eat `{`, then `block()` reads stmts + closing `}`
                self.eat(Token::LBrace)?;
                Some(self.block()?)
            } else {
                self.eat(Token::Semi)?;
                None
            };
            methods.push(TraitMethod {
                name: mname,
                params,
                param_types,
                return_type,
                default_body,
            });
        }
        self.eat(Token::RBrace)?;
        Ok(TraitDef {
            name,
            visibility: Visibility::Public,
            module_path: vec![],
            methods,
        })
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
        let mut type_params = Vec::new();
        if *self.current() == Token::LBracket {
            self.bump();
            loop {
                match self.bump() {
                    Token::Ident(t) => type_params.push(t),
                    _ => return Err(self.err("expected type parameter in enum")),
                }
                if *self.current() == Token::Comma {
                    self.bump();
                } else {
                    break;
                }
            }
            self.eat(Token::RBracket)?;
        }
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
        Ok(crate::ast::EnumDef {
            name,
            visibility: Visibility::Public,
            module_path: vec![],
            type_params,
            variants,
        })
    }

    fn struct_def(&mut self) -> Result<StructDef, String> {
        self.eat(Token::Struct)?;
        let name = match self.bump() {
            Token::Ident(n) => n,
            _ => return Err("expected struct name".into()),
        };
        let mut type_params = Vec::new();
        if *self.current() == Token::LBracket {
            self.bump();
            loop {
                match self.bump() {
                    Token::Ident(t) => type_params.push(t),
                    _ => return Err(self.err("expected type parameter in struct")),
                }
                if *self.current() == Token::Comma {
                    self.bump();
                } else {
                    break;
                }
            }
            self.eat(Token::RBracket)?;
        }
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
        Ok(StructDef {
            name,
            visibility: Visibility::Public,
            module_path: vec![],
            type_params,
            fields,
        })
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
            Token::LParen => {
                if *self.current() == Token::RParen {
                    self.bump();
                    Type::Tuple(vec![])
                } else {
                    let mut ts = vec![self.parse_type()?];
                    while *self.current() == Token::Comma {
                        self.bump();
                        if *self.current() == Token::RParen {
                            break;
                        }
                        ts.push(self.parse_type()?);
                    }
                    self.eat(Token::RParen)?;
                    if ts.len() == 1 {
                        ts.pop().unwrap()
                    } else {
                        Type::Tuple(ts)
                    }
                }
            }
            Token::Ident(name) => match name.as_str() {
                "Int" => Type::Int,
                "Float" => Type::Float,
                "Self" => Type::Struct("__Self__".into()), // rewritten in impl_block
                "String" => Type::Str,
                "Bool" => Type::Bool,
                "Unit" => Type::Unit,
                "Channel" => Type::Channel,
                "Thread" => Type::Thread,
                "List" => Type::List,
                "Map" => Type::Map,
                // Type params like `T` become Generic via normalize_type when listed in fn[T]
                other => Type::Struct(other.to_string()),
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
                Type::Struct(name) => {
                    let mut args = Vec::new();
                    args.push(self.parse_type()?);
                    while *self.current() == Token::Comma {
                        self.bump();
                        args.push(self.parse_type()?);
                    }
                    self.eat(Token::RBracket)?;
                    Ok(Type::StructApp(name, args))
                }
                other => {
                    let _ = self.parse_type()?;
                    while *self.current() == Token::Comma {
                        self.bump();
                        let _ = self.parse_type()?;
                    }
                    self.eat(Token::RBracket)?;
                    Ok(other)
                }
            }
        } else {
            Ok(base)
        }
    }

    fn parse_use(&mut self) -> Result<crate::ast::UseDecl, String> {
        self.eat(Token::Use)?;
        let mut path = Vec::new();
        loop {
            match self.bump() {
                Token::Ident(n) => path.push(n),
                _ => return Err(self.err("expected path in use")),
            }
            if *self.current() == Token::Dot {
                self.bump();
                // brace form after path: use a.b.{x, y}
                if *self.current() == Token::LBrace {
                    self.bump();
                    let mut names = Vec::new();
                    loop {
                        match self.bump() {
                            Token::Ident(n) => names.push(n),
                            _ => return Err(self.err("expected name in use { }")),
                        }
                        if *self.current() == Token::Comma {
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    self.eat(Token::RBrace)?;
                    if *self.current() == Token::Semi {
                        self.bump();
                    }
                    return Ok(crate::ast::UseDecl { path, names });
                }
                continue;
            }
            break;
        }
        if *self.current() == Token::Semi {
            self.bump();
        }
        // use a.b.c → path=[a,b,c], names=[]
        Ok(crate::ast::UseDecl {
            path,
            names: vec![],
        })
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
        // `where Type: Trait, ...`
        let mut trait_bounds = Vec::new();
        if *self.current() == Token::Where {
            self.bump();
            loop {
                let ty = match self.bump() {
                    Token::Ident(n) => n,
                    _ => return Err(self.err("expected type name in where-clause")),
                };
                self.eat(Token::Colon)?;
                let tr = match self.bump() {
                    Token::Ident(n) => n,
                    _ => return Err(self.err("expected trait name after `:` in where-clause")),
                };
                trait_bounds.push((ty, tr));
                if *self.current() == Token::Comma {
                    self.bump();
                } else {
                    break;
                }
            }
        }
        self.eat(Token::LBrace)?;
        let body = self.block()?;
        Ok(Function {
            name,
            visibility: Visibility::Public,
            module_path: vec![],
            params,
            param_types,
            return_type,
            type_params,
            trait_bounds,
            body,
        })
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
            Token::Const => {
                self.bump();
                let name = match self.bump() {
                    Token::Ident(n) => n,
                    _ => return Err(self.err("expected const name")),
                };
                self.eat(Token::Eq)?;
                let expr = self.expression()?;
                Ok(Stmt::Const(name, expr))
            }
            Token::Defer => {
                self.bump();
                self.eat(Token::LBrace)?;
                let body = self.block()?;
                Ok(Stmt::Defer(body))
            }
            Token::If => self.if_statement(),
            Token::While => self.while_statement(),
            Token::Loop => {
                self.bump();
                self.eat(Token::LBrace)?;
                // loop { } ≡ while true { }
                Ok(Stmt::While(Expr::Bool(true), self.block()?))
            }
            Token::For => self.for_statement(),
            Token::Match => self.match_statement(),
            Token::Return => { self.bump(); Ok(Stmt::Return(self.expression()?)) }
            Token::Break => { self.bump(); Ok(Stmt::Break) }
            Token::Continue => { self.bump(); Ok(Stmt::Continue) }
            Token::Ident(name) => {
                // assignment or compound assignment or field assign or expr
                if self.peek_at(1) == Some(&Token::Eq) {
                    let name = name.clone();
                    self.bump(); self.bump();
                    return Ok(Stmt::Assign(name, self.expression()?));
                }
                if matches!(
                    self.peek_at(1),
                    Some(
                        Token::PlusEq
                            | Token::MinusEq
                            | Token::StarEq
                            | Token::SlashEq
                            | Token::PercentEq,
                    )
                ) {
                    let name = name.clone();
                    self.bump();
                    let op = match self.bump() {
                        Token::PlusEq => Token::Plus,
                        Token::MinusEq => Token::Minus,
                        Token::StarEq => Token::Star,
                        Token::SlashEq => Token::Slash,
                        Token::PercentEq => Token::Percent,
                        _ => unreachable!(),
                    };
                    let rhs = self.expression()?;
                    return Ok(Stmt::Assign(
                        name.clone(),
                        Expr::Binary(Box::new(Expr::Var(name)), op, Box::new(rhs)),
                    ));
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
        // `if let PATTERN = expr { ... }`
        if *self.current() == Token::Let {
            self.bump();
            let pattern = self.pattern()?;
            self.eat(Token::Eq)?;
            let value = self.expression()?;
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
            return Ok(Stmt::IfLet {
                pattern,
                value,
                yes,
                no,
            });
        }
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
        // `while let PATTERN = expr { body }`
        if *self.current() == Token::Let {
            self.bump();
            let pattern = self.pattern()?;
            self.eat(Token::Eq)?;
            let value = self.expression()?;
            self.eat(Token::LBrace)?;
            let body = self.block()?;
            // desugar: loop { match value { pat => body, _ => break } }
            // value re-evaluated each time via temporary assign is hard; use while true + match
            let match_stmt = Stmt::Match {
                scrutinee: value,
                arms: vec![
                    crate::ast::MatchArm {
                        pattern,
                        guard: None,
                        body,
                    },
                    crate::ast::MatchArm {
                        pattern: crate::ast::Pattern::Wildcard,
                        guard: None,
                        body: vec![Stmt::Break],
                    },
                ],
            };
            return Ok(Stmt::While(Expr::Bool(true), vec![match_stmt]));
        }
        let condition = self.expression()?;
        self.eat(Token::LBrace)?;
        Ok(Stmt::While(condition, self.block()?))
    }

    fn for_statement(&mut self) -> Result<Stmt, String> {
        self.bump(); // for
        // `for x in ...` or `for (i, x) in ...` (indexed)
        let (index, var) = if *self.current() == Token::LParen {
            self.bump();
            let a = match self.bump() {
                Token::Ident(n) => n,
                _ => return Err("expected name in for-binding".into()),
            };
            if *self.current() == Token::Comma {
                self.bump();
                let b = match self.bump() {
                    Token::Ident(n) => n,
                    _ => return Err("expected value name after `,` in for (i, x)".into()),
                };
                self.eat(Token::RParen)?;
                (Some(a), b)
            } else {
                self.eat(Token::RParen)?;
                (None, a)
            }
        } else {
            let var = match self.bump() {
                Token::Ident(n) => n,
                _ => return Err("expected loop variable after `for`".into()),
            };
            (None, var)
        };
        self.eat(Token::In)?;
        let first = self.expression()?;
        if matches!(*self.current(), Token::DotDot | Token::DotDotEq) {
            if index.is_some() {
                return Err("`for (i, x) in a..b` is not supported; use `for x in a..b`".into());
            }
            let inclusive = *self.current() == Token::DotDotEq;
            self.bump();
            let end = self.expression()?;
            self.eat(Token::LBrace)?;
            let body = self.block()?;
            Ok(Stmt::For {
                var,
                start: first,
                end,
                inclusive,
                body,
            })
        } else {
            self.eat(Token::LBrace)?;
            let body = self.block()?;
            Ok(Stmt::ForIn {
                var,
                index,
                iter: first,
                body,
            })
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
            Token::Underscore => Ok(Pattern::Wildcard),
            Token::Ident(n) if n == "_" => Ok(Pattern::Wildcard),
            Token::LParen => {
                if *self.current() == Token::RParen {
                    self.bump();
                    return Ok(Pattern::Tuple(vec![]));
                }
                let first = self.pattern()?;
                if *self.current() == Token::Comma {
                    let mut items = vec![first];
                    while *self.current() == Token::Comma {
                        self.bump();
                        if *self.current() == Token::RParen {
                            break;
                        }
                        items.push(self.pattern()?);
                    }
                    self.eat(Token::RParen)?;
                    Ok(Pattern::Tuple(items))
                } else {
                    self.eat(Token::RParen)?;
                    Ok(first)
                }
            }
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


    fn expression(&mut self) -> Result<Expr, String> { self.logic_or() }

    fn logic_or(&mut self) -> Result<Expr, String> {
        let mut e = self.logic_and()?;
        while *self.current() == Token::PipePipe {
            self.bump();
            e = Expr::Binary(Box::new(e), Token::PipePipe, Box::new(self.logic_and()?));
        }
        Ok(e)
    }

    fn logic_and(&mut self) -> Result<Expr, String> {
        let mut e = self.pipeline()?;
        while *self.current() == Token::AmpAmp {
            self.bump();
            e = Expr::Binary(Box::new(e), Token::AmpAmp, Box::new(self.pipeline()?));
        }
        Ok(e)
    }

    /// `a |> f` → `f(a)`; `a |> f(x)` → `f(a, x)` (first-arg pipeline)
    fn pipeline(&mut self) -> Result<Expr, String> {
        let mut e = self.compare()?;
        while *self.current() == Token::PipeGt {
            self.bump();
            let right = self.compare()?;
            e = pipe_apply(e, right)?;
        }
        Ok(e)
    }

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
        while matches!(self.current(), Token::Star | Token::Slash | Token::Percent) {
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
                    // Uppercase path → module/type call (`Math.max`, `Option.Some`)
                    // lowercase receiver → method sugar (`xs.len()`, `s.trim()`)
                    if let Some(base) = dotted_var_path(obj.as_ref()) {
                        let first = base.split('.').next().unwrap_or("");
                        let is_type_or_mod = first
                            .chars()
                            .next()
                            .map(|c| c.is_uppercase())
                            .unwrap_or(false);
                        // Intrinsic methods on a single var → MethodCall (`xs.len()`).
                        // Otherwise dotted Call (`combat.damage`, `a.b.f`, `Option.Some`).
                        let _intrinsic = matches!(
                            method.as_str(),
                            "len" | "get" | "push" | "set" | "contains" | "rev" | "sort"
                                | "sum" | "min" | "max" | "is_empty" | "map" | "filter"
                                | "fold" | "slice" | "unique" | "find" | "flatten" | "chunk"
                                | "take" | "drop" | "any" | "all" | "split" | "replace"
                                | "starts_with" | "ends_with" | "upper" | "lower" | "lines"
                                | "index_of" | "substring" | "repeat" | "trim" | "has"
                                | "keys" | "values" | "remove"
                        );
                        // Type/module path → Call (`Player.heal`, `a.b.v`, `Option.Some`)
                        // Value receiver → MethodCall (`p.heal(5)`, `xs.len()`)
                        if is_type_or_mod || base.contains('.') {
                            e = Expr::Call(format!("{base}.{method}"), args);
                        } else {
                            e = Expr::MethodCall {
                                receiver: obj,
                                method,
                                args,
                            };
                        }
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
            } else if *self.current() == Token::Question {
                self.bump();
                e = Expr::Try(Box::new(e));
            } else {
                break;
            }
        }
        Ok(e)
    }

    fn atom(&mut self) -> Result<Expr, String> {
        match self.bump() {
            Token::Int(v) => Ok(Expr::Int(v)),
            Token::Float(v) => Ok(Expr::Float(v)),
            Token::Str(s) => Ok(self.maybe_interp(s)),
            Token::True => Ok(Expr::Bool(true)),
            Token::False => Ok(Expr::Bool(false)),
            Token::Underscore => Ok(Expr::Hole),
            Token::Fn => self.parse_lambda(),
            Token::LParen => {
                // grouping `(e)` or tuple `(a, b, ...)` or unit `()`
                if *self.current() == Token::RParen {
                    self.bump();
                    return Ok(Expr::Tuple(vec![]));
                }
                let first = self.expression()?;
                if *self.current() == Token::Comma {
                    let mut items = vec![first];
                    while *self.current() == Token::Comma {
                        self.bump();
                        if *self.current() == Token::RParen {
                            break; // trailing comma
                        }
                        items.push(self.expression()?);
                    }
                    self.eat(Token::RParen)?;
                    Ok(Expr::Tuple(items))
                } else {
                    self.eat(Token::RParen)?;
                    Ok(first) // grouping
                }
            }
            Token::If => {
                // if-expr: if cond { then } else { else }
                let cond = self.expression()?;
                self.eat(Token::LBrace)?;
                let then_branch = self.expression()?;
                self.eat(Token::RBrace)?;
                self.eat(Token::Else)?;
                self.eat(Token::LBrace)?;
                let else_branch = self.expression()?;
                self.eat(Token::RBrace)?;
                Ok(Expr::IfExpr {
                    cond: Box::new(cond),
                    then_branch: Box::new(then_branch),
                    else_branch: Box::new(else_branch),
                })
            }
            Token::Match => {
                // match-expr: match e { pat => expr, ... }
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
                    // expression form: not a block
                    if *self.current() == Token::LBrace {
                        return Err(self.err(
                            "match-expression arms use `=> expr` (not `{ ... }` blocks); use statement `match` for blocks",
                        ));
                    }
                    let value = self.expression()?;
                    arms.push(MatchExprArm {
                        pattern: pat,
                        guard,
                        value,
                    });
                    if *self.current() == Token::Comma {
                        self.bump();
                    }
                }
                self.eat(Token::RBrace)?;
                Ok(Expr::MatchExpr {
                    scrutinee: Box::new(scrutinee),
                    arms,
                })
            }
            Token::LBracket => {
                // list literal: [1, 2, 3]
                let mut args = Vec::new();
                if *self.current() != Token::RBracket {
                    loop {
                        args.push(self.expression()?);
                        if *self.current() == Token::Comma {
                            self.bump();
                        } else {
                            break;
                        }
                    }
                }
                self.eat(Token::RBracket)?;
                Ok(Expr::Call("list".into(), args))
            }
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

fn is_pipe_hole(e: &Expr) -> bool {
    match e {
        Expr::Hole => true,
        Expr::Var(n) if n == "_" => true,
        _ => false,
    }
}

fn fill_holes(left: Expr, args: Vec<Expr>) -> Vec<Expr> {
    let mut filled = Vec::with_capacity(args.len());
    let mut used = false;
    for a in args {
        if is_pipe_hole(&a) {
            filled.push(left.clone());
            used = true;
        } else {
            filled.push(a);
        }
    }
    if !used {
        // first-arg style when no `_` present
        let mut a = vec![left];
        a.extend(filled);
        a
    } else {
        filled
    }
}

fn pipe_apply(left: Expr, right: Expr) -> Result<Expr, String> {
    match right {
        Expr::Call(name, args) => Ok(Expr::Call(name, fill_holes(left, args))),
        Expr::Var(name) => Ok(Expr::Call(name, vec![left])),
        Expr::CallExpr(callee, args) => Ok(Expr::CallExpr(callee, fill_holes(left, args))),
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => Ok(Expr::MethodCall {
            receiver,
            method,
            args: fill_holes(left, args),
        }),
        other => Err(format!(
            "pipeline `|>` right-hand side must be a call or function name, got {:?}",
            std::mem::discriminant(&other)
        )),
    }
}

/// Flatten `a.b.c` Field chains into `"a.b.c"` when every step is a field on a path root Var.
fn dotted_var_path(e: &Expr) -> Option<String> {
    match e {
        Expr::Var(n) => Some(n.clone()),
        Expr::Field(inner, f) => {
            let base = dotted_var_path(inner)?;
            Some(format!("{base}.{f}"))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_type_params() {
        let src = "fn identity[T](x: T) -> T { return x; } fn main() -> Int { return identity(1); }";
        let p = parse_program(src).unwrap();
        let id = p.functions.iter().find(|f| f.name == "identity").unwrap();
        assert_eq!(id.type_params, vec!["T".to_string()], "{:?}", id.type_params);
        assert!(matches!(id.param_types[0], crate::ast::Type::Struct(ref n) if n == "T")
            || matches!(id.param_types[0], crate::ast::Type::Generic(ref n) if n == "T"),
            "{:?}", id.param_types);
    }

    #[test]
    fn parses_value_method_vs_module() {
        let src = r#"
            mod combat { pub fn hit(x: Int) -> Int { return x; } }
            struct P { x: Int }
            impl P { fn get(self) -> Int { return self.x; } }
            fn main() -> Int {
                let p = P { x: 1 };
                let a = combat.hit(1);
                let b = p.get();
                return a + b;
            }
        "#;
        let p = parse_program(src).expect("parse");
        // just ensure parse ok and functions include combat.hit and P.get
        let names: Vec<_> = p.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(names.iter().any(|n| *n == "combat.hit"), "{names:?}");
        assert!(names.iter().any(|n| *n == "P.get"), "{names:?}");
    }

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
