//! Falcon `SignalConnection`s to IR routes.
//!
//! The laws are v1's (`codex/uvi-latest-integration`, `src/uvi/modulation.rs`,
//! taken from UVI Workstation static inspection and authored native renders;
//! parameter units from <https://lua.uvi.net/_elements.html>). A connection
//! reads its source `s` (unipolar 0..1 or bipolar -1..1), inverts it when
//! `Inverted` (`1 - s` / `-s`), passes it through its `Mapper`, then:
//!
//! - `Gain` (factor): gain × (1 − max(r, 0) + r·u), r = Ratio clamped to
//!   −1..1 and u the unipolar view of the value;
//! - `Pitch` (add): + r·s semitones (`SamplePlayer` pitch is 2^(semitones/12));
//! - `Pan` (add): + r·s on the −1..1 pan position.
//!
//! A connection with Ratio 0 or Bypass 1 does nothing; a nested connection on
//! `Ratio` multiplies it by the factor law. Every stage is piecewise linear in
//! the IR modulator's unipolar view `w`, so each connection becomes one route
//! whose [`ir::Shape`] carries source polarity, LFO depth, inversion and the
//! mapper exactly. Constant sources (macros) fold into the zone's gain, tune or
//! pan. What has no exact route is reported, never approximated.

use crate::{Translation, number, path};
use roxmltree::Node;
use sampler_ir as ir;

/// A Falcon LFO `Smooth` (a 0..1 s parameter) below this is under one float32
/// step even at 192 kHz: the measured recurrence keeps (1/3)^(1/(Smooth·rate))
/// of the previous point per sample, which underflows to zero here.
const NEGLIGIBLE_SMOOTH: f64 = 3e-7;

/// Effects of an owner's connections on the zones it applies to.
#[derive(Clone, Debug)]
pub(crate) struct Modulation {
    pub routes: Vec<ir::RouteRef>,
    /// Static factor from constant sources.
    pub gain: f64,
    /// Static semitones from constant sources.
    pub pitch: f64,
    /// Static pan offset from constant sources.
    pub pan: f64,
}

impl Default for Modulation {
    fn default() -> Self {
        Self {
            routes: Vec::new(),
            gain: 1.0,
            pitch: 0.0,
            pan: 0.0,
        }
    }
}

/// Why a connection was not translated.
struct Gap {
    feature: &'static str,
    detail: String,
    reason: ir::Reason,
}

fn gap(feature: &'static str, detail: impl Into<String>, reason: ir::Reason) -> Gap {
    Gap {
        feature,
        detail: detail.into(),
        reason,
    }
}

/// What a connection reads.
enum Signal {
    /// An IR modulator; `curve` maps its unipolar view `w` to the UVI value.
    Live {
        modulator: ir::ModulatorRef,
        curve: Vec<(f64, f64)>,
        bipolar: bool,
    },
    /// A constant UVI value.
    Fixed { value: f64, bipolar: bool },
}

#[derive(Clone, Copy, PartialEq)]
enum Law {
    /// `Gain`: multiplies.
    Factor,
    /// `Pitch` or `Pan`: adds `ratio · value`.
    Add(ir::Target),
}

/// A `ControlSignalMapper`: an evenly spaced table over the input's unipolar
/// view, its negative outputs scaled by −Min and positive ones by Max.
struct Mapper {
    samples: Vec<f64>,
    min: f64,
    max: f64,
}

impl Mapper {
    fn position(value: f64, bipolar: bool) -> f64 {
        if bipolar { (value + 1.0) * 0.5 } else { value }
    }

    fn apply(&self, value: f64, bipolar: bool) -> f64 {
        let x = Self::position(value, bipolar).clamp(0.0, 1.0) * (self.samples.len() - 1) as f64;
        let i = (x as usize).min(self.samples.len() - 2);
        let raw = self.samples[i] + (self.samples[i + 1] - self.samples[i]) * (x - i as f64);
        if raw < 0.0 {
            raw * -self.min
        } else {
            raw * self.max
        }
    }

    /// Inputs (in source units) where the table's slope changes.
    fn knots(&self) -> impl Iterator<Item = f64> + '_ {
        let n = (self.samples.len() - 1) as f64;
        (0..self.samples.len()).map(move |j| j as f64 / n)
    }
}

/// A connection that cannot change anything.
fn inert(connection: Node) -> Result<bool, String> {
    Ok(number(connection, "Bypass", 0.0)? != 0.0 || number(connection, "Ratio", 1.0)? == 0.0)
}

/// The live (non-inert) connections a module's own parameters receive.
fn live_inputs<'a>(node: Node<'a, 'a>) -> Result<Vec<Node<'a, 'a>>, String> {
    let mut live = Vec::new();
    for c in node
        .children()
        .filter(|n| n.has_tag_name("Connections"))
        .flat_map(|c| c.children())
        .filter(|n| n.has_tag_name("SignalConnection"))
    {
        if !inert(c)? {
            live.push(c);
        }
    }
    Ok(live)
}

fn describe(connections: &[Node]) -> String {
    connections
        .iter()
        .map(|c| {
            format!(
                "{} -> {}",
                c.attribute("Source").unwrap_or_default(),
                c.attribute("Destination").unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Linear interpolation of ascending `(x, y)` points.
fn interpolate(points: &[(f64, f64)], x: f64) -> f64 {
    let i = points
        .windows(2)
        .position(|p| x <= p[1].0)
        .unwrap_or(points.len() - 2);
    let ((x0, y0), (x1, y1)) = (points[i], points[i + 1]);
    if x1 == x0 {
        y1
    } else {
        y0 + (y1 - y0) * (x - x0) / (x1 - x0)
    }
}

impl Translation {
    fn gap(&mut self, connection: Node, gap: Gap) {
        let what = format!(
            "{} -> {}",
            connection.attribute("Source").unwrap_or_default(),
            connection.attribute("Destination").unwrap_or_default()
        );
        self.ir.unsupported.push(ir::Unsupported {
            location: path(connection),
            feature: gap.feature.into(),
            value: if gap.detail.is_empty() {
                what
            } else {
                format!("{what}: {}", gap.detail)
            },
            reason: gap.reason,
        });
    }

    /// Translate `connection` (owned by a Keygroup or SamplePlayer) into `out`.
    pub(crate) fn connect(&mut self, connection: Node, out: &mut Modulation) -> Result<(), String> {
        if let Err(gap) = self.try_connect(connection, out)? {
            self.gap(connection, gap);
        }
        Ok(())
    }

    fn try_connect(
        &mut self,
        connection: Node,
        out: &mut Modulation,
    ) -> Result<Result<(), Gap>, String> {
        use ir::Reason::{NotModeled, UnknownLaw};
        if inert(connection)? {
            return Ok(Ok(()));
        }
        let mode = number(connection, "ConnectionMode", 0.0)?;
        if mode != 0.0 {
            return Ok(Err(gap("ConnectionMode", mode.to_string(), UnknownLaw)));
        }
        let law = match connection.attribute("Destination").unwrap_or_default() {
            "Gain" => Law::Factor,
            "Pitch" => Law::Add(ir::Target::Pitch),
            "Pan" => Law::Add(ir::Target::Pan),
            _ => return Ok(Err(gap("modulation destination", "", NotModeled))),
        };
        let mut ratio = number(connection, "Ratio", 1.0)?;
        // A nested connection modulates Ratio with the factor law.
        for nested in live_inputs(connection)? {
            if nested.attribute("Destination") != Some("Ratio") {
                return Ok(Err(gap(
                    "modulated connection parameter",
                    describe(&[nested]),
                    NotModeled,
                )));
            }
            match self.signal(connection, nested)? {
                Err(gap) => return Ok(Err(gap)),
                Ok(Signal::Live { .. }) => {
                    return Ok(Err(gap(
                        "ratio modulated by a live source (a product of sources)",
                        describe(&[nested]),
                        NotModeled,
                    )));
                }
                Ok(Signal::Fixed { value, bipolar }) => {
                    let value = match self.stage(nested, value, bipolar)? {
                        Ok(value) => value,
                        Err(gap) => return Ok(Err(gap)),
                    };
                    let r = number(nested, "Ratio", 1.0)?.clamp(-1.0, 1.0);
                    let u = Mapper::position(value, bipolar);
                    ratio *= 1.0 - r.max(0.0) + r * u;
                }
            }
        }
        if ratio == 0.0 {
            return Ok(Ok(()));
        }
        if law == Law::Factor {
            ratio = ratio.clamp(-1.0, 1.0);
        }
        let signal = match self.signal(connection, connection)? {
            Ok(signal) => signal,
            Err(gap) => return Ok(Err(gap)),
        };
        match signal {
            Signal::Fixed { value, bipolar } => {
                let value = match self.stage(connection, value, bipolar)? {
                    Ok(value) => value,
                    Err(gap) => return Ok(Err(gap)),
                };
                match law {
                    Law::Factor => {
                        out.gain *= 1.0 - ratio.max(0.0) + ratio * Mapper::position(value, bipolar)
                    }
                    Law::Add(ir::Target::Pitch) => out.pitch += ratio * value,
                    Law::Add(_) => out.pan += ratio * value,
                }
            }
            Signal::Live {
                modulator,
                curve,
                bipolar,
            } => {
                if self.ir.modulators[modulator.0].source == ir::ModulationSource::PitchBend
                    && law != Law::Add(ir::Target::Pitch)
                {
                    return Ok(Err(gap(
                        "pitch bend outside pitch (bend is native note expression)",
                        "",
                        NotModeled,
                    )));
                }
                let ir_bipolar = self.ir.modulators[modulator.0].source.bipolar();
                let mapper = match self.connection_mapper(connection)? {
                    Ok(mapper) => mapper,
                    Err(gap) => return Ok(Err(gap)),
                };
                let inverted = number(connection, "Inverted", 0.0)? != 0.0;
                let invert = |s: f64| match (inverted, bipolar) {
                    (false, _) => s,
                    (true, true) => -s,
                    (true, false) => 1.0 - s,
                };
                // Breakpoints: the source curve's, plus where the mapper's
                // table knots fall inside each of its segments.
                let mut inputs: Vec<f64> = curve.iter().map(|p| p.0).collect();
                if let Some(mapper) = &mapper {
                    for pair in curve.windows(2) {
                        let (w0, w1) = (pair[0].0, pair[1].0);
                        let p0 = Mapper::position(invert(pair[0].1), bipolar);
                        let p1 = Mapper::position(invert(pair[1].1), bipolar);
                        if p0 == p1 {
                            continue;
                        }
                        for knot in mapper.knots() {
                            let t = (knot - p0) / (p1 - p0);
                            if t > 0.0 && t < 1.0 {
                                inputs.push(w0 + t * (w1 - w0));
                            }
                        }
                    }
                }
                inputs.sort_by(f64::total_cmp);
                inputs.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
                let points: Vec<(f64, f64)> = inputs
                    .into_iter()
                    .map(|w| {
                        let s = invert(interpolate(&curve, w));
                        let m = mapper.as_ref().map_or(s, |m| m.apply(s, bipolar));
                        let y = match law {
                            Law::Factor if ratio >= 0.0 => Mapper::position(m, bipolar),
                            Law::Factor => 1.0 - Mapper::position(m, bipolar),
                            Law::Add(_) if ir_bipolar => (m + 1.0) * 0.5,
                            Law::Add(_) => m,
                        };
                        (w, y)
                    })
                    .collect();
                let (target, depth) = match law {
                    Law::Factor => (ir::Target::Amplitude, ir::Depth::Normalized(ratio.abs())),
                    Law::Add(ir::Target::Pitch) => (
                        ir::Target::Pitch,
                        ir::Depth::Pitch(ir::Pitch::Semitones(ratio)),
                    ),
                    Law::Add(target) => (target, ir::Depth::Normalized(ratio)),
                };
                let route = self.route(modulator, target, depth, points);
                out.routes.push(route);
            }
        }
        Ok(Ok(()))
    }

    /// Inversion and mapper of a constant value.
    fn stage(
        &mut self,
        connection: Node,
        value: f64,
        bipolar: bool,
    ) -> Result<Result<f64, Gap>, String> {
        let value = match (number(connection, "Inverted", 0.0)? != 0.0, bipolar) {
            (false, _) => value,
            (true, true) => -value,
            (true, false) => 1.0 - value,
        };
        Ok(match self.connection_mapper(connection)? {
            Ok(mapper) => Ok(mapper.map_or(value, |m| m.apply(value, bipolar))),
            Err(gap) => Err(gap),
        })
    }

    /// A route from `points` (`w` → shaped value), sharing identical routes and
    /// shapes so zones with the same connections share one voice program.
    fn route(
        &mut self,
        source: ir::ModulatorRef,
        target: ir::Target,
        depth: ir::Depth,
        points: Vec<(f64, f64)>,
    ) -> ir::RouteRef {
        let near = |f: &dyn Fn(f64) -> f64| points.iter().all(|&(w, y)| (y - f(w)).abs() < 1e-9);
        let mut route = ir::Route::new(source, target, depth);
        if near(&|w| 1.0 - w) && !near(&|w| w) {
            route.invert = true;
        } else if !near(&|w| w) {
            let key = format!("{points:?}");
            let shape = match self.shape_index.get(&key) {
                Some(&shape) => shape,
                None => {
                    self.ir.shapes.push(ir::Shape { points });
                    let shape = ir::ShapeRef(self.ir.shapes.len() - 1);
                    self.shape_index.insert(key, shape);
                    shape
                }
            };
            route.shape = Some(shape);
        }
        let key = format!("{route:?}");
        *self.route_index.entry(key).or_insert_with(|| {
            self.ir.routes.push(route);
            ir::RouteRef(self.ir.routes.len() - 1)
        })
    }

    /// One IR modulator per distinct built-in source or source node.
    fn modulator(&mut self, key: String, source: ir::ModulationSource) -> ir::ModulatorRef {
        *self.modulator_index.entry(key).or_insert_with(|| {
            self.ir.modulators.push(ir::Modulator {
                scope: ir::Scope::Voice,
                source,
            });
            ir::ModulatorRef(self.ir.modulators.len() - 1)
        })
    }

    /// What `connection`'s `Source` reads, seen from `owner`'s scope.
    fn signal(&mut self, owner: Node, connection: Node) -> Result<Result<Signal, Gap>, String> {
        use ir::Reason::{NotModeled, Unknown, UnknownLaw};
        let source = connection.attribute("Source").unwrap_or_default();
        let identity = vec![(0.0, 0.0), (1.0, 1.0)];
        let bipolar = vec![(0.0, -1.0), (1.0, 1.0)];
        let builtin = |this: &mut Self,
                       source: ir::ModulationSource,
                       curve: Vec<(f64, f64)>,
                       polar: bool|
         -> Result<Result<Signal, Gap>, String> {
            Ok(Ok(Signal::Live {
                modulator: this.modulator(format!("{source:?}"), source),
                curve,
                bipolar: polar,
            }))
        };
        // Key followers: exact at every MIDI key.
        let keys = |f: &dyn Fn(f64) -> f64| -> Vec<(f64, f64)> {
            (0..128)
                .map(|k| (f64::from(k) / 127.0, f(f64::from(k))))
                .collect()
        };
        match source {
            "@VoiceParam Velocity" => {
                return builtin(self, ir::ModulationSource::Velocity, identity, false);
            }
            "@VoiceParam Key" => return builtin(self, ir::ModulationSource::Key, identity, false),
            "@VoiceParam LinearKeyFollow" => {
                let curve = keys(&|k| ((k - 60.0) / 60.0).clamp(-1.0, 1.0));
                return builtin(self, ir::ModulationSource::Key, curve, true);
            }
            "@VoiceParam KeyFollow" => {
                // Measured in v1: quadratic below key 60, square root above.
                let curve = keys(&|k| {
                    let step = 1.0 / 60.0;
                    if k < 60.0 {
                        k * k * step * step - 1.0
                    } else {
                        ((k.min(120.0) - 60.0) * step).sqrt()
                    }
                });
                return builtin(self, ir::ModulationSource::Key, curve, true);
            }
            "@PitchBend" => return builtin(self, ir::ModulationSource::PitchBend, bipolar, true),
            "@ChanAfterTouch" => {
                return builtin(self, ir::ModulationSource::ChannelPressure, identity, false);
            }
            "@PolyAfterTouch" => {
                return builtin(self, ir::ModulationSource::PolyPressure, identity, false);
            }
            "@UnipolarRandom" => {
                return builtin(self, ir::ModulationSource::Random, identity, false);
            }
            "@Random" => return builtin(self, ir::ModulationSource::Random, bipolar, true),
            "@Alternate" | "@OrganPan" => {
                return Ok(Err(gap("modulation source", "", NotModeled)));
            }
            _ => {}
        }
        if let Some(cc) = source.strip_prefix("@MIDI CC ") {
            return match cc.parse::<u8>() {
                Ok(cc) if cc < 128 => {
                    builtin(self, ir::ModulationSource::Controller(cc), identity, false)
                }
                _ => Ok(Err(gap("modulation source", "", ir::Reason::InvalidValue))),
            };
        }
        if source.starts_with('@') {
            return Ok(Err(gap("modulation source", "", Unknown)));
        }
        let Some(node) = source_node(owner, source) else {
            return Ok(Err(gap(
                "unresolved modulation source",
                "",
                ir::Reason::InvalidValue,
            )));
        };
        let kind = node.tag_name().name();
        if number(node, "Bypass", 0.0)? != 0.0 {
            // A bypassed source reads 0.
            let polar_default = matches!(
                kind,
                "LFO" | "ScriptEventModulation" | "StdRandom" | "Drunk"
            );
            let polar = number(node, "Bipolar", f64::from(u8::from(polar_default)))? != 0.0;
            self.used.push(node.id());
            return Ok(Ok(Signal::Fixed {
                value: 0.0,
                bipolar: polar,
            }));
        }
        let inputs = live_inputs(node)?;
        if !inputs.is_empty() {
            return Ok(Err(gap(
                "modulated modulation source",
                format!("{kind} {}", describe(&inputs)),
                NotModeled,
            )));
        }
        match kind {
            "ConstantModulation" => {
                let value = number(node, "Value", 0.0)?.clamp(0.0, 1.0);
                let value = match number(node, "Style", 0.0)? {
                    0.0 => value,
                    1.0 => f64::from(u8::from(value > 0.5)),
                    style => {
                        return Ok(Err(gap(
                            "ConstantModulation Style",
                            style.to_string(),
                            UnknownLaw,
                        )));
                    }
                };
                let polar = number(node, "Bipolar", 0.0)? != 0.0;
                self.used.push(node.id());
                Ok(Ok(Signal::Fixed {
                    value: if polar { 2.0 * value - 1.0 } else { value },
                    bipolar: polar,
                }))
            }
            "DAHDSR" => {
                if number(node, "Retrigger", 1.0)? != 1.0 {
                    return Ok(Err(gap("DAHDSR Retrigger", "shared envelope", UnknownLaw)));
                }
                if number(node, "NoteOffRetrigger", 0.0)? != 0.0 {
                    return Ok(Err(gap("DAHDSR NoteOffRetrigger", "", NotModeled)));
                }
                let (modulator, velocity) = self.envelope(node)?;
                if velocity != ir::VelocityResponse::None {
                    return Ok(Err(gap(
                        "envelope VelocityAmount (envelope × velocity product)",
                        "",
                        NotModeled,
                    )));
                }
                let polar = number(node, "Bipolar", 0.0)? != 0.0;
                Ok(Ok(Signal::Live {
                    modulator,
                    curve: if polar { bipolar } else { identity },
                    bipolar: polar,
                }))
            }
            "LFO" => self.lfo(node),
            _ => Ok(Err(gap("modulation source", kind, NotModeled))),
        }
    }

    fn lfo(&mut self, node: Node) -> Result<Result<Signal, Gap>, String> {
        use ir::Reason::{NotModeled, UnknownLaw};
        let retrigger = match number(node, "Retrigger", 1.0)? {
            1.0 => true,
            0.0 => false,
            mode => return Ok(Err(gap("LFO Retrigger", mode.to_string(), UnknownLaw))),
        };
        let shape = match number(node, "WaveFormType", 0.0)? {
            0.0 => ir::LfoShape::Sine,
            2.0 => ir::LfoShape::Triangle,
            // v1 measured the square only per voice.
            1.0 if retrigger => ir::LfoShape::Square,
            wave @ (6.0 | 9.0) => {
                let what = format!("WaveFormType {wave} (random/user table)");
                return Ok(Err(gap("LFO waveform", what, NotModeled)));
            }
            wave => {
                let what = format!("WaveFormType {wave}, Retrigger {retrigger}");
                return Ok(Err(gap("LFO waveform", what, UnknownLaw)));
            }
        };
        let smooth = number(node, "Smooth", 0.0)?;
        if smooth >= NEGLIGIBLE_SMOOTH {
            return Ok(Err(gap("LFO Smooth", smooth.to_string(), NotModeled)));
        }
        let polar = number(node, "Bipolar", 1.0)? != 0.0;
        let delay = number(node, "DelayTime", 0.0)?.max(0.0);
        let rise = number(node, "RiseTime", 0.0)?.max(0.0);
        // The IR delays and fades an LFO per voice towards its centre; UVI
        // fades a unipolar LFO from 0 and times a free one from program start.
        if (delay > 0.0 || rise > 0.0) && (!polar || !retrigger) {
            let what = format!("Bipolar {polar}, Retrigger {retrigger}");
            return Ok(Err(gap("LFO delay or rise", what, NotModeled)));
        }
        let freq = number(node, "Freq", 0.5)?;
        let rate = if number(node, "SyncToHost", 0.0)? != 0.0 {
            // Synchronized Freq is the cycle length in beats: tempo/60/Freq Hz.
            ir::Frequency::Beats(freq)
        } else {
            ir::Frequency::Hertz(freq)
        };
        if !(freq > 0.0) {
            return Ok(Err(gap(
                "LFO Freq",
                freq.to_string(),
                ir::Reason::InvalidValue,
            )));
        }
        let depth = number(node, "Depth", 1.0)?.clamp(0.0, 1.0);
        let lfo = ir::Lfo {
            shape,
            rate,
            delay: ir::Time::Seconds(delay),
            fade_in: ir::Time::Seconds(rise),
            phase: number(node, "Phase", 0.0)?.rem_euclid(1.0),
            retrigger,
        };
        self.used.push(node.id());
        let modulator = self.modulator(
            format!("node {:?}", node.id()),
            ir::ModulationSource::Lfo(lfo),
        );
        Ok(Ok(Signal::Live {
            modulator,
            // IR LFO value x = 2w − 1; UVI is D·x bipolar, D·(x + 1)/2 unipolar.
            curve: if polar {
                vec![(0.0, -depth), (1.0, depth)]
            } else {
                vec![(0.0, 0.0), (1.0, depth)]
            },
            bipolar: polar,
        }))
    }

    /// The connection's `Mapper`, looked up by name from the nearest scope.
    fn connection_mapper(
        &mut self,
        connection: Node,
    ) -> Result<Result<Option<Mapper>, Gap>, String> {
        use ir::Reason::{InvalidValue, NotModeled};
        let name = connection.attribute("Mapper").unwrap_or_default();
        if name.is_empty() {
            return Ok(Ok(None));
        }
        let node = connection
            .ancestors()
            .filter(|n| matches!(n.tag_name().name(), "Keygroup" | "Layer" | "Program"))
            .find_map(|scope| {
                scope
                    .children()
                    .filter(|n| n.has_tag_name("Mappers"))
                    .flat_map(|m| m.children())
                    .find(|n| {
                        n.has_tag_name("ControlSignalMapper") && n.attribute("Name") == Some(name)
                    })
            });
        let Some(node) = node else {
            return Ok(Err(gap("unresolved mapper", name, InvalidValue)));
        };
        if number(node, "Discrete", 0.0)? != 0.0 || number(node, "Integer", 0.0)? != 0.0 {
            return Ok(Err(gap("discrete or integer mapper", name, NotModeled)));
        }
        let (min, max) = (number(node, "Min", 0.0)?, number(node, "Max", 1.0)?);
        let samples: Option<Vec<f64>> = node
            .text()
            .unwrap_or_default()
            .split_whitespace()
            .map(|v| {
                v.replace(',', ".")
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
            })
            .collect();
        match samples {
            Some(samples)
                if samples.len() >= 2 && samples.len() <= 65536 && min <= 0.0 && max >= 0.0 =>
            {
                Ok(Ok(Some(Mapper { samples, min, max })))
            }
            _ => Ok(Err(gap("mapper table or range", name, InvalidValue))),
        }
    }
}

/// The `ControlSignalSources` child a connection source names: `$Program/X`
/// and `$Layer/X` address those scopes, a bare name the nearest scope.
pub(crate) fn source_node<'a>(owner: Node<'a, 'a>, source: &str) -> Option<Node<'a, 'a>> {
    let (scope, name) = match source.split_once('/') {
        Some(("$Program", name)) => (Some("Program"), name),
        Some(("$Layer", name)) => (Some("Layer"), name),
        Some(("$Keygroup", name)) => (Some("Keygroup"), name),
        Some(_) => return None,
        None => (None, source),
    };
    owner
        .ancestors()
        .filter(|n| n.is_element() && scope.is_none_or(|s| n.has_tag_name(s)))
        .find_map(|scope| {
            scope
                .children()
                .filter(|n| n.has_tag_name("ControlSignalSources"))
                .flat_map(|s| s.children())
                .find(|n| n.is_element() && n.attribute("Name") == Some(name))
        })
}
