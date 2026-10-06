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

use crate::{number, path, Translation};
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
        /// A second live source multiplying the value (an LFO's modulated
        /// `Depth`): its modulator and `w` → factor points.
        scale: Option<(ir::ModulatorRef, Vec<(f64, f64)>)>,
        /// Source smoothing as a route lag: seconds to 99% of a step.
        smoothing: f64,
        /// A second modulator summed into the value (MultiLFO noise), with its
        /// own `w` → value curve.
        extra: Option<(ir::ModulatorRef, Vec<(f64, f64)>)>,
        /// The program- or layer-level source this reads, reported (and made
        /// instrument-wide) once a route uses it.
        shared: Option<Shared>,
    },
    /// A constant UVI value.
    Fixed { value: f64, bipolar: bool },
}

/// A shared UVI source instance; see [`Translation::share`].
struct Shared {
    node: roxmltree::NodeId,
    location: String,
    feature: String,
    /// Becomes the IR's instrument-wide retriggered LFO.
    master: bool,
    modulators: Vec<ir::ModulatorRef>,
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
        if bipolar {
            (value + 1.0) * 0.5
        } else {
            value
        }
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

/// Whether a source lag becomes an exact route lag: a free-running source's
/// lag is shared and already settled when a voice starts, and a per-voice lag
/// starts at the source's first value where UVI's starts at 0.
fn smoothable(retrigger: bool, starts_at_zero: bool) -> Result<(), &'static str> {
    match (retrigger, starts_at_zero) {
        (false, _) => Err("shared lag of a free-running source"),
        (true, false) => Err("lag from a nonzero start"),
        (true, true) => Ok(()),
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
        // At most one live Ratio input: the route's depth × its factor law.
        let mut scale = None;
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
                Ok(Signal::Live {
                    modulator,
                    curve,
                    bipolar,
                    scale: None,
                    smoothing: 0.0,
                    extra: None,
                    shared,
                }) if scale.is_none() => {
                    if let Some(shared) = shared {
                        self.share(shared);
                    }
                    let r = number(nested, "Ratio", 1.0)?.clamp(-1.0, 1.0);
                    let factor = |m: f64| 1.0 - r.max(0.0) + r * Mapper::position(m, bipolar);
                    let points = match self.live_points(nested, &curve, bipolar, factor)? {
                        Ok(points) => points,
                        Err(gap) => return Ok(Err(gap)),
                    };
                    scale = Some((modulator, points));
                }
                Ok(Signal::Live { .. }) => {
                    return Ok(Err(gap(
                        "ratio modulated by several, smoothed or summed live sources",
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
            Signal::Fixed { .. } if scale.is_some() => {
                return Ok(Err(gap(
                    "constant source with a live-modulated ratio",
                    "",
                    NotModeled,
                )));
            }
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
                scale: depth,
                smoothing,
                extra,
                shared,
            } => {
                let mapped = !connection
                    .attribute("Mapper")
                    .unwrap_or_default()
                    .is_empty();
                // A lag or a sum passes unchanged only through linear stages.
                if (smoothing > 0.0 || extra.is_some()) && mapped {
                    return Ok(Err(gap(
                        "smoothed or summed source through a mapper",
                        "",
                        NotModeled,
                    )));
                }
                if extra.is_some() && law == Law::Factor {
                    return Ok(Err(gap(
                        "summed waveforms into gain (not a product of routes)",
                        "",
                        NotModeled,
                    )));
                }
                let scale = match (scale, depth) {
                    (Some(_), Some(_)) => {
                        return Ok(Err(gap(
                            "ratio and source depth both live-modulated",
                            "",
                            NotModeled,
                        )));
                    }
                    (scale, None) => scale,
                    // The product is exact only where the connection is
                    // linear in the source: adding, no mapper, no `1 − s`.
                    (None, Some(_))
                        if law == Law::Factor
                            || mapped
                            || (!bipolar && number(connection, "Inverted", 0.0)? != 0.0) =>
                    {
                        return Ok(Err(gap(
                            "live LFO depth through a nonlinear connection",
                            "",
                            NotModeled,
                        )));
                    }
                    (None, depth) => depth,
                };
                if self.ir.modulators[modulator.0].source == ir::ModulationSource::PitchBend
                    && law != Law::Add(ir::Target::Pitch)
                {
                    return Ok(Err(gap(
                        "pitch bend outside pitch (bend is native note expression)",
                        "",
                        NotModeled,
                    )));
                }
                let (target, depth) = match law {
                    Law::Factor => (ir::Target::Amplitude, ir::Depth::Normalized(ratio.abs())),
                    Law::Add(ir::Target::Pitch) => (
                        ir::Target::Pitch,
                        ir::Depth::Pitch(ir::Pitch::Semitones(ratio)),
                    ),
                    Law::Add(target) => (target, ir::Depth::Normalized(ratio)),
                };
                for (modulator, curve) in std::iter::once((modulator, curve)).chain(extra) {
                    let ir_bipolar = self.ir.modulators[modulator.0].source.bipolar();
                    let points =
                        match self.live_points(connection, &curve, bipolar, |m| match law {
                            Law::Factor if ratio >= 0.0 => Mapper::position(m, bipolar),
                            Law::Factor => 1.0 - Mapper::position(m, bipolar),
                            Law::Add(_) if ir_bipolar => (m + 1.0) * 0.5,
                            Law::Add(_) => m,
                        })? {
                            Ok(points) => points,
                            Err(gap) => return Ok(Err(gap)),
                        };
                    let route =
                        self.route(modulator, target, depth, points, scale.clone(), smoothing);
                    out.routes.push(route);
                }
                if let Some(shared) = shared {
                    self.share(shared);
                }
            }
        }
        Ok(Ok(()))
    }

    /// Breakpoints `(w, f(m))` of a live source through `node`'s inversion and
    /// mapper: the source curve's knots plus where mapper knots fall inside it.
    fn live_points(
        &mut self,
        node: Node,
        curve: &[(f64, f64)],
        bipolar: bool,
        f: impl Fn(f64) -> f64,
    ) -> Result<Result<Vec<(f64, f64)>, Gap>, String> {
        let mapper = match self.connection_mapper(node)? {
            Ok(mapper) => mapper,
            Err(gap) => return Ok(Err(gap)),
        };
        let inverted = number(node, "Inverted", 0.0)? != 0.0;
        let invert = |s: f64| match (inverted, bipolar) {
            (false, _) => s,
            (true, true) => -s,
            (true, false) => 1.0 - s,
        };
        // Breakpoints: the source curve's (a repeated input is a jump, kept
        // as both sides), plus where the mapper's table knots fall inside
        // each of its segments.
        let mut points: Vec<(f64, f64)> = Vec::new();
        let mut push = |w: f64, s: f64| {
            let s = invert(s);
            let m = mapper.as_ref().map_or(s, |m| m.apply(s, bipolar));
            let point = (w, f(m));
            if points.last() != Some(&point) {
                points.push(point);
            }
        };
        for pair in curve.windows(2) {
            let ((w0, s0), (w1, s1)) = (pair[0], pair[1]);
            push(w0, s0);
            if let Some(mapper) = mapper.as_ref().filter(|_| w1 > w0) {
                let p0 = Mapper::position(invert(s0), bipolar);
                let p1 = Mapper::position(invert(s1), bipolar);
                if p0 != p1 {
                    let mut ts: Vec<f64> = mapper
                        .knots()
                        .map(|knot| (knot - p0) / (p1 - p0))
                        .filter(|t| *t > 0.0 && *t < 1.0)
                        .collect();
                    ts.sort_by(f64::total_cmp);
                    for t in ts {
                        push(w0 + t * (w1 - w0), s0 + t * (s1 - s0));
                    }
                }
            }
            push(w1, s1);
        }
        Ok(Ok(points))
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
        scale: Option<(ir::ModulatorRef, Vec<(f64, f64)>)>,
        smoothing: f64,
    ) -> ir::RouteRef {
        let near = |f: &dyn Fn(f64) -> f64| points.iter().all(|&(w, y)| (y - f(w)).abs() < 1e-9);
        let mut route = ir::Route::new(source, target, depth);
        route.smoothing = ir::Time::Seconds(smoothing);
        if near(&|w| 1.0 - w) && !near(&|w| w) {
            route.invert = true;
        } else if !near(&|w| w) {
            route.shape = Some(self.shape(points));
        }
        route.scale = scale.map(|(source, points)| ir::RouteScale {
            source,
            shape: (!points.iter().all(|&(w, y)| (y - w).abs() < 1e-9)).then(|| self.shape(points)),
        });
        let key = format!("{route:?}");
        *self.route_index.entry(key).or_insert_with(|| {
            self.ir.routes.push(route);
            ir::RouteRef(self.ir.routes.len() - 1)
        })
    }

    fn shape(&mut self, points: Vec<(f64, f64)>) -> ir::ShapeRef {
        let key = format!("{points:?}");
        if let Some(&shape) = self.shape_index.get(&key) {
            return shape;
        }
        self.ir.shapes.push(ir::Shape { points });
        let shape = ir::ShapeRef(self.ir.shapes.len() - 1);
        self.shape_index.insert(key, shape);
        shape
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
                scale: None,
                smoothing: 0.0,
                extra: None,
                shared: None,
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
            return Ok(Ok(Signal::Fixed {
                value: 0.0,
                bipolar: polar,
            }));
        }
        let inputs = live_inputs(node)?;
        let modulable = |c: &Node| {
            matches!(
                (kind, c.attribute("Destination")),
                ("LFO" | "MultiLFO", Some("Depth")) | ("LFO", Some("Freq"))
            )
        };
        if !inputs.iter().all(modulable) {
            return Ok(Err(gap(
                "modulated modulation source",
                format!("{kind} {}", describe(&inputs)),
                NotModeled,
            )));
        }
        // A modulated LFO `Depth` takes the factor law (v1-measured, as
        // `Ratio`): Depth × (1 − max(r, 0) + r·u). A constant input folds into
        // Depth; one live input becomes the route's depth scale.
        //
        // A modulated LFO `Freq` adds 20·r·s Hz (v1, measured on unsynced
        // LFOs); the IR has no rate input, so only constant sources fold.
        let (mut factor, mut scale, mut hertz) = (1.0, None, 0.0);
        for nested in inputs {
            if nested.attribute("Destination") == Some("Freq") {
                if number(node, "SyncToHost", 0.0)? != 0.0 {
                    let what = describe(&[nested]);
                    return Ok(Err(gap(
                        "synchronized LFO Freq modulation",
                        what,
                        UnknownLaw,
                    )));
                }
                match self.signal(node, nested)? {
                    Err(gap) => return Ok(Err(gap)),
                    Ok(Signal::Fixed { value, bipolar }) => {
                        match self.stage(nested, value, bipolar)? {
                            Ok(value) => hertz += 20.0 * number(nested, "Ratio", 1.0)? * value,
                            Err(gap) => return Ok(Err(gap)),
                        }
                    }
                    Ok(Signal::Live { .. }) => {
                        let retrigger = number(node, "Retrigger", 1.0)?;
                        let what = format!("{}, Retrigger {retrigger}", describe(&[nested]));
                        return Ok(Err(gap(
                            "LFO rate modulated by a live source",
                            what,
                            NotModeled,
                        )));
                    }
                }
                continue;
            }
            let r = number(nested, "Ratio", 1.0)?.clamp(-1.0, 1.0);
            match self.signal(node, nested)? {
                Err(gap) => return Ok(Err(gap)),
                Ok(Signal::Live {
                    modulator,
                    curve,
                    bipolar,
                    scale: None,
                    smoothing: 0.0,
                    extra: None,
                    shared,
                }) if scale.is_none() => {
                    if let Some(shared) = shared {
                        self.share(shared);
                    }
                    let f = |m: f64| 1.0 - r.max(0.0) + r * Mapper::position(m, bipolar);
                    match self.live_points(nested, &curve, bipolar, f)? {
                        Ok(points) => scale = Some((modulator, points)),
                        Err(gap) => return Ok(Err(gap)),
                    }
                }
                Ok(Signal::Live { .. }) => {
                    return Ok(Err(gap(
                        "LFO depth modulated by several, smoothed or summed live sources",
                        describe(&[nested]),
                        NotModeled,
                    )));
                }
                Ok(Signal::Fixed { value, bipolar }) => match self.stage(nested, value, bipolar)? {
                    Ok(value) => factor *= 1.0 - r.max(0.0) + r * Mapper::position(value, bipolar),
                    Err(gap) => return Ok(Err(gap)),
                },
            }
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
                Ok(Ok(Signal::Fixed {
                    value: if polar { 2.0 * value - 1.0 } else { value },
                    bipolar: polar,
                }))
            }
            "ScriptEventModulation" => {
                // The script sets it per voice or for all voices
                // (`sendScriptModulation`); its id is the number in the name,
                // as the scripts' own id tables show.
                let id = node
                    .attribute("Name")
                    .and_then(|n| n.rsplit(' ').next())
                    .and_then(|t| t.parse::<u16>().ok());
                let Some(id) = id else {
                    return Ok(Err(gap(
                        "ScriptEventModulation without an id",
                        "",
                        NotModeled,
                    )));
                };
                if number(node, "Bipolar", 1.0)? != 0.0 && self.shared_sources.insert(node.id()) {
                    self.ir.unsupported.push(ir::Unsupported {
                        location: path(node),
                        feature: "bipolar ScriptEventModulation (negative script values read as 0)"
                            .into(),
                        value: id.to_string(),
                        reason: NotModeled,
                    });
                }
                let source = ir::ModulationSource::Script(id);
                Ok(Ok(Signal::Live {
                    modulator: self.modulator(format!("{source:?}"), source),
                    curve: identity,
                    bipolar: false,
                    scale: None,
                    smoothing: 0.0,
                    extra: None,
                    shared: None,
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
                let shared = self.shared(node, Vec::new());
                let polar = number(node, "Bipolar", 0.0)? != 0.0;
                Ok(Ok(Signal::Live {
                    modulator,
                    curve: if polar { bipolar } else { identity },
                    bipolar: polar,
                    scale: None,
                    smoothing: 0.0,
                    extra: None,
                    shared,
                }))
            }
            "LFO" | "MultiLFO" | "StepEnvelope" => {
                let signal = match kind {
                    "LFO" => self.lfo(node, factor, scale, hertz)?,
                    "MultiLFO" => self.multi_lfo(node, factor, scale)?,
                    _ => self.steps(node)?,
                };
                let mut signal = signal;
                if let Ok(Signal::Live {
                    modulator,
                    extra,
                    shared,
                    ..
                }) = &mut signal
                {
                    let mut modulators = vec![*modulator];
                    modulators.extend(extra.as_ref().map(|e| e.0));
                    *shared = self.shared(node, modulators);
                }
                Ok(signal)
            }
            _ => Ok(Err(gap("modulation source", kind, NotModeled))),
        }
    }

    /// UVI runs a program- or layer-level source once, for every note it
    /// serves. A program-level retriggered LFO becomes the IR's instrument-wide
    /// LFO restarted by every voice start; how UVI's shared instance treats
    /// overlapping notes is inferred, not measured, so it is reported. A
    /// layer-level one (restarted by its layer's notes only) and shared
    /// envelopes stay per voice, reported as approximated. Free-running LFOs
    /// are already one shared cycle.
    fn shared(&self, node: Node, modulators: Vec<ir::ModulatorRef>) -> Option<Shared> {
        let scope = node.parent_element()?.parent_element()?.tag_name().name();
        let retriggered = modulators.iter().any(|m| {
            matches!(&self.ir.modulators[m.0].source, ir::ModulationSource::Lfo(l) if l.retrigger)
        });
        let lfo = !modulators.is_empty();
        if !matches!(scope, "Program" | "Layer")
            || (lfo && !retriggered)
            || self.shared_sources.contains(&node.id())
        {
            return None;
        }
        Some(Shared {
            node: node.id(),
            location: path(node),
            feature: format!("{scope}-level {}", node.tag_name().name()),
            master: scope == "Program" && lfo,
            modulators,
        })
    }

    fn share(&mut self, shared: Shared) {
        if !self.shared_sources.insert(shared.node) {
            return;
        }
        let value = if shared.master {
            for m in &shared.modulators {
                self.ir.modulators[m.0].scope = ir::Scope::Master;
            }
            "one instance restarted by every note-on (overlapping notes: inferred, not measured)"
        } else {
            "one shared instance in UVI, approximated per voice"
        };
        self.ir.unsupported.push(ir::Unsupported {
            location: shared.location,
            feature: shared.feature,
            value: value.into(),
            reason: ir::Reason::UnknownLaw,
        });
    }

    fn lfo(
        &mut self,
        node: Node,
        factor: f64,
        scale: Option<(ir::ModulatorRef, Vec<(f64, f64)>)>,
        hertz: f64,
    ) -> Result<Result<Signal, Gap>, String> {
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
        let phase = number(node, "Phase", 0.0)?.rem_euclid(1.0);
        // Measured (v1): a one-pole keeping (1/3)^(1/(Smooth·rate)) per
        // sample, so τ = Smooth/ln 3 and 99% of a step after Smooth·ln 100/ln 3.
        let smooth = number(node, "Smooth", 0.0)?;
        let starts_at_zero = shape != ir::LfoShape::Square && (phase == 0.0 || phase == 0.5);
        let smoothing = if smooth < NEGLIGIBLE_SMOOTH {
            0.0
        } else if let Err(why) = smoothable(retrigger, starts_at_zero) {
            return Ok(Err(gap(
                "LFO Smooth",
                format!("{smooth}: {why}"),
                NotModeled,
            )));
        } else {
            smooth * 100f64.ln() / 3f64.ln()
        };
        let polar = number(node, "Bipolar", 1.0)? != 0.0;
        let delay = number(node, "DelayTime", 0.0)?.max(0.0);
        let rise = number(node, "RiseTime", 0.0)?.max(0.0);
        // The IR delays and fades an LFO per voice towards its centre; UVI
        // fades a unipolar LFO from 0 and times a free one from program start.
        if (delay > 0.0 || rise > 0.0) && (!polar || !retrigger) {
            let what = format!("Bipolar {polar}, Retrigger {retrigger}");
            return Ok(Err(gap("LFO delay or rise", what, NotModeled)));
        }
        let freq = (number(node, "Freq", 0.5)? + hertz).clamp(0.0, 20.0);
        let rate = if number(node, "SyncToHost", 0.0)? != 0.0 {
            // Synchronized Freq is the cycle length in beats: tempo/60/Freq Hz.
            ir::Frequency::Beats(freq)
        } else {
            ir::Frequency::Hertz(freq)
        };
        if freq <= 0.0 {
            return Ok(Err(gap(
                "LFO Freq",
                freq.to_string(),
                ir::Reason::InvalidValue,
            )));
        }
        let depth = number(node, "Depth", 1.0)?.clamp(0.0, 1.0) * factor;
        let lfo = ir::Lfo {
            shape,
            rate,
            delay: ir::Time::Seconds(delay),
            fade_in: ir::Time::Seconds(rise),
            phase,
            retrigger,
        };
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
            scale,
            smoothing,
            extra: None,
            shared: None,
        }))
    }

    /// A `MultiLFO`. Measured (v1, `UVI_MULTILFO_ENDPOINT_EVIDENCE.md`) only for
    /// SineDepth 1 with NoiseDepth n ≥ 0, normalized and bipolar:
    /// Depth·(sin 2π(phase) + n·noise)/(1 + n), the noise a uniform −1..1 value
    /// drawn for each half cycle (an IR sample-and-hold at twice the rate),
    /// Smooth an Euler one-pole of time constant Smooth per 32-frame point.
    /// Other waveform mixes, normalization and polarity are unmeasured.
    fn multi_lfo(
        &mut self,
        node: Node,
        factor: f64,
        scale: Option<(ir::ModulatorRef, Vec<(f64, f64)>)>,
    ) -> Result<Result<Signal, Gap>, String> {
        use ir::Reason::{InvalidValue, NotModeled, UnknownLaw};
        let weights: Vec<f64> = ["SineDepth", "TriangleDepth", "SawDepth", "SquareDepth"]
            .into_iter()
            .map(|k| number(node, k, 0.0))
            .collect::<Result<_, _>>()?;
        let noise = number(node, "NoiseDepth", 0.0)?;
        let settings = [
            ("NormalizeOutput", 1.0),
            ("Bipolar", 1.0),
            ("Invert", 0.0),
            ("RiseTime", 0.0),
            ("ManualTrigger", 0.0),
        ];
        let mut unmeasured = Vec::new();
        if weights != [1.0, 0.0, 0.0, 0.0] || !(0.0..=1.0).contains(&noise) {
            unmeasured.push(format!("waveforms {weights:?} + noise {noise}"));
        }
        for (name, measured) in settings {
            let value = number(node, name, measured)?;
            if value != measured {
                unmeasured.push(format!("{name} {value}"));
            }
        }
        if !unmeasured.is_empty() {
            return Ok(Err(gap("MultiLFO", unmeasured.join(", "), UnknownLaw)));
        }
        let retrigger = match number(node, "Retrigger", 1.0)? {
            1.0 => true,
            0.0 => false,
            mode => return Ok(Err(gap("MultiLFO Retrigger", mode.to_string(), NotModeled))),
        };
        let phase = number(node, "Phase", 0.0)?.rem_euclid(1.0);
        let smooth = number(node, "Smooth", 0.0)?;
        // ponytail: the measured lag is Euler-stepped per 32 frames and starts
        // from 0; the IR lag is its continuous limit (τ = Smooth, within 5% for
        // every authored Smooth ≥ 0.0077 s at 48 kHz) and starts at the noise's
        // first value. Exact discretization needs a per-route lag law.
        let smoothing = if smooth <= 0.0 {
            0.0
        } else if let Err(why) = smoothable(retrigger, phase == 0.0 || phase == 0.5) {
            return Ok(Err(gap(
                "MultiLFO Smooth",
                format!("{smooth}: {why}"),
                NotModeled,
            )));
        } else {
            smooth * 100f64.ln()
        };
        let freq = number(node, "Freq", 0.5)?;
        if !(freq > 0.0 && freq <= 20.0) {
            return Ok(Err(gap("MultiLFO Freq", freq.to_string(), InvalidValue)));
        }
        let synced = number(node, "SyncToHost", 0.0)? != 0.0;
        // Synchronized Freq is the cycle length in beats, as for `LFO`.
        let rate = |cycles: f64| {
            if synced {
                ir::Frequency::Beats(freq / cycles)
            } else {
                ir::Frequency::Hertz(freq * cycles)
            }
        };
        let lfo = |shape, rate, phase| {
            ir::ModulationSource::Lfo(ir::Lfo {
                shape,
                rate,
                delay: ir::Time::ZERO,
                fade_in: ir::Time::ZERO,
                phase,
                retrigger,
            })
        };
        let id = node.id();
        let sine = self.modulator(
            format!("node {id:?}"),
            lfo(ir::LfoShape::Sine, rate(1.0), phase),
        );
        let depth = number(node, "Depth", 1.0)?.clamp(0.0, 1.0) * factor / (1.0 + noise);
        let extra = (noise > 0.0).then(|| {
            let held = lfo(
                ir::LfoShape::SampleAndHold,
                rate(2.0),
                (2.0 * phase).fract(),
            );
            let modulator = self.modulator(format!("node {id:?} noise"), held);
            (modulator, vec![(0.0, -depth * noise), (1.0, depth * noise)])
        });
        Ok(Ok(Signal::Live {
            modulator: sine,
            curve: vec![(0.0, -depth), (1.0, depth)],
            bipolar: true,
            scale,
            smoothing,
            extra,
            shared: None,
        }))
    }

    /// A `StepEnvelope`: `NumSteps` `Levels`, each held for one step of
    /// `Freq` beats when synchronized (v1-measured: step ⌊beat/Freq⌋ mod
    /// NumSteps, free-running on the host beat) or 1/`Freq` s otherwise
    /// (Falcon manual: steps per second), cycling. An IR rising saw spans one
    /// cycle; the staircase is the route shape. Measured unipolar, held,
    /// unsmoothed and at Depth 1 only.
    fn steps(&mut self, node: Node) -> Result<Result<Signal, Gap>, String> {
        use ir::Reason::{InvalidValue, NotModeled, UnknownLaw};
        let mut unmeasured = Vec::new();
        for (name, measured) in [
            ("Bipolar", 0.0),
            ("InterpolationMode", 0.0),
            ("Smooth", 0.0),
            ("Depth", 1.0),
        ] {
            let value = number(node, name, measured)?;
            if value != measured {
                unmeasured.push(format!("{name} {value}"));
            }
        }
        if !unmeasured.is_empty() {
            return Ok(Err(gap("StepEnvelope", unmeasured.join(", "), UnknownLaw)));
        }
        if number(node, "ManualTrigger", 0.0)? != 0.0 {
            return Ok(Err(gap("StepEnvelope ManualTrigger", "", NotModeled)));
        }
        // Trigger modes are the LFO's (manual): 0 free, 1 per note, 2 legato.
        let retrigger = match number(node, "Retrigger", 0.0)? {
            0.0 => false,
            1.0 => true,
            mode => {
                return Ok(Err(gap(
                    "StepEnvelope Retrigger",
                    mode.to_string(),
                    NotModeled,
                )));
            }
        };
        let count = number(node, "NumSteps", 16.0)?;
        let levels: Option<Vec<f64>> = node
            .attribute("Levels")
            .unwrap_or_default()
            .split_whitespace()
            .map(|v| {
                v.replace(',', ".")
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
            })
            .collect();
        let freq = number(node, "Freq", 1.0)?;
        let levels = match levels {
            Some(levels)
                if (1.0..=128.0).contains(&count)
                    && count.fract() == 0.0
                    && levels.len() >= count as usize
                    && freq > 0.0 =>
            {
                levels
            }
            _ => {
                return Ok(Err(gap(
                    "StepEnvelope steps, levels or Freq",
                    "",
                    InvalidValue,
                )));
            }
        };
        let n = count as usize;
        let rate = if number(node, "SyncToHost", 0.0)? != 0.0 {
            ir::Frequency::Beats(freq * count)
        } else {
            ir::Frequency::Hertz(freq / count)
        };
        let saw = ir::Lfo {
            shape: ir::LfoShape::SawUp,
            rate,
            delay: ir::Time::ZERO,
            fade_in: ir::Time::ZERO,
            phase: 0.0,
            retrigger,
        };
        let modulator = self.modulator(
            format!("node {:?}", node.id()),
            ir::ModulationSource::Lfo(saw),
        );
        let curve = levels[..n]
            .iter()
            .enumerate()
            .flat_map(|(k, &level)| [(k as f64 / count, level), ((k + 1) as f64 / count, level)])
            .collect();
        Ok(Ok(Signal::Live {
            modulator,
            curve,
            bipolar: false,
            scale: None,
            smoothing: 0.0,
            extra: None,
            shared: None,
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

#[cfg(test)]
mod tests {
    use sampler_ir as ir;

    /// One keygroup with `keygroup` connections and a player with `player` ones.
    fn translate(sources: &str, keygroup: &str, player: &str) -> ir::Instrument {
        let xml = format!(
            r#"<Program Name="t"><ControlSignalSources>{sources}</ControlSignalSources>
            <Mappers><ControlSignalMapper Name="Tent" Min="0" Max="1">0,0 1,0 0,0</ControlSignalMapper></Mappers>
            <Layers><Layer Name="L"><Keygroups><Keygroup Name="K">
            <Connections>{keygroup}</Connections>
            <Oscillators><SamplePlayer Name="P" SamplePath="a.wav"><Connections>{player}</Connections></SamplePlayer></Oscillators>
            </Keygroup></Keygroups></Layer></Layers></Program>"#
        );
        crate::translate_bank(&xml).unwrap().0
    }

    fn only_route(ir: &ir::Instrument) -> (&ir::Route, &ir::ModulationSource) {
        assert_eq!(ir.zones[0].routes.len(), 1, "{:?}", ir.unsupported);
        let route = &ir.routes[ir.zones[0].routes[0].0];
        (route, &ir.modulators[route.source.0].source)
    }

    fn shape<'a>(ir: &'a ir::Instrument, route: &ir::Route) -> &'a [(f64, f64)] {
        &ir.shapes[route.shape.unwrap().0].points
    }

    #[test]
    fn velocity_and_controller_gain_are_attenuation_routes() {
        let ir = translate(
            "",
            r#"<SignalConnection Source="@VoiceParam Velocity" Destination="Gain" Ratio="0.75"/>"#,
            "",
        );
        let (route, source) = only_route(&ir);
        assert_eq!(source, &ir::ModulationSource::Velocity);
        assert_eq!(
            (route.target, route.depth, route.invert, route.shape),
            (
                ir::Target::Amplitude,
                ir::Depth::Normalized(0.75),
                false,
                None
            )
        );
        // Negative ratio: gain × (1 + r·u) = 1 − |r|·u, the inverted source.
        let ir = translate(
            "",
            r#"<SignalConnection Source="@MIDI CC 11" Destination="Gain" Ratio="-1"/>"#,
            "",
        );
        let (route, source) = only_route(&ir);
        assert_eq!(source, &ir::ModulationSource::Controller(11));
        assert_eq!(
            (route.depth, route.invert),
            (ir::Depth::Normalized(1.0), true)
        );
        assert!(ir.unsupported.is_empty(), "{:?}", ir.unsupported);
    }

    #[test]
    fn pitch_bend_and_lfo_pitch_use_semitone_ratios() {
        let ir = translate(
            "",
            "",
            r#"<SignalConnection Source="@PitchBend" Destination="Pitch" Ratio="2"/>"#,
        );
        let (route, source) = only_route(&ir);
        assert_eq!(source, &ir::ModulationSource::PitchBend);
        assert_eq!(route.depth, ir::Depth::Pitch(ir::Pitch::Semitones(2.0)));
        assert_eq!(route.shape, None);

        // A unipolar LFO at depth 0.5 reads 0.5·(x + 1)/2: v = 2y − 1 = s.
        let ir = translate(
            r#"<LFO Name="V" Freq="0.5" SyncToHost="1" Depth="0.5" Bipolar="0" Retrigger="0" Phase="0.25" WaveFormType="2" Smooth="5.2776863e-09"/>"#,
            "",
            r#"<SignalConnection Source="$Program/V" Destination="Pitch" Ratio="1"/>"#,
        );
        let (route, source) = only_route(&ir);
        assert_eq!(
            source,
            &ir::ModulationSource::Lfo(ir::Lfo {
                shape: ir::LfoShape::Triangle,
                rate: ir::Frequency::Beats(0.5),
                delay: ir::Time::Seconds(0.0),
                fade_in: ir::Time::Seconds(0.0),
                phase: 0.25,
                retrigger: false,
            })
        );
        assert_eq!(shape(&ir, route), [(0.0, 0.5), (1.0, 0.75)]);
    }

    #[test]
    fn mappers_become_shapes_and_constants_fold() {
        let ir = translate(
            r#"<ConstantModulation Name="M" Value="0.25"/>"#,
            r#"<SignalConnection Source="$Program/M" Destination="Gain" Ratio="1"/>"#,
            r#"<SignalConnection Source="@MIDI CC 1" Destination="Pitch" Ratio="12" Mapper="Tent">
                 <Connections><SignalConnection Source="$Program/M" Destination="Ratio" Ratio="1"/></Connections>
               </SignalConnection>
               <SignalConnection Source="$Program/M" Destination="Pitch" Ratio="4"/>"#,
        );
        let zone = &ir.zones[0];
        assert_eq!(zone.gain, ir::Gain::Linear(0.25));
        assert_eq!(zone.tune, ir::Pitch::Semitones(1.0));
        let (route, _) = only_route(&ir);
        // Ratio 12 × the macro's factor 0.25.
        assert_eq!(route.depth, ir::Depth::Pitch(ir::Pitch::Semitones(3.0)));
        assert_eq!(shape(&ir, route), [(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)]);
    }

    #[test]
    fn key_follow_is_exact_at_every_key() {
        let ir = translate(
            "",
            r#"<SignalConnection Source="@VoiceParam LinearKeyFollow" Destination="Pan" Ratio="0.5"/>"#,
            "",
        );
        let (route, source) = only_route(&ir);
        assert_eq!(source, &ir::ModulationSource::Key);
        assert_eq!(route.depth, ir::Depth::Normalized(0.5));
        let points = shape(&ir, route);
        assert_eq!(points.len(), 128);
        assert_eq!(points[0], (0.0, -1.0));
        assert_eq!(points[90], (90.0 / 127.0, 0.5));
    }

    #[test]
    fn inert_connections_vanish_and_shared_ones_are_one_route() {
        let ir = translate(
            r#"<StepEnvelope Name="S"/>"#,
            r#"<SignalConnection Source="$Program/S" Destination="Gain" Ratio="0"/>
               <SignalConnection Source="$Program/S" Destination="Pan" Ratio="1" Bypass="1"/>
               <SignalConnection Source="@VoiceParam Velocity" Destination="Gain" Ratio="1"/>"#,
            r#"<SignalConnection Source="@VoiceParam Velocity" Destination="Gain" Ratio="1"/>"#,
        );
        assert!(ir.unsupported.is_empty(), "{:?}", ir.unsupported);
        assert_eq!(ir.routes.len(), 1);
        assert_eq!(ir.zones[0].routes, [ir::RouteRef(0), ir::RouteRef(0)]);
    }

    #[test]
    fn lfo_depth_by_mod_wheel_is_a_route_scale() {
        let lfo = r#"<LFO Name="V" Depth="0.5"><Connections>
               <SignalConnection Source="@MIDI CC 1" Destination="Depth" Ratio="1"/>
               <SignalConnection Source="$Program/M" Destination="Depth" Ratio="-0.5"/>
             </Connections></LFO><ConstantModulation Name="M" Value="1"/>"#;
        let ir = translate(
            lfo,
            "",
            r#"<SignalConnection Source="$Program/V" Destination="Pitch" Ratio="2"/>"#,
        );
        let (route, source) = only_route(&ir);
        assert!(matches!(source, ir::ModulationSource::Lfo(_)));
        // Depth 0.5 × the macro's factor 1 − 0.5·1 = 0.25: v = 0.25·x.
        assert_eq!(shape(&ir, route), [(0.0, 0.375), (1.0, 0.625)]);
        // × CC1's factor 1 − 1 + 1·u = u.
        let scale = route.scale.expect("depth by CC1 is a route scale");
        assert_eq!(
            ir.modulators[scale.source.0].source,
            ir::ModulationSource::Controller(1)
        );
        assert_eq!(scale.shape, None);
        // Through a gain (factor law) the product is not separable: reported.
        let ir = translate(
            lfo,
            r#"<SignalConnection Source="$Program/V" Destination="Gain" Ratio="1"/>"#,
            "",
        );
        assert!(ir.zones[0].routes.is_empty());
        assert_eq!(
            ir.unsupported[0].feature,
            "live LFO depth through a nonlinear connection"
        );
    }

    #[test]
    fn untranslatable_connections_are_reported() {
        let ir = translate(
            r#"<AHD Name="S"/><LFO Name="W" WaveFormType="7"/><LFO Name="V"/>"#,
            r#"<SignalConnection Source="$Program/S" Destination="Gain" Ratio="1"/>
               <SignalConnection Source="$Program/W" Destination="Pan" Ratio="1"/>
               <SignalConnection Source="@PitchBend" Destination="Gain" Ratio="1"/>"#,
            r#"<SignalConnection Source="$Program/V" Destination="Pitch" Ratio="1">
                 <Connections><SignalConnection Source="@VoiceParam Key" Destination="Ratio" Ratio="-1"/></Connections>
               </SignalConnection>"#,
        );
        let reasons: Vec<_> = ir
            .unsupported
            .iter()
            .map(|u| (u.feature.as_str(), u.reason))
            .collect();
        assert_eq!(
            reasons,
            [
                ("modulation source", ir::Reason::NotModeled),
                ("LFO waveform", ir::Reason::UnknownLaw),
                (
                    "pitch bend outside pitch (bend is native note expression)",
                    ir::Reason::NotModeled
                ),
                ("Program-level LFO", ir::Reason::UnknownLaw),
            ]
        );
        assert_eq!(ir.unsupported[0].value, "$Program/S -> Gain: AHD");
        assert_eq!(ir.zones[0].routes.len(), 1);
        // LFO pitch depth × (1 − key position): a modulator × modulator product.
        let route = &ir.routes[ir.zones[0].routes[0].0];
        let scale = route.scale.expect("ratio by key is a route scale");
        assert_eq!(
            ir.modulators[scale.source.0].source,
            ir::ModulationSource::Key
        );
        let points = &ir.shapes[scale.shape.unwrap().0].points;
        assert_eq!(points.first(), Some(&(0.0, 1.0)));
        assert_eq!(points.last(), Some(&(1.0, 0.0)));
    }

    fn routes(ir: &ir::Instrument) -> Vec<&ir::Route> {
        ir.zones[0].routes.iter().map(|r| &ir.routes[r.0]).collect()
    }

    fn lfo(ir: &ir::Instrument, route: &ir::Route) -> ir::Lfo {
        match &ir.modulators[route.source.0].source {
            ir::ModulationSource::Lfo(lfo) => *lfo,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn multi_lfo_sine_and_noise_are_two_smoothed_routes() {
        let ir = translate(
            r#"<MultiLFO Name="M" SineDepth="1" NoiseDepth="0.5" Depth="0.6" Freq="4" Phase="0.5" Smooth="0.1"/>"#,
            r#"<SignalConnection Source="$Program/M" Destination="Pitch" Ratio="2"/>"#,
            "",
        );
        let routes = routes(&ir);
        assert_eq!(routes.len(), 2, "{:?}", ir.unsupported);
        // Depth 0.6/(1 + 0.5) = 0.4 of sine, 0.2 of noise, as IR shapes over w.
        for (route, shape, hertz, phase, points) in [
            (
                routes[0],
                ir::LfoShape::Sine,
                4.0,
                0.5,
                [(0.0, 0.3), (1.0, 0.7)],
            ),
            (
                routes[1],
                ir::LfoShape::SampleAndHold,
                8.0,
                0.0,
                [(0.0, 0.4), (1.0, 0.6)],
            ),
        ] {
            let lfo = lfo(&ir, route);
            assert_eq!(
                (lfo.shape, lfo.rate, lfo.phase),
                (shape, ir::Frequency::Hertz(hertz), phase)
            );
            assert!(lfo.retrigger);
            assert_eq!(route.depth, ir::Depth::Pitch(ir::Pitch::Semitones(2.0)));
            let got = shape_points(&ir, route);
            assert!(
                got.iter()
                    .zip(points)
                    .all(|(a, b)| (a.0 - b.0).abs() < 1e-12 && (a.1 - b.1).abs() < 1e-12),
                "{got:?}"
            );
            assert!((route.smoothing.seconds() - 0.1 * 100f64.ln()).abs() < 1e-12);
        }
        // A sum cannot pass the gain law, nor can unmeasured waveform mixes.
        let ir = translate(
            r#"<MultiLFO Name="M" SineDepth="1" NoiseDepth="0.5"/><MultiLFO Name="T" TriangleDepth="1"/>"#,
            r#"<SignalConnection Source="$Program/M" Destination="Gain" Ratio="1"/>
               <SignalConnection Source="$Program/T" Destination="Pan" Ratio="1"/>"#,
            "",
        );
        assert!(ir.zones[0].routes.is_empty());
        let reasons: Vec<_> = ir.unsupported.iter().map(|u| u.reason).collect();
        assert_eq!(reasons, [ir::Reason::NotModeled, ir::Reason::UnknownLaw]);
    }

    fn shape_points(ir: &ir::Instrument, route: &ir::Route) -> Vec<(f64, f64)> {
        route
            .shape
            .map_or_else(Vec::new, |s| ir.shapes[s.0].points.clone())
    }

    #[test]
    fn step_envelope_is_a_free_saw_through_a_staircase() {
        let ir = translate(
            r#"<StepEnvelope Name="S" SyncToHost="1" Freq="0.25" NumSteps="4" Levels="0,25 1 0 0,5 0,9"/>
               <StepEnvelope Name="B" Bipolar="1" Levels="0"/>"#,
            r#"<SignalConnection Source="$Program/S" Destination="Gain" Ratio="1"/>
               <SignalConnection Source="$Program/B" Destination="Gain" Ratio="1"/>"#,
            "",
        );
        let (route, _) = only_route(&ir);
        let lfo = lfo(&ir, route);
        assert_eq!(
            (lfo.shape, lfo.rate, lfo.retrigger),
            (ir::LfoShape::SawUp, ir::Frequency::Beats(1.0), false)
        );
        assert_eq!(
            (route.target, route.depth),
            (ir::Target::Amplitude, ir::Depth::Normalized(1.0))
        );
        assert_eq!(
            shape_points(&ir, route),
            [
                (0.0, 0.25),
                (0.25, 0.25),
                (0.25, 1.0),
                (0.5, 1.0),
                (0.5, 0.0),
                (0.75, 0.0),
                (0.75, 0.5),
                (1.0, 0.5)
            ]
        );
        assert_eq!(ir.unsupported.len(), 1);
        assert_eq!(
            (ir.unsupported[0].feature.as_str(), ir.unsupported[0].reason),
            ("StepEnvelope", ir::Reason::UnknownLaw)
        );
    }

    #[test]
    fn lfo_freq_folds_constants_and_smooth_becomes_a_lag() {
        let ir = translate(
            r#"<ConstantModulation Name="M" Value="0.5"/>
               <LFO Name="V" Freq="1" Smooth="0.1"><Connections>
                 <SignalConnection Source="$Program/M" Destination="Freq" Ratio="0.1"/>
               </Connections></LFO>
               <LFO Name="W"><Connections>
                 <SignalConnection Source="@MIDI CC 1" Destination="Freq" Ratio="0.1"/>
               </Connections></LFO>"#,
            r#"<SignalConnection Source="$Program/V" Destination="Pitch" Ratio="1"/>
               <SignalConnection Source="$Program/W" Destination="Pitch" Ratio="1"/>"#,
            "",
        );
        let (route, _) = only_route(&ir);
        // 1 Hz + 20 × 0.1 × 0.5.
        assert_eq!(lfo(&ir, route).rate, ir::Frequency::Hertz(2.0));
        assert!((route.smoothing.seconds() - 0.1 * 100f64.ln() / 3f64.ln()).abs() < 1e-12);
        let features: Vec<_> = ir.unsupported.iter().map(|u| u.feature.as_str()).collect();
        assert_eq!(
            features,
            ["Program-level LFO", "LFO rate modulated by a live source"]
        );
        // A program-level retriggered LFO is one instance restarted by every note.
        assert_eq!(ir.modulators[route.source.0].scope, ir::Scope::Master);
        assert!(ir.unsupported[0].value.contains("inferred, not measured"));
    }
}
