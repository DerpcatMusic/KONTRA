//! Loads off the control thread: a part's instrument is read and its
//! samples prepared on a loader thread of the rack's own, and the control
//! tick only installs what is ready ([`Rack::finish_loads`]). The latest
//! load of a part wins; an older one that ends later is dropped.

use std::path::PathBuf;
use std::sync::{Arc, mpsc};

use super::{Built, Rack};
use crate::state::SavedPart;

/// One load: what to read into which part.
pub(crate) struct Job {
    pub part: usize,
    generation: u64,
    pub path: PathBuf,
    /// What the read needs of the rack as it is now.
    known: Arc<Vec<PathBuf>>,
    moved: Vec<(PathBuf, PathBuf)>,
    saves: Option<PathBuf>,
}

impl Job {
    pub fn new(part: usize, path: PathBuf) -> Self {
        Self {
            part,
            generation: 0,
            path,
            known: Arc::default(),
            moved: Vec::new(),
            saves: None,
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

/// Reads `job`'s instrument and prepares it with `store`.
fn read(job: &Job, rate: u32, store: &mut super::Store) -> Result<Built, crate::Error> {
    let instrument = crate::instrument::read_saved(&job.path, &[], &job.known)?;
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
    /// it is in. Without this, loads run where they are asked.
    pub fn load_in_background(&mut self) {
        let (jobs, take) = mpsc::channel::<Job>();
        let (give, done) = mpsc::channel();
        let (rate, mut store) = (self.rate, self.store.loader());
        let spawned = std::thread::Builder::new()
            .name("kontra-loader".into())
            .spawn(move || {
                for job in take {
                    let built = read(&job, rate, &mut store);
                    if give.send((job, built)).is_err() {
                        return;
                    }
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
        let saved = SavedPart::new(job.part, job.path.clone(), self.mix_of(job.part));
        let loads = &mut self.loads;
        loads.running.retain(|r| r.0 != job.part);
        loads.running.push((job.part, job.generation, saved, false));
        let job = match &loads.thread {
            Some((jobs, _)) => match jobs.send(job) {
                Ok(()) => return Ok(()),
                Err(mpsc::SendError(job)) => job,
            },
            None => job,
        };
        let built = read(&job, self.rate, &mut self.store);
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
        self.install_built(job.part, built?, Some(job.path), &[], &[])
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
            match self.land(job, built) {
                Ok(()) if place && self.parts.get(part).is_some_and(Option::is_some) => {
                    placed.push(part);
                }
                Ok(()) => {}
                Err(e) => failed.push(format!("{}: {e}", path.display())),
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
