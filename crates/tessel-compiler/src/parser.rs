//! Recursive-descent parser: tokens to syntax tree.
//!
//! Errors are recorded and parsing continues from the next line, so one
//! mistake doesn't hide the rest.

use crate::ast::*;
use crate::diag::{Diagnostic, Diagnostics};
use crate::lexer::{describe, lex, StrPiece, Tok, Token};
use crate::source::Span;

pub struct Parser<'a> {
    toks: Vec<Token>,
    pos: usize,
    file: u32,
    text: &'a str,
    diags: &'a mut Diagnostics,
    next_id: &'a mut ExprId,
    /// Inside `if`/`for`/`match` headers, `x {` is not a trailing block.
    no_trailing: bool,
}

/// Parses one file. `next_id` numbers expressions uniquely across files.
pub fn parse_file(file: u32, text: &str, diags: &mut Diagnostics, next_id: &mut ExprId) -> Program {
    let toks = lex(file, text, 0, diags);
    let mut p = Parser { toks, pos: 0, file, text, diags, next_id, no_trailing: false };
    p.program()
}

/// Marker for "an error was already reported".
struct Reported;

type PResult<T> = Result<T, Reported>;

const ITEM_START: &str = "a declaration (`app`, `view`, `struct`, `enum`, `interface` or `fn`)";

impl<'a> Parser<'a> {
    // ----- token helpers -----

    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, n: usize) -> &Tok {
        let i = (self.pos + n).min(self.toks.len() - 1);
        &self.toks[i].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }

    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn at(&self, tok: &Tok) -> bool {
        self.peek() == tok
    }

    fn eat(&mut self, tok: &Tok) -> bool {
        if self.at(tok) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn skip_newlines(&mut self) {
        while self.at(&Tok::Newline) {
            self.bump();
        }
    }

    fn error(&mut self, span: Span, msg: impl Into<String>) -> &mut Diagnostic {
        self.diags.push(Diagnostic::error(span, msg));
        self.diags.list.last_mut().unwrap()
    }

    /// Reports "expected X, found Y" at the current token.
    fn unexpected(&mut self, expected: &str) -> Reported {
        let span = self.span();
        let found = describe(self.peek());
        self.error(span, format!("expected {expected}, found {found}"));
        Reported
    }

    fn expect(&mut self, tok: Tok) -> PResult<Span> {
        if self.at(&tok) {
            Ok(self.bump().span)
        } else {
            Err(self.unexpected(&describe(&tok)))
        }
    }

    fn ident(&mut self, what: &str) -> PResult<Ident> {
        match self.peek().clone() {
            Tok::Ident(name) => Ok(Ident { name, span: self.bump().span }),
            tok if crate::lexer::keyword(&keyword_text(&tok)).is_some() && tok != Tok::Underscore => {
                let span = self.span();
                let word = keyword_text(&tok);
                self.error(span, format!("`{word}` is a keyword and can't be used as {what}"))
                    .help(format!("choose another name, like `{word}Value`"));
                self.bump();
                Err(Reported)
            }
            _ => Err(self.unexpected(what)),
        }
    }

    fn new_expr(&mut self, kind: ExprKind, span: Span) -> Expr {
        let id = *self.next_id;
        *self.next_id += 1;
        Expr { id, kind, span }
    }

    fn with_trailing<T>(&mut self, allowed: bool, f: impl FnOnce(&mut Self) -> T) -> T {
        let saved = self.no_trailing;
        self.no_trailing = !allowed;
        let r = f(self);
        self.no_trailing = saved;
        r
    }

    /// Skips to the end of the current line (or an unmatched `}`), for recovery.
    fn recover_line(&mut self) {
        let mut depth = 0usize;
        loop {
            match self.peek() {
                Tok::Eof => return,
                Tok::Newline if depth == 0 => return,
                Tok::RBrace if depth == 0 => return,
                Tok::LBrace | Tok::LParen | Tok::LBracket => depth += 1,
                Tok::RBrace | Tok::RParen | Tok::RBracket => depth = depth.saturating_sub(1),
                _ => {}
            }
            self.bump();
        }
    }

    /// Skips to the next line that starts a declaration, for top-level recovery.
    fn recover_item(&mut self) {
        let mut depth = 0usize;
        loop {
            match self.peek() {
                Tok::Eof => return,
                Tok::LBrace => depth += 1,
                Tok::RBrace => depth = depth.saturating_sub(1),
                Tok::Newline if depth == 0 => {
                    self.bump();
                    if matches!(self.peek(), Tok::App | Tok::View | Tok::Struct | Tok::Enum | Tok::Fn) {
                        return;
                    }
                    continue;
                }
                _ => {}
            }
            self.bump();
        }
    }

    // ----- declarations -----

    fn program(&mut self) -> Program {
        let mut items = Vec::new();
        loop {
            self.skip_newlines();
            let start = self.pos;
            let item = match self.peek() {
                Tok::Eof => break,
                Tok::Struct => self.struct_decl().map(Item::Struct),
                Tok::Enum => self.enum_decl().map(Item::Enum),
                Tok::Interface => self.interface_decl().map(Item::Interface),
                Tok::Fn => self.fn_decl().map(Item::Fn),
                Tok::View => self.view_decl().map(Item::View),
                Tok::App => self.app_decl().map(Item::App),
                Tok::Let | Tok::Var | Tok::State => {
                    let span = self.span();
                    self.error(span, "variables can't be declared at the top level of a file").help(
                        "move it into a `view` or `app` (as `state`), or into a function",
                    );
                    Err(Reported)
                }
                _ => Err(self.unexpected(ITEM_START)),
            };
            match item {
                Ok(item) => {
                    items.push(item);
                    if !matches!(self.peek(), Tok::Newline | Tok::Eof) {
                        self.unexpected("end of line");
                        self.recover_item();
                    }
                }
                Err(Reported) => {
                    if self.pos == start {
                        self.bump();
                    }
                    self.recover_item();
                }
            }
        }
        Program { items, next_id: 0 }
    }

    fn struct_decl(&mut self) -> PResult<StructDecl> {
        self.bump(); // struct
        let name = self.ident("a struct name")?;
        let generics = self.generic_params()?;
        let conforms = self.conformances()?;
        self.expect(Tok::LBrace)?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        self.members(|p| {
            if p.at(&Tok::Fn) {
                methods.push(p.fn_decl()?);
            } else {
                let name = p.ident("a field name")?;
                if !p.at(&Tok::Colon) {
                    let span = p.span();
                    p.error(span, format!("field `{}` needs a type", name.name))
                        .help(format!("write it like `{}: String`", name.name));
                    return Err(Reported);
                }
                p.bump();
                let ty = p.type_expr()?;
                let default = if p.eat(&Tok::Eq) { Some(p.expr()?) } else { None };
                fields.push(FieldDecl { name, ty, default });
            }
            Ok(())
        });
        Ok(StructDecl { name, generics, conforms, fields, methods })
    }

    fn enum_decl(&mut self) -> PResult<EnumDecl> {
        self.bump(); // enum
        let name = self.ident("an enum name")?;
        let generics = self.generic_params()?;
        let conforms = self.conformances()?;
        self.expect(Tok::LBrace)?;
        let mut cases = Vec::new();
        let mut methods = Vec::new();
        self.members(|p| {
            if p.at(&Tok::Fn) {
                methods.push(p.fn_decl()?);
            } else {
                let name = p.ident("a case name")?;
                let fields = if p.at(&Tok::LParen) { p.params()? } else { Vec::new() };
                cases.push(CaseDecl { name, fields });
            }
            Ok(())
        });
        Ok(EnumDecl { name, generics, conforms, cases, methods })
    }

    /// `<T, U: Shape>` after a declaration's name (none if there's no `<`).
    fn generic_params(&mut self) -> PResult<Vec<GenericParam>> {
        let mut out = Vec::new();
        if !self.eat(&Tok::Lt) {
            return Ok(out);
        }
        loop {
            let name = self.ident("a type parameter name, like `T`")?;
            let constraint = if self.eat(&Tok::Colon) { Some(self.ident("an interface name")?) } else { None };
            out.push(GenericParam { name, constraint });
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::Gt)?;
        Ok(out)
    }

    /// `<Int, [String]>` after a generic type's name.
    fn type_args(&mut self) -> PResult<Vec<TypeExpr>> {
        self.expect(Tok::Lt)?;
        let mut out = Vec::new();
        loop {
            out.push(self.type_expr()?);
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::Gt)?;
        Ok(out)
    }

    /// Is the `<` at the current position the start of type arguments (a
    /// matching `>` followed by `(` or `.`), rather than "less than"?
    fn type_args_ahead(&self) -> bool {
        let mut depth = 0;
        let mut i = 0;
        loop {
            match self.peek_at(i) {
                Tok::Lt => depth += 1,
                Tok::Gt => {
                    depth -= 1;
                    if depth == 0 {
                        return matches!(self.peek_at(i + 1), Tok::LParen | Tok::Dot);
                    }
                }
                Tok::Ident(_) | Tok::Comma | Tok::LBracket | Tok::RBracket | Tok::Colon | Tok::Question
                | Tok::Fn | Tok::LParen | Tok::RParen | Tok::Arrow => {}
                _ => return false,
            }
            i += 1;
        }
    }

    /// `: Shape, Named` after a struct or enum name.
    fn conformances(&mut self) -> PResult<Vec<Ident>> {
        let mut out = Vec::new();
        if self.eat(&Tok::Colon) {
            loop {
                out.push(self.ident("an interface name")?);
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        Ok(out)
    }

    fn interface_decl(&mut self) -> PResult<InterfaceDecl> {
        self.bump(); // interface
        let name = self.ident("an interface name")?;
        self.expect(Tok::LBrace)?;
        let mut methods = Vec::new();
        let mut props = Vec::new();
        self.members(|p| {
            if !p.at(&Tok::Fn) {
                // A required property: `name: String`.
                let name = p.ident("a method (`fn …`) or a property (`name: Type`)")?;
                if !p.eat(&Tok::Colon) {
                    let span = p.span();
                    p.error(span, format!("property `{}` needs a type", name.name))
                        .help(format!("write it like `{}: String`", name.name));
                    return Err(Reported);
                }
                let ty = p.type_expr()?;
                if p.at(&Tok::Eq) {
                    let span = p.span();
                    p.error(span, "an interface's properties have no default values")
                        .help("give the default in each struct instead");
                    return Err(Reported);
                }
                props.push((name, ty));
                return Ok(());
            }
            let lo = p.bump().span;
            let name = p.ident("a method name")?;
            if !p.at(&Tok::LParen) {
                let span = p.span();
                p.error(span, format!("method `{}` needs a parameter list", name.name))
                    .help(format!("write `fn {}()` even when it takes no parameters", name.name));
                return Err(Reported);
            }
            let params = p.params()?;
            let ret = if p.eat(&Tok::Arrow) { Some(p.type_expr()?) } else { None };
            if p.at(&Tok::LBrace) {
                let span = p.span();
                p.error(span, "an interface only lists methods; their bodies go in the types that conform to it")
                    .help(format!("remove the body here, and write it in each type: `struct Circle: {} {{ … }}`", "…"));
                return Err(Reported);
            }
            methods.push(MethodReq { name, params, ret, span: lo.to(p.prev_span()) });
            Ok(())
        });
        Ok(InterfaceDecl { name, methods, props })
    }

    /// Parses `member (, | newline) member ... }` after the opening brace.
    fn members(&mut self, mut member: impl FnMut(&mut Self) -> PResult<()>) {
        loop {
            while self.eat(&Tok::Newline) || self.eat(&Tok::Comma) {}
            if self.eat(&Tok::RBrace) || self.at(&Tok::Eof) {
                if self.at(&Tok::Eof) {
                    self.unexpected("`}`");
                }
                return;
            }
            let start = self.pos;
            match member(self) {
                Ok(()) => {
                    if !matches!(self.peek(), Tok::Newline | Tok::Comma | Tok::RBrace) {
                        self.unexpected("`,` or end of line");
                        self.recover_line();
                    }
                }
                Err(Reported) => {
                    if self.pos == start {
                        self.bump();
                    }
                    self.recover_line();
                }
            }
        }
    }

    fn fn_decl(&mut self) -> PResult<FnDecl> {
        self.bump(); // fn
        let name = self.ident("a function name")?;
        let generics = self.generic_params()?;
        if !self.at(&Tok::LParen) {
            let span = self.span();
            self.error(span, format!("function `{}` needs a parameter list", name.name))
                .help(format!("write `fn {}() {{ … }}` even when it takes no parameters", name.name));
            return Err(Reported);
        }
        let params = self.params()?;
        let ret = if self.eat(&Tok::Arrow) { Some(self.type_expr()?) } else { None };
        let body = self.block()?;
        Ok(FnDecl { name, generics, params, ret, body })
    }

    fn params(&mut self) -> PResult<Vec<Param>> {
        self.expect(Tok::LParen)?;
        let mut params = Vec::new();
        while !self.at(&Tok::RParen) {
            let start = self.span();
            let unlabeled = self.eat(&Tok::Underscore);
            let name = self.ident("a parameter name")?;
            if !self.at(&Tok::Colon) {
                let span = self.span();
                self.error(span, format!("parameter `{}` needs a type", name.name))
                    .help(format!("write it like `{}: Int`", name.name));
                return Err(Reported);
            }
            self.bump();
            let bind = self.eat(&Tok::Bind);
            let ty = self.type_expr()?;
            let default = if self.eat(&Tok::Eq) { Some(self.expr()?) } else { None };
            params.push(Param { unlabeled, name, bind, ty, default, span: start.to(self.prev_span()) });
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::RParen)?;
        Ok(params)
    }

    fn view_decl(&mut self) -> PResult<ViewDecl> {
        self.bump(); // view
        let name = self.ident("a view name")?;
        let params = if self.at(&Tok::LParen) { self.params()? } else { Vec::new() };
        let body = self.block()?;
        Ok(ViewDecl { name, params, body })
    }

    fn app_decl(&mut self) -> PResult<AppDecl> {
        self.bump(); // app
        let name = self.ident("an app name")?;
        let (args, args_span) = if self.at(&Tok::LParen) {
            let lo = self.span();
            let args = self.args()?;
            (args, lo.to(self.prev_span()))
        } else {
            (Vec::new(), name.span)
        };
        let body = self.block()?;
        Ok(AppDecl { name, args, args_span, body })
    }

    // ----- types -----

    fn type_expr(&mut self) -> PResult<TypeExpr> {
        let lo = self.span();
        let mut ty = match self.peek() {
            Tok::Ident(_) => {
                let name = self.ident("a type")?;
                if self.at(&Tok::Lt) {
                    let args = self.type_args()?;
                    TypeExpr::Generic(name, args, lo.to(self.prev_span()))
                } else {
                    TypeExpr::Named(name)
                }
            }
            Tok::LBracket => {
                self.bump();
                let inner = self.type_expr()?;
                if self.eat(&Tok::Colon) {
                    let value = self.type_expr()?;
                    self.expect(Tok::RBracket)?;
                    TypeExpr::Dict(Box::new(inner), Box::new(value), lo.to(self.prev_span()))
                } else {
                    self.expect(Tok::RBracket)?;
                    TypeExpr::List(Box::new(inner), lo.to(self.prev_span()))
                }
            }
            Tok::Fn => {
                self.bump();
                self.expect(Tok::LParen)?;
                let mut params = Vec::new();
                while !self.at(&Tok::RParen) {
                    params.push(self.type_expr()?);
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(Tok::RParen)?;
                let ret = if self.eat(&Tok::Arrow) { Some(Box::new(self.type_expr()?)) } else { None };
                TypeExpr::Fn(params, ret, lo.to(self.prev_span()))
            }
            _ => return Err(self.unexpected("a type")),
        };
        while self.at(&Tok::Question) {
            self.bump();
            ty = TypeExpr::Optional(Box::new(ty), lo.to(self.prev_span()));
        }
        Ok(ty)
    }

    // ----- statements -----

    fn block(&mut self) -> PResult<Block> {
        let lo = self.expect(Tok::LBrace)?;
        let stmts = self.with_trailing(true, |p| p.stmts_until_brace());
        Ok(Block { stmts, span: lo.to(self.prev_span()) })
    }

    /// Statements up to and including the closing `}`.
    fn stmts_until_brace(&mut self) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek() {
                Tok::RBrace => {
                    self.bump();
                    return stmts;
                }
                Tok::Eof => {
                    self.unexpected("`}`");
                    return stmts;
                }
                _ => {}
            }
            let start = self.pos;
            match self.stmt() {
                Ok(s) => {
                    stmts.push(s);
                    if !matches!(self.peek(), Tok::Newline | Tok::RBrace | Tok::Eof) {
                        let span = self.span();
                        let found = describe(self.peek());
                        self.error(span, format!("expected end of line, found {found}"))
                            .help("put each statement on its own line");
                        self.recover_line();
                    }
                }
                Err(Reported) => {
                    if self.pos == start {
                        self.bump();
                    }
                    self.recover_line();
                }
            }
        }
    }

    fn stmt(&mut self) -> PResult<Stmt> {
        let lo = self.span();
        match self.peek() {
            Tok::Let => Ok(Stmt::Let(self.local_decl()?)),
            Tok::Var => Ok(Stmt::Var(self.local_decl()?)),
            Tok::State => Ok(Stmt::State(self.local_decl()?)),
            Tok::Fn => Ok(Stmt::Fn(self.fn_decl()?)),
            Tok::Return => {
                self.bump();
                let value = if matches!(self.peek(), Tok::Newline | Tok::RBrace | Tok::Eof) {
                    None
                } else {
                    Some(self.expr()?)
                };
                Ok(Stmt::Return { value, span: lo.to(self.prev_span()) })
            }
            Tok::While => {
                self.bump();
                let cond = self.condition()?;
                if !self.at(&Tok::LBrace) {
                    let span = self.span();
                    self.error(span, "expected `{` after the `while` condition")
                        .help("the body of a `while` goes in braces: `while n > 0 { … }`");
                    return Err(Reported);
                }
                let body = self.block()?;
                Ok(Stmt::While { cond, body, span: lo.to(self.prev_span()) })
            }
            Tok::Break => Ok(Stmt::Break(self.bump().span)),
            Tok::Continue => Ok(Stmt::Continue(self.bump().span)),
            Tok::For => {
                self.bump();
                // `for _ in 0..3`: a loop variable that isn't used.
                let var = if self.at(&Tok::Underscore) {
                    let span = self.bump().span;
                    Ident { name: "_".into(), span }
                } else {
                    self.ident("a loop variable name")?
                };
                self.expect(Tok::In)?;
                let iter = self.with_trailing(false, |p| p.expr())?;
                let body = self.block()?;
                Ok(Stmt::For { var, iter, body, span: lo.to(self.prev_span()) })
            }
            _ => {
                let target = self.expr()?;
                let op = match self.peek() {
                    Tok::Eq => AssignOp::Set,
                    Tok::PlusEq => AssignOp::Add,
                    Tok::MinusEq => AssignOp::Sub,
                    Tok::StarEq => AssignOp::Mul,
                    Tok::SlashEq => AssignOp::Div,
                    _ => return Ok(Stmt::Expr(target)),
                };
                self.bump();
                self.skip_newlines();
                let value = self.expr()?;
                Ok(Stmt::Assign { target, op, value, span: lo.to(self.prev_span()) })
            }
        }
    }

    fn local_decl(&mut self) -> PResult<LocalDecl> {
        let kw = self.bump();
        let what = match kw.tok {
            Tok::Let => "a constant name",
            Tok::Var => "a variable name",
            _ => "a state name",
        };
        let name = self.ident(what)?;
        let ty = if self.eat(&Tok::Colon) { Some(self.type_expr()?) } else { None };
        if !self.at(&Tok::Eq) {
            let span = self.span();
            let kw_text = keyword_text(&kw.tok);
            self.error(span, format!("`{}` needs a starting value", name.name))
                .help(format!("write `{kw_text} {} = …`", name.name));
            return Err(Reported);
        }
        self.bump();
        self.skip_newlines();
        let value = self.expr()?;
        Ok(LocalDecl { name, ty, value, span: kw.span.to(self.prev_span()) })
    }

    // ----- expressions -----

    fn expr(&mut self) -> PResult<Expr> {
        self.or_expr()
    }

    fn binary_left(
        &mut self,
        next: fn(&mut Self) -> PResult<Expr>,
        op_of: fn(&Tok) -> Option<BinOp>,
    ) -> PResult<Expr> {
        let mut lhs = next(self)?;
        while let Some(op) = op_of(self.peek()) {
            self.bump();
            self.skip_newlines();
            let rhs = next(self)?;
            let span = lhs.span.to(rhs.span);
            lhs = self.new_expr(ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), span);
        }
        Ok(lhs)
    }

    fn or_expr(&mut self) -> PResult<Expr> {
        self.binary_left(Self::and_expr, |t| (*t == Tok::OrOr).then_some(BinOp::Or))
    }

    fn and_expr(&mut self) -> PResult<Expr> {
        self.binary_left(Self::cmp_expr, |t| (*t == Tok::AndAnd).then_some(BinOp::And))
    }

    fn cmp_expr(&mut self) -> PResult<Expr> {
        let cmp_op = |t: &Tok| {
            Some(match t {
                Tok::EqEq => BinOp::Eq,
                Tok::NotEq => BinOp::Ne,
                Tok::Lt => BinOp::Lt,
                Tok::Le => BinOp::Le,
                Tok::Gt => BinOp::Gt,
                Tok::Ge => BinOp::Ge,
                _ => return None,
            })
        };
        let lhs = self.cast_expr()?;
        let Some(op) = cmp_op(self.peek()) else { return Ok(lhs) };
        self.bump();
        self.skip_newlines();
        let rhs = self.cast_expr()?;
        let span = lhs.span.to(rhs.span);
        if cmp_op(self.peek()).is_some() {
            let s = self.span();
            self.error(s, "comparisons can't be chained")
                .help("combine them with `&&`, like `a < b && b < c`");
            return Err(Reported);
        }
        Ok(self.new_expr(ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), span))
    }

    /// `value is Circle`, `value as? Circle` (`is` and `as` aren't reserved words).
    fn cast_expr(&mut self) -> PResult<Expr> {
        let mut lhs = self.range_expr()?;
        loop {
            let test = match self.peek() {
                Tok::Ident(w) if w == "is" => true,
                Tok::Ident(w) if w == "as" && *self.peek_at(1) == Tok::Question => false,
                Tok::Ident(w) if w == "as" => {
                    let span = self.span();
                    self.error(span, "write `as?`: the value might be another type")
                        .help("`if let c = shape as? Circle { … }` uses it only when it is one");
                    return Err(Reported);
                }
                _ => return Ok(lhs),
            };
            self.bump();
            if !test {
                self.bump(); // ?
            }
            let ty = self.type_expr()?;
            let span = lhs.span.to(self.prev_span());
            lhs = self.new_expr(ExprKind::Cast { value: Box::new(lhs), ty, test }, span);
        }
    }

    fn range_expr(&mut self) -> PResult<Expr> {
        let lhs = self.coalesce_expr()?;
        if !self.eat(&Tok::DotDot) {
            return Ok(lhs);
        }
        let rhs = self.coalesce_expr()?;
        let span = lhs.span.to(rhs.span);
        Ok(self.new_expr(ExprKind::Binary(BinOp::Range, Box::new(lhs), Box::new(rhs)), span))
    }

    fn coalesce_expr(&mut self) -> PResult<Expr> {
        let lhs = self.add_expr()?;
        if !self.eat(&Tok::QuestionQuestion) {
            return Ok(lhs);
        }
        self.skip_newlines();
        let rhs = self.coalesce_expr()?; // right-associative
        let span = lhs.span.to(rhs.span);
        Ok(self.new_expr(ExprKind::Binary(BinOp::Coalesce, Box::new(lhs), Box::new(rhs)), span))
    }

    fn add_expr(&mut self) -> PResult<Expr> {
        self.binary_left(Self::mul_expr, |t| match t {
            Tok::Plus => Some(BinOp::Add),
            Tok::Minus => Some(BinOp::Sub),
            _ => None,
        })
    }

    fn mul_expr(&mut self) -> PResult<Expr> {
        self.binary_left(Self::unary_expr, |t| match t {
            Tok::Star => Some(BinOp::Mul),
            Tok::Slash => Some(BinOp::Div),
            Tok::Percent => Some(BinOp::Rem),
            _ => None,
        })
    }

    fn unary_expr(&mut self) -> PResult<Expr> {
        let op = match self.peek() {
            Tok::Minus => UnaryOp::Neg,
            Tok::Bang => UnaryOp::Not,
            _ => return self.postfix_expr(),
        };
        let lo = self.bump().span;
        let operand = self.unary_expr()?;
        let span = lo.to(operand.span);
        Ok(self.new_expr(ExprKind::Unary(op, Box::new(operand)), span))
    }

    /// Does the line starting at token index `i` look like a match arm (`.x -> …`)?
    fn line_is_match_arm(&self, mut i: usize) -> bool {
        let mut depth = 0usize;
        while i < self.toks.len() {
            match self.toks[i].tok {
                Tok::Newline | Tok::Eof if depth == 0 => return false,
                Tok::Eof => return false,
                Tok::Arrow if depth == 0 => return true,
                Tok::LParen | Tok::LBracket | Tok::LBrace => depth += 1,
                Tok::RParen | Tok::RBracket | Tok::RBrace => {
                    if depth == 0 {
                        return false;
                    }
                    depth -= 1;
                }
                _ => {}
            }
            i += 1;
        }
        false
    }

    fn postfix_expr(&mut self) -> PResult<Expr> {
        let mut e = self.primary_expr()?;
        loop {
            match self.peek() {
                Tok::LParen => {
                    let args = self.with_trailing(true, |p| p.args())?;
                    let trailing = self.trailing_block()?;
                    let span = e.span.to(self.prev_span());
                    e = self.new_expr(ExprKind::Call { callee: Box::new(e), args, trailing }, span);
                }
                // `for x in list.filter { y in … } {`: a block with parameters
                // can't be the loop body, so the user meant a trailing block.
                Tok::LBrace if self.no_trailing && is_callable_head(&e) && self.block_has_params() => {
                    let span = self.span();
                    let name = match &e.kind {
                        ExprKind::Member { name, .. } => name.name.clone(),
                        ExprKind::Name(n) => n.clone(),
                        _ => unreachable!(),
                    };
                    self.error(span, format!("a block can't follow `{name}` here"))
                        .label("this looks like a block for `{name}`".replace("{name}", &name))
                        .help(format!(
                            "in `if`, `for` and `match` headers, pass it in parentheses: `{name}({{ … }})`"
                        ));
                    let block = self.closure()?;
                    let span = e.span.to(block.span);
                    let trailing = Some(Box::new(block));
                    e = self.new_expr(ExprKind::Call { callee: Box::new(e), args: Vec::new(), trailing }, span);
                }
                Tok::LBrace if !self.no_trailing && is_callable_head(&e) => {
                    let trailing = self.trailing_block()?;
                    let span = e.span.to(self.prev_span());
                    e = self.new_expr(ExprKind::Call { callee: Box::new(e), args: Vec::new(), trailing }, span);
                }
                Tok::Dot | Tok::QuestionDot => {
                    let optional = self.bump().tok == Tok::QuestionDot;
                    let name = self.ident("a member name after `.`")?;
                    let span = e.span.to(name.span);
                    e = self.new_expr(ExprKind::Member { object: Box::new(e), name, optional }, span);
                }
                Tok::LBracket => {
                    self.bump();
                    let index = self.with_trailing(true, |p| p.expr())?;
                    self.expect(Tok::RBracket)?;
                    let span = e.span.to(self.prev_span());
                    e = self.new_expr(ExprKind::Index(Box::new(e), Box::new(index)), span);
                }
                // A line starting with `.` continues the chain (modifiers),
                // unless it's the next arm of a `match`.
                Tok::Newline
                    if *self.peek_at(1) == Tok::Dot && !self.line_is_match_arm(self.pos + 1) =>
                {
                    self.bump();
                }
                _ => return Ok(e),
            }
        }
    }

    /// Is the `{` at the current token the start of `{ a, b in … }`?
    fn block_has_params(&self) -> bool {
        let mut i = 1;
        while let Tok::Ident(_) | Tok::Underscore = self.peek_at(i) {
            i += 1;
            match self.peek_at(i) {
                Tok::Comma => i += 1,
                Tok::In => return true,
                _ => return false,
            }
        }
        false
    }

    fn trailing_block(&mut self) -> PResult<Option<Box<Expr>>> {
        if self.no_trailing || !self.at(&Tok::LBrace) {
            return Ok(None);
        }
        Ok(Some(Box::new(self.closure()?)))
    }

    fn args(&mut self) -> PResult<Vec<Arg>> {
        self.expect(Tok::LParen)?;
        let mut args = Vec::new();
        while !self.at(&Tok::RParen) {
            let label = if matches!(self.peek(), Tok::Ident(_)) && *self.peek_at(1) == Tok::Colon {
                let l = self.ident("a label")?;
                self.bump(); // :
                Some(l)
            } else {
                None
            };
            let value = self.expr()?;
            args.push(Arg { label, value });
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        if !self.at(&Tok::RParen) {
            return Err(self.unexpected("`,` or `)`"));
        }
        self.bump();
        Ok(args)
    }

    /// `{ a, b in body }` or `{ body }`.
    fn closure(&mut self) -> PResult<Expr> {
        let lo = self.span();
        // Look ahead for `ident (, ident)* in`.
        let mut i = 1;
        let mut has_params = false;
        while let Tok::Ident(_) | Tok::Underscore = self.peek_at(i) {
            i += 1;
            match self.peek_at(i) {
                Tok::Comma => i += 1,
                Tok::In => {
                    has_params = true;
                    break;
                }
                _ => break,
            }
        }
        self.bump(); // {
        let mut params = Vec::new();
        if has_params {
            while !self.at(&Tok::In) {
                if self.at(&Tok::Underscore) {
                    let span = self.bump().span;
                    params.push(Ident { name: "_".into(), span });
                } else {
                    params.push(self.ident("a parameter name")?);
                }
                self.eat(&Tok::Comma);
            }
            self.bump(); // in
        }
        let stmts = self.with_trailing(true, |p| p.stmts_until_brace());
        let span = lo.to(self.prev_span());
        let body = Block { stmts, span };
        Ok(self.new_expr(ExprKind::Closure { params, body }, span))
    }

    fn primary_expr(&mut self) -> PResult<Expr> {
        let span = self.span();
        let kind = match self.peek().clone() {
            Tok::Int(n) => {
                self.bump();
                ExprKind::Int(n)
            }
            Tok::Float(x) => {
                self.bump();
                ExprKind::Float(x)
            }
            Tok::True | Tok::False => ExprKind::Bool(self.bump().tok == Tok::True),
            Tok::Nil => {
                self.bump();
                ExprKind::Nil
            }
            Tok::Str(pieces) => {
                self.bump();
                ExprKind::Str(self.interpolate(pieces))
            }
            Tok::Ident(name) => {
                self.bump();
                // `Stack<Int>(…)`, `Result<Int>.ok(…)`
                if self.at(&Tok::Lt) && name.starts_with(char::is_uppercase) && self.type_args_ahead() {
                    let args = self.type_args()?;
                    return Ok(self.new_expr(ExprKind::GenericName(name, args), span.to(self.prev_span())));
                }
                ExprKind::Name(name)
            }
            Tok::Dot => {
                self.bump();
                let name = self.ident("a name after `.`")?;
                return Ok(self.new_expr(ExprKind::DotName(name.name), span.to(name.span)));
            }
            Tok::LParen => {
                self.bump();
                let mut inner = self.with_trailing(true, |p| p.expr())?;
                self.expect(Tok::RParen)?;
                inner.span = span.to(self.prev_span());
                return Ok(inner);
            }
            Tok::LBracket => return self.collection(),
            Tok::LBrace => return self.closure(),
            Tok::If => return self.if_expr(),
            Tok::Match => return self.match_expr(),
            _ => return Err(self.unexpected("a value")),
        };
        Ok(self.new_expr(kind, span))
    }

    fn interpolate(&mut self, pieces: Vec<StrPiece>) -> Vec<StrPart> {
        let mut parts = Vec::new();
        for piece in pieces {
            match piece {
                StrPiece::Text(t) => parts.push(StrPart::Text(t)),
                StrPiece::Expr(span) => {
                    let errors_before = self.diags.list.len();
                    let src = &self.text[span.lo as usize..span.hi as usize];
                    let toks = lex(self.file, src, span.lo as usize, self.diags);
                    let saved_toks = std::mem::replace(&mut self.toks, toks);
                    let saved_pos = std::mem::replace(&mut self.pos, 0);
                    let e = self.with_trailing(true, |p| p.expr());
                    let e = match e {
                        Ok(e) if self.at(&Tok::Eof) => e,
                        Ok(_) => {
                            self.unexpected("`}` to end the interpolation");
                            self.new_expr(ExprKind::Error, span)
                        }
                        Err(Reported) => self.new_expr(ExprKind::Error, span),
                    };
                    self.toks = saved_toks;
                    self.pos = saved_pos;
                    // `{…}` that isn't valid code is usually a brace meant literally:
                    // keep one error and say how to write it.
                    let e = if self.diags.list.len() > errors_before {
                        self.diags.list.truncate(errors_before + 1);
                        self.diags.list[errors_before]
                            .help("`{…}` in a string inserts a value; to write a brace itself, use \\{");
                        // Don't check a half-parsed expression (more confusing errors).
                        self.new_expr(ExprKind::Error, span)
                    } else {
                        e
                    };
                    parts.push(StrPart::Expr(e));
                }
            }
        }
        parts
    }

    /// `[a, b]`, `[]`, `[k: v, …]` or `[:]`.
    fn collection(&mut self) -> PResult<Expr> {
        let lo = self.bump().span; // [
        if self.eat(&Tok::Colon) {
            self.expect(Tok::RBracket)?;
            return Ok(self.new_expr(ExprKind::Dict(Vec::new()), lo.to(self.prev_span())));
        }
        let mut items = Vec::new();
        let mut pairs = Vec::new();
        let mut is_dict = None;
        self.with_trailing(true, |p| -> PResult<()> {
            while !p.at(&Tok::RBracket) {
                let first = p.expr()?;
                let dict_entry = p.eat(&Tok::Colon);
                if *is_dict.get_or_insert(dict_entry) != dict_entry {
                    let s = first.span;
                    p.error(s, "a list literal can't mix plain values and `key: value` pairs");
                    return Err(Reported);
                }
                if dict_entry {
                    let value = p.expr()?;
                    pairs.push((first, value));
                } else {
                    items.push(first);
                }
                if !p.eat(&Tok::Comma) {
                    break;
                }
            }
            Ok(())
        })?;
        self.expect(Tok::RBracket)?;
        let span = lo.to(self.prev_span());
        let kind = if is_dict == Some(true) { ExprKind::Dict(pairs) } else { ExprKind::List(items) };
        Ok(self.new_expr(kind, span))
    }

    /// The condition of `if` or `while`: `x > 0`, or `let x = optional`.
    fn condition(&mut self) -> PResult<Cond> {
        self.with_trailing(false, |p| -> PResult<Cond> {
            if p.eat(&Tok::Let) {
                let name = p.ident("a name")?;
                let ty = if p.eat(&Tok::Colon) { Some(p.type_expr()?) } else { None };
                p.expect(Tok::Eq)?;
                Ok(Cond::Let(name, ty, Box::new(p.expr()?)))
            } else {
                Ok(Cond::Expr(Box::new(p.expr()?)))
            }
        })
    }

    fn if_expr(&mut self) -> PResult<Expr> {
        let lo = self.bump().span; // if
        let cond = self.condition()?;
        if !self.at(&Tok::LBrace) {
            let span = self.span();
            self.error(span, "expected `{` after the `if` condition")
                .help("the body of an `if` goes in braces: `if ready { … }`");
            return Err(Reported);
        }
        let then = self.block()?;
        // Allow `else` on the line after the closing brace.
        if *self.peek() == Tok::Newline && *self.peek_at(1) == Tok::Else {
            self.bump();
        }
        let otherwise = if self.eat(&Tok::Else) {
            if self.at(&Tok::If) {
                Some(Else::If(Box::new(self.if_expr()?)))
            } else {
                Some(Else::Block(self.block()?))
            }
        } else {
            None
        };
        let span = lo.to(self.prev_span());
        Ok(self.new_expr(ExprKind::If { cond, then, otherwise }, span))
    }

    fn match_expr(&mut self) -> PResult<Expr> {
        let lo = self.bump().span; // match
        let subject = self.with_trailing(false, |p| p.expr())?;
        self.expect(Tok::LBrace)?;
        let mut arms = Vec::new();
        loop {
            self.skip_newlines();
            if self.eat(&Tok::RBrace) {
                break;
            }
            if self.at(&Tok::Eof) {
                self.unexpected("`}`");
                break;
            }
            let start = self.pos;
            let lo = self.span();
            match self.arm() {
                Ok(arm) => {
                    arms.push(arm);
                    if !matches!(self.peek(), Tok::Newline | Tok::RBrace) {
                        // `.a -> x = 1`: an assignment is a statement, not a value.
                        let assigns =
                            matches!(self.peek(), Tok::Eq | Tok::PlusEq | Tok::MinusEq | Tok::StarEq | Tok::SlashEq);
                        let span = self.span();
                        let found = describe(self.peek());
                        let e = self.error(span, format!("expected end of line after the match arm, found {found}"));
                        if assigns {
                            e.help("to change something in an arm, put it in braces: `.case -> { x = 1 }`");
                        }
                        self.recover_line();
                    }
                }
                Err(Reported) => {
                    if self.pos == start {
                        self.bump();
                    }
                    self.recover_line();
                    let span = lo.to(self.prev_span());
                    let body = self.new_expr(ExprKind::Error, span);
                    arms.push(Arm { pattern: Pattern::Error(span), body, span });
                }
            }
        }
        let span = lo.to(self.prev_span());
        Ok(self.new_expr(ExprKind::Match { subject: Box::new(subject), arms }, span))
    }

    fn arm(&mut self) -> PResult<Arm> {
        let lo = self.span();
        let pattern = match self.peek().clone() {
            Tok::Dot => {
                self.bump();
                let name = self.ident("a case name")?;
                let mut bindings = Vec::new();
                if self.eat(&Tok::LParen) {
                    while !self.at(&Tok::RParen) {
                        if self.at(&Tok::Underscore) {
                            let span = self.bump().span;
                            bindings.push(Ident { name: "_".into(), span });
                        } else {
                            bindings.push(self.ident("a name")?);
                        }
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    self.expect(Tok::RParen)?;
                }
                Pattern::Case { name, bindings, span: lo.to(self.prev_span()) }
            }
            Tok::Underscore => Pattern::Wildcard(self.bump().span),
            Tok::Int(_) | Tok::Float(_) | Tok::Str(_) | Tok::True | Tok::False | Tok::Minus | Tok::Nil => {
                Pattern::Literal(self.unary_expr()?)
            }
            Tok::Ident(name) => {
                let span = self.span();
                self.error(span, format!("match patterns for enum cases start with a dot: `.{name}`"))
                    .help(format!("write `.{name} -> …`"));
                return Err(Reported);
            }
            _ => return Err(self.unexpected("a pattern like `.name`, `_` or a literal")),
        };
        if !self.at(&Tok::Arrow) {
            let span = self.span();
            self.error(span, "expected `->` after the match pattern").help("write each arm as `pattern -> value`");
            return Err(Reported);
        }
        self.bump();
        let body = if self.at(&Tok::LBrace) {
            // `-> { … }` is a block, not a closure.
            let block = self.block()?;
            let span = block.span;
            self.new_expr(ExprKind::Block(block), span)
        } else {
            self.expr()?
        };
        Ok(Arm { pattern, span: lo.to(body.span), body })
    }
}

/// Only names, members and calls can take a trailing block without parentheses
/// (`VStack { … }`, `list.filter { … }`), so `x + y {` isn't misread.
fn is_callable_head(e: &Expr) -> bool {
    matches!(e.kind, ExprKind::Name(_) | ExprKind::Member { .. })
}

fn keyword_text(tok: &Tok) -> String {
    describe(tok).trim_matches('`').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> (Program, Diagnostics) {
        let mut d = Diagnostics::default();
        let mut id = 0;
        let p = parse_file(0, src, &mut d, &mut id);
        (p, d)
    }

    fn parse_ok(src: &str) -> Program {
        let (p, d) = parse(src);
        assert!(d.is_empty(), "unexpected errors: {:#?}", d.list);
        p
    }

    #[test]
    fn examples_parse() {
        for path in ["counter/counter.tsl", "todos/todos.tsl"] {
            let full = format!("{}/../../examples/{path}", env!("CARGO_MANIFEST_DIR"));
            let src = std::fs::read_to_string(&full).unwrap();
            parse_ok(&src);
        }
    }

    #[test]
    fn modifier_on_next_line_continues_chain() {
        let p = parse_ok("view V {\n    Text(\"a\")\n    .padding(4)\n}");
        let Item::View(v) = &p.items[0] else { panic!() };
        assert_eq!(v.body.stmts.len(), 1);
    }

    #[test]
    fn match_arms_starting_with_dot_are_not_modifiers() {
        let p = parse_ok("fn f(x: F) -> Int {\n match x {\n  .a -> 1\n  .b -> 2\n }\n}");
        let Item::Fn(f) = &p.items[0] else { panic!() };
        let Stmt::Expr(Expr { kind: ExprKind::Match { arms, .. }, .. }) = &f.body.stmts[0] else { panic!() };
        assert_eq!(arms.len(), 2);
    }

    #[test]
    fn if_condition_does_not_take_trailing_block() {
        let p = parse_ok("fn f(ready: Bool) {\n if ready { print(\"x\") }\n}");
        let Item::Fn(f) = &p.items[0] else { panic!() };
        assert!(matches!(&f.body.stmts[0], Stmt::Expr(Expr { kind: ExprKind::If { .. }, .. })));
    }

    #[test]
    fn errors_recover_on_next_line() {
        let (_, d) = parse("fn f() {\n let = 1\n let y 2\n}");
        assert_eq!(d.list.len(), 2, "{:#?}", d.list);
    }
}
