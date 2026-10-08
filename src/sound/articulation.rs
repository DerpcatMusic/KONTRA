//! User input/display preferences. Source articulations and zone references stay immutable.
use sampler_ir as ir;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Overlay {
    pub order: Vec<String>,
    pub inputs: BTreeMap<String, Inputs>,
    /// Replaced/cleared original keys otherwise remain silent control keys.
    pub keep_originals: bool,
    pub driver: Option<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Inputs {
    /// v1 participation: original keys remain available in alternative modes.
    pub enabled: Option<bool>,
    /// None inherits; an empty list explicitly clears the keys.
    pub keys: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "present")]
    pub channel: Option<Option<u8>>,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "present")]
    pub velocity: Option<Option<(u8, u8)>>,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "present")]
    pub controller: Option<Option<(u8, u8, u8)>>,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "present")]
    pub program: Option<Option<u8>>,
    pub color: Option<[u8; 3]>,
}

// A missing field inherits; explicit JSON null clears the input. Standard
// Option<Option<T>> decoding collapses these states, so retain field presence.
fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Keys(Vec<u8>),
    Velocity(Option<(u8, u8)>),
    Channel(Option<u8>),
    Controller(Option<(u8, u8, u8)>),
    Program(Option<u8>),
}

/// Stable source IDs, with a deterministic occurrence suffix for identical legacy records.
/// The suffix distinguishes axes; equal names never merge rows.
pub fn identities(arts: &[ir::Articulation]) -> Vec<String> {
    let mut seen = BTreeMap::<String, usize>::new();
    arts.iter().map(|a| {
        let base = if a.source.is_empty() { format!("legacy:{}:{:?}", a.name, a.switch_keys) } else { a.source.clone() };
        let rank = seen.entry(base.clone()).or_default();
        let id = format!("{base}#{rank}");
        *rank += 1;
        id
    }).collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Routing {
    pub keys: Vec<sampler_core::Keyswitch>,
    pub switching: sampler_core::Switching,
}

pub fn driver(value: u8) -> ir::Driver {
    [ir::Driver::Keys, ir::Driver::Velocity, ir::Driver::Channel, ir::Driver::Controller, ir::Driver::Program].get(value as usize).copied().unwrap_or_default()
}

impl Overlay {
    pub fn valid(&self) -> bool {
        self.driver.is_none_or(|d| d <= 4) && self.inputs.values().all(|i| {
            i.keys.as_ref().is_none_or(|keys| keys.iter().enumerate().all(|(n, k)| *k <= 127 && !keys[..n].contains(k)))
            && i.channel.flatten().is_none_or(|c| c <= 15)
            && i.velocity.flatten().is_none_or(|(lo, hi)| lo > 0 && lo <= hi && hi <= 127)
            && i.controller.flatten().is_none_or(|(cc, lo, hi)| cc <= 119 && lo <= hi && hi <= 127)
            && i.program.flatten().is_none_or(|p| p <= 127)
        })
    }

    pub fn display_order(&self, arts: &[ir::Articulation]) -> Vec<usize> {
        let ids = identities(arts);
        let mut order = Vec::with_capacity(ids.len());
        for id in &self.order {
            if let Some(n) = ids.iter().position(|i| i == id) && !order.contains(&n) { order.push(n); }
        }
        order.extend((0..ids.len()).filter(|n| !self.order.iter().any(|i| i == &ids[*n])));
        order
    }

    pub fn move_to(&mut self, arts: &[ir::Articulation], from: &str, to: usize) {
        let ids = identities(arts);
        let mut order: Vec<_> = self.display_order(arts).into_iter().map(|n| ids[n].clone()).collect();
        if let Some(at) = order.iter().position(|id| id == from) {
            let id = order.remove(at);
            order.insert(to.min(order.len()), id);
            self.order = order;
        }
    }

    pub fn input(&self, id: &str, a: &ir::Articulation, driver: ir::Driver) -> Input {
        let custom = self.inputs.get(id).cloned().unwrap_or_default();
        let alt = a.alternatives;
        match driver {
            ir::Driver::Keys => Input::Keys(custom.keys.unwrap_or_else(|| a.switch_keys.clone())),
            ir::Driver::Velocity => Input::Velocity(if custom.enabled == Some(false) { None } else { custom.velocity.unwrap_or(alt.velocities.map(|v| (v.low, v.high))) }),
            ir::Driver::Channel => Input::Channel(if custom.enabled == Some(false) { None } else { custom.channel.unwrap_or(alt.channel) }),
            ir::Driver::Controller => Input::Controller(custom.controller.unwrap_or(alt.controller.map(|c| (c.controller, c.low, c.high)))),
            ir::Driver::Program => Input::Program(custom.program.unwrap_or(alt.program)),
        }
    }

    /// Port from v1 0cb7a8a0:src/articulate.rs; use stable IDs and display order.
    pub fn split_velocities(&mut self, arts: &[ir::Articulation]) -> bool {
        let ids = identities(arts);
        let enabled: Vec<_> = self.display_order(arts).into_iter().filter(|&n| self.inputs.get(&ids[n]).is_none_or(|a| a.enabled != Some(false))).collect();
        if enabled.len() > 127 { return false; }
        let count = enabled.len().max(1);
        for (n, a) in enabled.into_iter().enumerate() {
            let low = (1 + n * 127 / count) as u8;
            let high = ((n + 1) * 127 / count) as u8;
            self.set(&ids[a], Input::Velocity(Some((low, high))));
        }
        true
    }

    pub fn set(&mut self, id: &str, input: Input) {
        let to = self.inputs.entry(id.into()).or_default();
        match input {
            Input::Keys(keys) => to.keys = Some(keys),
            Input::Velocity(v) => to.velocity = Some(v),
            Input::Channel(c) => to.channel = Some(c),
            Input::Controller(c) => to.controller = Some(c),
            Input::Program(p) => to.program = Some(p),
        }
    }

    pub fn conflicts(&self, arts: &[ir::Articulation], id: &str, mode: ir::Driver, input: &Input) -> Vec<usize> {
        identities(arts).iter().enumerate().filter_map(|(n, other)| {
            (other != id && overlaps(input, &self.input(other, &arts[n], mode))).then_some(n)
        }).collect()
    }

    /// Build only the routing table, with the immutable source's numbering/actions.
    pub fn routing(&self, inst: &ir::Instrument, legacy: u8) -> Result<(Vec<sampler_core::Keyswitch>, sampler_core::Switching), sampler_core::lower::LowerError> {
        if !self.valid() {
            return Err(sampler_core::lower::LowerError::Core { stage: sampler_core::lower::Stage::Articulations, owner: "user inputs".into(), error: sampler_core::Error::InvalidInput });
        }
        let ids = identities(&inst.articulations);
        // Saved preferences cross the same validation boundary as typed edits.
        // Inherited source axes may overlap; a user assignment must be unambiguous.
        for (n, id) in ids.iter().enumerate() {
            if let Some(custom) = self.inputs.get(id) {
                for (mode, edited) in [(ir::Driver::Keys, custom.keys.is_some()), (ir::Driver::Velocity, custom.velocity.is_some()), (ir::Driver::Channel, custom.channel.is_some()), (ir::Driver::Controller, custom.controller.is_some()), (ir::Driver::Program, custom.program.is_some())] {
                    if edited && !self.conflicts(&inst.articulations, id, mode, &self.input(id, &inst.articulations[n], mode)).is_empty() {
                        return Err(sampler_core::lower::LowerError::Core { stage: sampler_core::lower::Stage::Articulations, owner: "conflicting user inputs".into(), error: sampler_core::Error::InvalidInput });
                    }
                }
            }
        }
        let mut routed = ir::Instrument { articulations: inst.articulations.clone(), switching: inst.switching, ..Default::default() };
        for (a, id) in routed.articulations.iter_mut().zip(&ids) {
            if let Some(custom) = self.inputs.get(id) {
                if let Some(v) = custom.velocity { a.alternatives.velocities = v.map(|(low, high)| ir::VelocityRange { low, high }); }
                if let Some(v) = custom.channel { a.alternatives.channel = v; }
                if let Some(v) = custom.controller { a.alternatives.controller = v.map(|(controller, low, high)| ir::ControllerRange { controller, low, high }); }
                if let Some(v) = custom.program { a.alternatives.program = v; }
                if custom.enabled == Some(false) { a.alternatives.channel = None; a.alternatives.velocities = None; }
            }
        }
        let mode = self.driver.map(driver).unwrap_or_else(|| if legacy & 0x80 != 0 { driver(legacy >> 1 & 7) } else { inst.switching.driver });
        let switching = ir::Switching { driver: mode, ..inst.switching };
        let (keys, mut table) = sampler_core::lower::switching(&routed, switching)?;
        let default = inst.articulations.iter().position(|a| a.default).unwrap_or(0);
        let mut inputs = Vec::new();
        let mut blocked = 0u128;
        for (n, (a, id)) in inst.articulations.iter().zip(ids).enumerate() {
            let Input::Keys(effective) = self.input(&id, a, ir::Driver::Keys) else { unreachable!() };
            let changed = effective != a.switch_keys;
            if changed && !self.keep_originals {
                for &key in &a.switch_keys { blocked |= 1u128 << key; }
            }
            let plan_id = if n == default { 0 } else if n < default { n as u32 + 1 } else { n as u32 };
            let switch = if inst.switching.owner == ir::SwitchOwner::Native {
                Some(sampler_core::Switch::Articulation(plan_id))
            } else if let Some(&key) = a.switch_keys.first() {
                Some(sampler_core::Switch::Tap(key))
            } else {
                a.control.map(|id| sampler_core::Switch::Control { id, articulation: plan_id })
            };
            if mode == ir::Driver::Keys || inst.switching.keys == ir::SwitchKeys::Keep {
                for key in effective.into_iter().filter(|_| changed || a.switch_keys.is_empty()) {
                    if let Some(switch) = switch { inputs.push((key, switch)); }
                }
            }
        }
        // Explicit user inputs win over suppressed originals, including a swap.
        for &(key, _) in &inputs {
            if key > 127 { return Err(sampler_core::lower::LowerError::Core { stage: sampler_core::lower::Stage::Articulations, owner: "user inputs".into(), error: sampler_core::Error::InvalidInput }); }
            blocked &= !(1u128 << key);
        }
        table.set_key_inputs(inputs, blocked).map_err(|e| sampler_core::lower::LowerError::Core { stage: sampler_core::lower::Stage::Articulations, owner: "user inputs".into(), error: e })?;
        Ok((keys, table))
    }
}

fn overlaps(a: &Input, b: &Input) -> bool {
    let ranges = |a: (u8, u8), b: (u8, u8)| a.0 <= b.1 && b.0 <= a.1;
    match (a, b) {
        (Input::Keys(a), Input::Keys(b)) => a.iter().any(|k| b.contains(k)),
        (Input::Channel(Some(a)), Input::Channel(Some(b))) | (Input::Program(Some(a)), Input::Program(Some(b))) => a == b,
        (Input::Velocity(Some(a)), Input::Velocity(Some(b))) => ranges(*a, *b),
        (Input::Controller(Some((ac, al, ah))), Input::Controller(Some((bc, bl, bh)))) => ac == bc && ranges((*al, *ah), (*bl, *bh)),
        _ => false,
    }
}

/// Kontakt octave convention, strict bounds and accidentals (including B# / Cb).
pub fn parse_key(text: &str) -> Option<u8> {
    let text = text.trim();
    if let Ok(n) = text.parse::<u8>() { return (n < 128).then_some(n); }
    ir::parse_note(text)
}

pub fn parse_input(mode: ir::Driver, text: &str) -> Result<Input, &'static str> {
    let text = text.trim();
    let clear = matches!(text.to_ascii_lowercase().as_str(), "off" | "none" | "—");
    let number = |text: &str, max: u8| text.trim().parse::<u8>().ok().filter(|n| *n <= max);
    let range = |text: &str| -> Option<(u8, u8)> {
        let parts: Vec<_> = text.split(['-', '–', ' ']).filter(|s| !s.is_empty()).collect();
        let low = number(parts.first()?, 127)?;
        let high = match parts.as_slice() { [_] => low, [_, h] => number(h, 127)?, _ => return None };
        (low <= high).then_some((low, high))
    };
    match mode {
        ir::Driver::Keys if clear => Ok(Input::Keys(Vec::new())),
        ir::Driver::Keys => text.split(',').map(|t| parse_key(t).ok_or("Use C#2, Db2 or MIDI 0–127")).collect::<Result<Vec<_>, _>>().and_then(|keys| {
            let mut unique = keys.clone(); unique.sort_unstable(); unique.dedup();
            (unique.len() == keys.len()).then_some(Input::Keys(keys)).ok_or("A key may appear only once")
        }),
        ir::Driver::Channel if clear => Ok(Input::Channel(None)),
        ir::Driver::Channel => number(text.trim_start_matches("ch").trim(), 16).filter(|n| *n > 0).map(|n| Input::Channel(Some(n - 1))).ok_or("Use channel 1–16"),
        ir::Driver::Velocity if clear => Ok(Input::Velocity(None)),
        ir::Driver::Velocity => range(text).filter(|v| v.0 > 0).map(|v| Input::Velocity(Some(v))).ok_or("Use an ordered range within 1–127"),
        ir::Driver::Program if clear => Ok(Input::Program(None)),
        ir::Driver::Program => number(text.trim_start_matches("prog").trim(), 128).filter(|n| *n > 0).map(|n| Input::Program(Some(n - 1))).ok_or("Use program 1–128"),
        ir::Driver::Controller if clear => Ok(Input::Controller(None)),
        ir::Driver::Controller => {
            let text = text.trim_start_matches("CC").trim_start_matches("cc");
            let (cc, values) = text.split_once(' ').ok_or("Use CC32 3 or CC32 3–10")?;
            let controller = number(cc, 119).ok_or("Use CC 0–119; 120–127 are channel commands")?;
            let (low, high) = range(values).ok_or("Use values 0–127")?;
            Ok(Input::Controller(Some((controller, low, high))))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keyswitch_typed_entry_is_strict() {
        for (text, key) in [("60", 60), ("C3", 60), ("C+3", 60), ("c#2", 49), ("Db2", 49), ("C-2", 0), ("G8", 127), ("B#2", 60), ("Cb3", 59)] { assert_eq!(parse_key(text), Some(key), "{text}"); }
        for text in ["128", "-1", "C-3", "G#8", "H2", "C", "C2x", "C##2", "999999"] { assert_eq!(parse_key(text), None, "{text}"); }
        assert!(parse_input(ir::Driver::Velocity, "128-129").is_err());
        assert!(parse_input(ir::Driver::Velocity, "64-20").is_err());
        assert!(parse_input(ir::Driver::Channel, "17").is_err());
        assert_eq!(parse_input(ir::Driver::Controller, "CC32 3–10"), Ok(Input::Controller(Some((32, 3, 10)))));
    }
    #[test]
    fn keyswitch_reorder_preserves_identity_source_and_triggers() {
        let mut inst = ir::Instrument::default();
        inst.articulations = [24, 25, 26].map(|key| ir::Articulation { name: "same axis label".into(), switch_keys: vec![key], default: key == 25, ..Default::default() }).into();
        inst.assign_alternatives(32);
        let source = inst.clone();
        let ids = identities(&inst.articulations);
        let mut overlay = Overlay::default();
        overlay.set(&ids[0], Input::Keys(vec![49]));
        let inputs = overlay.inputs.clone();
        overlay.move_to(&inst.articulations, &ids[0], 2);
        assert_eq!(overlay.display_order(&inst.articulations), vec![1, 2, 0]);
        assert_eq!(overlay.inputs, inputs);
        assert_eq!(inst, source);
        let restored: Overlay = serde_json::from_str(&serde_json::to_string(&overlay).unwrap()).unwrap();
        assert_eq!(restored, overlay);
        assert_eq!(restored.conflicts(&inst.articulations, &ids[1], ir::Driver::Keys, &Input::Keys(vec![49])), vec![0]);
    }
}

#[cfg(all(test, feature = "plugin"))]
mod persistence_tests {
    use super::*;
    use moose::core::custom_state::{StateCursor, StateField};
    #[test]
    fn keyswitch_plugin_state_round_trip_retains_order_inputs_policy_and_driver() {
        let mut part = crate::plugin::Part::default();
        part.articulation_overlay.order = vec!["script:1:legato".into(), "script:2:legato".into()];
        part.articulation_overlay.set("script:1:legato", Input::Keys(vec![49, 60]));
        part.articulation_overlay.set("script:2:legato", Input::Channel(Some(7)));
        part.articulation_overlay.set("script:1:legato", Input::Velocity(None));
        part.articulation_overlay.set("script:1:legato", Input::Controller(None));
        part.articulation_overlay.set("script:2:legato", Input::Program(None));
        part.articulation_overlay.keep_originals = true;
        part.articulation_overlay.driver = Some(2);
        let mut bytes = Vec::new();
        part.write_field(&mut bytes);
        let restored = crate::plugin::Part::read_field(&mut StateCursor::new(&bytes)).unwrap();
        assert!(restored == part);
        let json = serde_json::to_string(&part).unwrap();
        let restored: crate::plugin::Part = serde_json::from_str(&json).unwrap();
        assert!(restored == part);
        let legacy: crate::plugin::Part = serde_json::from_str("{}").unwrap();
        assert_eq!(legacy.articulation_overlay, Overlay::default());
    }
}

#[cfg(test)]
mod v1_parity_tests {
    use super::*;
    #[test]
    fn v1_excluded_articulation_keeps_source_keys_but_leaves_alternative_modes() {
        let art = ir::Articulation { source: "native:legato".into(), switch_keys: vec![24],
            alternatives: ir::Alternatives { channel: Some(1), velocities: Some(ir::VelocityRange { low: 1, high: 127 }), ..Default::default() }, ..Default::default() };
        let id = &identities(std::slice::from_ref(&art))[0];
        let overlay: Overlay = serde_json::from_value(serde_json::json!({"inputs": {id: {"enabled": false}}})).unwrap();
        assert_eq!(overlay.input(id, &art, ir::Driver::Keys), Input::Keys(vec![24]));
        assert_eq!(overlay.input(id, &art, ir::Driver::Channel), Input::Channel(None));
        assert_eq!(overlay.input(id, &art, ir::Driver::Velocity), Input::Velocity(None));
    }
    #[test]
    fn v1_velocity_split_uses_only_participating_rows_in_display_order() {
        let arts = [24, 25, 26].map(|key| ir::Articulation { switch_keys: vec![key], ..Default::default() });
        let ids = identities(&arts);
        let mut overlay = Overlay::default();
        overlay.inputs.entry(ids[1].clone()).or_default().enabled = Some(false);
        overlay.move_to(&arts, &ids[2], 0);
        assert!(overlay.split_velocities(&arts));
        assert_eq!(overlay.input(&ids[2], &arts[2], ir::Driver::Velocity), Input::Velocity(Some((1, 63))));
        assert_eq!(overlay.input(&ids[0], &arts[0], ir::Driver::Velocity), Input::Velocity(Some((64, 127))));
        assert_eq!(overlay.input(&ids[1], &arts[1], ir::Driver::Velocity), Input::Velocity(None));
        assert!(overlay.valid());
        assert_eq!(overlay, serde_json::from_str(&serde_json::to_string(&overlay).unwrap()).unwrap());
    }
}
