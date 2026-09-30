//! Builtin implementations. Arguments arrive on the typed stacks in call order;
//! each builtin pops its own. Nothing here allocates on the event paths.

use super::builtins::{self as b, Builtin, event_par as par};
use super::compile::{Callback, Ty, VarId};
use super::engine::{EnginePar, Fade, GroupMask, VoicePar};
use super::runtime::{read_value, write_value};
use super::ui::{MenuItem, Prop};
use super::vm::{Exec, Fault, Kind, Machine, Step};
use super::{KeyState, Value};
use std::fmt::Write as _;

fn ints<const N: usize>(m: &mut Machine) -> [i32; N] {
    let mut a = [0; N];
    for x in a.iter_mut().rev() {
        *x = m.stk.int();
    }
    a
}

fn push_int(m: &mut Machine, v: i32) -> Exec<Step> {
    m.stk.ints.push(v);
    Ok(Step::Next)
}

fn push_real(m: &mut Machine, v: f64) -> Exec<Step> {
    if !v.is_finite() {
        return Err(Fault("Nonfinite real result"));
    }
    m.stk.reals.push(v);
    Ok(Step::Next)
}

fn push_fmt(m: &mut Machine, args: std::fmt::Arguments) -> Exec<Step> {
    let _ = m.stk.strs.push().write_fmt(args);
    Ok(Step::Next)
}

fn midi_note(n: i32) -> Exec<u8> {
    u8::try_from(n)
        .ok()
        .filter(|n| *n < 128)
        .ok_or(Fault("MIDI note must be 0..127"))
}

fn key_name_ok(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key.starts_with(|c: char| c.is_ascii_alphabetic())
        && key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

/// Control index for a UI variable argument.
fn control_of(m: &Machine, v: VarId) -> Exec<usize> {
    m.slot
        .ui
        .control_of(v)
        .ok_or(Fault("Command requires a declared UI control"))
}

fn control(m: &Machine, id: i32) -> Exec<usize> {
    m.slot.ui.control(id).ok_or(NO_CONTROL)
}

/// Expand an event ID or `by_marks` value into `env.targets`.
fn targets(m: &mut Machine, id: i32) -> usize {
    let env = &mut *m.env;
    env.events.targets(id, &mut env.targets);
    env.targets.len()
}

fn set_voice_par(m: &mut Machine, id: i32, which: VoicePar, value: i32, relative: bool) {
    for k in 0..targets(m, id) {
        let id = m.env.targets[k];
        let Some(e) = m.env.events.get_mut(id) else {
            continue;
        };
        let field = match which {
            VoicePar::VolumeMdb => &mut e.volume,
            VoicePar::TuneMc => &mut e.tune,
            VoicePar::Pan => &mut e.pan,
        };
        *field = if relative {
            field.saturating_add(value)
        } else {
            value
        };
        if which == VoicePar::Pan {
            *field = (*field).clamp(-1000, 1000);
        }
        let v = *field;
        if let Some(voice) = e.voice {
            m.engine.set_par(m.env.offset, voice, which, v);
        }
    }
}

fn engine_par(p: [i32; 4]) -> EnginePar {
    EnginePar {
        id: p[0],
        group: p[1],
        slot: p[2],
        generic: p[3],
    }
}

fn pgs_int_key(m: &mut Machine, key: u32) -> Option<usize> {
    let cached = m.slot.pgs_keys[key as usize];
    if cached != u32::MAX {
        return Some(cached as usize);
    }
    let name = &m.prog.strings[key as usize];
    let i = m
        .env
        .host
        .pgs_ints
        .iter()
        .position(|(k, _)| **k == **name)?;
    m.slot.pgs_keys[key as usize] = i as u32;
    Some(i)
}

fn pgs_str_key(m: &Machine, key: u32) -> Option<usize> {
    let name = &m.prog.strings[key as usize];
    m.env.host.pgs_strs.iter().position(|(k, _)| **k == **name)
}

fn async_done(m: &mut Machine, status: i32) -> i32 {
    let id = m.env.next_async();
    if m.env.async_done.len() < m.env.async_done.capacity() {
        m.env.async_done.push((m.slot.index, id, status));
    }
    id
}

/// Recoverable failures: recorded as diagnostics, the call yields its default value.
pub const NO_CONTROL: Fault = Fault("ID does not refer to a UI control");
pub const NO_PGS_KEY: Fault = Fault("Unknown PGS key");

pub fn call(m: &mut Machine, f: Builtin, argc: u8) -> Exec<Step> {
    use Builtin::*;
    let slot = m.slot.index;
    match f {
        // ---- Arithmetic --------------------------------------------------------------
        Abs => {
            let [a] = ints(m);
            push_int(m, a.wrapping_abs())
        }
        AbsReal => {
            let a = m.stk.real();
            push_real(m, a.abs())
        }
        Min | Max => {
            let [a, c] = ints(m);
            push_int(m, if f == Min { a.min(c) } else { a.max(c) })
        }
        MinReal | MaxReal => {
            let c = m.stk.real();
            let a = m.stk.real();
            push_real(m, if f == MinReal { a.min(c) } else { a.max(c) })
        }
        InRange => {
            let [x, lo, hi] = ints(m);
            push_int(m, (lo..=hi).contains(&x) as i32)
        }
        InRangeReal => {
            let hi = m.stk.real();
            let lo = m.stk.real();
            let x = m.stk.real();
            push_int(m, (lo..=hi).contains(&x) as i32)
        }
        ShLeft | ShRight => {
            let [a, n] = ints(m);
            if !(0..32).contains(&n) {
                return Err(Fault("Bit shift must be 0..31"));
            }
            push_int(
                m,
                if f == ShLeft {
                    a.wrapping_shl(n as u32)
                } else {
                    a >> n
                },
            )
        }
        Random => {
            let [lo, hi] = ints(m);
            let r = m.env.random(lo, hi);
            push_int(m, r)
        }
        IntToReal | Real => {
            let [a] = ints(m);
            push_real(m, f64::from(a))
        }
        RealToInt | Int => {
            let x = m.stk.real().trunc();
            if !(f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&x) {
                return Err(Fault("Real to integer overflow"));
            }
            push_int(m, x as i32)
        }
        Round | Floor | Ceil | Sqrt | Exp | Log | Log2 | Log10 | Sin | Cos | Tan | Asin | Acos
        | Atan => {
            let x = m.stk.real();
            let y = match f {
                Round => x.round(),
                Floor => x.floor(),
                Ceil => x.ceil(),
                Sqrt => x.sqrt(),
                Exp => x.exp(),
                Log => x.ln(),
                Log2 => x.log2(),
                Log10 => x.log10(),
                Sin => x.sin(),
                Cos => x.cos(),
                Tan => x.tan(),
                Asin => x.asin(),
                Acos => x.acos(),
                _ => x.atan(),
            };
            push_real(m, y)
        }
        Pow => {
            let e = m.stk.real();
            let x = m.stk.real();
            push_real(m, x.powf(e))
        }
        Msb | Lsb => {
            let [a] = ints(m);
            push_int(m, if f == Msb { (a >> 7) & 127 } else { a & 127 })
        }
        MsToTicks => {
            let [us] = ints(m);
            let q = i64::from(m.env.quarter_us().max(1));
            push_int(m, (i64::from(us) * 960 / q) as i32)
        }
        TicksToMs => {
            let [t] = ints(m);
            let q = i64::from(m.env.quarter_us());
            push_int(m, (i64::from(t) * q / 960) as i32)
        }
        // ---- Arrays ------------------------------------------------------------------
        NumElements => {
            let v = m.stk.var();
            push_int(m, m.prog.vars[v as usize].len.unwrap_or(1) as i32)
        }
        Search => {
            let v = m.stk.var();
            let var = &m.prog.vars[v as usize];
            let range = var.slot as usize..(var.slot + var.len.unwrap_or(1)) as usize;
            let found = match var.ty {
                Ty::Int => {
                    let x = m.stk.int();
                    m.slot.mem.ints[range].iter().position(|&y| y == x)
                }
                Ty::Real => {
                    let x = m.stk.real();
                    m.slot.mem.reals[range].iter().position(|&y| y == x)
                }
                Ty::Str => None,
            };
            push_int(m, found.map_or(-1, |i| i as i32))
        }
        Sort => {
            let (lo, hi) = if argc == 4 {
                let [lo, hi] = ints(m);
                (Some(lo), Some(hi))
            } else {
                (None, None)
            };
            let [descending] = ints(m);
            let v = m.stk.var();
            let var = &m.prog.vars[v as usize];
            let len = var.len.unwrap_or(1) as i32;
            let lo = lo.unwrap_or(0).clamp(0, len);
            let hi = hi.map_or(len, |h| (h + 1).clamp(lo, len));
            let range = (var.slot as i32 + lo) as usize..(var.slot as i32 + hi) as usize;
            let mem = &mut m.slot.mem;
            match var.ty {
                Ty::Int => sort(&mut mem.ints[range], descending != 0, |a, b| a.cmp(b)),
                Ty::Real => sort(&mut mem.reals[range], descending != 0, f64::total_cmp),
                Ty::Str => sort(&mut mem.strs[range], descending != 0, |a, b| a.cmp(b)),
            }
            Ok(Step::Next)
        }
        ArrayEqual => {
            let w = m.stk.var();
            let v = m.stk.var();
            let (a, c) = (&m.prog.vars[v as usize], &m.prog.vars[w as usize]);
            let equal = a.ty == c.ty && a.len == c.len && {
                let (x, y, n) = (
                    a.slot as usize,
                    c.slot as usize,
                    a.len.unwrap_or(1) as usize,
                );
                let mem = &m.slot.mem;
                match a.ty {
                    Ty::Int => mem.ints[x..x + n] == mem.ints[y..y + n],
                    Ty::Real => mem.reals[x..x + n] == mem.reals[y..y + n],
                    Ty::Str => mem.strs[x..x + n] == mem.strs[y..y + n],
                }
            };
            push_int(m, equal as i32)
        }
        LoadArray | LoadArrayStr | SaveArray | SaveArrayStr => {
            if matches!(f, LoadArray | SaveArray) {
                m.stk.int();
            } else {
                m.stk.strs.pop();
            }
            let v = m.stk.var();
            let var = &m.prog.vars[v as usize];
            let status = if matches!(f, SaveArray | SaveArrayStr) {
                let value = read_value(&m.slot.mem, var);
                m.env.saved_arrays.insert((slot, v), value);
                1
            } else if let Some(value) = m.env.saved_arrays.get(&(slot, v)) {
                write_value(&mut m.slot.mem, var, value);
                1
            } else {
                m.env.note("load_array: no saved data in this session");
                0
            };
            let id = async_done(m, status);
            push_int(m, id)
        }
        // ---- Events ------------------------------------------------------------------
        PlayNote => {
            let [note, velocity, offset, duration] = ints(m);
            if !(0..128).contains(&note) {
                m.env.note("play_note: note outside 0..127 ignored");
                return push_int(m, 0);
            }
            let parent = match m.t.ctx.kind {
                Kind::Cb(Callback::Note | Callback::Release) => m.t.ctx.event,
                _ => 0,
            };
            let id = m
                .env
                .play_note(slot, parent, note, velocity.clamp(1, 127), offset, duration);
            push_int(m, id)
        }
        NoteOff => {
            if argc == 2 {
                m.stk.int();
            }
            let [id] = ints(m);
            for k in 0..targets(m, id) {
                let id = m.env.targets[k];
                m.env.note_off(slot, id);
            }
            Ok(Step::Next)
        }
        IgnoreEvent => {
            let id = if argc == 1 {
                m.stk.int()
            } else {
                m.t.ctx.event
            };
            let in_release = m.t.ctx.kind == Kind::Cb(Callback::Release);
            let current = m.t.ctx.event;
            for k in 0..targets(m, id) {
                let id = m.env.targets[k];
                let Some(e) = m.env.events.get_mut(id) else {
                    continue;
                };
                if in_release && id == current {
                    e.release_ignored = true;
                } else if !e.at_engine {
                    e.ignored = true;
                }
            }
            Ok(Step::Next)
        }
        ChangeVol | ChangeTune | ChangePan => {
            let relative = if argc == 3 { m.stk.int() == 1 } else { false };
            let [id, value] = ints(m);
            let which = match f {
                ChangeVol => VoicePar::VolumeMdb,
                ChangeTune => VoicePar::TuneMc,
                _ => VoicePar::Pan,
            };
            set_voice_par(m, id, which, value, relative);
            Ok(Step::Next)
        }
        ChangeVelo | ChangeNote => {
            let [id, value] = ints(m);
            for k in 0..targets(m, id) {
                let id = m.env.targets[k];
                if let Some(e) = m.env.events.get_mut(id).filter(|e| !e.at_engine) {
                    if f == ChangeVelo {
                        e.velocity = value.clamp(1, 127);
                    } else {
                        e.note = value.clamp(0, 127);
                    }
                }
            }
            Ok(Step::Next)
        }
        FadeIn | FadeOut => {
            if (f == FadeIn && argc == 3) || (f == FadeOut && argc == 4) {
                m.stk.int();
            }
            let stop = f == FadeOut && argc >= 3 && m.stk.int() != 0;
            let [id, us] = ints(m);
            let us = us.max(0);
            for k in 0..targets(m, id) {
                let id = m.env.targets[k];
                let Some(e) = m.env.events.get_mut(id) else {
                    continue;
                };
                match (f, e.voice) {
                    (FadeIn, Some(v)) => {
                        m.engine.fade(m.env.offset, v, Fade::In { duration_us: us })
                    }
                    (FadeIn, None) => e.fade_in_us = us,
                    (_, Some(v)) => m.engine.fade(
                        m.env.offset,
                        v,
                        Fade::Out {
                            duration_us: us,
                            stop,
                        },
                    ),
                    (_, None) if stop && !e.at_engine => e.ignored = true,
                    _ => {}
                }
            }
            Ok(Step::Next)
        }
        SetEventPar => {
            let [id, p, value] = ints(m);
            match p {
                par::VOLUME => set_voice_par(m, id, VoicePar::VolumeMdb, value, false),
                par::TUNE => set_voice_par(m, id, VoicePar::TuneMc, value, false),
                par::PAN => set_voice_par(m, id, VoicePar::Pan, value, false),
                _ => {
                    for k in 0..targets(m, id) {
                        let id = m.env.targets[k];
                        let Some(e) = m.env.events.get_mut(id) else {
                            continue;
                        };
                        match p {
                            par::PAR_0..=par::PAR_3 => e.pars[p as usize] = value,
                            par::NOTE if !e.at_engine => e.note = value.clamp(0, 127),
                            par::VELOCITY if !e.at_engine => e.velocity = value.clamp(1, 127),
                            par::ZONE_ID => e.zone = value,
                            _ => m.env.note("set_event_par: parameter not settable here"),
                        }
                    }
                }
            }
            Ok(Step::Next)
        }
        GetEventPar => {
            let [id, p] = ints(m);
            let v = m.env.events.get(id).map_or(0, |e| match p {
                // Only a sounding voice has a zone; scripts read 0 as "voice gone" to
                // retire tracked notes (legato scripts pick the transition from them).
                // A released event reads 0 at once, though its tail may still sound.
                // ponytail: the engine does not report which zone a voice plays, so a
                // normally mapped voice reads 1; plumb the zone index if a script needs it.
                par::ZONE_ID if !e.voice.is_some_and(|v| m.engine.voice_active(v)) => 0,
                par::ZONE_ID => e.zone.max(1),
                par::PAR_0..=par::PAR_3 => e.pars[p as usize],
                par::VOLUME => e.volume,
                par::TUNE => e.tune,
                par::PAN => e.pan,
                par::NOTE => e.note,
                par::VELOCITY => e.velocity,
                par::SOURCE => e.source,
                _ => 0,
            });
            push_int(m, v)
        }
        SetEventParArr => {
            let [id, p, value, group] = ints(m);
            if p != par::ALLOW_GROUP {
                m.env
                    .note("set_event_par_arr: only $EVENT_PAR_ALLOW_GROUP is supported");
                return Ok(Step::Next);
            }
            for k in 0..targets(m, id) {
                let id = m.env.targets[k];
                if let Some(e) = m.env.events.get_mut(id) {
                    allow(&mut e.groups, group, value != 0);
                }
            }
            Ok(Step::Next)
        }
        GetEventParArr => {
            let [id, p, group] = ints(m);
            let v = match (p, m.env.events.get(id)) {
                (par::ALLOW_GROUP, Some(e)) => {
                    usize::try_from(group).is_ok_and(|g| e.groups.contains(g)) as i32
                }
                _ => 0,
            };
            push_int(m, v)
        }
        AllowGroup | DisallowGroup => {
            let [group] = ints(m);
            let id = m.t.ctx.event;
            match m.env.events.get_mut(id).filter(|e| !e.at_engine) {
                Some(e) => allow(&mut e.groups, group, f == AllowGroup),
                None => m
                    .env
                    .note("allow_group/disallow_group outside a note callback has no effect"),
            }
            Ok(Step::Next)
        }
        ByMarks => {
            let [marks] = ints(m);
            push_int(m, b::MARKS_FLAG | (marks & 0x0FFF_FFFF))
        }
        SetEventMark | DeleteEventMark => {
            let [id, mark] = ints(m);
            if let Some(e) = m.env.events.get_mut(id) {
                if f == SetEventMark {
                    e.marks |= mark as u32;
                } else {
                    e.marks &= !(mark as u32);
                }
            }
            Ok(Step::Next)
        }
        GetEventMark => {
            let [id, mark] = ints(m);
            let v = m
                .env
                .events
                .get(id)
                .is_some_and(|e| e.marks & mark as u32 != 0);
            push_int(m, v as i32)
        }
        EventStatus => {
            let [id] = ints(m);
            let live = m
                .env
                .events
                .get(id)
                .is_some_and(|e| e.live && e.voice.is_none_or(|v| m.engine.voice_active(v)));
            push_int(m, live as i32)
        }
        GetEventIds => {
            let v = m.stk.var();
            let var = &m.prog.vars[v as usize];
            let n = targets(m, b::ALL_EVENTS);
            let (base, len) = (var.slot as usize, var.len.unwrap_or(1) as usize);
            let dst = &mut m.slot.mem.ints[base..base + len];
            dst.fill(0);
            for (d, &id) in dst.iter_mut().zip(&m.env.targets[..n]) {
                *d = id;
            }
            Ok(Step::Next)
        }
        IgnoreController => {
            m.t.ctx.ignore_controller = true;
            Ok(Step::Next)
        }
        SetController => {
            let [cc, value] = ints(m);
            match u8::try_from(cc)
                .ok()
                .filter(|&c| usize::from(c) < b::CC_SLOTS)
            {
                Some(cc) => m.env.queue(super::runtime::Work::Controller {
                    cc,
                    value,
                    slot: slot + 1,
                }),
                None => m.env.note("set_controller: controller number out of range"),
            }
            Ok(Step::Next)
        }
        ResetRlsTrigCounter | WillNeverTerminate | SetMapEditorEventColor => {
            m.stk.int();
            Ok(Step::Next)
        }
        RedirectOutput => {
            ints::<2>(m);
            m.env.note("redirect_output is not applied");
            Ok(Step::Next)
        }
        Exit => Ok(Step::Exit),
        // ---- Time --------------------------------------------------------------------
        Wait | WaitTicks => {
            let [n] = ints(m);
            let us = if f == Wait {
                i64::from(n)
            } else {
                i64::from(n) * i64::from(m.env.quarter_us()) / 960
            };
            let at = m.env.clock() + m.env.samples(us).max(1);
            Ok(Step::Wait(at))
        }
        WaitAsync => {
            m.stk.int();
            Ok(Step::Next)
        }
        StopWait => {
            let [id, mode] = ints(m);
            if m.env.stop_waits.len() < m.env.stop_waits.capacity() {
                m.env.stop_waits.push((id, mode));
            }
            Ok(Step::Next)
        }
        ResetKspTimer => {
            m.env.timer_origin = m.env.clock();
            Ok(Step::Next)
        }
        SetListener | ChangeListenerPar => {
            let [signal, value] = ints(m);
            let valid = match signal {
                b::signal::TIMER_MS => value >= 1000 || value == 0,
                b::signal::TIMER_BEAT => (0..=24).contains(&value),
                b::signal::TRANSP_START | b::signal::TRANSP_STOP => {
                    f == SetListener && (0..=1).contains(&value)
                }
                _ => false,
            };
            if !valid {
                return Err(Fault("Invalid listener signal/parameter"));
            }
            let l = &mut m.slot.listener;
            match signal {
                b::signal::TIMER_MS => {
                    l.timer_us = value;
                    l.beats = 0;
                }
                b::signal::TIMER_BEAT => {
                    l.beats = value;
                    l.timer_us = 0;
                }
                _ => l.transport = value != 0,
            }
            l.generation = l.generation.wrapping_add(1);
            let name = match signal {
                b::signal::TIMER_MS => "$NI_SIGNAL_TIMER_MS",
                b::signal::TIMER_BEAT => "$NI_SIGNAL_TIMER_BEAT",
                b::signal::TRANSP_START => "$NI_SIGNAL_TRANSP_START",
                _ => "$NI_SIGNAL_TRANSP_STOP",
            };
            if value == 0 {
                m.slot.ui.listeners.remove(name);
            } else if !m.slot.ui.listeners.contains_key(name) || m.slot.ui.listeners[name] != value
            {
                m.slot.ui.listeners.insert(name, value);
            }
            m.env.listeners_changed |= 1 << slot;
            Ok(Step::Next)
        }
        // ---- Groups, modules and engine parameters -------------------------------------
        FindGroup => {
            let name = m.stk.strs.pop();
            let found = (0..m.engine.group_count()).find(|&g| m.engine.group_name(g) == name);
            if found.is_none() {
                m.env.note("find_group: group name not found; returned 0");
            }
            push_int(m, found.unwrap_or(0) as i32)
        }
        GroupName => {
            let [g] = ints(m);
            let name = usize::try_from(g)
                .ok()
                .filter(|&g| g < m.engine.group_count())
                .map_or("", |g| m.engine.group_name(g));
            m.stk.strs.push_str(name);
            Ok(Step::Next)
        }
        PurgeGroup => {
            ints::<2>(m);
            let id = m.env.next_async();
            push_int(m, id)
        }
        GetPurgeState => {
            m.stk.int();
            push_int(m, 1)
        }
        FindMod | FindTarget => {
            let (g, module) = if f == FindMod {
                let [g] = ints(m);
                (g, 0)
            } else {
                let [g, module] = ints(m);
                (g, module)
            };
            let name = m.stk.strs.pop();
            let found = match (usize::try_from(g), usize::try_from(module)) {
                (Ok(g), _) if f == FindMod => m.engine.find_mod(g, name),
                (Ok(g), Ok(module)) => m.engine.find_target(g, module, name),
                _ => None,
            };
            if found.is_none() {
                m.env
                    .note("find_mod/find_target: modulator unknown to the engine; returned 0");
            }
            push_int(m, found.unwrap_or(0) as i32)
        }
        GetEnginePar | GetEngineParDisp => {
            let p = engine_par(ints(m));
            let v = m
                .engine
                .engine_par(p)
                .or_else(|| m.env.engine_par(p))
                .unwrap_or(0);
            if f == GetEnginePar {
                push_int(m, v)
            } else {
                push_fmt(m, format_args!("{v}"))
            }
        }
        SetEnginePar => {
            let [id, value, group, s, generic] = ints(m);
            let p = EnginePar {
                id,
                group,
                slot: s,
                generic,
            };
            if !m.engine.set_engine_par(m.env.offset, p, value) {
                m.env
                    .note("set_engine_par: parameter not implemented by the engine; value stored");
                m.env.set_engine_par(p, value);
            }
            Ok(Step::Next)
        }
        OutputChannelName => {
            let [n] = ints(m);
            if n < 0 {
                m.stk.strs.push_str("Default");
                return Ok(Step::Next);
            }
            push_fmt(m, format_args!("Out {}", n + 1))
        }
        LoadIrSample => {
            ints::<2>(m);
            m.stk.strs.pop();
            m.env
                .note("load_ir_sample: impulse responses are not loaded");
            let id = async_done(m, 0);
            push_int(m, id)
        }
        // ---- User interface ------------------------------------------------------------
        SetControlPar => {
            let [id, p, value] = ints(m);
            if matches!(id, b::INST_ICON_ID | b::INST_WALLPAPER_ID) {
                return Ok(Step::Next);
            }
            let c = control(m, id)?;
            if p == b::CONTROL_PAR_VALUE {
                set_value(m, c, value);
            } else {
                m.slot.ui.controls[c].set_int(p, value);
            }
            Ok(Step::Next)
        }
        SetControlParStr => {
            let [id, p] = ints(m);
            let text = m.stk.strs.pop();
            if id == b::INST_WALLPAPER_ID && p == b::CONTROL_PAR_PICTURE {
                m.slot.ui.wallpaper.clear();
                m.slot.ui.wallpaper.push_str(text);
                return Ok(Step::Next);
            }
            if id == b::INST_ICON_ID || id == b::INST_WALLPAPER_ID {
                return Ok(Step::Next);
            }
            let c = m.slot.ui.control(id).ok_or(NO_CONTROL)?;
            let var = &m.prog.vars[m.slot.ui.controls[c].var as usize];
            if p == b::CONTROL_PAR_VALUE && var.ty == Ty::Str && var.len.is_none() {
                let dst = &mut m.slot.mem.strs[var.slot as usize];
                dst.clear();
                dst.push_str(text);
            } else {
                m.slot.ui.controls[c].set_str(p, text);
            }
            Ok(Step::Next)
        }
        SetControlParArr | SetControlParStrArr => {
            if f == SetControlParArr {
                ints::<4>(m);
            } else {
                ints::<3>(m);
                m.stk.strs.pop();
            }
            m.env.note("Array control parameters are not retained");
            Ok(Step::Next)
        }
        GetControlPar | GetControlParArr => {
            if f == GetControlParArr {
                m.stk.int();
            }
            let [id, p] = ints(m);
            if matches!(id, b::INST_ICON_ID | b::INST_WALLPAPER_ID) {
                return push_int(m, 0);
            }
            let c = control(m, id)?;
            let v = if p == b::CONTROL_PAR_VALUE {
                let var = &m.prog.vars[m.slot.ui.controls[c].var as usize];
                if var.ty == Ty::Int && var.len.is_none() {
                    m.slot.mem.ints[var.slot as usize]
                } else {
                    0
                }
            } else {
                match m.slot.ui.controls[c].get(p) {
                    Some(Prop::Int(n)) => *n,
                    _ => 0,
                }
            };
            push_int(m, v)
        }
        GetControlParStr | GetControlParStrArr => {
            if f == GetControlParStrArr {
                m.stk.int();
            }
            let [id, p] = ints(m);
            let c = control(m, id)?;
            let control = &m.slot.ui.controls[c];
            let var = &m.prog.vars[control.var as usize];
            let text = match control.get(p) {
                Some(Prop::Str(s)) => s.as_str(),
                _ if p == b::CONTROL_PAR_VALUE && var.ty == Ty::Str && var.len.is_none() => {
                    &m.slot.mem.strs[var.slot as usize]
                }
                _ => "",
            };
            m.stk.strs.push_str(text);
            Ok(Step::Next)
        }
        SetText | AddTextLine | SetKnobLabel | SetControlHelp => {
            let v = m.stk.var();
            let text = m.stk.strs.pop();
            let c = m
                .slot
                .ui
                .control_of(v)
                .ok_or(Fault("Command requires a declared UI control"))?;
            let p = match f {
                SetText | AddTextLine => b::CONTROL_PAR_TEXT,
                SetKnobLabel => b::CONTROL_PAR_LABEL,
                _ => b::CONTROL_PAR_HELP,
            };
            let s = m.slot.ui.controls[c].str_mut(p);
            if f == AddTextLine {
                if !s.is_empty() {
                    s.push('\n');
                }
            } else {
                s.clear();
            }
            s.push_str(text);
            if s.len() > 65536 {
                return Err(Fault("KSP string length limit"));
            }
            Ok(Step::Next)
        }
        SetKnobUnit | SetKnobDefval | HidePart | SetTableStepsShown => {
            let [value] = ints(m);
            let v = m.stk.var();
            let c = control_of(m, v)?;
            let p = match f {
                SetKnobUnit => b::CONTROL_PAR_UNIT,
                SetKnobDefval => b::CONTROL_PAR_DEFAULT_VALUE,
                HidePart => b::CONTROL_PAR_HIDE,
                _ => return Ok(Step::Next),
            };
            match m.prog.symbol_name(value).filter(|_| f == SetKnobUnit) {
                Some(name) => m.slot.ui.controls[c].set_str(p, name),
                None => m.slot.ui.controls[c].set_int(p, value),
            }
            Ok(Step::Next)
        }
        MoveControl | MoveControlPx => {
            let [mut x, mut y] = ints(m);
            let v = m.stk.var();
            let c = control_of(m, v)?;
            if f == MoveControl {
                if x == 0 || y == 0 {
                    m.slot.ui.controls[c].set_int(b::CONTROL_PAR_HIDE, 1);
                    return Ok(Step::Next);
                }
                x = x.saturating_sub(1).saturating_mul(92).saturating_add(66);
                y = y.saturating_sub(1).saturating_mul(21).saturating_add(2);
            }
            let control = &mut m.slot.ui.controls[c];
            control.set_int(b::CONTROL_PAR_POS_X, x);
            control.set_int(b::CONTROL_PAR_POS_Y, y);
            Ok(Step::Next)
        }
        AddMenuItem => {
            let [value] = ints(m);
            let v = m.stk.var();
            let text = m.stk.strs.pop();
            let c = m
                .slot
                .ui
                .control_of(v)
                .ok_or(Fault("Command requires a declared UI control"))?;
            let menu = &mut m.slot.ui.controls[c].menu;
            if menu.len() >= 4096 {
                return Err(Fault("Menu item limit"));
            }
            menu.push(MenuItem {
                text: text.to_owned(),
                value,
                visible: true,
            });
            Ok(Step::Next)
        }
        SetMenuItemStr => {
            let [id, index] = ints(m);
            let text = m.stk.strs.pop();
            let c = m.slot.ui.control(id).ok_or(NO_CONTROL)?;
            if let Some(item) = usize::try_from(index)
                .ok()
                .and_then(|i| m.slot.ui.controls[c].menu.get_mut(i))
            {
                item.text.clear();
                item.text.push_str(text);
            }
            Ok(Step::Next)
        }
        SetMenuItemVisibility | SetMenuItemValue => {
            let [id, index, value] = ints(m);
            let c = control(m, id)?;
            if let Some(item) = usize::try_from(index)
                .ok()
                .and_then(|i| m.slot.ui.controls[c].menu.get_mut(i))
            {
                if f == SetMenuItemValue {
                    item.value = value;
                } else {
                    item.visible = value != 0;
                }
            }
            Ok(Step::Next)
        }
        GetMenuItemStr => {
            let [id, index] = ints(m);
            let c = control(m, id)?;
            let menu = &m.slot.ui.controls[c].menu;
            let text = usize::try_from(index)
                .ok()
                .and_then(|i| menu.get(i))
                .map_or("", |item| item.text.as_str());
            m.stk.strs.push_str(text);
            Ok(Step::Next)
        }
        GetMenuItemValue | GetMenuItemVisibility => {
            let [id, index] = ints(m);
            let c = control(m, id)?;
            let menu = &m.slot.ui.controls[c].menu;
            let item = usize::try_from(index).ok().and_then(|i| menu.get(i));
            let v = match f {
                GetMenuItemValue => item.map_or(0, |i| i.value),
                _ => item.is_some_and(|i| i.visible) as i32,
            };
            push_int(m, v)
        }
        GetNumMenuItems => {
            let [id] = ints(m);
            let c = control(m, id)?;
            push_int(m, m.slot.ui.controls[c].menu.len() as i32)
        }
        SetSkinOffset | SetUiColor | SetSnapshotType | DisableLogging | FsNavigate
        | RemoveKeyrange => {
            if f == FsNavigate {
                m.stk.int();
            }
            m.stk.int();
            Ok(Step::Next)
        }
        SetUiHeight | SetUiHeightPx | SetUiWidthPx => {
            let [n] = ints(m);
            let ui = &mut m.slot.ui;
            match f {
                SetUiHeight => ui.height = n.clamp(1, 8) * 68,
                SetUiHeightPx => ui.height = n.clamp(1, 4096),
                _ => ui.width = n.clamp(1, 4096),
            }
            Ok(Step::Next)
        }
        SetScriptTitle => {
            let text = m.stk.strs.pop();
            m.slot.ui.title.clear();
            m.slot.ui.title.push_str(text);
            Ok(Step::Next)
        }
        MakePerfview => {
            m.slot.ui.performance = true;
            Ok(Step::Next)
        }
        ShowLibraryTab => Ok(Step::Next),
        SetUiWfProperty => {
            ints::<3>(m);
            m.stk.var();
            Ok(Step::Next)
        }
        GetFontId => {
            // Numeric IDs (older scripts) pass through; named fonts are not rendered.
            let n = m.stk.strs.pop().parse().unwrap_or(0);
            push_int(m, n)
        }
        GetFolder => {
            m.stk.int();
            m.stk.strs.push();
            Ok(Step::Next)
        }
        FsGetFilename => {
            ints::<2>(m);
            m.stk.strs.push();
            Ok(Step::Next)
        }
        // ---- Keyboard display ----------------------------------------------------------
        SetKeyPressedSupport => {
            let [mode] = ints(m);
            if !(0..=1).contains(&mode) {
                return Err(Fault("Pressed support mode must be 0 or 1"));
            }
            m.env.host.script_pressed = mode == 1;
            Ok(Step::Next)
        }
        SetKeyName => {
            let [note] = ints(m);
            let text = m.stk.strs.pop();
            let key = m
                .env
                .host
                .keyboard
                .entry(midi_note(note)?)
                .or_insert_with(KeyState::default);
            key.name.clear();
            key.name.push_str(text);
            Ok(Step::Next)
        }
        SetKeyColor | SetKeyType | SetKeyPressed => {
            let [note, value] = ints(m);
            let note = midi_note(note)?;
            if f == SetKeyPressed {
                if !(0..=1).contains(&value) {
                    return Err(Fault("Pressed state must be 0 or 1"));
                }
                if !m.env.host.script_pressed {
                    return Ok(Step::Next);
                }
            }
            let shown = match m.prog.symbol_name(value) {
                Some(name) if f != SetKeyPressed => Value::Text(name.to_owned()),
                _ => Value::Int(value),
            };
            let key = m
                .env
                .host
                .keyboard
                .entry(note)
                .or_insert_with(KeyState::default);
            match f {
                SetKeyColor => key.color = Some(shown),
                SetKeyType => key.kind = Some(shown),
                _ => key.pressed = value == 1,
            }
            Ok(Step::Next)
        }
        GetKeyName => {
            let [note] = ints(m);
            let note = midi_note(note)?;
            let name = m
                .env
                .host
                .keyboard
                .get(&note)
                .map_or("", |k| k.name.as_str());
            m.stk.strs.push_str(name);
            Ok(Step::Next)
        }
        GetKeyColor | GetKeyType | GetKeyTriggerstate => {
            let [note] = ints(m);
            let note = midi_note(note)?;
            if f == GetKeyTriggerstate && !m.env.host.script_pressed {
                return Err(Fault(
                    "get_key_triggerstate requires script pressed support",
                ));
            }
            let key = m.env.host.keyboard.get(&note);
            let v = match f {
                GetKeyTriggerstate => key.is_some_and(|k| k.pressed) as i32,
                _ => match key.and_then(|k| {
                    if f == GetKeyColor {
                        k.color.as_ref()
                    } else {
                        k.kind.as_ref()
                    }
                }) {
                    Some(Value::Int(n)) => *n,
                    _ => 0,
                },
            };
            push_int(m, v)
        }
        SetKeyrange => {
            ints::<2>(m);
            m.stk.strs.pop();
            Ok(Step::Next)
        }
        AttachZone => {
            ints::<2>(m);
            m.stk.var();
            Ok(Step::Next)
        }
        // ---- Diagnostics -------------------------------------------------------------
        Message => {
            let text = m.stk.strs.pop();
            m.env.message.clear();
            m.env.message.push_str(text);
            Ok(Step::Next)
        }
        SetCondition | ResetCondition => {
            m.stk.var();
            Ok(Step::Next)
        }
        // ---- Persistence ---------------------------------------------------------------
        MakePersistent | MakeInstrPersistent => {
            let v = m.stk.var();
            if !m.slot.persistent.contains(&v) {
                m.slot.persistent.push(v);
            }
            Ok(Step::Next)
        }
        ReadPersistentVar => {
            let v = m.stk.var();
            let var = &m.prog.vars[v as usize];
            if let Some(value) = m
                .env
                .persisted
                .get(slot as usize)
                .and_then(|p| p.get(&*var.name))
            {
                write_value(&mut m.slot.mem, var, value);
            }
            Ok(Step::Next)
        }
        // ---- Program global storage ----------------------------------------------------
        PgsCreateKey => {
            let [size] = ints(m);
            let key = m.stk.var();
            let name = &m.prog.strings[key as usize];
            if !key_name_ok(name) {
                return Err(Fault("Invalid PGS key identifier"));
            }
            if !(1..=256).contains(&size) {
                return Err(Fault("PGS key size must be 1..256"));
            }
            match pgs_int_key(m, key) {
                Some(i) if m.env.host.pgs_ints[i].1.len() != size as usize => {
                    return Err(Fault("PGS key redeclared with a different size"));
                }
                Some(_) => {}
                None if m.env.host.pgs_ints.len() >= 4096 => return Err(Fault("PGS key limit")),
                None => m
                    .env
                    .host
                    .pgs_ints
                    .push((name.to_string(), vec![0; size as usize])),
            }
            Ok(Step::Next)
        }
        PgsKeyExists => {
            let key = m.stk.var();
            let exists = pgs_int_key(m, key).is_some();
            push_int(m, exists as i32)
        }
        PgsSetKeyVal => {
            let [index, value] = ints(m);
            let key = m.stk.var();
            let i = pgs_int_key(m, key).ok_or(NO_PGS_KEY)?;
            let cell = usize::try_from(index)
                .ok()
                .and_then(|x| m.env.host.pgs_ints[i].1.get_mut(x))
                .ok_or(Fault("PGS index out of bounds"))?;
            *cell = value;
            m.env.pgs_changed = true;
            Ok(Step::Next)
        }
        PgsGetKeyVal => {
            let [index] = ints(m);
            let key = m.stk.var();
            let i = pgs_int_key(m, key).ok_or(NO_PGS_KEY)?;
            let v = usize::try_from(index)
                .ok()
                .and_then(|x| m.env.host.pgs_ints[i].1.get(x))
                .copied()
                .ok_or(Fault("PGS index out of bounds"))?;
            push_int(m, v)
        }
        PgsCreateStrKey => {
            let key = m.stk.var();
            let name = &m.prog.strings[key as usize];
            if !key_name_ok(name) {
                return Err(Fault("Invalid PGS key identifier"));
            }
            if pgs_str_key(m, key).is_none() {
                if m.env.host.pgs_strs.len() >= 4096 {
                    return Err(Fault("PGS key limit"));
                }
                m.env.host.pgs_strs.push((name.to_string(), String::new()));
            }
            Ok(Step::Next)
        }
        PgsStrKeyExists => {
            let key = m.stk.var();
            let exists = pgs_str_key(m, key).is_some();
            push_int(m, exists as i32)
        }
        PgsSetStrKeyVal => {
            let key = m.stk.var();
            let text = m.stk.strs.pop();
            let name = &*m.prog.strings[key as usize];
            let i = m
                .env
                .host
                .pgs_strs
                .iter()
                .position(|(k, _)| k == name)
                .ok_or(NO_PGS_KEY)?;
            let total: usize = m.env.host.pgs_strs.iter().map(|(_, s)| s.len()).sum();
            if text.len() > 65536 || total - m.env.host.pgs_strs[i].1.len() + text.len() > 4 << 20 {
                return Err(Fault("PGS string memory limit"));
            }
            let dst = &mut m.env.host.pgs_strs[i].1;
            dst.clear();
            dst.push_str(text);
            m.env.pgs_changed = true;
            Ok(Step::Next)
        }
        PgsGetStrKeyVal => {
            let key = m.stk.var();
            let i = pgs_str_key(m, key).ok_or(NO_PGS_KEY)?;
            let (strs, host) = (&mut m.stk.strs, &m.env.host);
            strs.push_str(&host.pgs_strs[i].1);
            Ok(Step::Next)
        }
    }
}

fn allow(groups: &mut GroupMask, group: i32, allowed: bool) {
    if group == b::ALL_GROUPS {
        *groups = if allowed {
            GroupMask::all()
        } else {
            GroupMask::none()
        };
    } else if let Ok(g) = usize::try_from(group) {
        groups.set(g, allowed);
    }
}

fn set_value(m: &mut Machine, c: usize, value: i32) {
    let var = &m.prog.vars[m.slot.ui.controls[c].var as usize];
    if var.ty == Ty::Int && var.len.is_none() {
        m.slot.mem.ints[var.slot as usize] = value;
    }
}

fn sort<T>(items: &mut [T], descending: bool, cmp: impl Fn(&T, &T) -> std::cmp::Ordering) {
    if descending {
        items.sort_unstable_by(|a, b| cmp(b, a));
    } else {
        items.sort_unstable_by(cmp);
    }
}
