//! Numeric-only load audit probe; enabled explicitly with KONTRA_AUDIT_LOAD.
use std::time::Instant;
pub struct Span(&'static str, Option<Instant>);
impl Span {
    pub fn new(name: &'static str) -> Self {
        Self(name, std::env::var_os("KONTRA_AUDIT_LOAD").map(|_| Instant::now()))
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        if let Some(t) = self.1 {
            let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
            let kb = |key: &str| status.lines().find_map(|l| l.strip_prefix(key)?.split_whitespace().next()?.parse::<u64>().ok()).unwrap_or(0);
            eprintln!("AUDIT {{\"stage\":\"{}\",\"ms\":{},\"rss_kb\":{},\"hwm_kb\":{}}}", self.0, t.elapsed().as_secs_f64()*1000., kb("VmRSS:"), kb("VmHWM:"));
        }
    }
}
