//! Builtin implementations. Arguments arrive on the typed stacks in call order;
//! each builtin pops its own. Nothing here allocates on the event paths.

use super::builtins::{self as b, Builtin, event_par as par};
use super::compile::{Callback, Ty, VarId};
use super::engine::{EnginePar, Fade, GroupMask, VoicePar};
use super::runtime::{read_value, refresh_value, write_value_rt};
use super::ui::{MenuItem, Prop};
use super::arrays::{read_path, nka, save_nka};
use super::vm::{Exec, Fault, Kind, Machine, Step, append_text, put_text, put_variable_text};
use super::{KeyState, Value};

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
        m.env.fault(m.slot.index, m.t.pc, super::vm::NONFINITE);
    }
    m.stk.reals.push(v);
    Ok(Step::Next)
}

fn push_fmt(m: &mut Machine, args: std::fmt::Arguments) -> Exec<Step> {
    super::vm::format_text(m.stk.strs.push()?, args, m.env.loading)?;
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
            m.engine.set_par_from(m.env.offset, e.channel, e.input_channel, voice, which, v);
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

/// `path` as Kontakt prints a folder: `/` separators and a trailing `/`.
fn dir(path: &std::path::Path) -> String {
    let mut s = path.to_string_lossy().replace('\\', "/");
    if !s.ends_with('/') {
        s.push('/');
    }
    s
}

/// `$GET_FOLDER_LIBRARY_DIR` of `instrument`: the nearest folder above it
/// with a `.nicnt`, else the one holding its `Instruments` folder, else
/// its own.
fn library_dir(instrument: &std::path::Path) -> String {
    let has_nicnt = |d: &std::path::Path| {
        std::fs::read_dir(d).into_iter().flatten().flatten().any(|e| {
            e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("nicnt"))
        })
    };
    let ancestors = || instrument.ancestors().skip(1);
    let found = ancestors().find(|d| has_nicnt(d)).or_else(|| {
        ancestors()
            .find(|d| d.file_name().is_some_and(|n| n.eq_ignore_ascii_case("instruments")))
            .and_then(|d| d.parent())
    });
    dir(found.unwrap_or_else(|| instrument.parent().unwrap_or(instrument)))
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
/// Kontakt keeps at most this many `set_keyrange` ranges per instrument.
const MAX_KEYRANGES: usize = 16;

// Recycle names so replacing/removing a range never frees text on the audio thread.
fn remove_keyranges(env: &mut super::runtime::Env, low: u8, high: u8, keep: Option<usize>) {
    for i in (0..env.host.keyranges.len()).rev() {
        let (l, h, _) = &env.host.keyranges[i];
        if Some(i) != keep && *h >= low && *l <= high {
            let (_, _, mut name) = env.host.keyranges.remove(i);
            if !env.loading {
                name.clear();
                env.spare_keyranges.push(name);
            }
        }
    }
}

fn charge(m: &Machine, fuel: &mut u64, count: u64) -> Exec<()> {
    if m.env.deadline.is_some() {
        if count > *fuel {
            return Err(Fault(
                "Synchronous array operation exceeds audio block budget",
            ));
        }
        *fuel -= count;
    }
    Ok(())
}

pub fn call(m: &mut Machine, f: Builtin, argc: u8, fuel: &mut u64) -> Exec<Step> {
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
        Sgn => {
            let [a] = ints(m);
            push_int(m, a.signum())
        }
        Signbit => {
            let [a] = ints(m);
            push_int(m, (a < 0) as i32)
        }
        SgnReal => {
            let x = m.stk.real();
            push_int(m, if x > 0.0 { 1 } else if x < 0.0 { -1 } else { 0 })
        }
        SignbitReal => {
            let x = m.stk.real();
            push_int(m, x.is_sign_negative() as i32)
        }
        Exp2 | Cbrt => {
            let x = m.stk.real();
            push_real(m, if f == Exp2 { x.exp2() } else { x.cbrt() })
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
                m.env.fault(m.slot.index, m.t.pc, "Real to integer overflow (saturated)");
            }
            // Saturating; NaN is 0.
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
            let bounds = if argc == 4 { Some(ints::<2>(m)) } else { None };
            let v = m.stk.var();
            let var = &m.prog.vars[v as usize];
            let len = var.len.unwrap_or(1) as i32;
            let [lo, hi] = bounds.unwrap_or([0, len.saturating_sub(1)]);
            let lo = lo.clamp(0, len);
            let end = hi.saturating_add(1).clamp(lo, len);
            let range = (var.slot + lo as u32) as usize..(var.slot + end as u32) as usize;
            charge(m, fuel, range.len() as u64)?;
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
            push_int(m, found.map_or(-1, |i| lo + i as i32))
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
            let hi = hi.map_or(len, |h| h.saturating_add(1).clamp(lo, len));
            let range = (var.slot as i32 + lo) as usize..(var.slot as i32 + hi) as usize;
            let n = range.len() as u64;
            charge(m, fuel, n)?;
            let width = if var.ty == Ty::Str { m.slot.mem.strs[range.clone()].iter().map(|s| s.len() as u64).max().unwrap_or(1) } else { 1 };
            charge(m, fuel, n.saturating_mul(u64::from(n.max(1).ilog2()) + 1).saturating_mul(width.max(1)))?;
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
            let n = u64::from(a.len.unwrap_or(1));
            charge(m, fuel, n)?;
            let width = if a.ty == Ty::Str {
                m.slot.mem.strs[a.slot as usize..a.slot as usize + n as usize].iter().map(|s| s.len() as u64).sum()
            } else { n };
            charge(m, fuel, width)?;
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
            if matches!(f, LoadArrayStr | SaveArrayStr) && !m.env.loading {
                let v = m.stk.var();
                let id = m.env.next_async();
                let saving = f == SaveArrayStr;
                if saving {
                    let var = &m.prog.vars[v as usize];
                    let n = u64::from(var.len.unwrap_or(1));
                    let fits = charge(m,fuel,n).is_ok() && {
                        let bytes = if var.ty == Ty::Str {
                            m.slot.mem.strs[var.slot as usize..var.slot as usize+n as usize]
                                .iter().map(|s| s.len() as u64).sum()
                        } else { n };
                        charge(m,fuel,bytes).is_ok()
                    };
                    if !fits {
                        m.stk.strs.pop();
                        m.env.note("save_array_str: snapshot exceeds audio block budget");
                        if m.env.async_done.len() < m.env.async_done.capacity() { m.env.async_done.push((slot,id,0)); }
                        return push_int(m,id);
                    }
                }
                let path = m.stk.strs.pop();
                let spare = m.env.array_spares.iter().position(|r| r.slot == slot && r.var == v);
                if m.env.array_inflight < super::arrays::ARRAY_QUEUE
                    && let Some(spare) = spare
                    && path.len() <= m.env.array_spares[spare].path.capacity()
                {
                    let mut request = m.env.array_spares.swap_remove(spare);
                    request.path.push_str(path);
                    request.id = id;
                    request.write = saving;
                    if saving {
                        refresh_value(&m.slot.mem, &m.prog.vars[v as usize],
                            request.snapshot.as_mut().expect("prepared array save snapshot"));
                    }
                    m.env.array_requests.push_back(request);
                    m.env.array_inflight += 1;
                } else {
                    m.env.note(if saving { "save_array_str: prepared request queue or path capacity exhausted" }
                        else { "load_array_str: prepared request queue or path capacity exhausted" });
                    if m.env.async_done.len() < m.env.async_done.capacity() {
                        m.env.async_done.push((slot, id, 0));
                    }
                }
                return push_int(m, id);
            }
            let (mode, path) = if matches!(f, LoadArray | SaveArray) {
                (m.stk.int(), None)
            } else {
                let path = m.stk.strs.pop();
                (-1, m.env.loading.then(|| path.to_owned()))
            };
            let v = m.stk.var();
            if f == SaveArray || (f == LoadArray && mode == 0) {
                m.env.note(if f == SaveArray {
                    "save_array: external file dialogs and mode-based saves are unavailable"
                } else { "load_array: no file dialog here; nothing saved in this session" });
                let id = async_done(m,0);
                return push_int(m,id);
            }
            let var = &m.prog.vars[v as usize];
            let n = u64::from(var.len.unwrap_or(1));
            charge(m, fuel, n)?;
            let cost = if var.ty == Ty::Str {
                m.slot.mem.strs[var.slot as usize..var.slot as usize + n as usize].iter().map(|s| s.len() as u64 + 1).sum()
            } else { n };
            charge(m, fuel, cost)?;
            let status = if f == SaveArrayStr {
                let name = var.name.trim_start_matches(['%', '!', '?', '$', '@', '~']);
                match save_nka(path.as_deref().unwrap_or(""),var.ty,name,&read_value(&m.slot.mem,var)) {
                    Ok(()) => 1,
                    Err(error) => {
                        if let Some(instrument) = m.engine.instrument_path() {
                            crate::diagnostics::resource(instrument,path.as_deref().unwrap_or(""),&error);
                        }
                        m.env.note("save_array_str: file could not be written; see diagnostics log");
                        0
                    }
                }
            } else if !m.env.loading {
                m.env.note("load_array: files load only during on init");
                0
            } else if let Some(instrument) = m.engine.instrument_path() {
                // Mode 1: the library's Data folder; 2: the resource
                // container's `data`; a path: that file. A file that is not
                // there is the script's business, as in Kontakt.
                let name = var.name.trim_start_matches(['%', '!', '?', '$', '@', '~']);
                let bytes = match (mode, &path) {
                    (_, Some(path)) => read_path(path),
                    (1, _) => read_path(&format!("{}Data/{name}.nka", library_dir(instrument))),
                    (2, _) => match crate::resources::data_file(instrument, &format!("{name}.nka")) {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            crate::diagnostics::resource(instrument, &format!("{name}.nka"), &e);
                            m.env.note("load_array: library data resource could not be read; see diagnostics log");
                            None
                        }
                    },
                    _ => None,
                };
                match bytes.and_then(|b| nka(&b, var.ty, name)) {
                    Some(value) => {
                        write_value_rt(&mut m.slot.mem, var, &value, true)?;
                        1
                    }
                    None => 0,
                }
            } else {
                m.env.note("load_array: files load only during on init");
                0
            };
            let id = async_done(m, status);
            push_int(m, id)
        }
        // ---- Events ------------------------------------------------------------------
        PlayNote => {
            let [note, velocity, offset, duration] = ints(m);
            if m.t.ctx.cleanup { return push_int(m, 0); }
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
                .play_note(slot, parent, m.t.ctx.channel, m.t.ctx.input_channel, note, velocity.clamp(1, 127), offset, duration);
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
                        m.engine.fade_from(m.env.offset, e.channel, e.input_channel, v, Fade::In { duration_us: us })
                    }
                    (FadeIn, None) => e.fade_in_us = us,
                    (_, Some(v)) => m.engine.fade_from(
                        m.env.offset,
                        e.channel,
                        e.input_channel,
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
                par::ZONE_ID => m.env.note("set_event_par: EVENT_PAR_ZONE_ID is read-only"),
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
                            par::MIDI_CHANNEL if !e.at_engine && (0..16).contains(&value) => e.channel = value as u8,
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
                par::ZONE_ID => e.voice.filter(|_| e.live).map_or(0, |v| {
                    m.engine.voice_zone(v).unwrap_or(if e.zone < 0 { -1 } else { 0 })
                }),
                par::PAR_0..=par::PAR_3 => e.pars[p as usize],
                par::VOLUME => e.volume,
                par::TUNE => e.tune,
                par::PAN => e.pan,
                par::NOTE => e.note,
                par::VELOCITY => e.velocity,
                par::MIDI_CHANNEL => i32::from(e.channel),
                par::SOURCE => e.source,
                _ => 0,
            });
            push_int(m, v)
        }
        SetEventParArr | SetEventParIndexed => {
            let [id, p, value, group] = if f == SetEventParIndexed {
                let [id, index, value] = ints(m);
                [id, par::CUSTOM, value, index]
            } else { ints(m) };
            if p == par::CUSTOM {
                let index = usize::try_from(group).ok().filter(|&n| n < 16)
                    .ok_or(Fault("Custom event parameter index outside 0..15"))?;
                for k in 0..targets(m, id) {
                    if let Some(e) = m.env.events.get_mut(m.env.targets[k]) {
                        e.pars[index] = value;
                    }
                }
                return Ok(Step::Next);
            }
            if p != par::ALLOW_GROUP {
                m.env
                    .note("set_event_par_arr: unsupported event array parameter");
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
                (par::CUSTOM, Some(e)) => usize::try_from(group).ok().and_then(|n| e.pars.get(n)).copied().unwrap_or(0),
                _ => 0,
            };
            push_int(m, v)
        }
        AllowGroup | DisallowGroup => {
            let [group] = ints(m);
            let id = m.t.ctx.event;
            let release = m.t.ctx.kind == Kind::Cb(Callback::Release);
            let note = m.t.ctx.kind == Kind::Cb(Callback::Note);
            match m.env.events.get_mut(id).filter(|e| release || (note && !e.at_engine)) {
                Some(e) => allow(&mut e.groups, group, f == AllowGroup),
                None => m
                    .env
                    .note("allow_group/disallow_group needs a pending note or release callback"),
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
                .is_some_and(|e| e.live && !e.silenced && e.voice.is_none_or(|v| m.engine.voice_active(v)));
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
                    channel: m.t.ctx.channel,
                    input_channel: m.t.ctx.input_channel,
                    cc,
                    value,
                    slot: slot + 1,
                }),
                None => m.env.note("set_controller: controller number out of range"),
            }
            Ok(Step::Next)
        }
        SetNoteController => {
            let [controller, note, value] = ints(m);
            if m.t.ctx.kind == super::vm::Kind::Cb(super::compile::Callback::Init) {
                return Err(Fault("set_note_controller is unavailable during initialization"));
            }
            let range = if controller == b::VNC_PITCH_BEND { -8192..=8191 } else { 0..=127 };
            if !(0..=b::VNC_PITCH_BEND).contains(&controller)
                || !(0..=127).contains(&note) || !range.contains(&value) {
                m.env.note("set_note_controller: controller, note or value out of range");
            } else {
                m.env.queue(super::runtime::Work::NoteController {
                    channel: m.t.ctx.channel,
                    input_channel: m.t.ctx.input_channel,
                    controller,
                    note: note as u8,
                    value,
                    slot: slot + 1,
                });
            }
            Ok(Step::Next)
        }
        SetRpn | SetNrpn => {
            let [address, value] = ints(m);
            if !(0..=16383).contains(&address) || !(0..=16383).contains(&value) {
                return Err(Fault("RPN address or value out of range"));
            }
            m.env.queue(super::runtime::Work::Rpn {
                channel: m.t.ctx.channel, input_channel: m.t.ctx.input_channel, nrpn: f == SetNrpn, address, value, slot: slot + 1,
            });
            Ok(Step::Next)
        }
        ResetRlsTrigCounter => {
            if let Ok(note) = u8::try_from(m.stk.int()).map(|n| n.min(127)) {
                m.engine.reset_release_counter_from(m.env.offset, m.t.ctx.channel, m.t.ctx.input_channel, note);
            }
            Ok(Step::Next)
        }
        WillNeverTerminate | SetMapEditorEventColor => {
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
            if m.t.ctx.ignore_wait { return Ok(Step::Next); }
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
                _ => {
                    let bit = if signal == b::signal::TRANSP_START { 1 } else { 2 };
                    if value == 0 { l.transport &= !bit; } else { l.transport |= bit; }
                }
            }
            l.generation = l.generation.wrapping_add(1);
            let name = match signal {
                b::signal::TIMER_MS => "$NI_SIGNAL_TIMER_MS",
                b::signal::TIMER_BEAT => "$NI_SIGNAL_TIMER_BEAT",
                b::signal::TRANSP_START => "$NI_SIGNAL_TRANSP_START",
                _ => "$NI_SIGNAL_TRANSP_STOP",
            };
            if value == 0 {
                if m.env.loading { m.slot.ui.listeners.remove(name); }
                else if let Some(v) = m.slot.ui.listeners.get_mut(name) { *v = 0; }
            } else if !m.slot.ui.listeners.contains_key(name) || m.slot.ui.listeners[name] != value
            {
                if let Some(v) = m.slot.ui.listeners.get_mut(name) { *v = value; }
                else if m.env.loading { m.slot.ui.listeners.insert(name, value); }
                else { return Err(Fault("KSP listener storage was not prepared")); }
            }
            m.env.listeners_changed |= 1 << slot;
            Ok(Step::Next)
        }
        // ---- Groups, modules and engine parameters -------------------------------------
        // An exact match, as Kontakt's. A miss returns 0 (`find_group`) or
        // `$NI_NOT_FOUND`; framework scripts look up names their instrument
        // lacks, so only a near miss (case or outer spaces) is reported: that
        // would be a name this importer decoded differently.
        FindGroup | GetGroupIdx => {
            let name = m.stk.strs.pop();
            let groups = 0..m.engine.group_count();
            let found = groups.clone().find(|&g| m.engine.group_name(g) == name);
            let near = |g| m.engine.group_name(g).trim().eq_ignore_ascii_case(name.trim());
            if found.is_none() && groups.clone().any(near) {
                m.env.note("find_group: group name matches only ignoring case or spaces; not found");
            }
            let miss = if f == FindGroup { 0 } else { b::NOT_FOUND };
            push_int(m, found.map_or(miss, |g| g as i32))
        }
        GetNumZones => {
            let count = m.engine.zone_count() as i32;
            push_int(m, count)
        }
        GetZoneId => {
            let [index] = ints(m);
            let id = usize::try_from(index).ok().and_then(|i| m.engine.zone_id(i));
            if id.is_none() { m.env.note("get_zone_id: zone index out of range"); }
            push_int(m, id.unwrap_or(-1))
        }
        GroupName => {
            let [g] = ints(m);
            let name = usize::try_from(g)
                .ok()
                .filter(|&g| g < m.engine.group_count())
                .map_or("", |g| m.engine.group_name(g));
            m.stk.strs.push_str(name)?;
            Ok(Step::Next)
        }
        PurgeGroup => {
            ints::<2>(m);
            // Samples stay loaded, but scripts wait for the completion.
            let id = async_done(m, 1);
            push_int(m, id)
        }
        GetPurgeState => {
            m.stk.int();
            push_int(m, 1)
        }
        FindMod | FindTarget | GetModIdx | GetTargetIdx => {
            let by_mod = matches!(f, FindMod | GetModIdx);
            let (g, module) = if by_mod {
                let [g] = ints(m);
                (g, 0)
            } else {
                let [g, module] = ints(m);
                (g, module)
            };
            let name = m.stk.strs.pop();
            // As find_group: report only names that match ignoring case or spaces.
            let find = |is: &dyn Fn(&str) -> bool| match (usize::try_from(g), usize::try_from(module)) {
                (Ok(g), _) if by_mod => m.engine.find_mod(g, is),
                (Ok(g), Ok(module)) => m.engine.find_target(g, module, is),
                _ => None,
            };
            let found = find(&|n| n == name);
            if found.is_none() && find(&|n| n.trim().eq_ignore_ascii_case(name.trim())).is_some() {
                m.env.note("find_mod/find_target: name matches only ignoring case or spaces; not found");
            }
            let miss = if matches!(f, FindMod | FindTarget) { 0 } else { b::NOT_FOUND };
            push_int(m, found.map_or(miss, |i| i as i32))
        }
        GetEnginePar | GetEngineParDisp | GetEngineParDispExt => {
            let (p, v) = if f == GetEngineParDispExt {
                // A hypothetical value, shown as if the parameter had it.
                let [id, value, group, slot, generic] = ints(m);
                (engine_par([id, group, slot, generic]), value)
            } else {
                let p = engine_par(ints(m));
                let v = m
                    .engine
                    .engine_par(p)
                    .or_else(|| m.env.engine_par(p))
                    .unwrap_or(0);
                (p, v)
            };
            if f == GetEnginePar {
                return push_int(m, v);
            }
            // Shown by Kontakt's value law, as its own knobs show it.
            match crate::engine::engine_par_display(p.id, v) {
                Some(shown) => push_fmt(m, format_args!("{shown}")),
                None => push_fmt(m, format_args!("{v}")),
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
            // Loading an effect is asynchronous: the script gets an ID that
            // `on async_complete` reports. Other parameters apply at once.
            let loads = ["$ENGINE_PAR_EFFECT_TYPE", "$ENGINE_PAR_EFFECT_SUBTYPE", "$ENGINE_PAR_SEND_EFFECT_TYPE"]
                .iter()
                .any(|n| b::engine_par_id(n) == Some(id));
            let implemented = m.engine.set_engine_par(m.env.offset, p, value);
            if !implemented {
                m.env.note(if loads && b::engine_par_name(id) != Some("$ENGINE_PAR_EFFECT_SUBTYPE") {
                    "set_engine_par: another effect loads only into instrument or bus racks during on init; value stored"
                } else {
                    "set_engine_par: parameter not implemented by the engine; value stored"
                });
                m.env.set_engine_par(p, value);
            }
            let result = if loads { async_done(m, i32::from(implemented)) } else { -1 };
            push_int(m, result)
        }
        GetVoiceLimit | SetVoiceLimit => {
            let voice_type = if f == SetVoiceLimit {
                let [voice_type, value] = ints(m);
                if value < 0 { return Err(Fault("Negative Time Machine Pro voice limit")); }
                voice_type
            } else {
                ints::<1>(m)[0]
            };
            if !(0..=1).contains(&voice_type) {
                return Err(Fault("Unknown Time Machine Pro voice type"));
            }
            // These are the stretch engine's limits, not ordinary sampler
            // polyphony. No stretch voices exist until that engine is supported.
            m.env.note("Time Machine Pro is unavailable; voice limit is 0 and allocation requests fail");
            let result = if f == SetVoiceLimit { async_done(m, 0) } else { 0 };
            push_int(m, result)
        }
        OutputChannelName => {
            let [n] = ints(m);
            if n < 0 {
                m.stk.strs.push_str("Default")?;
                return Ok(Step::Next);
            }
            push_fmt(m, format_args!("Out {}", n + 1))
        }
        LoadIrSample => {
            let [slot, generic] = ints(m);
            let file = m.stk.strs.pop();
            let id = m.env.next_async();
            let request = m.engine.request_ir_sample(&file, slot, generic, m.slot.index, id);
            if request == Some(true) {
                return push_int(m, id);
            }
            // Asynchronous in Kontakt: `on async_complete` reports 1 once
            // loaded, 0 when not found.
            let loaded = request.or_else(|| m.engine.load_ir_sample(&file, slot, generic));
            match loaded {
                Some(true) => {}
                Some(false) => m.env.note(
                    "load_ir_sample: file not found, or that slot holds no convolution effect",
                ),
                None => m.env.note("load_ir_sample: impulse responses load only while on init runs"),
            }
            let loaded = loaded == Some(true);
            if m.env.async_done.len() < m.env.async_done.capacity() {
                m.env.async_done.push((m.slot.index, id, i32::from(loaded)));
            }
            push_int(m, id)
        }
        // ---- User interface ------------------------------------------------------------
        AttachLevelMeter => {
            let [id, group, slot, channel, generic] = ints(m);
            let c = control(m, id)?;
            let var = &m.prog.vars[m.slot.ui.controls[c].var as usize];
            if var.ui.as_deref() != Some("ui_level_meter") {
                return Err(Fault("attach_level_meter requires a ui_level_meter"));
            }
            if group < -1 || slot < -1 || !(0..16).contains(&channel) || !(-4..16).contains(&generic) {
                return Err(Fault("Invalid level meter attachment"));
            }
            // ponytail: no per-group/FX taps yet; preserve initialization and
            // report the missing connection instead of showing fabricated levels.
            m.env.note("KSP level meter attachments are unavailable");
            Ok(Step::Next)
        }
        SetControlPar => {
            let [id, p, value] = ints(m);
            if p == b::CONTROL_PAR_NONE { return Ok(Step::Next); }
            if matches!(p, b::CONTROL_PAR_TYPE | b::CONTROL_PAR_NUM_ITEMS | b::CONTROL_PAR_SELECTED_ITEM_IDX) {
                return Err(Fault("Control parameter is read-only"));
            }
            if id == b::INST_WALLPAPER_ID && p == b::CONTROL_PAR_PICTURE_STATE {
                if value < 0 { return Err(Fault("Wallpaper picture state must be non-negative")); }
                m.slot.ui.wallpaper_state = value;
                return Ok(Step::Next);
            }
            if b::instrument_control(id) {
                return Ok(Step::Next);
            }
            let c = control(m, id)?;
            if p == b::CONTROL_PAR_VALUE {
                set_value(m, c, value);
            } else if let Some(unit) = b::named("$KNOB_UNIT_", value).filter(|_| p == b::CONTROL_PAR_UNIT) {
                // The UI reads units by name.
                m.slot.ui.controls[c].set_str(p, unit).map_err(Fault)?;
            } else {
                m.slot.ui.controls[c].set_int(p, value).map_err(Fault)?;
            }
            Ok(Step::Next)
        }
        SetControlParStr => {
            let [id, p] = ints(m);
            let text = m.stk.strs.pop();
            if p == b::CONTROL_PAR_NONE { return Ok(Step::Next); }
            if p == b::CONTROL_PAR_IDENTIFIER { return Err(Fault("Control identifier is read-only")); }
            if id == b::INST_WALLPAPER_ID && p == b::CONTROL_PAR_PICTURE {
                put_text(&mut m.slot.ui.wallpaper, text, m.env.loading)?;
                return Ok(Step::Next);
            }
            if b::instrument_control(id) {
                return Ok(Step::Next);
            }
            let c = m.slot.ui.control(id).ok_or(NO_CONTROL)?;
            let var = &m.prog.vars[m.slot.ui.controls[c].var as usize];
            if p == b::CONTROL_PAR_VALUE && var.ty == Ty::Str && var.len.is_none() {
                let dst = &mut m.slot.mem.strs[var.slot as usize];
                put_variable_text(dst, text, m.env.loading)?;
            } else if p == b::CONTROL_PAR_TEXTLINE {
                let dst = m.slot.ui.controls[c].str_mut(b::CONTROL_PAR_TEXT).map_err(Fault)?;
                if !dst.is_empty() { append_text(dst, "\n", m.env.loading)?; }
                append_text(dst, text, m.env.loading)?;
            } else {
                m.slot.ui.controls[c].set_str(p, text).map_err(Fault)?;
            }
            Ok(Step::Next)
        }
        SetControlParArr => {
            let [id, p, value, index] = ints(m);
            if p == b::CONTROL_PAR_VALUE {
                let slot = control_value_slot(m, id, Some(index), Ty::Int)?;
                m.slot.mem.ints[slot] = value;
            } else if p != b::CONTROL_PAR_NONE {
                m.env.note("Indexed control metadata is unavailable");
            }
            Ok(Step::Next)
        }
        SetControlParStrArr => {
            let [id, p, index] = ints(m);
            let slot = if p == b::CONTROL_PAR_VALUE { Some(control_value_slot(m, id, Some(index), Ty::Str)?) } else { None };
            let text = m.stk.strs.pop();
            if let Some(slot) = slot {
                put_variable_text(&mut m.slot.mem.strs[slot], text, m.env.loading)?;
            } else if p != b::CONTROL_PAR_NONE {
                m.env.note("Indexed control metadata is unavailable");
            }
            Ok(Step::Next)
        }
        SetControlParReal | SetControlParRealArr => {
            let index = if f == SetControlParRealArr { Some(m.stk.int()) } else { None };
            let [id, p] = ints(m);
            let value = m.stk.real();
            if p == b::CONTROL_PAR_VALUE {
                if !value.is_finite() { return Err(Fault(super::vm::NONFINITE)); }
                let slot = control_value_slot(m, id, index, Ty::Real)?;
                m.slot.mem.reals[slot] = value;
            } else if p != b::CONTROL_PAR_NONE {
                m.env.note("Real control metadata is unavailable");
            }
            Ok(Step::Next)
        }
        GetControlParReal | GetControlParRealArr => {
            let index = if f == GetControlParRealArr { Some(m.stk.int()) } else { None };
            let [id, p] = ints(m);
            if p == b::CONTROL_PAR_VALUE {
                let slot = control_value_slot(m, id, index, Ty::Real)?;
                push_real(m, m.slot.mem.reals[slot])
            } else {
                m.env.note("Real control metadata is unavailable");
                push_real(m, 0.0)
            }
        }
        GetUiWfProperty => {
            // ponytail: one value per property, the index ignored; per-slice
            // table values if a script reads them back.
            let [p, _] = ints(m);
            let v = m.stk.var();
            let c = control_of(m, v)?;
            let v = match m.slot.ui.controls[c].get(p) {
                Some(Prop::Int(n)) => *n,
                _ => 0,
            };
            push_int(m, v)
        }
        WatchVar | WatchArrayIdx => {
            // Creator Tools' debugger only.
            if f == WatchArrayIdx {
                m.stk.int();
            }
            m.stk.var();
            Ok(Step::Next)
        }
        GetControlPar | GetControlParArr => {
            let index = if f == GetControlParArr { Some(m.stk.int()) } else { None };
            let [id, p] = ints(m);
            if p == b::CONTROL_PAR_VALUE && index.is_some() {
                let slot = control_value_slot(m, id, index, Ty::Int)?;
                return push_int(m, m.slot.mem.ints[slot]);
            }
            if p == b::CONTROL_PAR_TYPE {
                if !m.slot.ui.has_id(id) { return Err(NO_CONTROL); }
                let kind = m.slot.ui.control(id).and_then(|c| m.prog.vars[m.slot.ui.controls[c].var as usize].ui.as_deref()).unwrap_or("");
                return push_int(m, b::control_type(kind));
            }
            if id == b::INST_WALLPAPER_ID && p == b::CONTROL_PAR_PICTURE_STATE { return push_int(m, m.slot.ui.wallpaper_state); }
            if b::instrument_control(id) {
                return push_int(m, 0);
            }
            let c = control(m, id)?;
            let v = if p == b::CONTROL_PAR_NUM_ITEMS {
                m.slot.ui.controls[c].menu.len() as i32
            } else if p == b::CONTROL_PAR_SELECTED_ITEM_IDX {
                m.slot.ui.controls[c].selected_menu(m.prog, &m.slot.mem).map_or(-1, |i| i as i32)
            } else if p == b::CONTROL_PAR_VALUE {
                let var = &m.prog.vars[m.slot.ui.controls[c].var as usize];
                if var.ty == Ty::Int && var.len.is_none() {
                    m.slot.mem.ints[var.slot as usize]
                } else {
                    0
                }
            } else {
                match m.slot.ui.controls[c].get(p) {
                    Some(Prop::Int(n)) => *n,
                    Some(Prop::Str(s)) if p == b::CONTROL_PAR_UNIT => b::constant(s).unwrap_or(0),
                    _ => 0,
                }
            };
            push_int(m, v)
        }
        GetControlParStr | GetControlParStrArr => {
            let index = if f == GetControlParStrArr { Some(m.stk.int()) } else { None };
            let [id, p] = ints(m);
            if p == b::CONTROL_PAR_VALUE && index.is_some() {
                let slot = control_value_slot(m, id, index, Ty::Str)?;
                m.stk.strs.push_str(&m.slot.mem.strs[slot])?;
                return Ok(Step::Next);
            }
            if b::instrument_control(id) {
                m.stk.strs.push_str("")?;
                return Ok(Step::Next);
            }
            let c = control(m, id)?;
            let control = &m.slot.ui.controls[c];
            let var = &m.prog.vars[control.var as usize];
            let text = match control.get(p) {
                _ if p == b::CONTROL_PAR_IDENTIFIER => var.name.get(1..).unwrap_or(""),
                Some(Prop::Str(s)) => s.as_str(),
                _ if p == b::CONTROL_PAR_VALUE && var.ty == Ty::Str && var.len.is_none() => {
                    &m.slot.mem.strs[var.slot as usize]
                }
                _ => "",
            };
            m.stk.strs.push_str(text)?;
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
            if f != AddTextLine && matches!(m.slot.ui.controls[c].get(p), Some(Prop::Str(s)) if s == text) {
                return Ok(Step::Next);
            }
            let s = m.slot.ui.controls[c].str_mut(p).map_err(Fault)?;
            if f == AddTextLine {
                if !s.is_empty() {
                    append_text(s, "\n", m.env.loading)?;
                }
            } else {
                s.clear();
            }
            append_text(s, text, m.env.loading)?;
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
            match b::named("$KNOB_UNIT_", value).filter(|_| f == SetKnobUnit) {
                Some(name) => m.slot.ui.controls[c].set_str(p, name),
                None => m.slot.ui.controls[c].set_int(p, value),
            }.map_err(Fault)?;
            Ok(Step::Next)
        }
        MoveControl | MoveControlPx => {
            let [mut x, mut y] = ints(m);
            let v = m.stk.var();
            let c = control_of(m, v)?;
            if f == MoveControl {
                // Grid position 0 hides the control; moving it back shows it.
                let control = &mut m.slot.ui.controls[c];
                if x == 0 || y == 0 {
                    control.set_int(b::CONTROL_PAR_HIDE, b::HIDE_WHOLE_CONTROL).map_err(Fault)?;
                    return Ok(Step::Next);
                }
                if matches!(control.get(b::CONTROL_PAR_HIDE), Some(Prop::Int(b::HIDE_WHOLE_CONTROL))) {
                    control.set_int(b::CONTROL_PAR_HIDE, 0).map_err(Fault)?;
                }
                x = x.saturating_sub(1).saturating_mul(92).saturating_add(66);
                y = y.saturating_sub(1).saturating_mul(21).saturating_add(2);
            }
            let control = &mut m.slot.ui.controls[c];
            control.set_int(b::CONTROL_PAR_POS_X, x).map_err(Fault)?;
            control.set_int(b::CONTROL_PAR_POS_Y, y).map_err(Fault)?;
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
            let control = &mut m.slot.ui.controls[c];
            if control.menu.len() >= 4096 { return Err(Fault("Menu item limit")); }
            if m.env.loading {
                control.menu.push(MenuItem { text: text.to_owned(), value, visible: true });
            } else {
                let item = control.spare_menu.last_mut().ok_or(Fault("KSP runtime menu capacity exhausted"))?;
                put_text(&mut item.text, text, false)?;
                item.value = value;
                item.visible = true;
                let item = control.spare_menu.pop().unwrap();
                control.menu.push(item);
            }
            control.revision = control.revision.wrapping_add(1);
            snap_menu(m, c);
            Ok(Step::Next)
        }
        SetMenuItemStr => {
            let [id, index] = ints(m);
            let text = m.stk.strs.pop();
            let c = m.slot.ui.control(id).ok_or(NO_CONTROL)?;
            let control = &mut m.slot.ui.controls[c];
            if let Some(item) = usize::try_from(index)
                .ok()
                .and_then(|i| control.menu.get_mut(i))
                && item.text != text
            {
                put_text(&mut item.text, text, m.env.loading)?;
                control.revision = control.revision.wrapping_add(1);
            }
            Ok(Step::Next)
        }
        SetMenuItemVisibility | SetMenuItemValue => {
            let [id, index, value] = ints(m);
            let c = control(m, id)?;
            let control = &mut m.slot.ui.controls[c];
            if let Some(item) = usize::try_from(index)
                .ok()
                .and_then(|i| control.menu.get_mut(i))
            {
                if f == SetMenuItemValue {
                    if item.value == value { return Ok(Step::Next); }
                    item.value = value;
                } else {
                    if item.visible == (value != 0) { return Ok(Step::Next); }
                    item.visible = value != 0;
                }
                control.revision = control.revision.wrapping_add(1);
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
            m.stk.strs.push_str(text)?;
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
        SetSkinOffset => {
            let [pixels] = ints(m);
            m.slot.ui.skin_offset = pixels;
            Ok(Step::Next)
        }
        SetUiColor | SetSnapshotType | DisableLogging | FsNavigate => {
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
            put_text(&mut m.slot.ui.title, text, m.env.loading)?;
            Ok(Step::Next)
        }
        MakePerfview => {
            m.slot.ui.performance = true;
            Ok(Step::Next)
        }
        ShowLibraryTab => Ok(Step::Next),
        SetUiWfProperty => {
            let [p, _, value] = ints(m);
            let v = m.stk.var();
            let c = control_of(m, v)?;
            m.slot.ui.controls[c].set_int(p, value).map_err(Fault)?;
            Ok(Step::Next)
        }
        GetFontId => {
            let name = m.stk.strs.pop();
            // Retain the historical numeric-ID form. Named fonts are an
            // init-only resource registry, never allocated during callbacks.
            let n = if let Ok(n) = name.parse() { n } else if !m.env.loading {
                m.env.note("get_font_id: named bitmap fonts can only be registered on init; using the default font");
                0
            } else if name.is_empty() || name.contains(['/', '\\']) {
                m.env.note("get_font_id: invalid bitmap font resource name; using the default font");
                0
            } else {
                let fonts = &mut m.slot.ui.fonts;
                let index = fonts.iter().position(|font| font == name).unwrap_or_else(|| {
                    fonts.push(name.to_owned());
                    fonts.len() - 1
                });
                i32::try_from(index).ok().and_then(|index| index.checked_add(26))
                    .ok_or(Fault("Bitmap font ID space exhausted"))?
            };
            push_int(m, n)
        }
        GetFolder => {
            let [which] = ints(m);
            let folder = match (m.engine.instrument_path().filter(|_| m.env.loading), which) {
                (Some(i), b::GET_FOLDER_LIBRARY_DIR) => library_dir(i),
                (Some(i), b::GET_FOLDER_PATCH_DIR) => dir(i.parent().unwrap_or(i)),
                // No factory library, and no other folder outside on init.
                _ => String::new(),
            };
            m.stk.strs.push_str(&folder)?;
            Ok(Step::Next)
        }
        FsGetFilename => {
            ints::<2>(m);
            m.stk.strs.push()?;
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
            put_text(&mut key.name, text, m.env.loading)?;
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
            let family = if f == SetKeyColor { "$KEY_COLOR_" } else { "$NI_KEY_TYPE_" };
            let name = b::named(family, value);
            let key = m.env.host.keyboard.entry(note).or_insert_with(KeyState::default);
            if f == SetKeyPressed { key.pressed = value == 1; }
            else { key.set_symbol(f == SetKeyColor, name, value, m.env.loading)?; }
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
            m.stk.strs.push_str(name)?;
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
                    Some(Value::Text(name)) => b::constant(name).unwrap_or(0),
                    _ => 0,
                },
            };
            push_int(m, v)
        }
        SetKeyrange => {
            let [lo, hi] = ints(m);
            let (lo, hi) = (midi_note(lo)?, midi_note(hi)?);
            let (lo, hi) = (lo.min(hi), lo.max(hi));
            let name = m.stk.strs.pop();
            // Reuse an overlapping range before recycling any others. Validate
            // its text first, so a capacity fault leaves existing ranges intact.
            let overlap = m.env.host.keyranges.iter().position(|&(l, h, _)| h >= lo && l <= hi);
            if let Some(at) = overlap {
                let range = &mut m.env.host.keyranges[at];
                put_text(&mut range.2, name, m.env.loading)?;
                (range.0, range.1) = (lo, hi);
                remove_keyranges(m.env, lo, hi, Some(at));
            } else if m.env.host.keyranges.len() < MAX_KEYRANGES {
                let text = if m.env.loading {
                    name.to_owned()
                } else {
                    let text = m.env.spare_keyranges.last_mut().ok_or(Fault("Key range text capacity exhausted"))?;
                    put_text(text, name, false)?;
                    m.env.spare_keyranges.pop().unwrap()
                };
                m.env.host.keyranges.push((lo, hi, text));
            } else {
                m.env.note("set_keyrange: at most 16 key ranges; range dropped");
            }
            Ok(Step::Next)
        }
        RemoveKeyrange => {
            let [note] = ints(m);
            let note = midi_note(note)?;
            remove_keyranges(m.env, note, note, None);
            Ok(Step::Next)
        }
        GetKeyrangeMinNote | GetKeyrangeMaxNote | GetKeyrangeName => {
            let [note] = ints(m);
            let range = m
                .env
                .host
                .keyranges
                .iter()
                .find(|&&(l, h, _)| (i32::from(l)..=i32::from(h)).contains(&note));
            match (f, range) {
                (GetKeyrangeName, r) => {
                    let name = r.map_or("", |r| r.2.as_str());
                    m.stk.strs.push_str(name)?;
                    Ok(Step::Next)
                }
                (_, None) => Err(Fault("No key range at this note")),
                (GetKeyrangeMinNote, Some(r)) => push_int(m, i32::from(r.0)),
                (_, Some(r)) => push_int(m, i32::from(r.1)),
            }
        }
        AttachZone => {
            let [zone, flags] = ints(m);
            let v = m.stk.var();
            let c = control_of(m, v)?;
            let c = &mut m.slot.ui.controls[c];
            c.set_int(b::ATTACHED_ZONE, zone).map_err(Fault)?;
            c.set_int(b::UI_WF_PROP_FLAGS, flags).map_err(Fault)?;
            Ok(Step::Next)
        }
        // ---- Diagnostics -------------------------------------------------------------
        Message => {
            let text = m.stk.strs.pop();
            put_text(&mut m.env.message, text, m.env.loading)?;
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
                write_value_rt(&mut m.slot.mem, var, value, m.env.loading)?;
            }
            if let Some(c) = m.slot.ui.control_of(v) {
                snap_menu(m, c);
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
                None if m.env.loading => m.env.host.pgs_ints.push((name.to_string(), vec![0; size as usize])),
                None => {
                    let at = m.env.spare_pgs_ints.iter().position(|(k, _)| k == &**name)
                        .ok_or(Fault("KSP PGS storage was not prepared"))?;
                    let mut key = m.env.spare_pgs_ints.swap_remove(at);
                    key.1.truncate(size as usize);
                    m.env.host.pgs_ints.push(key);
                }
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
                if m.env.loading { m.env.host.pgs_strs.push((name.to_string(), String::new())); }
                else {
                    let at = m.env.spare_pgs_strs.iter().position(|(k, _)| k == &**name)
                        .ok_or(Fault("KSP PGS string storage was not prepared"))?;
                    let key = m.env.spare_pgs_strs.swap_remove(at);
                    m.env.host.pgs_strs.push(key);
                }
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
            put_text(dst, text, m.env.loading)?;
            m.env.pgs_changed = true;
            Ok(Step::Next)
        }
        PgsGetStrKeyVal => {
            let key = m.stk.var();
            let i = pgs_str_key(m, key).ok_or(NO_PGS_KEY)?;
            let (strs, host) = (&mut m.stk.strs, &m.env.host);
            strs.push_str(&host.pgs_strs[i].1)?;
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

/// A menu shows one of its items: a value that is none of theirs (a fresh
/// menu's 0, a saved value from an older version) becomes the first item's,
/// as Kontakt selects it. Scripts index arrays by it in `on init`.
fn snap_menu(m: &mut Machine, c: usize) {
    m.slot.ui.controls[c].snap_menu(m.prog, &mut m.slot.mem);
}

fn set_value(m: &mut Machine, c: usize, value: i32) {
    let var = &m.prog.vars[m.slot.ui.controls[c].var as usize];
    if var.ty == Ty::Int && var.len.is_none() {
        m.slot.mem.ints[var.slot as usize] = value;
    }
    snap_menu(m, c);
}

fn control_value_slot(m: &Machine, id: i32, index: Option<i32>, ty: Ty) -> Exec<usize> {
    let c = control(m, id)?;
    let var = &m.prog.vars[m.slot.ui.controls[c].var as usize];
    if var.ty != ty || var.len.is_some() != index.is_some() {
        return Err(Fault("Control value type or array access mismatch"));
    }
    let i = usize::try_from(index.unwrap_or(0)).ok()
        .filter(|&i| i < var.len.unwrap_or(1) as usize)
        .ok_or(Fault("Control value index out of bounds"))?;
    Ok(var.slot as usize + i)
}

fn sort<T>(items: &mut [T], descending: bool, cmp: impl Fn(&T, &T) -> std::cmp::Ordering) {
    if descending {
        items.sort_unstable_by(|a, b| cmp(b, a));
    } else {
        items.sort_unstable_by(cmp);
    }
}
