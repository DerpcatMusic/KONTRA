//! Convolution responses: the record names a file in the instrument's
//! other-files table, and how Kontakt's wrapper prepares it (reverse, Auto
//! Gain, predelay).

use std::collections::HashMap;

use kontra_ir as ir;
use ni_file::kontakt::KontaktChunks;
use ni_file::kontakt::objects::{EffectParameters, EffectValue, FNTableImpl, FileNameListPreK51};

use std::path::Path;

use crate::resources::Resources;
use crate::samples::Resolver;
use kontra_format::Notes;

/// The instrument's sample and other-files tables, read once: a large
/// library's table holds about 100k names.
pub(crate) fn file_tables(chunks: &KontaktChunks) -> Option<Result<Tables, ni_file::Error>> {
    if let Some(c) = chunks.find_first(0x4b) {
        return Some(FNTableImpl::try_from(c).map(|t| (t.sample_filetable, t.other_filetable)));
    }
    let c = chunks.find_first(0x3d)?;
    Some(FileNameListPreK51::try_from(c).map(|t| (t.sample_filetable, t.other_filetable)))
}

/// Sample names and other files, by id.
pub(crate) type Tables = (HashMap<u32, String>, HashMap<u32, String>);

pub(crate) struct Impulses<'a> {
    files: HashMap<u32, String>,
    resolver: &'a mut Resolver,
    /// The library's resource containers, opened on the first miss.
    resources: Option<Resources>,
    path: &'a Path,
    pub(crate) out: Vec<ir::Impulse>,
    /// The responses' files, appended to the instrument's samples.
    pub(crate) samples: Vec<ir::Sample>,
}

impl<'a> Impulses<'a> {
    /// `files`: the instrument's other-files table ([`file_tables`]);
    /// `resources`: the library's containers if the scripts opened them.
    pub(crate) fn new(
        files: HashMap<u32, String>,
        resolver: &'a mut Resolver,
        path: &'a Path,
        resources: Option<Resources>,
    ) -> Self {
        Self {
            files,
            resolver,
            resources,
            path,
            out: Vec::new(),
            samples: Vec::new(),
        }
    }

    /// The Convolution (0x16) record's response, prepared, as an index into
    /// [`Self::out`]. Field order: v2 sampler-kontakt effects/mod.rs (reader
    /// 0x16): S.Rate, block, predelay, early and late size/low/high, crossover,
    /// five flags (reverse, Auto Gain, preserve length, latency bypass,
    /// envelope), two curves, the file index.
    pub(crate) fn convolution(
        &mut self,
        version: u16,
        public: &[u8],
        at: &str,
        notes: &mut Notes,
    ) -> Option<u16> {
        let fields = EffectParameters::read(0x16, version, public).ok()??.fields;
        let float = |n: usize| match fields.get(n).map(|f| &f.value) {
            Some(EffectValue::Float(v)) => Some(*v).filter(|v| v.is_finite()),
            _ => None,
        };
        let flag = |n: usize| matches!(fields.get(n).map(|f| &f.value), Some(EffectValue::Byte(b)) if *b != 0);
        let index = match fields.last().map(|f| &f.value) {
            Some(EffectValue::Integer(i)) => u32::try_from(*i).ok()?,
            _ => return None,
        };
        let name = self.files.get(&index)?.clone();
        let floats = |n: usize| match fields.get(n).map(|f| &f.value) {
            Some(EffectValue::Floats(v)) => <[f32; 8]>::try_from(v.as_slice())
                .ok()
                .filter(|v| v.iter().all(|x| x.is_finite())),
            _ => None,
        };
        // The size setter 0x1419b6750 keeps only sizes in 0.5..=1.5.
        let size = |n: usize| float(n).filter(|s| (0.5..=1.5).contains(s)).unwrap_or(1.0);
        // ponytail: Kontakt stretches by the size with zplane's Élastique
        // (0x1419b3450, offline into the response); not copied (one corpus
        // instrument, 2026-10-28).
        if size(3) != 1.0 || size(6) != 1.0 {
            let what = format!("{at}: early {} late {}", size(3), size(6));
            notes.add(ir::Impact::Audible, "impulse response size", what);
        }
        // S.Rate runs the wet path at rate/d between a resampler pair
        // (0x140ab5e00, 0x140a84580). Auto (d < 0) with Preserve Length keeps
        // the response's length and picks the largest d of 1, 1.5 .. 8 that
        // keeps rate/d at or above the response's own rate: it drops only what
        // the response cannot hold, so the full-rate convolution here is the
        // same signal. Every corpus use (366 instruments) is that one.
        let auto_kept = float(0).is_some_and(|d| d < 0.0) && flag(12);
        if let Some(d) = float(0).filter(|d| (*d > 1.0 || *d < 0.0) && !auto_kept) {
            let what = format!("{at}: 1/{d}, preserve length {}", flag(12));
            notes.add(ir::Impact::Audible, "impulse response S.Rate", what);
        }
        let mut lost = |feature: &str| notes.add(ir::Impact::Audible, feature, at);
        let cut = |low: usize, high: usize| {
            [
                float(low).unwrap_or(0.0),
                float(high).unwrap_or(f32::INFINITY),
            ]
        };
        let (early, late) = (cut(4, 5), cut(7, 8));
        // A negative crossover keeps CConvolver's own split (0x1419b16b0,
        // run on every response it loads): left as None, the engine finds it.
        let crossover = float(9);
        if early != late && crossover.is_none() {
            lost("impulse response early/late crossover");
        }
        let crossover = crossover.filter(|x| *x >= 0.0);
        // Library defaults: 20 Hz and 20 kHz (Una Corda stores 19999.992).
        let open = |[low, high]: [f32; 2]| low <= 20.0 && high >= 19_999.0;
        let cuts = (!(open(early) && open(late))).then_some(ir::ImpulseCuts {
            early,
            late,
            crossover,
        });
        let envelope = if flag(14) {
            let knots = floats(15).zip(floats(16)).map(|(x, db)| {
                std::array::from_fn(|i| {
                    [
                        x.get(i).copied().unwrap_or(0.0),
                        db.get(i).copied().unwrap_or(0.0),
                    ]
                })
            });
            if knots.is_none() {
                lost("impulse response envelope");
            }
            knots
        } else {
            None
        };
        let sample = match self.sample(name) {
            Ok(sample) => sample,
            Err((name, e)) => {
                notes.add(
                    ir::Impact::Audible,
                    "missing impulse response",
                    format!("{at}: {name}: {e}"),
                );
                return None;
            }
        };
        self.out.push(ir::Impulse {
            sample,
            reverse: flag(10),
            normalize: flag(11).then_some(crate::laws::AUTO_GAIN),
            predelay_ms: float(2).unwrap_or(0.0).max(0.0),
            cuts,
            envelope,
            mix: None,
        });
        u16::try_from(self.out.len() - 1).ok()
    }

    /// The response file `name` as an index into [`Self::samples`], added
    /// on first use: from the resolver, else the library's resources.
    fn sample(&mut self, name: String) -> Result<usize, (String, String)> {
        if let Some(found) = self.samples.iter().position(|s| s.name == name) {
            return Ok(found);
        }
        let found = self.resolver.resolve(&name).and_then(|l| match l {
            Some(l) => Ok(l),
            // A container's own resources (`.nicnt`): the name after `Resources/`.
            None => {
                let inner = name
                    .rsplit_once("Resources/")
                    .map_or(name.as_str(), |(_, n)| n);
                let path = self.path;
                let resources = self.resources.get_or_insert_with(|| Resources::open(path));
                if let Some(found) = resources.locate(inner) {
                    return Ok(found);
                }
                let archives: Vec<_> = resources
                    .archives()
                    .map(|a| format!("{}/Resources/{inner}", a.display()))
                    .collect();
                archives
                    .iter()
                    .find_map(|member| self.resolver.resolve(member).ok().flatten())
                    .ok_or_else(|| "not found".to_string())
            }
        });
        let location = found.map_err(|e| (name.clone(), e.to_string()))?;
        self.samples.push(ir::Sample {
            name,
            file: location.file,
            offset: location.offset,
            len: location.size,
            channels: 0,
            frames: 0,
            rate: 0,
            keystream: location.keystream,
        });
        Ok(self.samples.len() - 1)
    }

    /// The responses in the library's `ir_samples` folder, which a script's
    /// `load_ir_sample` names, as (lowercase name without extension, index
    /// into [`Self::samples`]). Only located, not read.
    pub(crate) fn library(&mut self) -> Vec<(String, usize)> {
        let path = self.path;
        let names = self
            .resources
            .get_or_insert_with(|| Resources::open(path))
            .listed("ir_samples");
        names
            .into_iter()
            .filter_map(|n| {
                let stem = n.rsplit_once('.').map_or(n.as_str(), |(stem, _)| stem);
                let stem = stem.to_lowercase();
                Some((stem, self.sample(format!("Resources/ir_samples/{n}")).ok()?))
            })
            .collect()
    }
}

/// The Convolution's script knobs, measured natively: a write rendered
/// through the response (rig: tools/kontakt-oracle/fxrig.py, set from
/// `on note`, `generic` 1; a write in `on init` is lost to the preset's
/// own load). Predelay moves the wet onset `2·(151^x − 1)` ms (x 0.1, .25,
/// .5, .75, 1 → 1.29, 5.0, 22.6, 84.2, 300 ms, within 0.2 ms); the cuts
/// are `20·1000^x` Hz (low cut 0, .25, .5 → 20.6, 112, 600 Hz; high-pass
/// .5, .75 → 634, 3591 Hz, fitted as the second-order Butterworths the
/// response is shaped with).
///
/// ponytail: the length ratios (ER and LR size: Kontakt stretches the
/// response offline) and Auto Gain, Reverse and the ER/LR boundary (no
/// corpus script writes them) are not carried.
pub(crate) fn script_knobs() -> [(&'static str, ir::KnobTarget, ir::Law); 5] {
    use ir::ImpulseKnob::{Cut, PredelayMs};
    use ir::KnobTarget::Impulse;
    let predelay: Vec<f32> = (0..=32)
        .map(|i| 2.0 * (151f32.powf(i as f32 / 32.0) - 1.0))
        .collect();
    let hz = || ir::Law::Exp {
        lo: 20.0,
        hi: 20_000.0,
    };
    let cut = |late, high| Impulse(Cut { late, high });
    [
        (
            "$ENGINE_PAR_IRC_PREDELAY",
            Impulse(PredelayMs),
            ir::Law::Table(predelay.into()),
        ),
        ("$ENGINE_PAR_IRC_FREQ_HIGHPASS_ER", cut(false, false), hz()),
        ("$ENGINE_PAR_IRC_FREQ_LOWPASS_ER", cut(false, true), hz()),
        ("$ENGINE_PAR_IRC_FREQ_HIGHPASS_LR", cut(true, false), hz()),
        ("$ENGINE_PAR_IRC_FREQ_LOWPASS_LR", cut(true, true), hz()),
    ]
}
