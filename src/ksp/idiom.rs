//! Native execution of the two loop shapes that dominate real scripts: an
//! element-wise array copy (`%a[$i] := %a[$i + 1]`, list removal) and a
//! linear scan (`if (%a[$i] # 0)` over a large array). The compiler finds them
//! in the bytecode and replaces the loop test with [`Op::Loop`]. At run time
//! the op does the iterations in plain Rust while every index is in bounds
//! and the fuel lasts, then behaves exactly like the loop test it replaced,
//! so every other case (and the rest of the budget) runs interpreted.

use super::builtins::SysVar;
use super::compile::{Cmp, Op, Var, VarId};

/// A `while ($i < n)` / `while ($i <= n)` loop ending in `inc($i)`.
#[derive(Clone, Copy, Debug)]
pub struct Loop {
    counter: u32,
    cmp: Cmp,
    bound: i32,
    /// First op after the loop.
    exit: u32,
    /// Interpreted ops one iteration costs, charged as fuel.
    cost: u64,
    body: Body,
}

#[derive(Clone, Copy, Debug)]
enum Body {
    /// `dst[$i + d] := src[$i + s]`.
    Copy {
        dst: VarId,
        d: i32,
        src: VarId,
        s: i32,
    },
    /// `if (array[$i] <cmp> value ...) ... end if`: iterations failing the
    /// test (or the first term of an `and`) only advance the counter. `then`
    /// is the op after the test.
    Scan {
        array: VarId,
        cmp: Cmp,
        value: Operand,
        then: u32,
    },
}

/// The right side of a scan test: fixed while iterations are skipped, since
/// no script code runs in between.
#[derive(Clone, Copy, Debug)]
pub enum Operand {
    Imm(i32),
    Int(u32),
    Sys(SysVar),
}

/// The loop whose test starts at `code[at]`, if it has a native shape.
pub fn find(code: &[Op], at: usize) -> Option<Loop> {
    let &[
        Op::LdI(counter),
        Op::PushI(bound),
        test,
        Op::JumpIfZero(exit),
        ..,
    ] = code.get(at..)?
    else {
        return None;
    };
    let cmp = Cmp::of(test).filter(|c| matches!(c, Cmp::Lt | Cmp::Le))?;
    let end = exit as usize;
    let tail = [
        Op::LdI(counter),
        Op::PushI(1),
        Op::IAdd,
        Op::StI(counter),
        Op::Jump(at as u32),
    ];
    let body = code.get(at + 4..end.checked_sub(tail.len())?)?;
    if code[end - tail.len()..end] != tail {
        return None;
    }
    // An index is `$i` or `$i + k`.
    let index = |ops: &[Op]| match *ops {
        [Op::LdI(v), Op::PushI(k), Op::IAdd, ..] if v == counter => Some((k, 3)),
        [Op::LdI(v), ..] if v == counter => Some((0, 1)),
        _ => None,
    };
    let body = if let Some((d, n)) = index(body)
        && let Some((s, m)) = index(&body[n..])
        && let [Op::LdIA(src), Op::StIA(dst)] = body[n + m..]
    {
        Body::Copy { dst, d, src, s }
    } else if let [
        Op::LdI(v),
        Op::LdIA(array),
        operand,
        test,
        Op::JumpIfZero(miss),
        ..,
    ] = *body
        && v == counter
        && let Some(cmp) = Cmp::of(test)
        && let Some(value) = match operand {
            Op::PushI(n) => Some(Operand::Imm(n)),
            Op::LdI(v) if v != counter => Some(Operand::Int(v)),
            Op::Sys(v) => Some(Operand::Sys(v)),
            _ => None,
        }
    {
        // A miss goes to the increment, directly or through an `and`'s false.
        let skip = (end - tail.len()) as u32;
        let miss = miss as usize;
        if miss != skip as usize
            && code.get(miss..miss + 2) != Some(&[Op::PushI(0), Op::JumpIfZero(skip)])
        {
            return None;
        }
        Body::Scan {
            array,
            cmp,
            value,
            then: (at + 9) as u32,
        }
    } else {
        return None;
    };
    let cost = match body {
        Body::Copy { .. } => end - at,
        Body::Scan { .. } => 4 + 5 + tail.len(),
    } as u64;
    Some(Loop {
        counter,
        cmp,
        bound,
        exit,
        cost,
        body,
    })
}

impl Loop {
    /// The scan operand, if any, for the caller to resolve.
    pub fn operand(&self) -> Option<Operand> {
        match self.body {
            Body::Scan { value, .. } => Some(value),
            Body::Copy { .. } => None,
        }
    }

    /// Run iterations natively from `code[at]` and return where the
    /// interpreter continues. `value` is the resolved [`Loop::operand`].
    pub fn run(
        &self,
        at: usize,
        vars: &[Var],
        ints: &mut [i32],
        fuel: &mut u64,
        value: i32,
    ) -> usize {
        let i = ints[self.counter as usize];
        let end = i64::from(self.bound) + i64::from(self.cmp == Cmp::Le);
        let left = (end - i64::from(i)).max(0) as u64;
        let n = left.min(*fuel / self.cost) as usize;
        // Absolute slot of `var[i + offset]` if all `n` elements from it exist.
        let slots = |var: VarId, offset: i32| {
            let var = &vars[var as usize];
            let first = usize::try_from(i64::from(i) + i64::from(offset)).ok()?;
            (first + n <= var.len.unwrap_or(1) as usize).then_some(var.slot as usize + first)
        };
        match self.body {
            Body::Copy { dst, d, src, s } => {
                // Front to back, like the loop: a memmove unless it would
                // read elements this loop already overwrote.
                if let (Some(to), Some(from)) = (slots(dst, d), slots(src, s))
                    && (to <= from || from + n <= to)
                {
                    ints.copy_within(from..from + n, to);
                    self.advance(ints, n, fuel);
                }
            }
            Body::Scan {
                array, cmp, then, ..
            } => {
                if let Some(from) = slots(array, 0) {
                    let hit = ints[from..from + n]
                        .iter()
                        .position(|&x| cmp.test(x, value));
                    self.advance(ints, hit.unwrap_or(n), fuel);
                    if hit.is_some() {
                        return then as usize;
                    }
                }
            }
        }
        // The loop test itself.
        if self.cmp.test(ints[self.counter as usize], self.bound) {
            at + 4
        } else {
            self.exit as usize
        }
    }

    fn advance(&self, ints: &mut [i32], n: usize, fuel: &mut u64) {
        let counter = &mut ints[self.counter as usize];
        *counter = counter.wrapping_add(n as i32);
        *fuel -= n as u64 * self.cost;
    }
}
