//! Recursive-descent KSP parser. Each top-level block parses independently so a
//! syntax error disables only that callback or function, never the whole script.

use super::lexer::{Punct, Sym, Tok, Tokens, kw};
use anyhow::{Result, bail, ensure};

const MAX_DEPTH: usize = 256;

#[derive(Clone, Debug)]
pub enum Expr {
    Int(i32),
    Real(f64),
    Str(Sym),
    /// Bare identifier argument, e.g. a PGS key.
    Ident(Sym),
    Var(Sym, Option<Box<Expr>>),
    Call(Sym, Vec<Expr>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
    BitNot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
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

impl BinOp {
    fn binding_power(self) -> u8 {
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
const NOT_OPERAND_BP: u8 = 3;
const UNARY_BP: u8 = 10;

#[derive(Debug)]
pub struct Stmt {
    pub line: u32,
    pub kind: StmtKind,
}

#[derive(Debug)]
pub enum StmtKind {
    Declare(Box<Declare>),
    Assign(Expr, Expr),
    Command(Sym, Vec<Expr>),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    While(Expr, Vec<Stmt>),
    Select(Expr, Vec<Case>),
    Call(Sym),
}

#[derive(Debug)]
pub struct Case {
    pub line: u32,
    pub low: Expr,
    pub high: Option<Expr>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Default)]
pub struct Declare {
    pub constant: bool,
    pub polyphonic: bool,
    pub persistent: bool,
    pub ui: Option<Sym>,
    pub name: Sym,
    pub size: Option<Expr>,
    pub params: Vec<Expr>,
    pub init: Vec<Expr>,
}

#[derive(Debug)]
pub struct Block {
    pub line: u32,
    /// `on <name>` callback or `function <name>`.
    pub function: bool,
    pub name: Sym,
    /// `on ui_control($var)` argument.
    pub arg: Option<Sym>,
    pub body: Result<Vec<Stmt>, String>,
}

pub struct Parser<'a> {
    toks: &'a [Tok],
    lines: &'a [u32],
    reals: &'a [f64],
    pos: usize,
    end: usize,
}

/// Split into top-level blocks, parsing each body on its own.
pub fn parse(t: &Tokens) -> Result<Vec<Block>> {
    let mut p = Parser {
        toks: &t.toks,
        lines: &t.lines,
        reals: &t.reals,
        pos: 0,
        end: t.toks.len(),
    };
    let mut blocks = Vec::new();
    loop {
        p.skip_newlines();
        if p.peek() == Tok::Eof {
            return Ok(blocks);
        }
        let line = p.line();
        let function = match p.next() {
            Tok::Ident(kw::ON) => false,
            Tok::Ident(kw::FUNCTION) => true,
            _ => bail!("Unexpected top-level KSP statement at line {line}"),
        };
        let Tok::Ident(name) = p.next() else {
            bail!("Expected callback or function name at line {line}")
        };
        let arg = if !function && p.eat(Punct::LParen) {
            let Tok::Var(v) = p.next() else {
                bail!("Expected ui_control variable at line {line}")
            };
            p.need(Punct::RParen)?;
            Some(v)
        } else {
            None
        };
        // Find the terminator first so one bad body cannot swallow later blocks.
        let closer = if function { kw::FUNCTION } else { kw::ON };
        let start = p.pos;
        let mut end = start;
        loop {
            match t.toks[end] {
                Tok::Eof => bail!("Unclosed KSP callback/function starting at line {line}"),
                Tok::Ident(kw::END) if t.toks[end + 1] == Tok::Ident(closer) => break,
                Tok::Ident(kw::ON | kw::FUNCTION) if end > 0 && t.toks[end - 1] == Tok::Newline => {
                    bail!("Unclosed KSP callback/function starting at line {line}")
                }
                _ => end += 1,
            }
        }
        let mut body_parser = Parser {
            toks: p.toks,
            lines: p.lines,
            reals: p.reals,
            pos: start,
            end,
        };
        let body = body_parser.body().map_err(|e| format!("{e:#}"));
        blocks.push(Block {
            line,
            function,
            name,
            arg,
            body,
        });
        p.pos = end + 2;
    }
}

impl Parser<'_> {
    fn peek(&self) -> Tok {
        if self.pos < self.end {
            self.toks[self.pos]
        } else {
            Tok::Eof
        }
    }

    fn line(&self) -> u32 {
        self.lines[self.pos.min(self.lines.len() - 1)]
    }

    fn next(&mut self) -> Tok {
        let t = self.peek();
        if self.pos < self.end {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, p: Punct) -> bool {
        let hit = self.peek() == Tok::Punct(p);
        if hit {
            self.pos += 1;
        }
        hit
    }

    fn eat_kw(&mut self, k: Sym) -> bool {
        let hit = self.peek() == Tok::Ident(k);
        if hit {
            self.pos += 1;
        }
        hit
    }

    fn need(&mut self, p: Punct) -> Result<()> {
        ensure!(
            self.eat(p),
            "Expected {p:?}, found {:?} at line {}",
            self.peek(),
            self.line()
        );
        Ok(())
    }

    fn need_kw(&mut self, k: Sym, what: &str) -> Result<()> {
        ensure!(self.eat_kw(k), "Expected '{what}' at line {}", self.line());
        Ok(())
    }

    fn skip_newlines(&mut self) {
        while self.peek() == Tok::Newline {
            self.pos += 1;
        }
    }

    fn end_of_statement(&mut self) -> Result<()> {
        match self.peek() {
            Tok::Newline => {
                self.skip_newlines();
                Ok(())
            }
            Tok::Eof => Ok(()),
            t => bail!("Unexpected token {t:?} at line {}", self.line()),
        }
    }

    fn body(&mut self) -> Result<Vec<Stmt>> {
        let body = self.block(0)?;
        ensure!(
            self.peek() == Tok::Eof,
            "Unexpected block terminator at line {}",
            self.line()
        );
        Ok(body)
    }

    fn at_block_end(&self) -> bool {
        matches!(
            self.peek(),
            Tok::Eof | Tok::Ident(kw::END | kw::ELSE | kw::CASE)
        )
    }

    fn block(&mut self, depth: usize) -> Result<Vec<Stmt>> {
        ensure!(
            depth < MAX_DEPTH,
            "Statement nesting limit at line {}",
            self.line()
        );
        let mut out = Vec::new();
        self.skip_newlines();
        while !self.at_block_end() {
            out.push(self.statement(depth)?);
        }
        Ok(out)
    }

    fn close(&mut self, k: Sym, what: &str) -> Result<()> {
        self.need_kw(kw::END, what)?;
        self.need_kw(k, what)?;
        self.end_of_statement()
    }

    fn statement(&mut self, depth: usize) -> Result<Stmt> {
        let line = self.line();
        let kind = match self.next() {
            Tok::Ident(kw::IF) => {
                let cond = self.expr(0, 0)?;
                self.end_of_statement()?;
                let yes = self.block(depth + 1)?;
                let no = if self.eat_kw(kw::ELSE) {
                    self.end_of_statement()?;
                    self.block(depth + 1)?
                } else {
                    Vec::new()
                };
                self.close(kw::IF, "end if")?;
                return Ok(Stmt {
                    line,
                    kind: StmtKind::If(cond, yes, no),
                });
            }
            Tok::Ident(kw::WHILE) => {
                let cond = self.expr(0, 0)?;
                self.end_of_statement()?;
                let body = self.block(depth + 1)?;
                self.close(kw::WHILE, "end while")?;
                return Ok(Stmt {
                    line,
                    kind: StmtKind::While(cond, body),
                });
            }
            Tok::Ident(kw::SELECT) => {
                let value = self.expr(0, 0)?;
                self.end_of_statement()?;
                let mut cases = Vec::new();
                while self.eat_kw(kw::CASE) {
                    let line = self.line();
                    let low = self.expr(0, 0)?;
                    let high = if self.eat_kw(kw::TO) {
                        Some(self.expr(0, 0)?)
                    } else {
                        None
                    };
                    self.end_of_statement()?;
                    let body = self.block(depth + 1)?;
                    cases.push(Case {
                        line,
                        low,
                        high,
                        body,
                    });
                }
                self.close(kw::SELECT, "end select")?;
                return Ok(Stmt {
                    line,
                    kind: StmtKind::Select(value, cases),
                });
            }
            Tok::Ident(kw::DECLARE) => StmtKind::Declare(Box::new(self.declare()?)),
            Tok::Ident(kw::CALL) => {
                let Tok::Ident(name) = self.next() else {
                    bail!("Expected function name at line {line}")
                };
                StmtKind::Call(name)
            }
            Tok::Var(name) => {
                let index = self.index()?;
                self.need(Punct::Assign)?;
                StmtKind::Assign(Expr::Var(name, index), self.expr(0, 0)?)
            }
            Tok::Ident(name) => {
                let args = if self.eat(Punct::LParen) {
                    self.args(0)?
                } else {
                    Vec::new()
                };
                StmtKind::Command(name, args)
            }
            t => bail!("Unexpected token {t:?} at line {line}"),
        };
        self.end_of_statement()?;
        Ok(Stmt { line, kind })
    }

    fn declare(&mut self) -> Result<Declare> {
        let mut d = Declare::default();
        loop {
            match self.peek() {
                Tok::Ident(kw::CONST) => d.constant = true,
                Tok::Ident(kw::POLYPHONIC) => d.polyphonic = true,
                Tok::Ident(kw::PERS | kw::INSTPERS) => d.persistent = true,
                Tok::Ident(ui) => {
                    ensure!(
                        d.ui.is_none(),
                        "Unexpected declaration keyword at line {}",
                        self.line()
                    );
                    d.ui = Some(ui);
                }
                _ => break,
            }
            self.pos += 1;
        }
        let Tok::Var(name) = self.next() else {
            bail!("Expected variable name at line {}", self.line())
        };
        d.name = name;
        d.size = self.index()?.map(|e| *e);
        if self.eat(Punct::LParen) {
            d.params = self.args(0)?;
        }
        if self.eat(Punct::Assign) {
            if d.size.is_some() && self.eat(Punct::LParen) {
                d.init = self.args(0)?;
            } else {
                d.init.push(self.expr(0, 0)?);
            }
        }
        Ok(d)
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
            self.need(Punct::Comma)?;
        }
    }

    fn binop(&self) -> Option<BinOp> {
        Some(match self.peek() {
            Tok::Ident(kw::OR) => BinOp::Or,
            Tok::Ident(kw::XOR) => BinOp::Xor,
            Tok::Ident(kw::AND) => BinOp::And,
            Tok::Ident(kw::MOD) => BinOp::Mod,
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
        ensure!(
            depth < MAX_DEPTH,
            "Expression nesting limit at line {}",
            self.line()
        );
        let line = self.line();
        let mut lhs = match self.next() {
            Tok::Punct(Punct::LParen) => {
                let e = self.expr(0, depth + 1)?;
                self.need(Punct::RParen)?;
                e
            }
            Tok::Punct(Punct::Minus) => match self.expr(UNARY_BP, depth + 1)? {
                Expr::Int(n) => Expr::Int(n.wrapping_neg()),
                Expr::Real(n) => Expr::Real(-n),
                e => Expr::Unary(UnOp::Neg, Box::new(e)),
            },
            Tok::Punct(Punct::Plus) => self.expr(UNARY_BP, depth + 1)?,
            Tok::Punct(Punct::BitNot) => {
                Expr::Unary(UnOp::BitNot, Box::new(self.expr(UNARY_BP, depth + 1)?))
            }
            Tok::Ident(kw::NOT) => {
                Expr::Unary(UnOp::Not, Box::new(self.expr(NOT_OPERAND_BP, depth + 1)?))
            }
            Tok::Int(n) => Expr::Int(n),
            Tok::Real(i) => Expr::Real(self.reals[i as usize]),
            Tok::Str(s) => Expr::Str(s),
            Tok::Var(v) => Expr::Var(v, self.index()?),
            Tok::Ident(name) if self.peek() == Tok::Punct(Punct::LParen) => {
                self.pos += 1;
                Expr::Call(name, self.args(depth + 1)?)
            }
            Tok::Ident(name) => Expr::Ident(name),
            t => bail!("Unexpected token {t:?} in expression at line {line}"),
        };
        // Left-associative chains deepen the tree too; bound them like nesting.
        let mut depth = depth;
        while let Some(op) = self.binop() {
            let bp = op.binding_power();
            if bp < min {
                break;
            }
            depth += 1;
            ensure!(
                depth < MAX_DEPTH,
                "Expression nesting limit at line {}",
                self.line()
            );
            self.pos += 1;
            let rhs = self.expr(bp + 1, depth)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ksp::lexer::lex;

    #[test]
    fn blocks_fail_independently() {
        let t = lex(
            "on init\n$a := 1 + 2 * 3\nend on\non note\n$a := (\nend on\nfunction f\nend function",
        )
        .unwrap();
        let blocks = parse(&t).unwrap();
        assert_eq!(blocks.len(), 3);
        assert!(blocks[0].body.is_ok());
        assert!(blocks[1].body.is_err());
        let Ok(body) = &blocks[0].body else {
            unreachable!()
        };
        let StmtKind::Assign(_, Expr::Binary(BinOp::Add, _, rhs)) = &body[0].kind else {
            panic!()
        };
        assert!(matches!(**rhs, Expr::Binary(BinOp::Mul, ..)));
        assert!(parse(&lex("on init\n").unwrap()).is_err());
        assert!(parse(&lex("on init\nend on\non note\non release\nend on").unwrap()).is_err());
    }
}
