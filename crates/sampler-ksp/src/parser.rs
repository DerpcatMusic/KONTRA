//! Newline-insensitive recursive-descent parser with Pratt expressions.
use crate::ast::*;
use crate::diag::{Result, fault};
use crate::lexer::{Interner, Punct, Sym, Tok, Token};

const MAX_DEPTH: usize = 256;

/// Interned keyword symbols, resolved once so the parser compares integers.
struct Keywords {
    on: Option<Sym>,
    end: Option<Sym>,
    function: Option<Sym>,
    if_: Option<Sym>,
    else_: Option<Sym>,
    while_: Option<Sym>,
    select: Option<Sym>,
    case: Option<Sym>,
    to: Option<Sym>,
    call: Option<Sym>,
    declare: Option<Sym>,
    and: Option<Sym>,
    or: Option<Sym>,
    not: Option<Sym>,
    xor: Option<Sym>,
    mod_: Option<Sym>,
    const_: Option<Sym>,
    polyphonic: Option<Sym>,
    pers: Option<Sym>,
    instpers: Option<Sym>,
}

pub struct Parser<'t> {
    toks: &'t [Token],
    pos: usize,
    kw: Keywords,
}

pub fn parse(toks: &[Token], syms: &Interner<'_>) -> Result<Ast> {
    let kw = Keywords {
        on: syms.get("on"),
        end: syms.get("end"),
        function: syms.get("function"),
        if_: syms.get("if"),
        else_: syms.get("else"),
        while_: syms.get("while"),
        select: syms.get("select"),
        case: syms.get("case"),
        to: syms.get("to"),
        call: syms.get("call"),
        declare: syms.get("declare"),
        and: syms.get("and"),
        or: syms.get("or"),
        not: syms.get("not"),
        xor: syms.get("xor"),
        mod_: syms.get("mod"),
        const_: syms.get("const"),
        polyphonic: syms.get("polyphonic"),
        pers: syms.get("pers"),
        instpers: syms.get("instpers"),
    };
    let mut p = Parser { toks, pos: 0, kw };
    let mut items = Vec::new();
    loop {
        let token = p.peek();
        if token.tok == Tok::Eof {
            return Ok(Ast { items });
        }
        if p.eat_kw(p.kw.on) {
            let Tok::Ident(name) = p.peek().tok else {
                return fault(p.peek().span, "expected callback name after on");
            };
            p.pos += 1;
            let arg = if p.eat(Punct::LParen) {
                let t = p.next();
                let Tok::Var(v) = t.tok else {
                    return fault(t.span, "expected control variable");
                };
                p.need(Punct::RParen)?;
                Some((v, t.span))
            } else {
                None
            };
            let body = p.block(0)?;
            p.close(p.kw.on, "end on")?;
            items.push(Item::Callback(Callback {
                name,
                span: token.span,
                arg,
                body,
            }));
        } else if p.eat_kw(p.kw.function) {
            let t = p.next();
            let Tok::Ident(name) = t.tok else {
                return fault(t.span, "expected function name");
            };
            if p.eat(Punct::LParen) {
                p.need(Punct::RParen)?;
            }
            let body = p.block(0)?;
            p.close(p.kw.function, "end function")?;
            items.push(Item::Function(Function {
                name,
                span: t.span,
                body,
            }));
        } else {
            return fault(token.span, "expected callback or function");
        }
    }
}

impl Parser<'_> {
    fn peek(&self) -> Token {
        self.toks[self.pos]
    }
    fn next(&mut self) -> Token {
        let t = self.peek();
        if t.tok != Tok::Eof {
            self.pos += 1;
        }
        t
    }
    fn is_kw(&self, kw: Option<Sym>) -> bool {
        kw.is_some_and(|k| self.peek().tok == Tok::Ident(k))
    }
    fn eat_kw(&mut self, kw: Option<Sym>) -> bool {
        let hit = self.is_kw(kw);
        if hit {
            self.pos += 1;
        }
        hit
    }
    fn eat(&mut self, p: Punct) -> bool {
        let hit = self.peek().tok == Tok::Punct(p);
        if hit {
            self.pos += 1;
        }
        hit
    }
    fn need(&mut self, p: Punct) -> Result<()> {
        if self.eat(p) {
            return Ok(());
        }
        let what = match p {
            Punct::LParen => "'('",
            Punct::RParen => "')'",
            Punct::RBracket => "']'",
            Punct::Comma => "','",
            Punct::Assign => "':='",
            _ => "punctuation",
        };
        fault(self.peek().span, format!("expected {what}"))
    }
    fn close(&mut self, kw: Option<Sym>, what: &str) -> Result<()> {
        if self.eat_kw(self.kw.end) && self.eat_kw(kw) {
            return Ok(());
        }
        fault(self.peek().span, format!("expected {what}"))
    }

    fn at_block_end(&self) -> bool {
        self.peek().tok == Tok::Eof
            || self.is_kw(self.kw.end)
            || self.is_kw(self.kw.else_)
            || self.is_kw(self.kw.case)
            || self.is_kw(self.kw.on)
            || self.is_kw(self.kw.function)
    }

    fn block(&mut self, depth: usize) -> Result<Vec<Stmt>> {
        if depth >= MAX_DEPTH {
            return fault(self.peek().span, "statement nesting limit exceeded");
        }
        let mut out = Vec::new();
        while !self.at_block_end() {
            out.push(self.statement(depth)?);
        }
        Ok(out)
    }

    fn statement(&mut self, depth: usize) -> Result<Stmt> {
        let start = self.peek();
        let kw = &self.kw;
        let kind = match start.tok {
            Tok::Ident(k) if Some(k) == kw.if_ => {
                self.pos += 1;
                let cond = self.expr(0, 0)?;
                let yes = self.block(depth + 1)?;
                let no = if self.eat_kw(self.kw.else_) {
                    self.block(depth + 1)?
                } else {
                    Vec::new()
                };
                self.close(self.kw.if_, "end if")?;
                StmtKind::If(cond, yes, no)
            }
            Tok::Ident(k) if Some(k) == kw.while_ => {
                self.pos += 1;
                let cond = self.expr(0, 0)?;
                let body = self.block(depth + 1)?;
                self.close(self.kw.while_, "end while")?;
                StmtKind::While(cond, body)
            }
            Tok::Ident(k) if Some(k) == kw.select => {
                self.pos += 1;
                let value = self.expr(0, 0)?;
                let mut cases = Vec::new();
                while self.is_kw(self.kw.case) {
                    let span = self.next().span;
                    let low = self.expr(0, 0)?;
                    let high = if self.eat_kw(self.kw.to) {
                        Some(self.expr(0, 0)?)
                    } else {
                        None
                    };
                    let body = self.block(depth + 1)?;
                    cases.push(Case {
                        span,
                        low,
                        high,
                        body,
                    });
                }
                self.close(self.kw.select, "case or end select")?;
                StmtKind::Select(value, cases)
            }
            Tok::Ident(k) if Some(k) == kw.declare => {
                self.pos += 1;
                StmtKind::Declare(Box::new(self.declare()?))
            }
            Tok::Ident(k) if Some(k) == kw.call => {
                self.pos += 1;
                let t = self.next();
                let Tok::Ident(name) = t.tok else {
                    return fault(t.span, "expected function name");
                };
                if self.eat(Punct::LParen) {
                    self.need(Punct::RParen)?;
                }
                StmtKind::Call(name)
            }
            Tok::Var(name) => {
                self.pos += 1;
                let target = Expr {
                    span: start.span,
                    kind: ExprKind::Var(name, self.index()?),
                };
                self.need(Punct::Assign)?;
                StmtKind::Assign(target, self.expr(0, 0)?)
            }
            Tok::Ident(name) => {
                self.pos += 1;
                let args = if self.eat(Punct::LParen) {
                    self.args(0)?
                } else {
                    Vec::new()
                };
                StmtKind::Command(name, args)
            }
            _ => return fault(start.span, "expected statement"),
        };
        let end = self.toks[self.pos.saturating_sub(1)].span;
        Ok(Stmt {
            span: start.span.to(end),
            kind,
        })
    }

    fn declare(&mut self) -> Result<Declare> {
        let mut storage = Storage::Plain;
        let mut ui = None;
        loop {
            let t = self.peek();
            let Tok::Ident(word) = t.tok else { break };
            let kw = &self.kw;
            storage = if Some(word) == kw.const_ {
                Storage::Const
            } else if Some(word) == kw.polyphonic {
                Storage::Polyphonic
            } else if Some(word) == kw.pers {
                Storage::Persistent
            } else if Some(word) == kw.instpers {
                Storage::InstrumentPersistent
            } else if ui.is_none() {
                ui = Some(word);
                storage
            } else {
                return fault(t.span, "unexpected declaration keyword");
            };
            self.pos += 1;
        }
        let t = self.next();
        let Tok::Var(name) = t.tok else {
            return fault(t.span, "expected variable name");
        };
        let size = self.index()?.map(|e| *e);
        let params = if self.eat(Punct::LParen) {
            self.args(0)?
        } else {
            Vec::new()
        };
        let (mut init, mut init_list) = (Vec::new(), false);
        if self.eat(Punct::Assign) {
            if size.is_some() && self.peek().tok == Tok::Punct(Punct::LParen) {
                self.pos += 1;
                init = self.args(0)?;
                init_list = true;
            } else {
                init.push(self.expr(0, 0)?);
            }
        }
        Ok(Declare {
            storage,
            ui,
            name,
            name_span: t.span,
            size,
            params,
            init,
            init_list,
        })
    }

    fn index(&mut self) -> Result<Option<Box<Expr>>> {
        if !self.eat(Punct::LBracket) {
            return Ok(None);
        }
        let e = self.expr(0, 0)?;
        self.need(Punct::RBracket)?;
        Ok(Some(Box::new(e)))
    }

    fn args(&mut self, depth: usize) -> Result<Vec<Expr>> {
        let mut a = Vec::new();
        if self.eat(Punct::RParen) {
            return Ok(a);
        }
        loop {
            a.push(self.expr(0, depth + 1)?);
            if self.eat(Punct::RParen) {
                return Ok(a);
            }
            if !self.eat(Punct::Comma) {
                return fault(self.peek().span, "expected ',' or ')'");
            }
        }
    }

    fn binop(&self) -> Option<BinOp> {
        let kw = &self.kw;
        Some(match self.peek().tok {
            Tok::Ident(k) if Some(k) == kw.or => BinOp::Or,
            Tok::Ident(k) if Some(k) == kw.xor => BinOp::Xor,
            Tok::Ident(k) if Some(k) == kw.and => BinOp::And,
            Tok::Ident(k) if Some(k) == kw.mod_ => BinOp::Mod,
            Tok::Punct(p) => match p {
                Punct::Amp => BinOp::Concat,
                Punct::Eq => BinOp::Eq,
                Punct::Ne => BinOp::Ne,
                Punct::Lt => BinOp::Lt,
                Punct::Gt => BinOp::Gt,
                Punct::Le => BinOp::Le,
                Punct::Ge => BinOp::Ge,
                Punct::BitOr => BinOp::BitOr,
                Punct::BitXor => BinOp::BitXor,
                Punct::BitAnd => BinOp::BitAnd,
                Punct::Plus => BinOp::Add,
                Punct::Minus => BinOp::Sub,
                Punct::Star => BinOp::Mul,
                Punct::Slash => BinOp::Div,
                _ => return None,
            },
            _ => return None,
        })
    }

    pub fn expr(&mut self, min: u8, depth: usize) -> Result<Expr> {
        if depth >= MAX_DEPTH {
            return fault(self.peek().span, "expression nesting limit exceeded");
        }
        let t = self.next();
        let unary = |op, e: Expr| Expr {
            span: t.span.to(e.span),
            kind: ExprKind::Unary(op, Box::new(e)),
        };
        let mut lhs = match t.tok {
            Tok::Punct(Punct::LParen) => {
                let mut e = self.expr(0, depth + 1)?;
                self.need(Punct::RParen)?;
                e.span = t.span.to(self.toks[self.pos - 1].span);
                e
            }
            Tok::Punct(Punct::Minus) => unary(UnOp::Neg, self.expr(UNARY_BP, depth + 1)?),
            Tok::Punct(Punct::Plus) => self.expr(UNARY_BP, depth + 1)?,
            Tok::Punct(Punct::BitNot) => unary(UnOp::BitNot, self.expr(UNARY_BP, depth + 1)?),
            Tok::Ident(k) if Some(k) == self.kw.not => {
                unary(UnOp::Not, self.expr(NOT_OPERAND_BP, depth + 1)?)
            }
            Tok::Int(n, hex) => Expr {
                span: t.span,
                kind: ExprKind::Int(n, hex),
            },
            Tok::Real(n) => Expr {
                span: t.span,
                kind: ExprKind::Real(n),
            },
            Tok::Str(s) => Expr {
                span: t.span,
                kind: ExprKind::Str(s),
            },
            Tok::Var(v) => {
                let index = self.index()?;
                let end = self.toks[self.pos - 1].span;
                Expr {
                    span: t.span.to(end),
                    kind: ExprKind::Var(v, index),
                }
            }
            Tok::Ident(name) if self.peek().tok == Tok::Punct(Punct::LParen) => {
                self.pos += 1;
                let args = self.args(depth + 1)?;
                Expr {
                    span: t.span.to(self.toks[self.pos - 1].span),
                    kind: ExprKind::Call(name, args),
                }
            }
            Tok::Ident(name) => Expr {
                span: t.span,
                kind: ExprKind::Ident(name),
            },
            _ => return fault(t.span, "expected expression"),
        };
        let mut depth = depth;
        while let Some(op) = self.binop() {
            let bp = op.binding_power();
            if bp < min {
                break;
            }
            depth += 1;
            if depth >= MAX_DEPTH {
                return fault(self.peek().span, "expression nesting limit exceeded");
            }
            self.pos += 1;
            let rhs = self.expr(bp + 1, depth)?;
            lhs = Expr {
                span: lhs.span.to(rhs.span),
                kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)),
            };
        }
        Ok(lhs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;

    fn ast(source: &str) -> Result<Ast> {
        let mut syms = Interner::default();
        let toks = lex(source, &mut syms)?;
        parse(&toks, &syms)
    }

    #[test]
    fn single_line_blocks_and_precedence() {
        let a = ast("on init declare $a := 1 + 2 * 3 end on function f() call g end function on note if ($a = 1 and not $a > 2) message(\"x\" & $a) else exit end if end on").unwrap();
        assert_eq!(a.items.len(), 3);
        let Item::Callback(init) = &a.items[0] else {
            panic!()
        };
        let StmtKind::Declare(d) = &init.body[0].kind else {
            panic!()
        };
        let ExprKind::Binary(BinOp::Add, _, rhs) = &d.init[0].kind else {
            panic!()
        };
        assert!(matches!(rhs.kind, ExprKind::Binary(BinOp::Mul, ..)));
        assert!(ast("on init end on on note").is_err());
        assert!(ast("on note if (1) end on").is_err());
        assert!(ast("on note select ($x) message(1) end select end on").is_err());
    }
}
