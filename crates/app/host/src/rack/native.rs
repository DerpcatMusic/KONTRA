//! A snapshot that saves the instrument's effects too (Kontakt's native
//! snapshot state), recalled into a part playing its base: the part's plan
//! with the snapshot's effects is made on a thread of its own (it reads the
//! snapshot's instrument and its responses), and a later [`Rack::tick`]
//! restarts the part on it with the snapshot's values. Neither the audio
//! thread nor the rack waits for the read.

use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};

use kontra_engine::{Plan, Saved, ir};
use kontra_format_kontakt::SnapshotBase;

use super::Rack;
use super::recall::{Started, started};

/// A recall whose plan is being made.
pub(crate) struct Native {
    pub(super) part: usize,
    /// The part's load: another load or recall makes this one stale.
    generation: u64,
    started: mpsc::Receiver<Started>,
    loaded: Vec<Vec<String>>,
}

/// `plan` (a part's, of the base instrument `base`) with the effects of
/// the snapshot at `path`, and what its reading and those effects report;
/// as it is when the snapshot cannot be read (the values still land),
/// with why.
///
/// Reuses an already-open [`SnapshotBase`] when available (or opens it once
/// and returns it for caching) so repeated `.nksn` preset swaps do not re-open
/// or re-decrypt the base `.nki`.
fn rebuilt(
    plan: &Plan,
    path: &Path,
    base: &[PathBuf],
    cached_base: Option<Arc<SnapshotBase>>,
    rate: u32,
) -> (Plan, Vec<ir::Diagnostic>, Option<Arc<SnapshotBase>>) {
    let mut plan = plan.clone();
    let is_nksn = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("nksn"));
    let mut opened_base = None;
    let instrument = if is_nksn {
        if let Some(cached) = cached_base {
            kontra_format_kontakt::load_snapshot_onto(&cached, path, base)
                .map_err(crate::Error::from)
        } else if let Some(base_path) = base
            .first()
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nki")) && p.is_file())
        {
            match kontra_format_kontakt::open_snapshot_base(base_path) {
                Ok(opened) => {
                    let arc = Arc::new(opened);
                    let res = kontra_format_kontakt::load_snapshot_onto(&arc, path, base)
                        .map_err(crate::Error::from);
                    opened_base = Some(arc);
                    res
                }
                Err(_) => crate::instrument::read_saved(path, &[], base),
            }
        } else {
            crate::instrument::read_saved(path, &[], base)
        }
    } else {
        crate::instrument::read_saved(path, &[], base)
    };
    match instrument {
        Ok(instrument) => {
            let mut diagnostics = Vec::new();
            kontra_prepare::effects(&mut plan, &instrument, rate, &mut diagnostics);
            diagnostics.extend(instrument.diagnostics);
            (plan, diagnostics, opened_base)
        }
        Err(e) => {
            let why = ir::Diagnostic {
                impact: ir::Impact::Audible,
                feature: "snapshot effects unread".into(),
                detail: e.to_string(),
            };
            (plan, vec![why], opened_base)
        }
    }
}

impl Rack {
    /// Starts making `part`'s plan with the effects of the snapshot at
    /// `path`; `saved` and `loaded` are its values ([`Rack::recall`]).
    pub(super) fn recall_native(
        &mut self,
        part: usize,
        plan: Arc<Plan>,
        path: &Path,
        (saved, loaded): (Vec<Saved>, Vec<Vec<String>>),
    ) {
        let Some(Some(p)) = self.parts.get(part) else {
            return;
        };
        let (send, started_rx) = mpsc::channel();
        let (path, rate, client) = (path.to_owned(), self.rate, self.store.client());
        let (faces, near) = (p.faces.clone(), p.source.clone());
        let generation = p.script.generation();
        let cached_base = near
            .as_deref()
            .and_then(|base| self.snapshot_base_for(part, base));
        // The snapshot's racks are the effects: no slots a script loaded.
        std::thread::spawn(move || {
            let (plan, diagnostics, opened_base) =
                rebuilt(&plan, &path, near.as_slice(), cached_base, rate);
            let face = (faces.as_slice(), near.as_deref().or(Some(path.as_path())));
            // None waits when the part moved on (another load or recall).
            let mut started = started(plan, &Default::default(), saved, (client, rate), face);
            started.diagnostics = diagnostics;
            started.snapshot_base = opened_base;
            let _ = send.send(started);
        });
        self.native.push(Native {
            part,
            generation,
            started: started_rx,
            loaded,
        });
    }

    /// Puts each part whose snapshot engine is started in its place.
    pub(super) fn native_ready(&mut self) {
        let mut made = Vec::new();
        for n in std::mem::take(&mut self.native) {
            match n.started.try_recv() {
                Ok(started) => made.push((started, n)),
                Err(mpsc::TryRecvError::Empty) => self.native.push(n),
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
        for (started, n) in made {
            let live = self.parts.get(n.part).and_then(Option::as_ref);
            if live.is_some_and(|p| p.script.generation() == n.generation) {
                self.restarted(n.part, started, n.loaded);
            }
        }
    }
}
