//! Zone and sample commands that read what the instrument saved
//! (`get_sample`, `get_zone_status`, `get_loop_par`), the music-information
//! `detect_*` commands, and `redirect_output`.
//!
//! KONTRA has no user zones (`set_num_user_zones` makes none), so every
//! command that needs one (`set_sample`, `set_loop_par`) fails the way
//! Kontakt's do for a zone that is not a user zone, and says so in the
//! unhandled-action count.

use kontra_ir as ir;
use kontra_plan::files::{DetectJob, FileReply, FileRequest};
use kontra_vm::{Action, HostCall, HostValue, Text};

use crate::Engine;
use crate::detect::{self, Audio, Kind};
use crate::queue::Exit;

/// Longest sample a detection reads: a minute at 48 kHz.
const MAX_FRAMES: u64 = 48_000 * 60;

/// The detection `what` codes of [`DetectJob`].
fn kind_of(what: u8) -> Kind {
    match what {
        0 => Kind::Key,
        1 => Kind::Pitch,
        2 => Kind::Loudness,
        3 => Kind::Peak,
        4 => Kind::Rms,
        5 => Kind::Tempo,
        _ => Kind::Class,
    }
}

/// Measures `sample` for `what`; None when it cannot be read or the measure
/// fails. Off the audio thread (it reads the whole file).
#[must_use]
pub fn detect_sample(sample: &ir::Sample, what: u8) -> Option<f64> {
    let kind = kind_of(what);
    if kind == Kind::Class {
        return None;
    }
    let (rate, frames) = kontra_stream::decode(sample, MAX_FRAMES).ok()?;
    let audio = Audio {
        frames: &frames,
        rate,
    };
    match kind {
        Kind::Key => detect::key(&audio).map(f64::from),
        Kind::Pitch => detect::pitch(&audio),
        Kind::Loudness => detect::loudness(&audio),
        Kind::Peak => detect::peak(&audio),
        Kind::Rms => detect::rms(&audio),
        Kind::Tempo => detect::tempo(&audio),
        Kind::Class => None,
    }
}

/// What a failed detection reads, as the constants `~NI_DETECT_*_INVALID`.
fn invalid(what: u8) -> f64 {
    match kind_of(what) {
        Kind::Key | Kind::Class => -1.0,
        Kind::Pitch | Kind::Tempo => -1.0,
        Kind::Loudness | Kind::Peak | Kind::Rms => -1000.0,
    }
}

/// The cell value of a detection: an integer for the tags, a real's bits else.
fn cell_value(what: u8, value: Option<f64>) -> i64 {
    let v = value.unwrap_or_else(|| invalid(what));
    match kind_of(what) {
        Kind::Key | Kind::Class => v as i64,
        _ => v.to_bits() as i64,
    }
}

impl Engine {
    fn facts_of(&self, zone: i32) -> Option<&kontra_plan::ZoneFacts> {
        let id = u32::try_from(zone).ok()?;
        self.plan.zone_facts.iter().find(|z| z.id == id)
    }

    /// The zone commands above; None for any other action.
    pub(crate) fn zone_call(&mut self, call: &mut HostCall<'_>) -> Option<HostValue> {
        use Action::*;
        let int = |i: usize| call.ints.get(i).map_or(0, |&v| v as i32);
        Some(match call.action {
            GetSample => {
                let path = self
                    .facts_of(int(0))
                    .and_then(|z| z.source.as_ref())
                    .map(|s| s.file.clone());
                let text = path.map_or(String::new(), |p| match int(1) {
                    // $NI_FILE_NAME: without folder or extension.
                    0 => p
                        .file_stem()
                        .map_or(String::new(), |s| s.to_string_lossy().into_owned()),
                    // $NI_FILE_FULL_PATH uses forward slashes, the _OS one the system's.
                    1 => p.to_string_lossy().replace('\\', "/"),
                    2 => p.to_string_lossy().into_owned(),
                    _ => p
                        .extension()
                        .map_or(String::new(), |e| e.to_string_lossy().into_owned()),
                });
                *call.text_out = Text::from_str_cut(&text).0;
                HostValue::Text
            }
            // The zone at an index of the instrument's list; -1 past its end.
            GetZoneId => HostValue::Int(
                usize::try_from(int(0))
                    .ok()
                    .and_then(|i| self.plan.zone_facts.get(i))
                    .map_or(-1, |z| i32::try_from(z.id).unwrap_or(-1)),
            ),
            // Every zone the instrument saved, played or not.
            GetNumZones => {
                HostValue::Int(self.plan.zone_facts.len().max(self.plan.zones.len()) as i32)
            }
            // The whole sample, microseconds; -1 for a zone the instrument lacks.
            GetSampleLength => HostValue::Int(self.facts_of(int(0)).map_or(-1, |z| {
                let frames = z.source.as_ref().map_or(z.end, |s| s.frames);
                i32::try_from(frames * 1_000_000 / u64::from(z.rate.max(1))).unwrap_or(i32::MAX)
            })),
            GetZoneStatus => {
                let group = self.facts_of(int(0)).map(|z| z.group as usize);
                // $NI_ZONE_STATUS_LOADED 1, _PURGED 2; -1 for a zone the instrument lacks.
                HostValue::Int(match group {
                    None => -1,
                    Some(g) if self.groups.get(g).is_some_and(|g| g.purged) => 2,
                    Some(_) => 1,
                })
            }
            GetLoopPar => {
                let at = usize::try_from(int(1)).ok();
                let l = self
                    .facts_of(int(0))
                    .and_then(|z| z.loops.get(at?))
                    .copied();
                // $LOOP_PAR_MODE reads 1 for a loop that exists (the mode laws are unmeasured).
                HostValue::Int(l.map_or(0, |l| match int(2) {
                    0 => 1,
                    1 => l.start as i32,
                    2 => l.end.saturating_sub(l.start) as i32,
                    3 => l.crossfade as i32,
                    4 => l.count as i32,
                    _ => 0,
                }))
            }
            // Only user zones take a sample or a loop, and there are none.
            SetSample | SetLoopPar => {
                self.count_unhandled(call.action);
                HostValue::Int(if self.building {
                    -1
                } else {
                    self.start_async(Exit::Failed)
                })
            }
            SetNumUserZones | SetMidi => {
                self.count_unhandled(call.action);
                HostValue::Nothing
            }
            // The Mapping Editor selects nothing here: the list is just its end mark.
            GetSelZonesIdx => {
                if let Some(cell) = usize::try_from(int(0))
                    .ok()
                    .and_then(|b| call.cells.get_mut(b))
                {
                    *cell = -1;
                }
                HostValue::Nothing
            }
            DetectKey | DetectPitch | DetectLoudness | DetectPeak | DetectRms | DetectTempo
            | DetectSampleType | DetectDrumType | DetectInstrumentType => {
                let what = match call.action {
                    DetectKey => 0,
                    DetectPitch => 1,
                    DetectLoudness => 2,
                    DetectPeak => 3,
                    DetectRms => 4,
                    DetectTempo => 5,
                    _ => 6,
                };
                let (zone, cell) = (int(0), u32::try_from(int(1)).unwrap_or(u32::MAX));
                if what == 6 {
                    self.count_unhandled(call.action);
                }
                let sample = self.facts_of(zone).and_then(|z| z.source.clone());
                if self.building {
                    // Under construction (off the audio thread): measured at once.
                    let value = sample.and_then(|s| detect_sample(&s, what));
                    if let Some(c) = call.cells.get_mut(cell as usize) {
                        *c = cell_value(what, value);
                    }
                    return Some(HostValue::Int(-1));
                }
                let Some(z) = u32::try_from(zone).ok().filter(|_| sample.is_some()) else {
                    if let Some(c) = call.cells.get_mut(cell as usize) {
                        *c = cell_value(what, None);
                    }
                    return Some(HostValue::Int(self.start_async(Exit::Failed)));
                };
                let request = FileRequest {
                    slot: self.running.slot,
                    id: 0,
                    base: 0,
                    len: 1,
                    kind: kontra_plan::files::ArrayKind::Int,
                    path: Text::EMPTY,
                    name: Text::EMPTY,
                    save: false,
                    midi: None,
                    detect: Some(DetectJob {
                        zone: z,
                        what,
                        cell,
                    }),
                };
                HostValue::Int(self.request_file(request))
            }
            RedirectOutput => {
                // $OUTPUT_TYPE_BUS_OUT n is plan bus n + 1; the master and aux
                // outputs are the one stereo output, the main bus.
                let (kind, index) = (int(1), int(2));
                let buses = self.buses.len();
                let to = match kind {
                    3 => usize::try_from(index)
                        .ok()
                        .map(|n| n + 1)
                        .filter(|&b| b < buses),
                    1 | 2 => Some(0),
                    _ => None,
                };
                self.each(int(0), |n| {
                    n.redirect = to.and_then(|b| u8::try_from(b).ok())
                });
                HostValue::Nothing
            }
            _ => return None,
        })
    }

    /// A detection's answer: the value lands in its variable and `on
    /// async_complete` follows. Returns whether the reply was a detection.
    pub(crate) fn detect_done(&mut self, reply: &FileReply) -> bool {
        let r = reply.request;
        let Some(job) = r.detect else { return false };
        let value = cell_value(job.what, reply.detected);
        let exit = match self.vm_mut(r.slot) {
            Some(vm) => {
                vm.set_cell(job.cell as usize, value);
                Exit::from(reply.detected.is_some())
            }
            None => Exit::Failed,
        };
        self.queue_async(r.slot, r.id, exit);
        true
    }
}
