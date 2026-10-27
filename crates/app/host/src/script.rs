//! A compiled KSP script as the engine's [`ScriptPlan`].

use kontra_engine::{Callbacks, ScriptPlan};
use kontra_ksp::{CallbackKind, Script};

/// Callbacks a KSP instrument may have running or waiting at once.
const COROUTINES: usize = 64;

/// The engine's view of `script`. UI metadata stays with the caller.
#[must_use]
pub fn plan(script: &Script) -> ScriptPlan {
    let entry = |kind| script.callback(kind, None);
    ScriptPlan {
        program: script.program.clone(),
        callbacks: Callbacks {
            init: entry(CallbackKind::Init),
            note: entry(CallbackKind::Note),
            release: entry(CallbackKind::Release),
            controller: entry(CallbackKind::Controller),
        },
        coroutines: COROUTINES,
    }
}
