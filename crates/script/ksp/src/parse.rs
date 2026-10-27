//! Syntax tree and a newline-insensitive recursive-descent parser.

use crate::lex::{P, Tok, Token};
use crate::{Error, Span};

const MAX_DEPTH: usize = 256;

#[derive(Debug)]
pub enum Item<'a> {
    Callback {
        name: &'a str,
        span: Span,
        /// `on ui_control ($control)`.
        arg: Option<(&'a str, Span)>,
        body: Vec<Stmt<'a>>,
    },
    Function {
        name: &'a str,
        span: Span,
        body: Vec<Stmt<'a>>,
    },
}

#[derive(Debug)]
pub struct Stmt<'a> {
    pub span: Span,
    pub kind: StmtKind<'a>,
}

#[derive(Debug)]
pub enum StmtKind<'a> {
    Declare(Box<Declare<'a>>),
    /// The target is always [`ExprKind::Var`].
    Assign(Expr<'a>, Expr<'a>),
    /// A builtin or a user function, by name.
    Command(&'a str, Vec<Expr<'a>>),
    If(Expr<'a>, Vec<Stmt<'a>>, Vec<Stmt<'a>>),
    While(Expr<'a>, Vec<Stmt<'a>>),
    Select(Expr<'a>, Vec<Case<'a>>),
    Call(&'a str),
}

#[derive(Debug)]
pub struct Case<'a> {
    pub low: Expr<'a>,
    pub high: Option<Expr<'a>>,
    pub body: Vec<Stmt<'a>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Storage {
    Plain,
    Const,
    Polyphonic,
    /// Saved with the instrument and snapshots.
    Persistent,
    /// Saved with the instrument only.
    InstrumentPersistent,
}

#[derive(Debug)]
pub struct Declare<'a> {
    pub storage: Storage,
    /// The `ui_*` keyword.
    pub ui: Option<(&'a str, Span)>,
    pub name: &'a str,
    pub span: Span,
    pub size: Option<Expr<'a>>,
    pub params: Vec<Expr<'a>>,
    /// One value, or every value of a `:= (a, b, ...)` list.
    pub init: Vec<Expr<'a>>,
    pub list: bool,
}

#[derive(Clone, Debug)]
pub struct Expr<'a> {
    pub span: Span,
    pub kind: ExprKind<'a>,
}

#[derive(Clone, Debug)]
pub enum ExprKind<'a> {
    Int(u64),
    Real(f64),
    Str(&'a str),
    /// A bare word: a PGS key name or a parameterless builtin.
    Word(&'a str),
    Var(&'a str, Option<Box<Expr<'a>>>),
    Call(&'a str, Vec<Expr<'a>>),
    Unary(Un, Box<Expr<'a>>),
    Binary(Bin, Box<Expr<'a>>, Box<Expr<'a>>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Un {
    Neg,
    Not,
    BitNot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bin {
    Or,
    Xor,
    And,
    Concat,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    BitOr,
    BitXor,
    BitAnd,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

impl Bin {
    fn power(self) -> u8 {
        match self {
            Self::Or | Self::Xor => 1,
            Self::And => 2,
            Self::Concat => 4,
            Self::Eq | Self::Ne | Self::Lt | Self::Gt | Self::Le | Self::Ge => 5,
            Self::BitOr | Self::BitXor => 6,
            Self::BitAnd => 7,
            Self::Add | Self::Sub => 8,
            Self::Mul | Self::Div | Self::Mod => 9,
        }
    }
}

const KEYWORDS: &[&str] = &[
    "and", "call", "case", "declare", "else", "end", "function", "if", "mod", "not", "on", "or",
    "select", "to", "while", "xor",
];

/// `not` binds looser than comparisons: `not $a = 1` is `not ($a = 1)`.
const NOT_OPERAND: u8 = 3;
const UNARY: u8 = 10;

pub fn parse<'a>(tokens: &[Token<'a>]) -> Result<Vec<Item<'a>>, Error> {
    let mut p = Parser { tokens, pos: 0 };
    let mut items = Vec::new();
    loop {
        let start = p.peek();
        if start.tok == Tok::Eof {
            return Ok(items);
        }
        if p.eat_word("on") {
            let Tok::Word(name) = p.peek().tok else {
                return p.fail("expected a callback name");
            };
            p.pos += 1;
            let arg = if p.eat(P::LParen) {
                let t = p.next();
                let Tok::Var(v) = t.tok else {
                    return Err(Error::new(t.span, "expected a control variable"));
                };
                p.need(P::RParen)?;
                Some((v, t.span))
            } else {
                None
            };
            let body = p.block(0)?;
            p.close("on")?;
            items.push(Item::Callback {
                name,
                span: start.span,
                arg,
                body,
            });
        } else if p.eat_word("function") {
            let t = p.next();
            let Tok::Word(name) = t.tok else {
                return Err(Error::new(t.span, "expected a function name"));
            };
            if p.eat(P::LParen) {
                p.need(P::RParen)?;
            }
            let body = p.block(0)?;
            p.close("function")?;
            items.push(Item::Function {
                name,
                span: t.span,
                body,
            });
        } else {
            return p.fail("expected a callback or function");
        }
    }
}

struct Parser<'t, 'a> {
    tokens: &'t [Token<'a>],
    pos: usize,
}

impl<'a> Parser<'_, 'a> {
    fn peek(&self) -> Token<'a> {
        let eof = Token {
            tok: Tok::Eof,
            span: Span::default(),
        };
        self.tokens.get(self.pos).copied().unwrap_or(eof)
    }

    fn next(&mut self) -> Token<'a> {
        let t = self.peek();
        if t.tok != Tok::Eof {
            self.pos += 1;
        }
        t
    }

    fn last_span(&self) -> Span {
        self.tokens
            .get(self.pos.saturating_sub(1))
            .map_or_else(Span::default, |t| t.span)
    }

    fn fail<T>(&self, message: &str) -> Result<T, Error> {
        Err(Error::new(self.peek().span, message))
    }

    fn is_word(&self, word: &str) -> bool {
        self.peek().tok == Tok::Word(word)
    }

    fn eat_word(&mut self, word: &str) -> bool {
        let hit = self.is_word(word);
        self.pos += usize::from(hit);
        hit
    }

    fn eat(&mut self, p: P) -> bool {
        let hit = self.peek().tok == Tok::P(p);
        self.pos += usize::from(hit);
        hit
    }

    fn need(&mut self, p: P) -> Result<(), Error> {
        if self.eat(p) {
            return Ok(());
        }
        let what = match p {
            P::LParen => "expected '('",
            P::RParen => "expected ')'",
            P::RBracket => "expected ']'",
            P::Assign => "expected ':='",
            _ => "unexpected token",
        };
        self.fail(what)
    }

    fn close(&mut self, word: &str) -> Result<(), Error> {
        if self.eat_word("end") && self.eat_word(word) {
            return Ok(());
        }
        Err(Error::new(
            self.peek().span,
            format!("expected 'end {word}'"),
        ))
    }

    fn at_block_end(&self) -> bool {
        matches!(
            self.peek().tok,
            Tok::Eof | Tok::Word("end" | "else" | "case" | "on" | "function")
        )
    }

    fn block(&mut self, depth: usize) -> Result<Vec<Stmt<'a>>, Error> {
        if depth >= MAX_DEPTH {
            return self.fail("statements nested too deeply");
        }
        let mut out = Vec::new();
        while !self.at_block_end() {
            out.push(self.statement(depth)?);
        }
        Ok(out)
    }

    fn statement(&mut self, depth: usize) -> Result<Stmt<'a>, Error> {
        let start = self.next();
        let kind = match start.tok {
            Tok::Word("if") => {
                let cond = self.expr(0, 0)?;
                let yes = self.block(depth + 1)?;
                let no = if self.eat_word("else") {
                    self.block(depth + 1)?
                } else {
                    Vec::new()
                };
                self.close("if")?;
                StmtKind::If(cond, yes, no)
            }
            Tok::Word("while") => {
                let cond = self.expr(0, 0)?;
                let body = self.block(depth + 1)?;
                self.close("while")?;
                StmtKind::While(cond, body)
            }
            Tok::Word("select") => {
                let value = self.expr(0, 0)?;
                let mut cases = Vec::new();
                while self.eat_word("case") {
                    let low = self.expr(0, 0)?;
                    let high = if self.eat_word("to") {
                        Some(self.expr(0, 0)?)
                    } else {
                        None
                    };
                    let body = self.block(depth + 1)?;
                    cases.push(Case { low, high, body });
                }
                self.close("select")?;
                StmtKind::Select(value, cases)
            }
            Tok::Word("declare") => StmtKind::Declare(Box::new(self.declare()?)),
            Tok::Word("call") => {
                let t = self.next();
                let Tok::Word(name) = t.tok else {
                    return Err(Error::new(t.span, "expected a function name"));
                };
                if self.eat(P::LParen) {
                    self.need(P::RParen)?;
                }
                StmtKind::Call(name)
            }
            Tok::Var(name) => {
                let target = Expr {
                    span: start.span,
                    kind: ExprKind::Var(name, self.index(0)?),
                };
                self.need(P::Assign)?;
                StmtKind::Assign(target, self.expr(0, 0)?)
            }
            Tok::Word(name) => {
                let args = if self.eat(P::LParen) {
                    self.args(0)?
                } else {
                    Vec::new()
                };
                StmtKind::Command(name, args)
            }
            _ => return Err(Error::new(start.span, "expected a statement")),
        };
        Ok(Stmt {
            span: start.span.to(self.last_span()),
            kind,
        })
    }

    fn declare(&mut self) -> Result<Declare<'a>, Error> {
        let mut storage = Storage::Plain;
        let mut ui = None;
        while let Tok::Word(word) = self.peek().tok {
            let span = self.peek().span;
            match word {
                "const" => storage = Storage::Const,
                "polyphonic" => storage = Storage::Polyphonic,
                "pers" => storage = Storage::Persistent,
                "instpers" => storage = Storage::InstrumentPersistent,
                _ if ui.is_none() => ui = Some((word, span)),
                _ => return self.fail("unexpected declaration keyword"),
            }
            self.pos += 1;
        }
        let t = self.next();
        let Tok::Var(name) = t.tok else {
            return Err(Error::new(t.span, "expected a variable name"));
        };
        let size = self.index(0)?.map(|e| *e);
        let params = if self.eat(P::LParen) {
            self.args(0)?
        } else {
            Vec::new()
        };
        let (mut init, mut list) = (Vec::new(), false);
        if self.eat(P::Assign) {
            if size.is_some() && self.eat(P::LParen) {
                init = self.args(0)?;
                if init.is_empty() {
                    return Err(Error::new(self.last_span(), "empty initializer list"));
                }
                list = true;
            } else {
                init.push(self.expr(0, 0)?);
            }
        }
        Ok(Declare {
            storage,
            ui,
            name,
            span: t.span,
            size,
            params,
            init,
            list,
        })
    }

    fn index(&mut self, depth: usize) -> Result<Option<Box<Expr<'a>>>, Error> {
        if !self.eat(P::LBracket) {
            return Ok(None);
        }
        let e = self.expr(0, depth + 1)?;
        self.need(P::RBracket)?;
        Ok(Some(Box::new(e)))
    }

    /// Arguments after an opening parenthesis, through the closing one.
    fn args(&mut self, depth: usize) -> Result<Vec<Expr<'a>>, Error> {
        let mut out = Vec::new();
        if self.eat(P::RParen) {
            return Ok(out);
        }
        loop {
            out.push(self.expr(0, depth + 1)?);
            if self.eat(P::RParen) {
                return Ok(out);
            }
            if !self.eat(P::Comma) {
                return self.fail("expected ',' or ')'");
            }
        }
    }

    fn binary(&self) -> Option<Bin> {
        Some(match self.peek().tok {
            Tok::Word("or") => Bin::Or,
            Tok::Word("xor") => Bin::Xor,
            Tok::Word("and") => Bin::And,
            Tok::Word("mod") => Bin::Mod,
            Tok::P(P::Amp) => Bin::Concat,
            Tok::P(P::Eq) => Bin::Eq,
            Tok::P(P::Ne) => Bin::Ne,
            Tok::P(P::Lt) => Bin::Lt,
            Tok::P(P::Gt) => Bin::Gt,
            Tok::P(P::Le) => Bin::Le,
            Tok::P(P::Ge) => Bin::Ge,
            Tok::P(P::BitOr) => Bin::BitOr,
            Tok::P(P::BitXor) => Bin::BitXor,
            Tok::P(P::BitAnd) => Bin::BitAnd,
            Tok::P(P::Plus) => Bin::Add,
            Tok::P(P::Minus) => Bin::Sub,
            Tok::P(P::Star) => Bin::Mul,
            Tok::P(P::Slash) => Bin::Div,
            _ => return None,
        })
    }

    fn expr(&mut self, min: u8, depth: usize) -> Result<Expr<'a>, Error> {
        if depth >= MAX_DEPTH {
            return self.fail("expression nested too deeply");
        }
        let t = self.next();
        let unary = |op, e: Expr<'a>| Expr {
            span: t.span.to(e.span),
            kind: ExprKind::Unary(op, Box::new(e)),
        };
        let leaf = |kind| Expr { span: t.span, kind };
        let mut lhs = match t.tok {
            Tok::P(P::LParen) => {
                let mut e = self.expr(0, depth + 1)?;
                self.need(P::RParen)?;
                e.span = t.span.to(self.last_span());
                e
            }
            Tok::P(P::Minus) => unary(Un::Neg, self.expr(UNARY, depth + 1)?),
            Tok::P(P::Plus) => self.expr(UNARY, depth + 1)?,
            Tok::P(P::BitNot) => unary(Un::BitNot, self.expr(UNARY, depth + 1)?),
            Tok::Word("not") => unary(Un::Not, self.expr(NOT_OPERAND, depth + 1)?),
            Tok::Int(n) => leaf(ExprKind::Int(n)),
            Tok::Real(n) => leaf(ExprKind::Real(n)),
            Tok::Str(s) => leaf(ExprKind::Str(s)),
            Tok::Var(v) => {
                let index = self.index(depth)?;
                Expr {
                    span: t.span.to(self.last_span()),
                    kind: ExprKind::Var(v, index),
                }
            }
            Tok::Word(name) if self.eat(P::LParen) => {
                let args = self.args(depth)?;
                Expr {
                    span: t.span.to(self.last_span()),
                    kind: ExprKind::Call(name, args),
                }
            }
            Tok::Word(name) if !KEYWORDS.contains(&name) => leaf(ExprKind::Word(name)),
            _ => return Err(Error::new(t.span, "expected an expression")),
        };
        let mut depth = depth;
        while let Some(op) = self.binary() {
            if op.power() < min {
                break;
            }
            depth += 1;
            if depth >= MAX_DEPTH {
                return self.fail("expression nested too deeply");
            }
            self.pos += 1;
            let rhs = self.expr(op.power() + 1, depth)?;
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
    use crate::lex::lex;

    fn items(source: &str) -> Result<Vec<Item<'_>>, Error> {
        parse(&lex(source)?)
    }

    #[test]
    fn blocks_on_one_line_and_precedence() {
        let a = items(
            "on init declare $a := 1 + 2 * 3 end on function f() call g end function \
             on note if ($a = 1 and not $a > 2) message(\"x\" & $a) else exit end if end on",
        )
        .unwrap();
        assert_eq!(a.len(), 3);
        let Some(Item::Callback { body, .. }) = a.first() else {
            panic!("init")
        };
        let Some(StmtKind::Declare(d)) = body.first().map(|s| &s.kind) else {
            panic!("declare")
        };
        let Some(ExprKind::Binary(Bin::Add, _, rhs)) = d.init.first().map(|e| &e.kind) else {
            panic!("add")
        };
        assert!(matches!(rhs.kind, ExprKind::Binary(Bin::Mul, ..)));
        assert!(items("on init end on on note").is_err());
        assert!(items("on note if (1) end on").is_err());
        assert!(items("on note select ($x) message(1) end select end on").is_err());
    }
}
