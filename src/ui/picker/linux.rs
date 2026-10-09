//! Portal results belong to the GUI; file-operation threads belong to the plugin.
use super::{Answer, Ask, Picked};
use crate::ui::lock;
use mui_native_dialog::{
    CancelHandle, DialogFilter, DialogJob, DialogKind, DialogRequest, DialogService, Parent,
};
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, Ordering},
};
use std::thread::JoinHandle;

#[derive(Default)]
struct Workers {
    closed: bool,
    handles: Vec<JoinHandle<()>>,
}

/// Retained by Shared until plugin teardown, including when the editor closes.
#[derive(Default)]
pub(crate) struct Runtime {
    service: DialogService,
    workers: Mutex<Workers>,
    draining: Mutex<()>,
}

impl Runtime {
    fn spawn(&self, run: impl FnOnce() + Send + 'static) -> Result<(), String> {
        let mut workers = lock(&self.workers);
        if workers.closed {
            return Err("The plugin is closing.".into());
        }
        let worker = std::thread::Builder::new()
            .name("kontakto-file-operation".into())
            .spawn(run)
            .map_err(|error| error.to_string())?;
        workers.handles.push(worker);
        Ok(())
    }

    /// Only final plugin teardown joins; native window close signals cancellation.
    pub(crate) fn shutdown(&self) {
        let _draining = lock(&self.draining);
        let handles = {
            let mut workers = lock(&self.workers);
            workers.closed = true;
            std::mem::take(&mut workers.handles)
        };
        self.service.shutdown();
        for worker in handles {
            let _ = worker.join();
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[derive(Default)]
pub(super) struct Operation {
    cancelled: AtomicBool,
    cancel: Mutex<Option<CancelHandle>>,
}

impl Operation {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(cancel) = lock(&self.cancel).as_ref() {
            cancel.cancel();
        }
    }
}

struct Pending {
    ask: Ask,
    job: DialogJob,
    operation: Arc<Operation>,
}

#[derive(Default)]
pub(super) struct State {
    pub(super) runtime: Arc<Runtime>,
    parent: AtomicU32,
    pending: Mutex<Option<Pending>>,
}

impl State {
    pub(super) fn new(runtime: Arc<Runtime>) -> Self {
        Self {
            runtime,
            parent: AtomicU32::new(0),
            pending: Mutex::default(),
        }
    }

    pub(super) fn parent(&self, parent: Option<u32>, answer: &Answer) {
        if let Some(parent) = parent {
            self.parent.store(parent, Ordering::Release);
        } else {
            self.close(answer);
        }
    }

    pub(super) fn close(&self, answer: &Answer) {
        self.parent.store(0, Ordering::Release);
        if let Some(operation) = lock(&answer.current).take() {
            operation.cancel();
        }
        let pending = lock(&self.pending).take();
        drop(pending);
        *lock(&answer.picked) = None;
        answer.open.store(false, Ordering::Release);
    }

    pub(super) fn start(&self, ask: Ask, answer: Arc<Answer>) -> bool {
        let operation = Arc::new(Operation::default());
        *lock(&answer.current) = Some(Arc::clone(&operation));
        if let Ask::Reveal(path) = ask {
            return self.work(answer, operation, move || {
                Some(Picked::Revealed(crate::ui::menu::reveal(&path)))
            });
        }
        let parent = self.parent.load(Ordering::Acquire);
        let request = request(&ask, (parent != 0).then_some(Parent::X11(parent)));
        match request.and_then(|request| self.runtime.service.spawn(request)) {
            Ok(job) => {
                *lock(&operation.cancel) = Some(job.cancel_handle());
                if operation.cancelled.load(Ordering::Acquire) {
                    job.cancel();
                }
                *lock(&self.pending) = Some(Pending {
                    ask,
                    job,
                    operation,
                });
            }
            Err(error) => finish(&answer, &operation, Some(Picked::DialogError(error))),
        }
        true
    }

    pub(super) fn poll(&self, answer: &Arc<Answer>) {
        let completed = {
            let mut pending = lock(&self.pending);
            let Some(active) = pending.as_ref() else {
                return;
            };
            let result = if active.job.is_cancelled() {
                Some(Ok(None))
            } else {
                active.job.try_result()
            };
            result.map(|result| (pending.take().unwrap(), result))
        };
        let Some((pending, result)) = completed else {
            return;
        };
        match result {
            Ok(Some(paths)) => {
                self.work(Arc::clone(answer), pending.operation, move || {
                    selected(pending.ask, paths.into_iter().next())
                });
            }
            Ok(None) => finish(answer, &pending.operation, None),
            Err(error) => finish(answer, &pending.operation, Some(Picked::DialogError(error))),
        }
    }

    fn work(
        &self,
        answer: Arc<Answer>,
        operation: Arc<Operation>,
        run: impl FnOnce() -> Option<Picked> + Send + 'static,
    ) -> bool {
        if operation.cancelled.load(Ordering::Acquire) {
            return false;
        }
        let (result, token) = (Arc::clone(&answer), Arc::clone(&operation));
        match self.runtime.spawn(move || {
            if !token.cancelled.load(Ordering::Acquire) {
                finish(&result, &token, run());
            }
        }) {
            Ok(()) => true,
            Err(error) => {
                finish(&answer, &operation, Some(Picked::DialogError(error)));
                false
            }
        }
    }
}

fn finish(answer: &Answer, operation: &Arc<Operation>, picked: Option<Picked>) {
    let mut current = lock(&answer.current);
    if operation.cancelled.load(Ordering::Acquire)
        || !current
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(active, operation))
    {
        return;
    }
    *lock(&answer.picked) = picked;
    *current = None;
    answer.open.store(false, Ordering::Release);
    crate::ui::logs::wake_worker();
}

fn request(ask: &Ask, parent: Option<Parent>) -> Result<DialogRequest, String> {
    let open = DialogKind::OpenFile { multiple: false };
    let folder = DialogKind::PickFolder { multiple: false };
    let (title, kind, directory, filter): (_, _, _, Option<(&str, Vec<String>)>) = match ask {
        Ask::Samples { out } => (
            "A folder of WAV or AIFF samples",
            folder,
            Some(out.clone()),
            None,
        ),
        Ask::Folder { from, single } => (
            if *single {
                "A Kontakt library folder"
            } else {
                "A folder of Kontakt libraries"
            },
            folder,
            Some(from.clone()),
            None,
        ),
        Ask::Snapshot { from, .. } => (
            "Load a preset for this instrument",
            open,
            Some(from.clone()),
            Some(("Kontakt preset", vec!["nksn".into()])),
        ),
        Ask::Artwork { library } => (
            "A picture for the library's cover",
            open,
            Some(library.clone()),
            Some(("Pictures", vec!["png".into(), "jpg".into(), "jpeg".into()])),
        ),
        Ask::Multi { from, name } => (
            "Save the rack as a multi",
            DialogKind::SaveFile {
                file_name: Some(format!("{name}.{}", crate::library::MULTI)),
            },
            Some(from.clone()),
            Some(("KONTRA multi", vec![crate::library::MULTI.into()])),
        ),
        Ask::Reveal(_) => return Err("Reveal does not use a file dialog.".into()),
    };
    Ok(DialogRequest {
        parent,
        title: title.into(),
        kind,
        directory,
        filters: filter
            .into_iter()
            .map(|(name, extensions)| DialogFilter {
                name: name.into(),
                extensions,
            })
            .collect(),
    })
}

fn selected(ask: Ask, path: Option<PathBuf>) -> Option<Picked> {
    let path = path?;
    Some(match ask {
        Ask::Samples { out } => super::created(path, out),
        Ask::Folder { single, .. } => Picked::Folder(path, single),
        Ask::Artwork { library } => Picked::Artwork {
            library,
            picture: path,
        },
        Ask::Snapshot { slot, source, .. } => Picked::Snapshot { slot, source, path },
        Ask::Multi { .. } => Picked::Multi(path),
        Ask::Reveal(path) => Picked::Revealed(crate::ui::menu::reveal(&path)),
    })
}
