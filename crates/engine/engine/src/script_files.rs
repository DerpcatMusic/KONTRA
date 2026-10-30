//! Array files a script loads or saves by path after `init`: the engine
//! queues the request, the control side reads or writes the file off the
//! audio thread and answers, and `on async_complete` follows the answer.

use kontra_plan::files::{ArrayKind, FileReply, FileRequest};

use crate::Engine;
use crate::queue::Exit;

/// File requests queued at once; past that a request fails at once.
pub(crate) const FILES: usize = 16;

impl Engine {
    /// Queues `request` under a new async id and returns the id.
    pub(crate) fn request_file(&mut self, request: FileRequest) -> i32 {
        if self.files.len() >= FILES {
            return self.start_async(Exit::Failed);
        }
        let id = self.next_async_id();
        self.files.push(FileRequest { id, ..request });
        id
    }

    /// Hands every queued file request to `f`; one it refuses fails.
    pub fn drain_files(&mut self, mut f: impl FnMut(FileRequest) -> bool) {
        let mut files = std::mem::take(&mut self.files);
        for r in files.drain(..).filter(|&r| !f(r)) {
            self.queue_async(r.slot, r.id, Exit::Failed);
        }
        self.files = files;
    }

    /// Takes the control side's answer: a load's values land in their
    /// cells; a save's room fills with the array's values (no allocation:
    /// the room is reserved) for the control side to write. Then, once the
    /// load landed or the save was written, `on async_complete` follows.
    pub fn file_done(&mut self, reply: &mut FileReply) {
        let r = reply.request;
        let Some(vm) = self.vm_mut(r.slot) else {
            return self.queue_async(r.slot, r.id, Exit::Failed);
        };
        let cells = r.base as usize..(r.base + r.len) as usize;
        let exit = match (&reply.loaded, r.save, reply.written) {
            (_, true, Some(written)) => Exit::from(written),
            (_, true, None) if r.kind == ArrayKind::Text => {
                return (reply.texts).extend(cells.filter_map(|c| vm.text(c).copied()));
            }
            (_, true, None) => return reply.ints.extend(cells.filter_map(|c| vm.cell(c))),
            (Some((ints, texts)), false, _) => {
                cells
                    .clone()
                    .zip(ints)
                    .for_each(|(c, &v)| vm.set_cell(c, v));
                cells.zip(texts).for_each(|(c, &t)| vm.set_text(c, t));
                Exit::Done
            }
            (None, false, _) => Exit::Failed,
        };
        self.queue_async(r.slot, r.id, exit);
    }
}
