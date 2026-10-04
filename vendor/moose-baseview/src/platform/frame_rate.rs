//! MOOSE: display-rate frame pacing shared by the platform backends.

use std::time::Duration;

/// Refresh rate assumed when the display's rate can't be queried.
const FALLBACK_REFRESH_HZ: f64 = 60.0;

/// The `on_frame` interval for a display refreshing at `hz`. Unknown or implausible rates fall
/// back to 60 Hz.
pub(crate) fn frame_interval(hz: Option<f64>) -> Duration {
    let hz = hz.filter(|hz| (10.0..=1000.0).contains(hz)).unwrap_or(FALLBACK_REFRESH_HZ);
    Duration::from_secs_f64(hz.recip())
}

#[cfg(test)]
mod tests {
    use super::frame_interval;
    use std::time::Duration;

    #[test]
    fn interval_follows_refresh_rate() {
        assert_eq!(frame_interval(Some(100.0)), Duration::from_millis(10));
        assert_eq!(frame_interval(Some(160.0)), Duration::from_micros(6250));
        let ms = frame_interval(Some(143.912)).as_secs_f64() * 1000.0;
        assert!((ms - 6.9487).abs() < 0.001, "{ms}");
    }

    #[test]
    fn unknown_or_bogus_rates_fall_back_to_60hz() {
        let sixty = frame_interval(Some(60.0));
        assert_eq!(frame_interval(None), sixty);
        for bogus in [0.0, -60.0, 1.0, 5000.0, f64::NAN, f64::INFINITY] {
            assert_eq!(frame_interval(Some(bogus)), sixty, "{bogus}");
        }
    }
}
