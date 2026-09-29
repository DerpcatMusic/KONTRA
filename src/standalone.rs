const OUTPUT_ENV: &str = "MOOSE_STANDALONE_OUTPUT";

fn main() {
    prefer_sound_server();
    moose_standalone::run::<kontakto::Plugin>();
}

/// cpal's default ALSA device (and the PipeWire one at small buffers) fails
/// with endless POLLERR on PipeWire systems and plays nothing, so unless the
/// user picked a device, route through the PulseAudio plugin, which works at
/// any buffer size.
fn prefer_sound_server() {
    let chosen = std::env::var_os(OUTPUT_ENV).is_some()
        || std::env::args().any(|a| a == "--output" || a.starts_with("--output="));
    if chosen {
        return;
    }
    let (_, devices) = moose_standalone::audio::list_output_devices();
    if let Some(server) = ["PulseAudio Sound Server", "PipeWire Sound Server"]
        .into_iter()
        .find(|s| devices.iter().any(|d| d == s))
    {
        // SAFETY: single-threaded; no other thread exists yet to read the environment.
        unsafe { std::env::set_var(OUTPUT_ENV, server) };
    }
}
