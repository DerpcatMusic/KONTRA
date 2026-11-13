//! The interpreter loop: runs one coroutine until it ends or waits.

use std::hash::{Hash, Hasher};

use crate::{
    Action, Cmp, Coroutine, Faults, HostApi, HostCall, HostValue, LOOP_LIMIT, LOOP_PROBE, Op,
    Program, Returns, Src, Step, Text, pure,
};

pub(crate) fn execute(
    program: &Program,
    cells: &mut [i64],
    texts: &mut [Text],
    poly: &mut [i64],
    c: &mut Coroutine,
    faults: &mut Faults,
    host: &mut dyn HostApi,
) -> Step {
    // The position, the stack depth and the top value live in registers
    // while it runs. Value `k` lives in slot `k + 1` (slot 0 is spare, so a
    // push never branches on an empty stack); the top's slot is stale until
    // `flush!` writes it back.
    let (mut pc, mut sp) = (c.pc, c.sp);
    let stack = &mut c.stack[..];
    let room = stack.len().saturating_sub(1);
    let mut tos = stack.get(sp).copied().unwrap_or(0);
    macro_rules! flush {
        () => {
            if let Some(slot) = stack.get_mut(sp) {
                *slot = tos;
            }
        };
    }
    macro_rules! leave {
        ($step:expr) => {{
            flush!();
            (c.pc, c.sp) = (pc, sp);
            return $step;
        }};
    }
    macro_rules! pop {
        () => {{
            if sp == 0 {
                faults.bad_code += 1;
                leave!(Step::Done);
            }
            let v = tos;
            sp -= 1;
            tos = stack.get(sp).copied().unwrap_or(0);
            v
        }};
    }
    macro_rules! push {
        ($v:expr) => {{
            let v: i64 = $v;
            if sp >= room {
                faults.stack_overflow += 1;
                leave!(Step::Done);
            }
            flush!();
            tos = v;
            sp += 1;
        }};
    }
    // `loop_check(exit)`: counts a turn, leaving at the limit or when stuck.
    macro_rules! loop_check {
        ($exit:expr) => {{
            let exit: u32 = $exit;
            c.loop_count += 1;
            c.turns = c.turns.wrapping_add(1);
            let mut stuck = false;
            // The hash reads every cell: probe once the loops turned as
            // often as there are cells, so it never costs more than they did.
            let cost = u32::try_from(cells.len() + poly.len()).unwrap_or(u32::MAX);
            if c.loop_count >= LOOP_PROBE.max(cost) && c.loop_count.is_power_of_two() {
                // A host call moved state outside the hash: not stuck, no hash.
                c.loop_hash = if std::mem::take(&mut c.hosted) {
                    0
                } else {
                    flush!();
                    let hash = state_hash(
                        cells,
                        texts,
                        poly,
                        stack.get(1..=sp).unwrap_or_default(),
                        &c.texts,
                    );
                    stuck = hash == c.loop_hash;
                    hash
                };
            }
            // The limit holds for the rest of the run (every loop exits
            // until the next wait); a stuck loop frees only itself.
            if c.loop_count >= LOOP_LIMIT {
                faults.loop_limits += 1;
                pc = exit;
            } else if stuck {
                faults.loop_limits += 1;
                c.loop_count = 0;
                pc = exit;
            }
            // Out of fuel: the run goes on from here at the next block.
            if c.fuel == 1 {
                c.fuel = 0;
                c.waits = c.waits.wrapping_add(1);
                leave!(Step::Yield);
            }
            c.fuel = c.fuel.saturating_sub(1);
        }};
    }
    macro_rules! pop_text {
        () => {
            match c.texts.pop() {
                Some(v) => v,
                None => {
                    faults.bad_code += 1;
                    leave!(Step::Done);
                }
            }
        };
    }
    macro_rules! push_text {
        ($v:expr) => {{
            if c.texts.len() == c.texts.capacity() {
                faults.stack_overflow += 1;
                leave!(Step::Done);
            }
            c.texts.push($v);
        }};
    }
    // A fused op's operand: the slot value its plain op would have pushed.
    macro_rules! slot {
        ($s:expr) => {
            match $s {
                Src::Cell(n) => cells.get(n as usize).copied().unwrap_or(0),
                Src::Poly(n) => poly.get(usize::from(n)).copied().unwrap_or(0),
                Src::Int(v) => i64::from(v),
            }
        };
    }
    // ... read as an integer, as the plain op that pops it does.
    macro_rules! src {
        ($s:expr) => {
            slot!($s) as i32
        };
    }
    let int = |v: i64| v as i32;
    let real = |v: i64| f64::from_bits(v as u64);
    let from_real = |v: f64| v.to_bits() as i64;

    loop {
        let Some(op) = program.code.get(pc as usize) else {
            faults.bad_code += 1;
            leave!(Step::Done);
        };
        pc += 1;
        match *op {
            Op::Int(v) => push!(i64::from(v)),
            Op::Real(v) => push!(from_real(v)),
            Op::Text(n) => {
                let t = program
                    .texts
                    .get(n as usize)
                    .copied()
                    .unwrap_or(Text::EMPTY);
                push_text!(t);
            }
            Op::Pop => {
                pop!();
            }
            Op::Dup => {
                let v = pop!();
                push!(v);
                push!(v);
            }
            Op::Load(n) => push!(cells.get(n as usize).copied().unwrap_or(0)),
            Op::Store(n) => {
                let v = pop!();
                if let Some(cell) = cells.get_mut(n as usize) {
                    *cell = v;
                }
            }
            Op::LoadAt { base, len } => {
                let i = int(pop!());
                match element(base, len, i) {
                    Some(at) => push!(cells.get(at).copied().unwrap_or(0)),
                    None => {
                        faults.out_of_range += 1;
                        faults.first_out_of_range.get_or_insert((base, false, i));
                        push!(0);
                    }
                }
            }
            Op::StoreAt { base, len } => {
                let v = pop!();
                let i = int(pop!());
                match element(base, len, i).and_then(|at| cells.get_mut(at)) {
                    Some(cell) => *cell = v,
                    None => {
                        faults.out_of_range += 1;
                        faults.first_out_of_range.get_or_insert((base, false, i));
                    }
                }
            }
            Op::LoadPoly(n) => push!(poly.get(usize::from(n)).copied().unwrap_or(0)),
            Op::StorePoly(n) => {
                let v = pop!();
                if let Some(cell) = poly.get_mut(usize::from(n)) {
                    *cell = v;
                }
            }
            Op::TLoad(n) => push_text!(texts.get(n as usize).copied().unwrap_or(Text::EMPTY)),
            Op::TStore(n) => {
                let t = pop_text!();
                if let Some(cell) = texts.get_mut(n as usize) {
                    *cell = t;
                }
            }
            Op::TLoadAt { base, len } => {
                let i = int(pop!());
                match element(base, len, i) {
                    Some(at) => push_text!(texts.get(at).copied().unwrap_or(Text::EMPTY)),
                    None => {
                        faults.out_of_range += 1;
                        faults.first_out_of_range.get_or_insert((base, true, i));
                        push_text!(Text::EMPTY);
                    }
                }
            }
            Op::TStoreAt { base, len } => {
                let t = pop_text!();
                let i = int(pop!());
                match element(base, len, i).and_then(|at| texts.get_mut(at)) {
                    Some(cell) => *cell = t,
                    None => {
                        faults.out_of_range += 1;
                        faults.first_out_of_range.get_or_insert((base, true, i));
                    }
                }
            }
            Op::TFromInt => {
                let mut t = Text::EMPTY;
                t.push_fmt(format_args!("{}", int(pop!())));
                push_text!(t);
            }
            Op::TFromReal => {
                let mut t = Text::EMPTY;
                t.push_fmt(format_args!("{:.6}", real(pop!())));
                push_text!(t);
            }
            Op::TConcat => {
                let b = pop_text!();
                let mut a = pop_text!();
                if !a.push(b.as_str()) {
                    faults.texts_cut += 1;
                }
                push_text!(a);
            }
            Op::TEq => {
                let b = pop_text!();
                let a = pop_text!();
                push!(i64::from(a == b));
            }
            Op::IAdd
            | Op::ISub
            | Op::IMul
            | Op::IDiv
            | Op::IMod
            | Op::IAnd
            | Op::IOr
            | Op::IXor
            | Op::IShl
            | Op::IShr => {
                let b = int(pop!());
                let a = int(pop!());
                let v = match op {
                    Op::IAdd => a.wrapping_add(b),
                    Op::ISub => a.wrapping_sub(b),
                    Op::IMul => a.wrapping_mul(b),
                    Op::IDiv | Op::IMod if b == 0 => {
                        faults.divide_by_zero += 1;
                        faults.first_divide_by_zero.get_or_insert(pc - 1);
                        0
                    }
                    Op::IDiv => a.wrapping_div(b),
                    Op::IMod => a.wrapping_rem(b),
                    Op::IAnd => a & b,
                    Op::IOr => a | b,
                    Op::IXor => a ^ b,
                    Op::IShl => a.wrapping_shl(b as u32),
                    _ => a.wrapping_shr(b as u32),
                };
                push!(i64::from(v));
            }
            Op::INeg => {
                let a = int(pop!());
                push!(i64::from(a.wrapping_neg()));
            }
            Op::IBitNot => {
                let a = int(pop!());
                push!(i64::from(!a));
            }
            Op::FAdd | Op::FSub | Op::FMul | Op::FDiv => {
                let b = real(pop!());
                let a = real(pop!());
                let v = match op {
                    Op::FAdd => a + b,
                    Op::FSub => a - b,
                    Op::FMul => a * b,
                    _ => a / b,
                };
                push!(from_real(v));
            }
            Op::FNeg => {
                let a = real(pop!());
                push!(from_real(-a));
            }
            Op::ICmp(cmp) => {
                let b = int(pop!());
                let a = int(pop!());
                push!(i64::from(compare(cmp, a.cmp(&b))));
            }
            Op::FCmp(cmp) => {
                let b = real(pop!());
                let a = real(pop!());
                let ordering = a.partial_cmp(&b);
                push!(i64::from(
                    ordering.is_some_and(|o| compare(cmp, o))
                        || (cmp == Cmp::Ne && ordering.is_none())
                ));
            }
            Op::Not => {
                let a = pop!();
                push!(i64::from(a == 0));
            }
            Op::IntToReal => {
                let a = int(pop!());
                push!(from_real(f64::from(a)));
            }
            Op::RealToInt(nan) => {
                let a = real(pop!());
                push!(i64::from(if a.is_nan() { nan } else { a as i32 }));
            }
            Op::Jump(to) => pc = to,
            Op::JumpIfZero(to) => {
                if pop!() == 0 {
                    pc = to;
                }
            }
            Op::LoopCheck { exit } => loop_check!(exit),
            Op::LoopExit => c.loop_hash = 0,
            Op::Call(to) => {
                if c.frames.len() == c.frames.capacity() {
                    faults.stack_overflow += 1;
                    leave!(Step::Done);
                }
                c.frames.push(pc);
                pc = to;
            }
            Op::Ret => match c.frames.pop() {
                Some(back) => pc = back,
                None => leave!(Step::Done),
            },
            Op::Exit => leave!(Step::Done),
            Op::BinTo { bin, a, b, to } => {
                let v = i64::from(bin.apply(src!(a), src!(b)));
                let cell = match to {
                    Src::Cell(n) => cells.get_mut(n as usize),
                    Src::Poly(n) => poly.get_mut(usize::from(n)),
                    Src::Int(_) => None,
                };
                if let Some(cell) = cell {
                    *cell = v;
                }
                pc += 3;
            }
            Op::Branch { cmp, a, b, to } => {
                pc = if compare(cmp, src!(a).cmp(&src!(b))) {
                    pc + 3
                } else {
                    to
                };
            }
            Op::Bin { bin, a, b } => {
                push!(i64::from(bin.apply(src!(a), src!(b))));
                pc += 2;
            }
            Op::BranchTop { cmp, b, to } => {
                let a = int(pop!());
                pc = if compare(cmp, a.cmp(&src!(b))) {
                    pc + 2
                } else {
                    to
                };
            }
            Op::BinTop { bin, b } => {
                let a = int(pop!());
                push!(i64::from(bin.apply(a, src!(b))));
                pc += 1;
            }
            Op::LoadAtCell { base, len, i } => {
                let i = src!(Src::Cell(i));
                match element(base, len, i) {
                    Some(at) => push!(cells.get(at).copied().unwrap_or(0)),
                    None => {
                        faults.out_of_range += 1;
                        faults.first_out_of_range.get_or_insert((base, false, i));
                        push!(0);
                    }
                }
                pc += 1;
            }
            Op::LoadAtBin {
                base,
                len,
                bin,
                a,
                b,
            } => {
                let i = bin.apply(src!(a), src!(b));
                match element(base, len, i) {
                    Some(at) => push!(cells.get(at).copied().unwrap_or(0)),
                    None => {
                        faults.out_of_range += 1;
                        faults.first_out_of_range.get_or_insert((base, false, i));
                        push!(0);
                    }
                }
                pc += 3;
            }
            Op::LoadAtTop { base, len, bin, b } => {
                let i = bin.apply(int(pop!()), src!(b));
                match element(base, len, i) {
                    Some(at) => push!(cells.get(at).copied().unwrap_or(0)),
                    None => {
                        faults.out_of_range += 1;
                        faults.first_out_of_range.get_or_insert((base, false, i));
                        push!(0);
                    }
                }
                pc += 2;
            }
            Op::CmpTop { cmp, b } => {
                let a = int(pop!());
                push!(i64::from(compare(cmp, a.cmp(&src!(b)))));
                pc += 1;
            }
            Op::CmpAnd { cmp } => {
                let b = int(pop!());
                let a = int(pop!());
                let p = int(pop!());
                push!(i64::from(p & i32::from(compare(cmp, a.cmp(&b)))));
                pc += 1;
            }
            Op::CmpTopAnd { cmp, b } => {
                let a = int(pop!());
                let p = int(pop!());
                push!(i64::from(p & i32::from(compare(cmp, a.cmp(&src!(b))))));
                pc += 2;
            }
            Op::AndJump { to } => {
                let b = int(pop!());
                let a = int(pop!());
                pc = if a & b == 0 { to } else { pc + 1 };
            }
            Op::AndTopJump { b, to } => {
                let a = int(pop!());
                pc = if a & src!(b) == 0 { to } else { pc + 2 };
            }
            Op::CmpJump { cmp, to } => {
                let b = int(pop!());
                let a = int(pop!());
                pc = if compare(cmp, a.cmp(&b)) { pc + 1 } else { to };
            }
            Op::BinStore { bin, to } => {
                let b = int(pop!());
                let a = int(pop!());
                let v = i64::from(bin.apply(a, b));
                let cell = match to {
                    Src::Cell(n) => cells.get_mut(n as usize),
                    Src::Poly(n) => poly.get_mut(usize::from(n)),
                    Src::Int(_) => None,
                };
                if let Some(cell) = cell {
                    *cell = v;
                }
                pc += 1;
            }
            Op::BinBin { inner, outer } => {
                let z = int(pop!());
                let y = int(pop!());
                let x = int(pop!());
                push!(i64::from(outer.apply(x, inner.apply(y, z))));
                pc += 1;
            }
            Op::LoopJump { top, exit } => {
                pc = top + 1;
                loop_check!(exit);
            }
            Op::NextTurn { x, by, top, check } => {
                let v = i64::from(src!(x).wrapping_add(by));
                let cell = match x {
                    Src::Cell(n) => cells.get_mut(n as usize),
                    Src::Poly(n) => poly.get_mut(usize::from(n)),
                    Src::Int(_) => None,
                };
                if let Some(cell) = cell {
                    *cell = v;
                }
                match check {
                    Some(exit) => {
                        pc = top + 1;
                        loop_check!(exit);
                    }
                    None => pc = top,
                }
            }
            Op::StoreAtSrc { base, len, v } => {
                let v = slot!(v);
                let i = int(pop!());
                match element(base, len, i).and_then(|at| cells.get_mut(at)) {
                    Some(cell) => *cell = v,
                    None => {
                        faults.out_of_range += 1;
                        faults.first_out_of_range.get_or_insert((base, false, i));
                    }
                }
                pc += 1;
            }
            Op::Wait => {
                let micros = pop!();
                c.loop_count = 0;
                if !c.no_wait {
                    c.waits = c.waits.wrapping_add(1);
                    leave!(Step::Wait(u64::try_from(micros).unwrap_or(0)));
                }
            }
            Op::Host {
                action,
                ints,
                texts: text_args,
                returns,
            } => {
                let (ints, text_args) = (usize::from(ints), usize::from(text_args));
                if sp < ints || c.texts.len() < text_args {
                    faults.bad_code += 1;
                    leave!(Step::Done);
                }
                let mut text_out = Text::EMPTY;
                let int_from = sp - ints;
                let text_from = c.texts.len() - text_args;
                flush!();
                let args = stack.get(int_from + 1..=sp).unwrap_or_default();
                let value = match pure::eval(action, args, cells) {
                    Some(value) => value,
                    None => {
                        // A message changes nothing the loop can read.
                        c.hosted |= action != Action::Message;
                        if let Some(v) = host.query(action, args) {
                            HostValue::Int(v)
                        } else {
                            host.call(HostCall {
                                action,
                                context: c.context,
                                ints: args,
                                texts: c.texts.get(text_from..).unwrap_or_default(),
                                cells,
                                text_cells: texts,
                                text_out: &mut text_out,
                            })
                        }
                    }
                };
                if let HostValue::Wait(micros) = value
                    && !c.no_wait
                {
                    pc -= 1;
                    c.loop_count = 0;
                    c.waits = c.waits.wrapping_add(1);
                    leave!(Step::Wait(micros));
                }
                sp = int_from;
                tos = stack.get(sp).copied().unwrap_or(0);
                c.texts.truncate(text_from);
                match (returns, value) {
                    (Returns::Nothing, _) => {}
                    (Returns::Int, HostValue::Int(v)) => push!(i64::from(v)),
                    (Returns::Real, HostValue::Real(v)) => push!(from_real(v)),
                    (Returns::Text, HostValue::Text) => push_text!(text_out),
                    (Returns::Int, _) => push!(0),
                    (Returns::Real, _) => push!(from_real(0.0)),
                    (Returns::Text, _) => push_text!(Text::EMPTY),
                }
            }
        }
    }
}

fn element(base: u32, len: u32, index: i32) -> Option<usize> {
    let i = u32::try_from(index).ok().filter(|&i| i < len)?;
    Some(base as usize + i as usize)
}

fn compare(cmp: Cmp, o: std::cmp::Ordering) -> bool {
    use std::cmp::Ordering::{Equal, Greater, Less};
    match cmp {
        Cmp::Eq => o == Equal,
        Cmp::Ne => o != Equal,
        Cmp::Lt => o == Less,
        Cmp::Le => o != Greater,
        Cmp::Gt => o == Greater,
        Cmp::Ge => o != Less,
    }
}

/// State a loop iteration can change: if equal at two checkpoints, the loop
/// can never exit and the limit applies now instead of after ten million turns.
fn state_hash(
    cells: &[i64],
    texts: &[Text],
    poly: &[i64],
    stack: &[i64],
    text_stack: &[Text],
) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    cells.hash(&mut h);
    poly.hash(&mut h);
    stack.hash(&mut h);
    for t in texts.iter().chain(text_stack) {
        t.as_str().hash(&mut h);
    }
    h.finish()
}
