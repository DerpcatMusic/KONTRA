//! Counted-reader immutable publication. Only publishers reclaim old values.
#![allow(
    unsafe_code,
    reason = "SeqCst reader admission protects immutable pointers; reclamation runs only on publishers"
)]
use std::{
    ops::Deref,
    sync::{
        Arc, Mutex,
        atomic::{AtomicPtr, AtomicUsize, Ordering::SeqCst},
    },
};

#[derive(Debug)]
pub struct Snapshot<T: Send + Sync> {
    current: AtomicPtr<T>,
    readers: AtomicUsize,
    retired: Mutex<Vec<Box<T>>>,
}
impl<T: Send + Sync> Snapshot<T> {
    pub fn new(value: T) -> Self {
        Self {
            current: AtomicPtr::new(Box::into_raw(Box::new(value))),
            readers: AtomicUsize::new(0),
            retired: Mutex::new(Vec::new()),
        }
    }
    /// No allocation, waiting, or destruction of a published value.
    pub fn read(&self) -> SnapshotRead<'_, T> {
        #[allow(deprecated, reason = "fetch_update supports Rust 1.92")]
        self.readers
            .fetch_update(SeqCst, SeqCst, |n| n.checked_add(1))
            .expect("snapshot reader count overflow");
        let value = self.current.load(SeqCst);
        SnapshotRead {
            owner: self,
            pointer: value,
        }
    }
    /// Pin a generation independently of a lexical borrow. Dropping the pin
    /// releases a reader count; it never reclaims retired values. A real-time
    /// caller must keep another Arc owner alive until after the pin is dropped.
    pub fn read_owned(self: &Arc<Self>) -> SnapshotPin<T> {
        let owner = Arc::clone(self);
        #[allow(deprecated, reason = "fetch_update supports Rust 1.92")]
        self.readers.fetch_update(SeqCst, SeqCst, |n| n.checked_add(1))
            .expect("snapshot reader count overflow");
        let pointer = self.current.load(SeqCst);
        SnapshotPin { owner, pointer }
    }

    /// Control side only. Readers that entered before the swap protect the
    /// old pointer; readers entering afterwards see the new one. SeqCst orders
    /// admission, pointer selection, swap and the zero-reader observation.
    pub fn swap(&self, value: T) -> SnapshotRead<'_, T> {
        let value = Box::new(value);
        let mut retired = self.retired.lock().unwrap_or_else(|e| e.into_inner());
        // Reserve before publication: a failed allocation cannot destroy a value
        // which existing readers still borrow.
        retired.reserve(1);
        #[allow(deprecated, reason = "fetch_update supports Rust 1.92")]
        self.readers
            .fetch_update(SeqCst, SeqCst, |n| n.checked_add(1))
            .expect("snapshot reader count overflow");
        let old = self.current.swap(Box::into_raw(value), SeqCst);
        // SAFETY: swap transfers this Box to the serialized publisher; existing
        // and returned readers retain access until collect observes no readers.
        retired.push(unsafe { Box::from_raw(old) });
        SnapshotRead {
            owner: self,
            pointer: old,
        }
    }
    pub fn replace<R>(&self, value: T, describe: impl FnOnce(&T) -> R) -> R {
        let old = self.swap(value);
        let result = describe(&old);
        drop(old);
        self.collect();
        result
    }
    /// Control-side reclamation status; never queried by render readers.
    pub fn has_retired(&self) -> bool {
        !self.retired.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
    }
    /// Control-side ticks may reclaim a generation after its last reader left.
    pub fn collect(&self) {
        let mut retired = self.retired.lock().unwrap_or_else(|e| e.into_inner());
        if self.readers.load(SeqCst) == 0 {
            retired.clear();
        }
    }
}
impl<T: Send + Sync + Default> Default for Snapshot<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}
impl<T: Send + Sync> Drop for Snapshot<T> {
    fn drop(&mut self) {
        // SAFETY: exclusive access requires every borrowing reader to have left.
        drop(unsafe { Box::from_raw(*self.current.get_mut()) });
    }
}
pub struct SnapshotRead<'a, T: Send + Sync> {
    owner: &'a Snapshot<T>,
    pointer: *const T,
}
impl<T: Send + Sync> Deref for SnapshotRead<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: admission precedes pointer selection; publishers retain all
        // replaced values until every admitted read guard has been released.
        unsafe { &*self.pointer }
    }
}
impl<T: Send + Sync> Drop for SnapshotRead<'_, T> {
    fn drop(&mut self) {
        self.owner.readers.fetch_sub(1, SeqCst);
    }
}

/// An owned immutable generation; the publisher alone reclaims retired values.
pub struct SnapshotPin<T: Send + Sync> {
    owner: Arc<Snapshot<T>>,
    pointer: *const T,
}
// SAFETY: the admitted reader count retains the immutable T, the Arc retains
// its counter, and T is Send + Sync. Readers never mutate or reclaim T.
unsafe impl<T: Send + Sync> Send for SnapshotPin<T> {}
unsafe impl<T: Send + Sync> Sync for SnapshotPin<T> {}
impl<T: Send + Sync> Deref for SnapshotPin<T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: read_owned admitted this reader before selecting its pointer.
        unsafe { &*self.pointer }
    }
}
impl<T: Send + Sync> Drop for SnapshotPin<T> {
    fn drop(&mut self) { self.owner.readers.fetch_sub(1, SeqCst); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct Tracked {
        value: usize,
        drops: Arc<Mutex<Vec<(usize, std::thread::ThreadId)>>>,
    }
    impl Drop for Tracked {
        fn drop(&mut self) {
            self.drops
                .lock()
                .unwrap()
                .push((self.value, std::thread::current().id()));
        }
    }

    #[test]
    fn publishing_preserves_pinned_generations_until_control_collection() {
        let drops = Arc::new(Mutex::new(Vec::new()));
        let snapshot = Arc::new(Snapshot::new(Tracked {
            value: 0,
            drops: drops.clone(),
        }));
        let old = snapshot.read();
        let publisher = snapshot.clone();
        let records = drops.clone();
        std::thread::spawn(move || {
            publisher.replace(
                Tracked {
                    value: 1,
                    drops: records,
                },
                |_| (),
            );
        })
        .join()
        .unwrap();
        assert_eq!(old.value, 0);
        assert_eq!(snapshot.read().value, 1);
        assert!(drops.lock().unwrap().is_empty());
        drop(old);
        assert!(
            drops.lock().unwrap().is_empty(),
            "last reader must not free old PCM"
        );
        snapshot.collect();
        assert_eq!(*drops.lock().unwrap(), [(0, std::thread::current().id())]);
        drop(snapshot);
        assert_eq!(drops.lock().unwrap().len(), 2);
    }

    #[test]
    fn concurrent_readers_observe_complete_generations_and_publication_survives_panic() {
        let snapshot = Arc::new(Snapshot::new(vec![0usize; 64]));
        let reader = snapshot.clone();
        let worker = std::thread::spawn(move || {
            for _ in 0..10000 {
                let value = reader.read();
                assert!(value.iter().all(|&v| v == value[0]));
            }
        });
        for n in 1..500 {
            snapshot.replace(vec![n; 64], |_| ());
        }
        worker.join().unwrap();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                snapshot.replace(vec![500; 64], |_| panic!("description failed"));
            }))
            .is_err()
        );
        snapshot.replace(vec![501; 64], |_| ());
        assert_eq!(&**snapshot.read(), &[501; 64]);
        snapshot.collect();
        assert!(snapshot.retired.lock().unwrap().is_empty());
    }
}

#[cfg(test)]
mod owned_pin_tests {
    use super::*;
    #[test]
    fn owned_pin_survives_publication_and_cross_thread_drop_without_reclaiming_pcm() {
        struct Value(usize, Arc<std::sync::atomic::AtomicUsize>);
        impl Drop for Value {
            fn drop(&mut self) { self.1.fetch_add(1, SeqCst); }
        }
        let drops = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let value = Arc::new(Snapshot::new(Value(1, drops.clone())));
        let pin = value.read_owned();
        value.replace(Value(2, drops.clone()), |_| ());
        value.collect();
        assert_eq!(drops.load(SeqCst), 0);
        std::thread::spawn(move || { assert_eq!(pin.0, 1); drop(pin); }).join().unwrap();
        assert_eq!(drops.load(SeqCst), 0, "last reader cannot reclaim the retired payload");
        value.collect();
        assert_eq!(drops.load(SeqCst), 1);
    }
}
