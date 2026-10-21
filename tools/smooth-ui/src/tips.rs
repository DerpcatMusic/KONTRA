//! What each control says about itself, on hover.
//!
//! Keyed by the control's id, so a part asks `tips::of(id)` and a control
//! with no entry is simply silent. The words are the reference's own,
//! adjusted for the product's name.

/// Why a band's depth is greyed out.
pub const NO_DEPTH: &str = "Depth control not available for selected band shape";

use std::sync::atomic::{AtomicBool, Ordering};

/// The "Show tooltips" preference: global, as the reference's is.
static ENABLED: AtomicBool = AtomicBool::new(true);

pub fn enable(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// The tooltip for a control, if it has one and tooltips are on.
/// Why the quality choice cannot be changed while low latency holds.
pub const QUALITY_DISABLED: &str = "Quality settings are disabled in low latency mode.";

pub fn of(id: &str) -> Option<&'static str> {
    if !ENABLED.load(Ordering::Relaxed) {
        return None;
    }
    Some(match id {
        "gear" => "Adjust plug-in settings.",
        "help" => "Help and other helpful resources.",
        "help-copy" => "Copy the version and system details to the clipboard, for a support request.",
        "pref-size" => "Adjust the display size of the plug-in interface.",
        "pref-ultra" => "\"Always render on ultra\" disables \"Low latency for all instances\".",
        "pref-latency" => "\"Low latency for all instances\" disables \"Always render on ultra\".",
        "mode" => "Select the processing mode.\nsoft: adaptive threshold, reacts to relative levels of resonances.\nhard: fixed threshold, reacts to absolute levels of resonances.",
        "depth" => "Set the amount of processing. Higher values result in bigger cuts.",
        "detail" => "Control how detailed SMOOTH's cuts are. Higher detail results in narrower cuts.",
        "attack" => "Adjust how quickly the processing starts to react to resonances.",
        "release" => "Adjust how quickly the notches disappear.",
        "mix" => "Adjust the dry/wet mix of the output signal.\n0% for fully dry signal, 100% for fully processed signal.",
        "out" => "Adjust the output gain level.",
        "max-cut" => "Set the maximum reduction SMOOTH is allowed to make in dB.",
        "wet-trim" => "Adjust the level of the processed signal before the mix. The output gain comes after it.",
        "bypass" => "Bypass the processing.",
        "delta" => "Monitor the difference between wet and dry signals.",
        "quality" => "Set the quality of processing and choose between minimum phase (default) and linear phase processing. Higher quality settings increase CPU load while linear phase processing increases latency.",
        "lowlatency" => "Toggle the low latency mode on/off. The latency drops to 0-1 ms depending on sample rate.",
        "undo" => "Undo last action.",
        "redo" => "Redo last undone action.",
        "ab-switch" => "Switch to compare between two parameter sets.",
        "ab-copy" => "Replace the other parameter set with a copy of current settings.",
        "preset-bar" | "preset-title" => "Currently selected preset. Click to open/close the preset menu.",
        "preset-prev" => "Load the previous preset.",
        "preset-next" => "Load the next preset.",
        "preset-heart" => "Mark or unmark this preset as a Favorite.",
        "preset-default" => "Set this preset as the default for new SMOOTH instances.",
        "preset-filter" => "Filter the preset list: all, factory, user, or favourite presets.",
        "preset-save" => "Save current parameters as a new preset.",
        "preset-delete" => "Delete the current preset. This cannot be undone.",
        "preset-search" => "Search for presets. The search terms are matched against names and categories.",
        "stereo-mode" => "Toggle the stereo processing mode between left/right and mid/side.",
        "link" => "Adjust whether the channels are analyzed separately (0 %) or as a sum (100 %).",
        "focus" => "Adjust the relative amount of processing between the channels.",
        "detail-low" => "Gradually scale the detail setting for frequencies under 500Hz.",
        "detail-high" => "Gradually scale the detail setting for frequencies above 2kHz.",
        "attack-low" => "Gradually scale the attack setting for frequencies under 500Hz.",
        "attack-high" => "Gradually scale the attack setting for frequencies above 2kHz.",
        "release-low" => "Gradually scale the release setting for frequencies under 500Hz.",
        "release-high" => "Gradually scale the release setting for frequencies above 2kHz.",
        "band-shape" => "Select the shape of the selected band.",
        "band-frequency" => "Set the center frequency of the selected band.",
        "band-depth" => "Set the amount of boost or cut to the depth curve in the area of the selected band.",
        "band-width" => "Adjust the bandwidth of the selected band.",
        "band-slope" => "Adjust the slope of the selected band.",
        "band-balance" => "Set the stereo balance of the selected band.\nL/R or M/S, depending on the main stereo mode.",
        "band-bypass" => "Enable or bypass the selected band.\nBypassing a band removes its effect on the Depth curve.",
        "band-delete" => "Delete the selected band.",
        "rail-stereo" | "rail-tilt" | "rail-scale" => "Closed side panel. Click to open.",
        "lock-quality" => "Lock quality so that it doesn't change when switching presets.",
        "lock-latency" => "Lock low latency so that it doesn't change when switching presets.",
        "range" => "Adjust the display range of the reduction graph.",
        "side-panel" => "Hide the side panel by double clicking on its background.",
        _ => return None,
    })
}
