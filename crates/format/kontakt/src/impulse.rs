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
    pub(crate) fn new(chunks: &KontaktChunks, resolver: &'a mut Resolver, path: &'a Path) -> Self {
        let files = if let Some(c) = chunks.find_first(0x4b) {
            FNTableImpl::try_from(c).map(|t| t.other_filetable).ok()
        } else {
            chunks
                .find_first(0x3d)
                .and_then(|c| FileNameListPreK51::try_from(c).ok())
                .map(|t| t.other_filetable)
        };
        Self {
            files: files.unwrap_or_default(),
            resolver,
            resources: None,
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
            Some(EffectValue::Float(v)) => Some(*v),
            _ => None,
        };
        let flag = |n: usize| matches!(fields.get(n).map(|f| &f.value), Some(EffectValue::Byte(b)) if *b != 0);
        let index = match fields.last().map(|f| &f.value) {
            Some(EffectValue::Integer(i)) => u32::try_from(*i).ok()?,
            _ => return None,
        };
        let name = self.files.get(&index)?.clone();
        let floats = |n: usize| match fields.get(n).map(|f| &f.value) {
            Some(EffectValue::Floats(v)) => <[f32; 8]>::try_from(v.as_slice()).ok(),
            _ => None,
        };
        // The size setter 0x1419b6750 keeps only sizes in 0.5..=1.5.
        let size = |n: usize| float(n).filter(|s| (0.5..=1.5).contains(s)).unwrap_or(1.0);
        let mut lost = |feature: &str| notes.add(ir::Impact::Audible, feature, at);
        // ponytail: Kontakt stretches by the size with zplane's Élastique
        // (0x1419b3450) and resamples the wet path by S.Rate; neither runs here.
        if size(3) != 1.0 || size(6) != 1.0 {
            lost("impulse response size");
        }
        if float(0).is_some_and(|d| d > 1.0) {
            lost("impulse response S.Rate");
        }
        let cut = |low: usize, high: usize| {
            [
                float(low).unwrap_or(0.0),
                float(high).unwrap_or(f32::INFINITY),
            ]
        };
        let (early, late) = (cut(4, 5), cut(7, 8));
        let crossover = float(9).filter(|x| *x >= 0.0);
        if early != late && crossover.is_none() {
            lost("impulse response early/late crossover");
        }
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
        let sample = match self.samples.iter().position(|s| s.name == name) {
            Some(found) => found,
            None => {
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
                let location = match found {
                    Ok(location) => location,
                    Err(e) => {
                        notes.add(
                            ir::Impact::Audible,
                            "missing impulse response",
                            format!("{at}: {name}: {e}"),
                        );
                        return None;
                    }
                };
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
                self.samples.len() - 1
            }
        };
        self.out.push(ir::Impulse {
            sample,
            reverse: flag(10),
            normalize: flag(11).then_some(crate::laws::AUTO_GAIN),
            predelay_ms: float(2).unwrap_or(0.0).max(0.0),
            cuts,
            envelope,
        });
        u16::try_from(self.out.len() - 1).ok()
    }
}
