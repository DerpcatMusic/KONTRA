//! A compiled KSP script as the engine's [`ScriptPlan`].

use kontra_engine::{Callbacks, Controls, EnginePars, ScriptPlan, Text};
use kontra_ksp::{CallbackKind, Cells, FIRST_UI_ID, SYMBOL_BASE, Script};

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
        },
        coroutines: COROUTINES,
        controls: controls(script),
        engine_pars: EnginePars {
            attack: symbol(script, "$ENGINE_PAR_ATTACK"),
            hold: symbol(script, "$ENGINE_PAR_HOLD"),
            decay: symbol(script, "$ENGINE_PAR_DECAY"),
            sustain: symbol(script, "$ENGINE_PAR_SUSTAIN"),
            release: symbol(script, "$ENGINE_PAR_RELEASE"),
            intensity: symbol(script, "$ENGINE_PAR_MOD_TARGET_INTENSITY"),
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

/// Control ids, the value parameter's symbol and each control's value cell.
fn controls(script: &Script) -> Controls {
    let value_par = symbol(script, "$CONTROL_PAR_VALUE");
    let cells = script
        .controls
        .iter()
        .map(|c| match script.variables.get(c.variable as usize)?.cells {
            Cells::Int { base, .. } | Cells::Real { base, .. } => Some(base),
            _ => None,
        })
        .collect();
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
