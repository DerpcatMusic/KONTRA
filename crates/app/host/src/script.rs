//! A compiled KSP script as the engine's [`ScriptPlan`].

use kontra_engine::{Callbacks, Controls, EnginePars, ScriptPlan, Text};
use kontra_ksp::{CallbackKind, Cells, FIRST_UI_ID, SYMBOL_BASE, Script};

/// The catalog names the engine understands (every other one is an opaque
/// symbol it cannot act on), for coverage reports.
pub const UNDERSTOOD_SYMBOLS: &[&str] = &[
    ATTACK,
    HOLD,
    DECAY,
    SUSTAIN,
    RELEASE,
    INTENSITY,
    ATTACK_CURVE,
    EFFECT_BYPASS,
    EFFECT_OUTPUT,
    SEND_BYPASS,
    SEND_OUTPUT,
    SEND_DRY,
    GAINER_GAIN,
    STEREO_WIDTH,
    STEREO_PAN,
    VOLUME,
    PAN,
    TUNE,
    CONTROL_VALUE,
];
const ATTACK: &str = "$ENGINE_PAR_ATTACK";
const HOLD: &str = "$ENGINE_PAR_HOLD";
const DECAY: &str = "$ENGINE_PAR_DECAY";
const SUSTAIN: &str = "$ENGINE_PAR_SUSTAIN";
const RELEASE: &str = "$ENGINE_PAR_RELEASE";
const INTENSITY: &str = "$ENGINE_PAR_MOD_TARGET_INTENSITY";
const MOD_BYPASS: &str = "$ENGINE_PAR_INTMOD_BYPASS";
const ATTACK_CURVE: &str = "$ENGINE_PAR_ATK_CURVE";
const EFFECT_BYPASS: &str = "$ENGINE_PAR_EFFECT_BYPASS";
const EFFECT_OUTPUT: &str = "$ENGINE_PAR_INSERT_EFFECT_OUTPUT_GAIN";
const SEND_BYPASS: &str = "$ENGINE_PAR_SEND_EFFECT_BYPASS";
const SEND_OUTPUT: &str = "$ENGINE_PAR_SEND_EFFECT_OUTPUT_GAIN";
const SEND_DRY: &str = "$ENGINE_PAR_SEND_EFFECT_DRY_LEVEL";
const GAINER_GAIN: &str = "$ENGINE_PAR_GN_GAIN";
const STEREO_WIDTH: &str = "$ENGINE_PAR_STEREO";
const STEREO_PAN: &str = "$ENGINE_PAR_STEREO_PAN";
const VOLUME: &str = "$ENGINE_PAR_VOLUME";
const PAN: &str = "$ENGINE_PAR_PAN";
const TUNE: &str = "$ENGINE_PAR_TUNE";
const CONTROL_VALUE: &str = "$CONTROL_PAR_VALUE";

/// Callbacks a KSP instrument may have running or waiting at once.
const COROUTINES: usize = 64;

/// The engine's view of `script` with `saved` persistent values restored and
/// the library `data` files it loads. UI metadata stays with the caller.
#[must_use]
pub fn plan(script: &Script, saved: &[String], data: &[(String, Vec<u8>)]) -> ScriptPlan {
    let entry = |kind| script.callback(kind, None);
    let restore = script.restore(saved);
    ScriptPlan {
        program: script.program.clone(),
        callbacks: Callbacks {
            init: entry(CallbackKind::Init),
            note: entry(CallbackKind::Note),
            release: entry(CallbackKind::Release),
            controller: entry(CallbackKind::Controller),
            persistence_changed: entry(CallbackKind::PersistenceChanged),
            listener: entry(CallbackKind::Listener),
            pgs_changed: entry(CallbackKind::PgsChanged),
        },
        coroutines: COROUTINES,
        controls: controls(script),
        engine_pars: EnginePars {
            attack: symbol(script, ATTACK),
            hold: symbol(script, HOLD),
            decay: symbol(script, DECAY),
            sustain: symbol(script, SUSTAIN),
            release: symbol(script, RELEASE),
            intensity: symbol(script, INTENSITY),
            mod_bypass: symbol(script, MOD_BYPASS),
            attack_curve: symbol(script, ATTACK_CURVE),
            effect_bypass: symbol(script, EFFECT_BYPASS),
            effect_output: symbol(script, EFFECT_OUTPUT),
            send_bypass: symbol(script, SEND_BYPASS),
            send_output: symbol(script, SEND_OUTPUT),
            send_dry: symbol(script, SEND_DRY),
            gainer_gain: symbol(script, GAINER_GAIN),
            stereo_width: symbol(script, STEREO_WIDTH),
            stereo_pan: symbol(script, STEREO_PAN),
            volume: symbol(script, VOLUME),
            pan: symbol(script, PAN),
            tune: symbol(script, TUNE),
        },
        data: data
            .iter()
            .filter_map(|(name, bytes)| data_array(script, name, bytes))
            .collect(),
        restore: restore.ints,
        restore_texts: restore
            .texts
            .iter()
            .map(|(cell, text)| (*cell, Text::from_str_cut(text).0))
            .collect(),
    }
}

/// A data file (`.nka`: a name line, then one value per line) as the cells of
/// the number array it names. A short file fills a prefix; extra lines are
/// ignored, as in Kontakt.
fn data_array(script: &Script, name: &str, bytes: &[u8]) -> Option<(u32, Vec<i64>)> {
    let var = script.variables.iter().find(|v| {
        v.name
            .get(1..)
            .is_some_and(|n| n.eq_ignore_ascii_case(name))
    })?;
    let (base, len, real) = match var.cells {
        Cells::Int { base, len } => (base, len, false),
        Cells::Real { base, len } => (base, len, true),
        _ => return None,
    };
    let text = String::from_utf8_lossy(bytes);
    let values = text
        .lines()
        .skip(1)
        .take(len as usize)
        .map_while(|line| {
            let line = line.trim();
            if real {
                line.parse::<f64>().ok().map(|v| v.to_bits() as i64)
            } else {
                line.parse::<i32>().ok().map(i64::from)
            }
        })
        .collect();
    Some((base, values))
}

/// Control ids, the value parameter's symbol and, by variable (a UI id
/// counts every variable), each control's value cell.
fn controls(script: &Script) -> Controls {
    let value_par = symbol(script, CONTROL_VALUE);
    let mut cells = vec![None; script.variables.len()];
    for c in &script.controls {
        let value = script
            .variables
            .get(c.variable as usize)
            .and_then(|v| match v.cells {
                Cells::Int { base, .. } | Cells::Real { base, .. } => Some(base),
                _ => None,
            });
        if let Some(cell) = cells.get_mut(c.variable as usize) {
            *cell = value;
        }
    }
    Controls {
        first: FIRST_UI_ID,
        value_par,
        cells,
    }
}

/// The value a catalog constant has in `script`; `i32::MIN` (matches nothing)
/// when the script never names it.
fn symbol(script: &Script, name: &str) -> i32 {
    script
        .symbols
        .iter()
        .position(|s| s.eq_ignore_ascii_case(name))
        .and_then(|n| i32::try_from(n).ok())
        .map_or(i32::MIN, |n| SYMBOL_BASE + n)
}
