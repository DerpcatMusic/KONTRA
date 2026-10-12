//! Native release-counter key cells and their host processing span.
//!
//! Kontakt resets an integer sample offset to negative float milliseconds,
//! then advances all key cells once per balanced processing span. MIDI events,
//! render lanes and KSP timers may divide DSP without dividing that span.

#[derive(Clone, Copy)]
struct Window {
    start: u64,
    end: u64,
    finish: u64,
    frames: u64,
}

pub(super) struct Counter {
    age: [f32; 128],
    window: Option<Window>,
}

impl Default for Counter {
    fn default() -> Self {
        // Deterministic local initialization, not a claimed native allocator
        // default. Ordinary admission or explicit reset defines a native key.
        Self { age: [0.; 128], window: None }
    }
}

impl Counter {
    pub(super) fn begin(&mut self, now: u64, frames: usize, rate: f64) {
        self.close(now, rate);
        if frames == 0 { return; }
        let frames = frames as u64;
        let spans = frames.div_ceil(1024);
        let count = frames.div_ceil(spans);
        self.window = Some(Window {
            start: now,
            end: now.saturating_add(count),
            finish: now.saturating_add(frames),
            frames: count,
        });
    }

    pub(super) fn ensure(&mut self, now: u64, frames: usize, rate: f64) {
        self.advance(now, rate);
        if self.window.is_none() { self.begin(now, frames, rate); }
    }

    /// The one balanced host span, shared with native source-generation caches.
    pub(super) fn span(&self) -> Option<(u64, u64)> {
        self.window.map(|w| (w.start, w.end))
    }

    /// Current declared geometry without advancing release-counter cells.
    pub(super) fn span_at(&self, now: u64) -> Option<(u64, u64)> {
        let window = self.window?;
        if now < window.start || now >= window.finish { return None; }
        let start = window.start + (now - window.start) / window.frames * window.frames;
        Some((start, start.saturating_add(window.frames).min(window.finish)))
    }

    pub(super) fn next_boundary(&self) -> Option<u64> {
        self.window.map(|window| window.end)
    }

    pub(super) fn advance(&mut self, now: u64, rate: f64) {
        while let Some(window) = self.window.filter(|window| window.end <= now) {
            self.add(window.end.saturating_sub(window.start), rate);
            self.window = (window.end < window.finish).then(|| Window {
                start: window.end,
                end: window.end.saturating_add(window.frames).min(window.finish),
                ..window
            });
        }
    }

    /// Abandon a host window using only DSP that actually elapsed. No pending
    /// portion of a replaced/empty callback is advanced into the musical clock.
    pub(super) fn close(&mut self, now: u64, rate: f64) {
        self.advance(now, rate);
        if let Some(window) = self.window.take() {
            self.add(now.saturating_sub(window.start).min(window.end - window.start), rate);
        }
    }

    fn add(&mut self, frames: u64, rate: f64) {
        let rate = rate as f32;
        if frames == 0 || !rate.is_finite() || rate <= 0. { return; }
        let delta = ((frames as f64 * 1000.) * (1. / f64::from(rate))) as f32;
        for age in &mut self.age { *age += delta; }
    }

    pub(super) fn reset(&mut self, now: u64, key: u8, rate: f64) {
        self.advance(now, rate);
        let rate = rate as f32;
        if !rate.is_finite() || rate <= 0. { return; }
        let offset = self.window.map_or(0, |window| now.saturating_sub(window.start));
        self.age[key as usize & 127] = (offset as f32 * -1000.) / rate;
    }

    pub(super) fn factor(&self, key: u8, time_ms: i32) -> f32 {
        // Native admission leaves the source's initialized unity value when
        // its RTC enable/time branch is inactive. Compatibility keeps its own
        // historical zero result for a group with no counter.
        if time_ms <= 0 { return 1.; }
        let time = time_ms as f32;
        (time - self.age[key as usize & 127]).max(0.) / time
    }
}

#[cfg(all(test, feature = "plugin"))]
#[path = "rtc_tests.rs"]
mod tests;
