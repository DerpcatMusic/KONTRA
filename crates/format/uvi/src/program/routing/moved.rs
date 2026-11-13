//! Live connections on a bus effect's parameters: the Flanger's and the
//! WaveShaper's `Mix`, the Phasor's `Depth`, the WaveShaper's `Amount`.
//! Those take the generic parameter constructor (0x140fffcb0), whose
//! AdditiveParameterModulation (vtable 0x141e37a38, the law 0x141429650)
//! adds `r·m` to the value, held within its range. The effect is rebuilt
//! at the moved values over the source's travel, and each level or control
//! that changes follows that table, so a law that is not linear (the
//! Phasor's `(x + Depth·y)/√(1 + Depth²)`) is exact at its points. The
//! parameters one source moves are tabulated together; ponytail: those of
//! different sources add their changes (exact unless two move one level).

use kontra_format::Notes;
use kontra_ir as ir;
use roxmltree::{Node, NodeId};

use super::super::{Library, Translator, flag, index_of, matrix, source_node};
use super::Params;
use super::shaper::wave_shaper;
use super::sweep::{flanger, phasor};

/// Points a moved effect is tabulated at over its source's travel.
const POINTS: usize = 33;

/// The levels and controls a moved parameter can change.
const FIELDS: [ir::EffectParam; 8] = [
    ir::EffectParam::Output,
    ir::EffectParam::Dry,
    ir::EffectParam::ShaperAmount,
    ir::EffectParam::ModRate,
    ir::EffectParam::ModBeats,
    // A flanger's feedback is the matrix's diagonal; a phaser's is one knob.
    ir::EffectParam::DelayLoop(0),
    ir::EffectParam::DelayLoop(3),
    ir::EffectParam::PhaserFeedback,
];

type Build = fn(Params, &mut Notes) -> Result<Option<ir::Effect>, String>;

/// A moved parameter: its name, saved value, ratio and connection.
type Moved = (&'static str, f64, f32, NodeId);

impl<L: Library> Translator<'_, '_, L> {
    /// Routes the live connections on the moved parameters of the effect at
    /// `node`, in slot `insert` of bus `at`.
    pub(super) fn moves(&mut self, node: Node, at: usize, insert: u8) -> Result<(), String> {
        let (params, build): (&[&'static str], Build) = match node.tag_name().name() {
            "Flanger" => (&["Mix", "Speed", "Feedback"], |p, _| flanger(p).map(Some)),
            "Phasor" => (&["Depth", "Speed", "Feedback"], |p, n| {
                phasor(p, n).map(Some)
            }),
            "WaveShaper" => (&["Mix", "Amount"], wave_shaper),
            _ => return Ok(()),
        };
        let program = self.program;
        // Each moved parameter's one live connection, by its source.
        let mut by_source: Vec<(ir::Transfer, Vec<Moved>)> = Vec::new();
        let mut extra: Vec<NodeId> = Vec::new();
        let mut seen: Vec<NodeId> = Vec::new();
        let mut divisions: Vec<(u8, ir::Blend, NodeId)> = Vec::new();
        // The bus's oscillators; a bus has no voice, so each runs free.
        let mut lfos = self
            .out
            .buses
            .get_mut(at)
            .map(|b| std::mem::take(&mut b.lfos))
            .unwrap_or_default();
        for &name in params {
            let live = matrix::children(node).filter(|c| {
                c.attribute("Destination") == Some(name)
                    && c.attribute("Ratio").and_then(|r| r.parse::<f64>().ok()) != Some(0.0)
            });
            for c in live {
                let Some(term) = matrix::term(program, c, Some(&mut lfos)) else {
                    continue;
                };
                let readable = matches!(
                    term.input.source,
                    ir::Source::Controller(_) | ir::Source::Script(_) | ir::Source::Lfo(_)
                );
                let Some(saved) = node.attribute(name).and_then(|v| v.parse().ok()) else {
                    continue;
                };
                if !readable || !term.factors.is_empty() {
                    continue;
                }
                // A synced oscillator's rate knob moves along the note ladder.
                if let (ir::Source::Lfo(n), Some(lfo)) =
                    (term.input.source, source_node(program, c))
                    && flag(lfo, "SyncToHost")
                    && !seen.contains(&lfo.id())
                {
                    seen.push(lfo.id());
                    let rates = matrix::children(lfo)
                        .filter(|r| r.attribute("Destination") == Some("Freq"))
                        .collect::<Vec<_>>();
                    for r in rates {
                        let t = matrix::term(program, r, Some(&mut lfos));
                        if let Some(t) = t.filter(|t| {
                            t.factors.is_empty()
                                && matches!(
                                    t.input.source,
                                    ir::Source::Controller(_)
                                        | ir::Source::Script(_)
                                        | ir::Source::Lfo(_)
                                )
                        }) {
                            let blend = ir::Blend {
                                base: 0.0,
                                terms: vec![t],
                            };
                            divisions.push((n, blend, r.id()));
                        }
                    }
                }
                let moved = (name, saved, term.amount, c.id());
                match by_source.iter_mut().find(|(t, _)| *t == term.input) {
                    Some((_, m)) => match m.iter_mut().find(|x| x.0 == name) {
                        // One source on one parameter twice: the ratios add.
                        Some(x) => {
                            x.2 += moved.2;
                            extra.push(c.id());
                        }
                        None => m.push(moved),
                    },
                    None => by_source.push((term.input, vec![moved])),
                }
            }
        }
        for l in &mut lfos {
            l.clock = ir::Clock::Free;
        }
        if let Some(b) = self.out.buses.get_mut(at) {
            b.lfos = lfos;
            for (lfo, blend, id) in divisions {
                let source = ir::Source::Blend(index_of(&mut b.blends, blend));
                b.mods.push(ir::ModKind::LfoDivision {
                    source,
                    lfo,
                    depth: 1.0,
                });
                extra.push(id);
            }
        }
        let scratch = &mut Notes::default();
        let Some(rest) = build(Params::of(node), scratch)? else {
            return Ok(());
        };
        for (input, moved) in by_source {
            let mut effects = Vec::with_capacity(POINTS);
            for k in 0..POINTS {
                let m = input.apply(k as f32 / (POINTS - 1) as f32);
                let set: Vec<_> = moved
                    .iter()
                    .map(|&(name, saved, r, _)| {
                        // A synced `Speed` moves along the note ladder.
                        if name == "Speed" && flag(node, "SyncToHost") {
                            let beats = ir::ladder_beats(saved as f32, r * m * 29.0);
                            return (name, f64::from(beats));
                        }
                        (name, saved + f64::from(r * m))
                    })
                    .collect();
                let Some(e) = build(Params { node, set: &set }, scratch)? else {
                    break;
                };
                effects.push(e);
            }
            if effects.len() < POINTS {
                continue;
            }
            let Some(b) = self.out.buses.get_mut(at) else {
                continue;
            };
            for param in FIELDS {
                let Some(base) = field(param, rest) else {
                    continue;
                };
                let table: Vec<f32> = effects
                    .iter()
                    .map(|&e| field(param, e).unwrap_or(base) - base)
                    .collect();
                if table.iter().all(|d| d.abs() < 1e-7) {
                    continue;
                }
                let term = ir::Term {
                    input: ir::Transfer {
                        source: input.source,
                        table,
                        ..Default::default()
                    },
                    amount: 1.0,
                    factors: Vec::new(),
                };
                let blend = ir::Blend {
                    base: 0.0,
                    terms: vec![term],
                };
                let source = ir::Source::Blend(index_of(&mut b.blends, blend));
                b.mods.push(ir::ModKind::EffectParam {
                    source,
                    insert,
                    param,
                    depth: 1.0,
                    span: 1.0,
                    unipolar: false,
                });
            }
            self.carried.extend(moved.iter().map(|m| m.3));
        }
        self.carried.extend(extra);
        Ok(())
    }
}

/// `param`'s value on `e`.
fn field(param: ir::EffectParam, mut e: ir::Effect) -> Option<f32> {
    match param {
        ir::EffectParam::Output => Some(e.output),
        ir::EffectParam::Dry => Some(e.dry),
        _ => param.field_mut(&mut e.kind).copied(),
    }
}
