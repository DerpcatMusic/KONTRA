//! Opt-in, thread-local tracing of actual load-time law applications.
use std::cell::RefCell;

/// Stable identifier assigned by the law's author, never inferred from a curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LawId(pub &'static str);

/// The input and output at a concrete translator location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Application {
    pub id: LawId,
    pub location: String,
    pub input: String,
    pub output: String,
}

thread_local! {
    static RECORDING: RefCell<Option<Vec<Application>>> = const { RefCell::new(None) };
}

/// Records lazily: the closure is never called when no recorder is active.
pub fn record(application: impl FnOnce() -> Application) {
    RECORDING.with(|slot| {
        if let Some(events) = slot.borrow_mut().as_mut() {
            events.push(application());
        }
    });
}

struct Scope(Option<Vec<Application>>);
impl Drop for Scope {
    fn drop(&mut self) {
        RECORDING.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}

/// Collects only applications made on this thread during `f`; nested scopes
/// are isolated, and the prior recorder is restored even if `f` panics.
pub fn collect<T>(f: impl FnOnce() -> T) -> (T, Vec<Application>) {
    let prior = RECORDING.with(|slot| slot.replace(Some(Vec::new())));
    let _scope = Scope(prior);
    let value = f();
    let events = RECORDING.with(|slot| slot.borrow_mut().take().unwrap_or_default());
    (value, events)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> Application {
        Application {
            id: LawId("test.v1"),
            location: "group/0".into(),
            input: "in".into(),
            output: "out".into(),
        }
    }

    #[test]
    fn disabled_is_lazy_and_nested_scopes_restore() {
        record(|| panic!("disabled recorder evaluated"));
        let (_, outer) = collect(|| {
            record(event);
            let (_, inner) = collect(|| record(event));
            assert_eq!(inner.len(), 1);
            record(event);
        });
        assert_eq!(outer.len(), 2);
        assert_eq!(outer[0].id, LawId("test.v1"));
    }
}
