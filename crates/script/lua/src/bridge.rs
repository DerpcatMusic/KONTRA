//! The engine lent to `__host` for one resume. The function is made once
//! per state; a run lends it the host instead of making a scoped closure per
//! resume (6 arena allocations each, `tests/dispatch.rs`). `__host` is still
//! the global only while a callback runs, so the prelude sees the same thing.

// The lend erases the host's lifetime for the length of one resume (09-BUILD.md
// §1.8): each unsafe block carries its SAFETY note.
#![allow(unsafe_code)]

use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, Ordering};

use kontra_vm::HostApi;

/// What a resume lends: the host and the running callback's context.
pub(crate) struct Lent<'a> {
    pub host: &'a mut dyn HostApi,
    pub context: u64,
}

/// Where `__host` finds the lent host: null outside [`Lend::lend`].
#[derive(Clone, Default)]
pub(crate) struct Lend(Arc<AtomicPtr<()>>);

impl Lend {
    /// Runs `f` with `lent` reachable from [`Lend::with`].
    pub(crate) fn lend<R>(&self, lent: &mut Lent<'_>, f: impl FnOnce() -> R) -> R {
        struct Clear<'a>(&'a AtomicPtr<()>);
        impl Drop for Clear<'_> {
            fn drop(&mut self) {
                self.0.store(ptr::null_mut(), Ordering::Release);
            }
        }
        let previous = self.0.swap(ptr::from_mut(lent).cast(), Ordering::AcqRel);
        debug_assert!(previous.is_null(), "a lend inside a lend");
        let _clear = Clear(&self.0);
        f()
    }

    /// `f` on the lent host, or `None` outside a lend.
    pub(crate) fn with<R>(&self, f: impl FnOnce(&mut Lent<'_>) -> R) -> Option<R> {
        let lent = self.0.load(Ordering::Acquire).cast::<Lent<'_>>();
        // SAFETY: a non-null pointer was stored by `lend` from a `&mut Lent`
        // that outlives the `f` it runs, and is cleared (even on unwind)
        // before `lend` returns. `lend` makes no other use of the `Lent`
        // while `f` runs, and `__host` runs on the thread that called `lend`
        // (a state's resume is not shared), so this is the only reference.
        // A `__host` that calls back into `__host` would alias, but the host
        // never runs Lua.
        let lent = unsafe { lent.as_mut()? };
        Some(f(lent))
    }
}
