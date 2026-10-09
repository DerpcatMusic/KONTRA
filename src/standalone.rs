const OUTPUT_ENV: &str = "MOOSE_STANDALONE_OUTPUT";

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--version" | "-V") => {
            println!("{}", kontakto::build_info::SUMMARY);
            return;
        }
        Some("--build-info") => {
            print!("{}", kontakto::build_info::MANIFEST_JSON);
            return;
        }
        _ => {}
    }
    prefer_sound_server();
    let _crash = kontakto::support::start_standalone_session();
    if std::panic::catch_unwind(|| {
        let _diagnostics = kontakto::diagnostics::acquire();
        moose_standalone::run::<kontakto::Plugin>();
    }).is_err() {
        // Retain the crash session and panic marker rather than marking this exit clean.
        std::process::exit(101);
    }
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
