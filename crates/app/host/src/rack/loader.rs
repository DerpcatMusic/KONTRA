//! Loads off the control thread: a part's instrument is read and its
//! samples prepared on a loader thread of the rack's own, and the control
//! tick only installs what is ready ([`Rack::finish_loads`]). The latest
//! load of a part wins; an older one that ends later is dropped.

use std::path::PathBuf;
use std::sync::{Arc, mpsc};

use super::{Built, Rack};
use crate::state::{SavedId, SavedPart};

/// One load: what to read into which part.
pub(crate) struct Job {
    pub part: usize,
    generation: u64,
    pub path: PathBuf,
    /// A saved part brought back: its scripts' values, how it sat in the
    /// rack ([`Rack::restored`]), and the state's identities.
    pub restore: Option<(SavedPart, Vec<SavedId>)>,
    /// What the read needs of the rack as it is now.
    known: Arc<Vec<PathBuf>>,
    moved: Vec<(PathBuf, PathBuf)>,
    saves: Option<PathBuf>,
    /// The Kontakt snapshot bases the rack holds decoded, the part's own
    /// first: a snapshot onto one of them reads only the snapshot.
    pub bases: Vec<Arc<kontra_format_kontakt::SnapshotBase>>,
    /// The base a snapshot read onto, kept for the part's next one.
    snapshot: Option<Arc<kontra_format_kontakt::SnapshotBase>>,
}

impl Job {
    pub fn new(part: usize, path: PathBuf) -> Self {
        Self {
            part,
            generation: 0,
            path,
            restore: None,
            known: Arc::default(),
            moved: Vec::new(),
            saves: None,
            bases: Vec::new(),
            snapshot: None,
        }
    }
}

type Done = (Job, Result<Built, crate::Error>);

/// The rack's loads: its loader thread, once started, and each part's
/// latest load.
#[derive(Default)]
pub(crate) struct Loads {
    thread: Option<(mpsc::Sender<Job>, mpsc::Receiver<Done>)>,
    /// Per part, the generation of its latest load.
    latest: Vec<u64>,
    /// Loads not installed yet, as a project would save them.
    running: Vec<(usize, u64, SavedPart, bool)>,
}

/// A Kontakt snapshot at `job.path` onto its base, decoded once per base
/// ([`Job::bases`]); None for any other file, or a base that does not
/// open (the snapshot then reads as any file does).
fn snapshot(job: &mut Job) -> Option<Result<kontra_engine::ir::Instrument, crate::Error>> {
    let nksn = (job.path.extension()).is_some_and(|e| e.eq_ignore_ascii_case("nksn"));
    if !nksn || job.restore.is_some() {
        return None;
    }
    let preset = crate::instrument::preset(&job.path, &job.known)?;
    let held = std::mem::take(&mut job.bases);
    let held = held.into_iter().find(|b| b.path() == preset.base);
    // A snapshot read a moment ago reads as it did, onto the base held.
    let key = crate::instrument::read_key(&job.path, &[], &job.known);
    if let Some(hit) = key.as_ref().and_then(crate::instrument::recalled) {
        job.snapshot = held;
        return Some(Ok(hit));
    }
    let base = match held {
        Some(base) => base,
        None => Arc::new(kontra_format_kontakt::open_snapshot_base(&preset.base).ok()?),
    };
    let read = kontra_format_kontakt::load_snapshot_onto(&base, &job.path, &job.known);
    job.snapshot = Some(base);
    if let (Some(key), Ok(read)) = (key, &read) {
        crate::instrument::remember(key, read);
    }
    Some(read.map_err(Into::into))
}

/// Reads `job`'s instrument and prepares it with `store`.
fn read(job: &mut Job, rate: u32, store: &mut super::Store) -> Result<Built, crate::Error> {
    let snapshot = snapshot(job);
    let scripts = job
        .restore
        .as_ref()
        .map_or(&[][..], |(p, _)| &p.scripts[..]);
    let mut instrument = match snapshot {
        Some(read) => read?,
        None => crate::instrument::read_saved(&job.path, scripts, &job.known)?,
    };
    if job.restore.is_some() {
        for (script, saved) in instrument.scripts.iter_mut().zip(scripts) {
            script.saved.clone_from(saved);
        }
        // Its values are the state the scripts reached after the preset was
        // recalled; recalling them again would run that recall twice.
        instrument.recalled = false;
    }
    Ok(Rack::build_part(
        instrument,
        rate,
        store,
        &job.moved,
        job.saves.as_deref(),
    ))
}

impl Rack {
    /// Loads from now on run on a thread of the rack's own: the control
    /// thread never waits on a disk. It ends with the rack, after the load
    /// it is in. Without this, loads run where they are asked. `ready` runs
    /// on that thread after each load, for the caller to come and
    /// [`Rack::finish_loads`].
    pub fn load_in_background(&mut self, ready: impl Fn() + Send + 'static) {
        let (jobs, take) = mpsc::channel::<Job>();
        let (give, done) = mpsc::channel();
        let (rate, mut store) = (self.rate, self.store.loader());
        let spawned = std::thread::Builder::new()
            .name("kontra-loader".into())
            .spawn(move || {
                for mut job in take {
                    let built = read(&mut job, rate, &mut store);
                    if give.send((job, built)).is_err() {
                        return;
                    }
                    ready();
                }
            });
        if spawned.is_ok() {
            self.loads.thread = Some((jobs, done));
        }
    }

    /// Starts `job`, superseding any load of its part. In the background
    /// this returns at once and [`Rack::finish_loads`] installs it; else it
    /// loads now.
    pub(crate) fn post(&mut self, mut job: Job) -> Result<(), crate::Error> {
        if let Some(progress) = self.store.progress() {
            progress.begin();
        }
        let loads = &mut self.loads;
        loads.latest.resize(loads.latest.len().max(job.part + 1), 0);
        if let Some(latest) = loads.latest.get_mut(job.part) {
            *latest += 1;
            job.generation = *latest;
        }
        (job.known, job.moved) = (Arc::clone(&self.known.1), self.moved.clone());
        job.saves.clone_from(&self.saves);
        let saved = job.restore.as_ref().map_or_else(
            || SavedPart::new(job.part, job.path.clone(), self.mix_of(job.part)),
            |(p, _)| p.clone(),
        );
        let loads = &mut self.loads;
        loads.running.retain(|r| r.0 != job.part);
        loads.running.push((job.part, job.generation, saved, false));
        let mut job = match &loads.thread {
            Some((jobs, _)) => match jobs.send(job) {
                Ok(()) => return Ok(()),
                Err(mpsc::SendError(job)) => job,
            },
            None => job,
        };
        let built = read(&mut job, self.rate, &mut self.store);
        self.land(job, built)
    }

    /// Installs a finished load, unless a later one of its part superseded
    /// it.
    fn land(&mut self, job: Job, built: Result<Built, crate::Error>) -> Result<(), crate::Error> {
        let at = (job.part, job.generation);
        self.loads.running.retain(|r| (r.0, r.1) != at);
        if self.loads.latest.get(job.part) != Some(&job.generation) {
            return Ok(());
        }
        let (types, responses) = job
            .restore
            .as_ref()
            .map_or_else(Default::default, |(p, _)| {
                crate::state::live_slots(&p.slot_types, &p.slot_responses)
            });
        self.install_built(job.part, built?, Some(job.path), &types, &responses)?;
        if let (Some(base), Some(slot)) = (job.snapshot, self.snapshot_bases.get_mut(job.part)) {
            *slot = Some(base);
        }
        if let Some((p, _)) = &job.restore {
            self.restored(p);
        }
        Ok(())
    }

    /// Installs the loads finished in the background: the parts to place
    /// ([`Rack::place_later`]), and what failed, for the report.
    pub fn finish_loads(&mut self) -> (Vec<usize>, Vec<String>) {
        let (mut placed, mut failed) = (Vec::new(), Vec::new());
        let Some(done) = self
            .loads
            .thread
            .as_ref()
            .map(|t| t.1.try_iter().collect::<Vec<_>>())
        else {
            return (placed, failed);
        };
        for (job, built) in done {
            let (part, path) = (job.part, job.path.clone());
            let place = (self.loads.running.iter())
                .any(|r| (r.0, r.1, r.3) == (part, job.generation, true));
            let restore = job.restore.clone();
            match (self.land(job, built), restore) {
                (Ok(()), _) if place && self.parts.get(part).is_some_and(Option::is_some) => {
                    placed.push(part);
                }
                (Ok(()), _) => {}
                // As a restore says it at once ([`Rack::restore`]).
                (Err(e), Some((p, ids))) if !crate::relink::present(&p.path) => {
                    self.awaiting.push((p, ids));
                    failed.push(format!(
                        "part {}: {e}; its library moved or is missing: it loads once found (Locate\u{2026} in the browser)",
                        part + 1
                    ));
                }
                (Err(e), Some(_)) => failed.push(format!("part {}: {e}", part + 1)),
                (Err(e), None) => failed.push(format!("{}: {e}", path.display())),
            }
        }
        (placed, failed)
    }

    /// Marks `part`'s running load to be placed once in; false when none
    /// runs (it is in already).
    pub fn place_later(&mut self, part: usize) -> bool {
        let running = self.loads.running.iter_mut().find(|r| r.0 == part);
        running.map(|r| r.3 = true).is_some()
    }

    /// Loads not installed yet.
    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.loads.running.len()
    }

    /// Drops every running load of `part`, or of every part (a restore).
    pub(crate) fn supersede(&mut self, part: Option<usize>) {
        let loads = &mut self.loads;
        for (n, g) in loads.latest.iter_mut().enumerate() {
            if part.is_none_or(|p| p == n) {
                *g += 1;
            }
        }
        loads.running.retain(|r| part.is_some_and(|p| p != r.0));
    }

    /// The running loads, as a project saves them.
    pub(crate) fn running(&self) -> impl Iterator<Item = &SavedPart> {
        self.loads.running.iter().map(|r| &r.2)
    }
}
