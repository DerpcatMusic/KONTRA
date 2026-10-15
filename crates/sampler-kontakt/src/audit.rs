//! Numeric-only load audit probe; enabled explicitly with KONTRA_AUDIT_LOAD.
use std::{sync::atomic::{AtomicU64, Ordering}, thread::ThreadId, time::Instant};

static NEXT_SPAN: AtomicU64 = AtomicU64::new(1);

pub struct Span {
    name: &'static str,
    started: Option<Instant>,
    caller: Option<ThreadId>,
    cpu_started: Option<u64>,
    id: Option<u64>,
}
impl Span {
    pub fn new(name: &'static str) -> Self {
        let enabled = std::env::var_os("KONTRA_AUDIT_LOAD").is_some();
        Self::start(name, enabled, enabled && std::env::var_os("KONTRA_AUDIT_BEGIN").is_some())
    }

    fn start(name: &'static str, enabled: bool, breadcrumbs: bool) -> Self {
        let started = enabled.then(Instant::now);
        let caller = enabled.then(|| std::thread::current().id());
        let cpu_started = enabled.then(thread_cpu_nanos).flatten();
        let id = (enabled && breadcrumbs).then(|| NEXT_SPAN.fetch_add(1, Ordering::Relaxed));
        if let Some(id) = id {
            eprintln!("AUDIT_BEGIN {}", serde_json::json!({"stage":name,"span":id}));
        }
        Self { name, started, caller, cpu_started, id }
    }

    fn elapsed(&self) -> Option<(f64, Option<f64>)> {
        let ms = self.started?.elapsed().as_secs_f64() * 1000.;
        // A moved Span cannot subtract clocks from different callers.
        let cpu = (self.caller == Some(std::thread::current().id()))
            .then(|| thread_cpu_nanos()?.checked_sub(self.cpu_started?))
            .flatten().map(|ns| ns as f64 / 1_000_000.);
        Some((ms, cpu))
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        if let Some((ms, thread_cpu_ms)) = self.elapsed() {
            // Sample clocks before the diagnostic's own procfs read/formatting.
            let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
            let kb = |key: &str| {
                status.lines().find_map(|l| {
                    l.strip_prefix(key)?.split_whitespace().next()?.parse::<u64>().ok()
                }).unwrap_or(0)
            };
            eprintln!("AUDIT {}", serde_json::json!({"stage":self.name,"ms":ms,
                "thread_cpu_ms":thread_cpu_ms,"span":self.id,
                "rss_kb":kb("VmRSS:"),"hwm_kb":kb("VmHWM:")}));
        }
    }
}

/// Worker-only CPU clock. None means unsupported; never substitute wall time.
pub(crate) fn thread_cpu_nanos() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let t = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
        u64::try_from(t.tv_sec)
            .ok()?
            .checked_mul(1_000_000_000)?
            .checked_add(u64::try_from(t.tv_nsec).ok()?)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_audit_has_no_clock_or_breadcrumb_state() {
        let span = Span::start("authored_disabled", false, true);
        assert!(span.started.is_none() && span.caller.is_none() && span.cpu_started.is_none() && span.id.is_none());
        assert!(span.elapsed().is_none());
    }

    #[test]
    fn elapsed_clocks_are_nonnegative_and_moved_callers_are_unknown() {
        let span = Span::start("authored_clock", true, false);
        let (wall, cpu) = span.elapsed().unwrap();
        assert!(wall.is_finite() && wall >= 0.);
        #[cfg(target_os = "linux")]
        assert!(cpu.is_some_and(|n| n.is_finite() && n >= 0.));
        #[cfg(not(target_os = "linux"))]
        assert!(cpu.is_none());
        std::thread::spawn(move || {
            assert!(span.elapsed().unwrap().1.is_none());
        }).join().unwrap();
    }
}
