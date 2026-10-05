// Test-only private census: call between callbacks, never on the audio thread.
pub(crate) fn zones(engine: &super::Engine) -> Vec<(u32, u32, u8, u8, u32, bool)> {
    engine
        .player
        .voices
        .iter()
        .map(|v| {
            (
                v.zone_id, v.group, v.note, v.velocity, v.event.0, v.released,
            )
        })
        .collect()
}
