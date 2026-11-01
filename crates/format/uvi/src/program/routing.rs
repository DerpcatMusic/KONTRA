//! The program's mixer as buses. Bus 0 is the main output; each layer and
//! each aux is a strip of buses: its voices or sends enter the first, its
//! inserts run in order, and its fader and pan sit on the last, which feeds
//! the auxes a layer sends to (before or after the fader). An active
//! EffectRack ends a bus: it feeds one bus per chain at the chain's gain,
//! and the chains feed a bus where the rest of the inserts run. Every strip
//! ends on the main output ahead of the program's own inserts, as an aux set
//! `PreInsert` does. Of the effects, Gain, GainMatrix, TrackDelay (at 0),
//! DigitalEq, Convolver, SampledReverb, ThreeBandShelves, DualDelay,
//! Maximizer, SparkVerb, WaveShaper, DiodeClipper, Flanger and Phasor are
//! carried; the rest pass their input and are reported (see `effects`).

use kontra_format::Notes;
use kontra_ir as ir;
use kontra_law::common::DB_GAIN;
use roxmltree::Node;

use super::delay::dual_delay;
use super::digital_eq::{digital_eq, gain_scale};
use super::reverb::spark_verb;
mod moved;
mod shaper;
mod sweep;

use super::{Library, Translator, db, flag, index_of, matrix, num, source_node};
use shaper::{diode_clipper, wave_shaper};
use sweep::{flanger, phasor};

/// An effect's saved parameters, some set to other values (to tabulate
/// what a connection moving them does: `moved`).
#[derive(Clone, Copy)]
pub(super) struct Params<'a, 'input> {
    pub(super) node: Node<'a, 'input>,
    set: &'a [(&'static str, f64)],
}

impl<'a, 'input> Params<'a, 'input> {
    pub(super) fn of(node: Node<'a, 'input>) -> Self {
        Self { node, set: &[] }
    }

    /// `name`'s value, or `default` when the effect has none.
    pub(super) fn num(&self, name: &str, default: f64) -> Result<f64, String> {
        match self.set.iter().find(|(n, _)| *n == name) {
            Some(&(_, v)) => Ok(v),
            None => num(self.node, name, default),
        }
    }
}

/// Effects a keygroup's voices run themselves (see [`Translator::voice_slots`]).
const PER_VOICE: [&str; 2] = ["Gain", "ThreeBandShelves"];

/// Whether `node` is an effect this module carries: in the program's
/// inserts, a layer's, an aux's or a rack chain's, or a per-voice one in a
/// keygroup's.
pub(super) fn carried(node: Node) -> bool {
    let Some(owner) = node
        .parent()
        .filter(|p| p.has_tag_name("Inserts"))
        .and_then(|p| p.parent())
    else {
        return false;
    };
    let laid = owner.has_tag_name("Layer") || owner.has_tag_name("AuxEffect");
    let kind = [
        "Gain",
        "GainMatrix",
        "TrackDelay",
        "DigitalEq",
        "Convolver",
        "SampledReverb",
        "ThreeBandShelves",
        "DualDelay",
        "Maximizer",
        "SparkVerb",
        "WaveShaper",
        "DiodeClipper",
        "Flanger",
        "Phasor",
    ]
    .iter()
    .any(|t| node.has_tag_name(*t));
    if owner.has_tag_name("Keygroup") {
        return PER_VOICE.iter().any(|t| node.has_tag_name(*t));
    }
    let in_keygroup = node.ancestors().any(|a| a.has_tag_name("Keygroup"));
    !in_keygroup
        && ((laid && (kind || node.has_tag_name("EffectRack")))
            || (owner.has_tag_name("Program") && kind))
}

/// A keygroup's TriggerRule as a start rule: every oscillator (0), one per
/// note in turn (1, "Cycle") or at random (2, "Random Cycle"). The names
/// follow the keygroup's trigger parameters in UVIWorkstation's string table
/// (0x141e1ef00, before TriggerSync's "Immediate"/"Next Beat"/"Next Bar").
pub(super) fn cycle(kg: Node) -> Option<(ir::Join, ir::StartRule)> {
    let random = match kg.attribute("TriggerRule")? {
        "1" => false,
        "2" => true,
        _ => return None,
    };
    Some((ir::Join::And, ir::StartRule::Cycle { random }))
}

/// Latent for `node` on a program aux no open send reaches at load (by the
/// name [`Translator::sends_of`] resolves: AO's Feedback aux, its sends off
/// until a script turns one on), or for a connection from a bypassed
/// modulator (it moves nothing until a script turns it on); else Audible.
pub(super) fn impact(program: Node, node: Node) -> ir::Impact {
    if super::source_node(program, node).is_some_and(|n| flag(n, "Bypass")) {
        return ir::Impact::Latent;
    }
    let aux = node
        .ancestors()
        .filter(|a| a.has_tag_name("AuxEffect"))
        .last();
    let open = |aux: Node| {
        program.descendants().any(|s| {
            s.has_tag_name("BusRouter")
                && !flag(s, "Bypass")
                && s.attribute("Destination")
                    .and_then(|d| d.rsplit('/').next())
                    == aux.attribute("Name")
                && num(s, "Gain", 1.0).is_ok_and(|g| g > 0.0)
        })
    };
    match aux {
        Some(aux) if aux.parent().and_then(|p| p.parent()) == Some(program) && !open(aux) => {
            ir::Impact::Latent
        }
        _ => ir::Impact::Audible,
    }
}

fn children<'a, 'input>(
    owner: Node<'a, 'input>,
    container: &'static str,
    tag: &'static str,
) -> impl Iterator<Item = Node<'a, 'input>> {
    owner
        .children()
        .filter(move |n| n.has_tag_name(container))
        .flat_map(|n| n.children())
        .filter(move |n| n.is_element() && (tag.is_empty() || n.has_tag_name(tag)))
}

impl<L: Library> Translator<'_, '_, L> {
    /// Every bus, the program's inserts, and each layer's entry bus and each
    /// layer's and aux's fader bus. Runs before the layers.
    pub(super) fn buses(&mut self) -> Result<(), String> {
        let program = self.program;
        self.out.buses.push(ir::Bus::new("Main"));
        let layers: Vec<Node> = program
            .descendants()
            .filter(|n| n.has_tag_name("Layer"))
            .collect();
        for &layer in &layers {
            let entry = self.push(layer);
            self.entry.insert(layer.id(), entry);
            let end = if flag(layer, "BypassInsertFX") {
                entry
            } else {
                self.lay(layer, entry)?
            };
            self.strip(layer, end)?;
        }
        let mut entries = Vec::new();
        for aux in children(program, "Auxs", "AuxEffect") {
            let entry = self.push(aux);
            entries.push((aux, entry));
            // A bypassed aux takes no sends.
            if flag(aux, "Bypass") {
                continue;
            }
            if !flag(aux, "PreInsert") {
                self.notes.add(
                    ir::Impact::Audible,
                    "aux after the program's inserts",
                    aux.attribute("DisplayName").unwrap_or_default(),
                );
            }
            let end = self.lay(aux, entry)?;
            self.strip(aux, end)?;
        }
        for &layer in &layers {
            self.sends_of(layer, &entries)?;
        }
        self.out.inserts = Vec::new();
        for node in children(program, "Inserts", "") {
            let slot = self.slot(node)?;
            self.out.inserts.push(slot);
        }
        Ok(())
    }

    /// A keygroup's inserts its voices run, by slot; the rest are empty.
    pub(super) fn voice_slots(&mut self, kg: Node) -> Result<Vec<Option<ir::Effect>>, String> {
        let mut slots = Vec::new();
        for node in children(kg, "Inserts", "") {
            let per_voice = PER_VOICE.iter().any(|t| node.has_tag_name(*t));
            slots.push(if per_voice { self.slot(node)? } else { None });
        }
        if slots.iter().all(Option::is_none) {
            slots.clear();
        }
        Ok(slots)
    }

    /// A new bus named for `owner`, off the main output.
    fn push(&mut self, owner: Node) -> usize {
        let name = owner
            .attribute("DisplayName")
            .or_else(|| owner.attribute("Name"))
            .unwrap_or_default();
        self.out.buses.push(bus(name));
        self.out.buses.len() - 1
    }

    /// `owner`'s gain and pan on its last bus `end`, which scripts move.
    fn strip(&mut self, owner: Node, end: usize) -> Result<(), String> {
        let (gain, pan) = (num(owner, "Gain", 1.0)?, num(owner, "Pan", 0.0)?);
        self.bus_of.insert(owner.id(), end);
        if let Some(b) = self.out.buses.get_mut(end) {
            b.volume_db = db(gain);
            b.to_main = true;
            // A layer's pan is on its groups.
            if !owner.has_tag_name("Layer") {
                b.pan = pan.clamp(-1.0, 1.0) as f32;
            }
        }
        Ok(())
    }

    /// `owner`'s inserts from bus `at` on; returns the bus they end on.
    fn lay(&mut self, owner: Node, mut at: usize) -> Result<usize, String> {
        for node in children(owner, "Inserts", "") {
            if node.has_tag_name("DigitalEq") && !flag(node, "Bypass") {
                self.routed_eq(node, at)?;
                continue;
            }
            if !node.has_tag_name("EffectRack") || flag(node, "Bypass") {
                let slot = self.slot(node)?;
                if let Some(b) = self.out.buses.get_mut(at) {
                    let insert = u8::try_from(b.inserts.len()).map_err(|e| e.to_string())?;
                    b.inserts.push(slot);
                    if slot.is_some() {
                        self.moves(node, at, insert)?;
                    }
                }
                continue;
            }
            let mut ends = Vec::new();
            // A bypassed chain is built silent: a script may enable it.
            for chain in children(node, "Chains", "AuxEffect") {
                let entry = self.push(chain);
                self.feed(at, entry, 1.0, false);
                let end = self.lay(chain, entry)?;
                self.bus_of.insert(chain.id(), end);
                let gain = if flag(chain, "Bypass") {
                    0.0
                } else {
                    num(chain, "Gain", 1.0)?
                };
                if let Some(b) = self.out.buses.get_mut(end) {
                    b.volume_db = db(gain);
                }
                let pan = num(chain, "Pan", 0.0)?.clamp(-1.0, 1.0) as f32;
                if pan != 0.0
                    && let Some(b) = self.out.buses.get_mut(end)
                {
                    let kind = ir::EffectKind::Stereo { width: 0.5, pan };
                    b.inserts.push(Some(effect(kind, 1.0, 0.0)));
                }
                ends.push(end);
            }
            // The chains' gains are their faders: scripts move them.
            at = self.push(node);
            for end in ends {
                self.feed(end, at, 1.0, true);
            }
        }
        Ok(at)
    }

    /// A DigitalEq on bus `at` (one slot per four bands), its GainScale
    /// connections as the bus's routes.
    fn routed_eq(&mut self, node: Node, at: usize) -> Result<(), String> {
        let bands = digital_eq(node, &mut self.notes)?;
        let program = self.program;
        if let Some(b) = self.out.buses.get_mut(at) {
            let insert = u8::try_from(b.inserts.len()).map_err(|e| e.to_string())?;
            b.inserts.extend(bands.effects.iter().copied().map(Some));
            for c in gain_scale(program, node, &bands, insert, b) {
                self.carried.insert(c.id());
            }
        }
        Ok(())
    }

    fn feed(&mut self, from: usize, to: usize, level: f64, post_fader: bool) {
        if let Some(b) = self.out.buses.get_mut(from) {
            b.sends.push(ir::BusSend {
                bus: to,
                level: level as f32,
                post_fader,
            });
        }
    }

    /// A layer's active sends to active auxes, from its fader bus.
    fn sends_of(&mut self, layer: Node, auxes: &[(Node, usize)]) -> Result<(), String> {
        let Some(&from) = self.bus_of.get(&layer.id()) else {
            return Ok(());
        };
        for send in children(layer, "BusRouters", "BusRouter").filter(|n| !flag(*n, "Bypass")) {
            let target = send
                .attribute("Destination")
                .and_then(|d| d.rsplit('/').next())
                .unwrap_or_default();
            let found = auxes
                .iter()
                .find(|(n, _)| n.attribute("Name") == Some(target) && !flag(*n, "Bypass"));
            if let Some(&(_, to)) = found {
                let (level, factors) = self.send_level(send)?;
                self.feed(from, to, level, !flag(send, "PreFader"));
                self.move_send(from, factors);
            }
        }
        Ok(())
    }

    /// A send's saved level and the live factors its `Gain` connections
    /// scale it by: the parameter is multiplicative (BusRouter's ctor
    /// 0x141002450 gives it a MultiplicativeParameterModulation, vtable
    /// 0x141e37a50: the law at 0x141429a80). A macro folds in at its value;
    /// a source no bus reads (an LFO, an envelope) stays reported.
    fn send_level(&mut self, send: Node) -> Result<(f64, Vec<ir::Factor>), String> {
        let program = self.program;
        let mut level = num(send, "Gain", 1.0)?;
        let mut factors = Vec::new();
        let gains = matrix::children(send).filter(|c| c.attribute("Destination") == Some("Gain"));
        for c in gains {
            let Some(f) = matrix::factor(program, c, None) else {
                continue;
            };
            match f.input.source {
                ir::Source::Constant => {
                    let held = source_node(program, c)
                        .is_some_and(|n| n.has_tag_name("ConstantModulation"));
                    if !held {
                        continue;
                    }
                    level *= f64::from(f.at(f.input.apply(1.0)));
                }
                _ => factors.push(f),
            }
            self.carried.insert(c.id());
        }
        Ok((level, factors))
    }

    /// Moves the last send of bus `from` by `factors`, as a blend of them.
    fn move_send(&mut self, from: usize, factors: Vec<ir::Factor>) {
        let Some(b) = self.out.buses.get_mut(from).filter(|_| !factors.is_empty()) else {
            return;
        };
        let Ok(send) = u8::try_from(b.sends.len().saturating_sub(1)) else {
            return;
        };
        let term = ir::Term {
            input: ir::Transfer {
                source: ir::Source::Constant,
                table: Vec::new(),
            },
            amount: 1.0,
            factors,
        };
        let blend = ir::Blend {
            base: 0.0,
            terms: vec![term],
        };
        let source = ir::Source::Blend(index_of(&mut b.blends, blend));
        b.mods.push(ir::ModKind::Send { source, send });
    }

    /// One insert as a slot; one not carried is empty (its input passes).
    fn slot(&mut self, node: Node) -> Result<Option<ir::Effect>, String> {
        Ok(if flag(node, "Bypass") {
            None
        } else if node.has_tag_name("Gain") {
            let kind = ir::EffectKind::Gain(num(node, "Volume", 1.0)? as f32);
            Some(effect(kind, 1.0, 0.0))
        } else if node.has_tag_name("DigitalEq") {
            // Off a bus a slot holds one EQ: its first four bands.
            let bands = digital_eq(node, &mut self.notes)?;
            if bands.effects.len() > 1 {
                self.notes
                    .add(ir::Impact::Audible, "DigitalEq band past the fourth", "");
            }
            bands.effects.first().copied()
        } else if node.has_tag_name("TrackDelay") {
            // A track delay at 0 (every one in VWinds) passes its input.
            if num(node, "DelayTime", 0.0)? != 0.0 {
                self.notes.add(ir::Impact::Audible, "TrackDelay time", "");
            }
            None
        } else if node.has_tag_name("GainMatrix") {
            Some(matrix(node, &mut self.notes)?)
        } else if node.has_tag_name("ThreeBandShelves") {
            Some(shelves(node)?)
        } else if node.has_tag_name("SparkVerb") {
            Some(spark_verb(node, &mut self.notes)?)
        } else if node.has_tag_name("Maximizer") {
            Some(maximizer(node, &mut self.notes)?)
        } else if node.has_tag_name("WaveShaper") {
            wave_shaper(Params::of(node), &mut self.notes)?
        } else if node.has_tag_name("Flanger") {
            Some(flanger(Params::of(node))?)
        } else if node.has_tag_name("Phasor") {
            Some(phasor(Params::of(node), &mut self.notes)?)
        } else if node.has_tag_name("DiodeClipper") {
            Some(diode_clipper(node)?)
        } else if node.has_tag_name("DualDelay") {
            Some(dual_delay(node, &mut self.notes)?)
        } else if node.has_tag_name("Convolver") || node.has_tag_name("SampledReverb") {
            let raw = node.has_tag_name("SampledReverb") && !flag(node, "NormalizePower");
            let wet = num(node, "Wet", 1.0)? as f32 * if raw { RAW_WET } else { 1.0 };
            let dry = num(node, "Dry", 0.0)? as f32;
            Some(match self.impulse(node)? {
                Some(impulse) => effect(ir::EffectKind::Convolution { impulse }, wet, dry),
                None => effect(ir::EffectKind::OtherWet, 1.0, dry),
            })
        } else {
            // ponytail: an effect with a wet path not carried yet plays only
            // its dry level; passing its input instead would be louder.
            wet_only(node)?.map(|dry| effect(ir::EffectKind::OtherWet, 1.0, dry))
        })
    }

    /// A Convolver's or SampledReverb's response, `None` (reported) when its
    /// file is missing.
    fn impulse(&mut self, node: Node) -> Result<Option<u16>, String> {
        let path = node.attribute("SamplePath").unwrap_or_default();
        let sample = match self.sample(path) {
            Ok(s) => s,
            Err(e) => {
                self.notes.add(
                    ir::Impact::Audible,
                    "missing impulse response",
                    format!("{path}: {e}"),
                );
                return Ok(None);
            }
        };
        let reverb = node.has_tag_name("SampledReverb");
        for (name, unset) in [("DampingLow", 0.0), ("DampingHigh", 0.0)] {
            if reverb && num(node, name, unset)? != unset {
                let feature = format!("sampled reverb {name}");
                self.notes.add(ir::Impact::Audible, &feature, path);
            }
        }
        let time = if reverb {
            num(node, "Time", 1.0)?.clamp(0.0, 1.0) as f32
        } else {
            1.0
        };
        let width = if reverb {
            num(node, "Width", 0.0)?.clamp(-1.0, 1.0) as f32
        } else {
            0.0
        };
        let stereo = sample.channels >= 2;
        let sample = self.sample_index(sample);
        self.out.impulses.push(ir::Impulse {
            sample,
            reverse: false,
            normalize: flag(node, "NormalizePower").then_some(POWER),
            predelay_ms: if reverb {
                num(node, "PreDelay", 0.0)?.max(0.0) as f32
            } else {
                0.0
            },
            cuts: None,
            envelope: (time < 1.0 || flag(node, "UseWindowAtOriginalSize")).then(|| fade(time)),
            mix: (width != 0.0 && stereo).then(|| widen(width)),
        });
        Ok(u16::try_from(self.out.impulses.len() - 1).ok())
    }
}

/// A sampled reverb's wet level when its response is not normalized: UVI's
/// SampledReverbState audio path scales the convolved signal by 0.1 (−20 dB)
/// unless NormalizePower is set (0x14103a07f, 0.1 at 0x141cfa9ec), before
/// Wet. The Convolver's paths (0x1412294c7, 0x14122d2e6) apply only Wet.
const RAW_WET: f32 = 0.1;

/// `NormalizePower`: the response scaled to unit energy (sum of squares).
/// UVI (SampledReverb/Convolver IR build 0x141532fe0) divides by the root
/// of the channels' mean energy, floored at 1e-10 (0x141e6e4f0), after
/// resampling to the host rate.
const POWER: ir::Normalize = ir::Normalize {
    energy: 1.0,
    max_gain: 1e5,
    floor: 0.0,
    channels: ir::EnergyChannels::Mean,
    after_resample: true,
};

/// A sampled reverb's `Time` window: UVI keeps `time` of the response and
/// fades it by a half cosine, `(cos(πn/(N+1)) + 1)/2` over the kept `N`
/// frames (0x141532fe0, π at 0x141cfab4c). Six linear knots trace the
/// cosine within 2%; the rest is silent.
fn fade(time: f32) -> [[f32; 2]; 8] {
    let mut knots = [[1.0, SILENT]; 8];
    for (k, knot) in knots.iter_mut().take(7).enumerate() {
        let x = k as f32 / 6.0;
        let gain = ((std::f32::consts::PI * x).cos() + 1.0) * 0.5;
        *knot = [time * x, 20.0 * gain.max(1e-10).log10()];
    }
    knots
}

/// A sampled reverb's `Width` (-1..1) on a stereo response: UVI (IR build
/// 0x141532fe0, from setter 0x14103f8f0) takes `c = -w`, `d = 2 + w` (2.0
/// at 0x141cfaab8) scaled to unit length, then `L = dL + cR` and
/// `R = dR + cL` with the new `L`, before NormalizePower. A response with
/// one channel has no pair and is left alone.
fn widen(w: f32) -> [[f32; 2]; 2] {
    let (c, d) = (-w, 2.0 + w);
    let n = c.hypot(d).recip();
    let (c, d) = (c * n, d * n);
    [[d, c], [c * d, d + c * c]]
}

/// Silence for an envelope knot, dB.
const SILENT: f32 = -200.0;

/// ThreeBandShelves: UVI's one biquad (0x1412927b0 → 0x141643350) is the
/// mid gain times a first-order low shelf at FreqLowMid and a first-order
/// high shelf at FreqMidHigh, each by its band's gain over the mid's
/// (bilinear, tan-prewarped, 0x141649120). A cookbook shelf at slope 0.5 is
/// that first-order shelf exactly.
fn shelves(node: Node) -> Result<ir::Effect, String> {
    let mid = num(node, "GainMid", 0.0)? as f32;
    let shelf = |shape, hz: &str, default, gain: &str| -> Result<ir::Bell, String> {
        Ok(ir::Bell {
            shape,
            hz: num(node, hz, default)? as f32,
            q: 0.5,
            gain_db: num(node, gain, 0.0)? as f32 - mid,
        })
    };
    let bands = [
        shelf(ir::Shape::LowShelf, "FreqLowMid", 200.0, "GainLow")?,
        shelf(ir::Shape::HighShelf, "FreqMidHigh", 4000.0, "GainHigh")?,
        ir::Bell::FLAT,
        ir::Bell::FLAT,
    ];
    Ok(effect(
        ir::EffectKind::Eq(bands),
        DB_GAIN.eval_unclamped(mid),
        0.0,
    ))
}

/// A bus GainMatrix: `Gain_i_j` is input channel `i` into output `j`. The
/// buses carry two channels (a keygroup's matrix sums its sample channels
/// into the first two), so the top-left block is the whole matrix; a row
/// past the second feeding them is reported.
fn matrix(node: Node, notes: &mut Notes) -> Result<ir::Effect, String> {
    let cell = |i: u8, j: u8| num(node, &format!("Gain_{i}_{j}"), f64::from(u8::from(i == j)));
    for i in 3..=12 {
        if cell(i, 1)? != 0.0 || cell(i, 2)? != 0.0 {
            notes.add(
                ir::Impact::Audible,
                "GainMatrix channel past the second",
                "",
            );
            break;
        }
    }
    let [ll, lr, rl, rr] = [(1, 1), (2, 1), (1, 2), (2, 2)].map(|(i, j)| cell(i, j));
    let m = [[ll? as f32, lr? as f32], [rl? as f32, rr? as f32]];
    Ok(effect(ir::EffectKind::Matrix(m), 1.0, 0.0))
}

/// UVI's Maximizer (engine 0x141665620, parameter ids 0x1427fd2c0..2e4):
/// the peak level in dB floored at Threshold, smoothed, gives a gain of
/// `Ceiling − level` dB. That is Threshold-to-Ceiling of input gain into a
/// limiter at Ceiling. Its release falls to a third in Release ms
/// (`e^(ln(1/3)/frames)`), close to the limiter's halving in the same time.
///
/// ponytail: the 2 ms lookahead, ReleaseBlend's slow release (100× longer,
/// 0.1 in every corpus program) and SlewRate are not carried; Knee and
/// Attack, 0 in the corpus, are reported when set.
fn maximizer(node: Node, notes: &mut Notes) -> Result<ir::Effect, String> {
    let threshold = num(node, "Threshold", 0.0)? as f32;
    let ceiling = num(node, "Ceiling", -0.1)? as f32;
    for name in ["Knee", "Attack"] {
        if num(node, name, 0.0)? != 0.0 {
            notes.add(ir::Impact::Audible, &format!("maximizer {name}"), "");
        }
    }
    let kind = ir::EffectKind::Limiter(ir::Limiter {
        input_db: ceiling - threshold,
        ceiling_db: ceiling,
        release_ms: num(node, "Release", 10.0)?.max(0.0) as f32,
    });
    Ok(effect(kind, 1.0, 0.0))
}

/// The dry level of an effect whose output is mostly a wet path (a reverb,
/// a delay); `None` for one that processes in place.
fn wet_only(node: Node) -> Result<Option<f32>, String> {
    let tag = node.tag_name().name();
    Ok(match tag {
        "FeedbackMachine" => Some(1.0 - num(node, "Mix", 1.0)?.clamp(0.0, 1.0) as f32),
        _ => None,
    })
}

/// A bus that only feeds its sends until [`Translator::strip`] gives it a
/// fader on the main output.
fn bus(name: &str) -> ir::Bus {
    ir::Bus {
        to_main: false,
        ..ir::Bus::new(name)
    }
}

fn effect(kind: ir::EffectKind, output: f32, dry: f32) -> ir::Effect {
    ir::Effect {
        kind,
        bypass: false,
        output,
        dry,
    }
}

/// The part a program plays in: UVIWorkstation builds each part
/// (0x140f0f290, always with defaults on) with three connections
/// (0x140f106e5..0x140f108c6): `@MIDI CC 7` and `@MIDI CC 10` in Edition mode
/// (ConnectionMode 1: the controller sets the knob) on the part's Gain
/// (linear 0..10^(6/20), 0x140ecfce1) and Pan (−1..1, 0x140ed007e), and
/// `@MIDI CC 11` in Modulation mode at ratio 1 on Gain, `(1 − r) + r·v/127`.
/// The script MIDI state starts CC 11 at 127. Gain and pan reach the mixer
/// as signals (per-sample buffers or per-block constants) from the
/// parameter; no smoother on them is found yet, so the output's measured
/// glide stands in.
pub(super) const PART_CONTROLLERS: ir::OutputControllers = ir::OutputControllers {
    volume: 7,
    pan: 10,
    top_db: 6.0,
    power: 1,
    expression: Some(11),
};

/// A stereo signal's pan in UVIWorkstation's mixer (0x141352420, the
/// stereo-to-stereo case of the layout switch at 0x141351ae0, whatever the
/// element's PanLaw) turns the far channel to `(1 + sin(π/2 + π·|pan|))/2`
/// via sinf 0x1419ede10. PanLaw (0 by default, never saved by the programs
/// read) picks only the mono-to-stereo law: 0 is `(1 ∓ sin(π/2·pan))/2`
/// (0x141353c40), 1 the equal-power `sin(π/4·(1 ∓ pan))` (0x141353820).
pub(super) const PART_BALANCE: ir::Balance = ir::Balance::Cosine;

/// The voices a program sounds: `Polyphony` times the most zones a keygroup
/// makes, one per oscillator or bundle member. A UVI voice is a keygroup's,
/// all of them (VWinds' Alto Flute 2: 8 voices, a note's 10 sustain zones
/// beside its chiff, air and key click); here each zone is a voice.
pub(super) fn voices(program: Node) -> Result<u16, String> {
    let zones = |p: Node| {
        p.attribute("SamplePath")
            .map_or(1, |s| s.split('*').count().max(2) - 1)
    };
    let players = |o: Node| {
        o.children()
            .filter(Node::is_element)
            .map(zones)
            .sum::<usize>()
    };
    let oscillators = program
        .descendants()
        .filter(|n| n.has_tag_name("Oscillators"));
    let widest = oscillators.map(players).max().unwrap_or(1).max(1) as f64;
    Ok((num(program, "Polyphony", 64.0)? * widest).clamp(1.0, 1024.0) as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_maximizer_lifts_by_its_threshold_into_a_limiter_at_its_ceiling() {
        // Augmented Orchestra's most common setting (90 programs).
        let xml = r#"<Maximizer Threshold="-6" Attack="0" Lookahead="2" Release="10" Ceiling="-0.1" Knee="0"/>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let e = maximizer(doc.root_element(), &mut Notes::default()).unwrap();
        let ir::EffectKind::Limiter(l) = e.kind else {
            panic!("{e:?}")
        };
        assert_eq!((l.input_db, l.ceiling_db, l.release_ms), (5.9, -0.1, 10.0));
        assert_eq!((e.output, e.dry), (1.0, 0.0));
    }

    #[test]
    fn three_band_shelves_are_two_half_slope_shelves_around_the_mid_gain() {
        let xml = r#"<ThreeBandShelves FreqLowMid="250" FreqMidHigh="5000" GainLow="3" GainMid="-1" GainHigh="4"/>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let e = shelves(doc.root_element()).unwrap();
        let ir::EffectKind::Eq([low, high, ..]) = e.kind else {
            panic!("{e:?}")
        };
        assert_eq!(
            (low.shape, low.hz, low.q, low.gain_db),
            (ir::Shape::LowShelf, 250.0, 0.5, 4.0)
        );
        assert_eq!(
            (high.shape, high.hz, high.gain_db),
            (ir::Shape::HighShelf, 5000.0, 5.0)
        );
        assert!((e.output - 0.891).abs() < 1e-3, "{}", e.output);
    }

    #[test]
    fn width_is_the_native_sequential_pair_mix() {
        assert_eq!(widen(0.0), [[1.0, 0.0], [0.0, 1.0]]);
        // 0x141532fe0's loop on (L, R) = (0.3, -0.7), in its own order.
        for w in [1.0f32, 0.5, -0.4, -1.0] {
            let n = (-w).hypot(2.0 + w).recip();
            let (c, d) = (-w * n, (2.0 + w) * n);
            let l = c * -0.7 + d * 0.3;
            let r = l * c + d * -0.7;
            let [a, b] = widen(w);
            let (x, y) = (a[0] * 0.3 + a[1] * -0.7, b[0] * 0.3 + b[1] * -0.7);
            assert!(
                (x - l).abs() < 1e-6 && (y - r).abs() < 1e-6,
                "{w}: {x} {y} vs {l} {r}"
            );
        }
        // Full width: the sides lean apart, a quarter of each into the other.
        let [a, _] = widen(1.0);
        assert!((a[1] / a[0] + 1.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn a_shortened_reverb_fades_out_by_its_time() {
        let knots = fade(0.6);
        assert_eq!(knots[0], [0.0, 0.0]);
        // Halfway through the kept part, the cosine is at half (-6 dB).
        let [x, db] = knots[3];
        assert!(
            (x - 0.3).abs() < 1e-6 && (db + 6.02).abs() < 0.01,
            "{x} {db}"
        );
        assert_eq!(knots[6], [0.6, SILENT]);
        assert_eq!(knots[7], [1.0, SILENT]);
    }
}
