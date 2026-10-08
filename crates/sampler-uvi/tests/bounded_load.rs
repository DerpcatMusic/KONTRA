#![cfg(feature = "library-access")]

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[test]
fn exhausted_initialization() {
    for script in [
        "function onInit() while true do end end",
        "local function again() spawn(again) end; spawn(again)",
    ] {
        let xml = format!(
            "<UVI4><Program><EventProcessors><ScriptProcessor><script>{script}</script></ScriptProcessor></EventProcessors></Program></UVI4>"
        );
        let result = sampler_uvi::script::ScriptHost::new(
            &xml,
            (),
            sampler_uvi::script::Config {
                load: Duration::from_millis(50),
                ..Default::default()
            },
        );
        let error = result
            .err()
            .expect("budget-exhausted initialization must reject");
        assert!(error.contains("uvi_lua_init"), "{error}");
        assert!(error.contains("unsupported"), "{error}");
    }
}

#[test]
#[ignore = "installed AO bank required; run through kontakto-heavy with a process timeout"]
fn ambient_program_load_finishes_inside_scanner_budget() {
    let bank = PathBuf::from(
        "/mnt/MAIN_STORAGE/Libraries/UVI/UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs",
    );
    let member = std::env::var("UVI_LOAD_PRESET")
        .unwrap_or_else(|_| "Presets/08 Ambient/Coline MW.uvip".into());
    let started = Instant::now();
    let mut translated = sampler_uvi::translate_path(&bank.join(member)).unwrap();
    let span = sampler_kontakt::audit::Span::new("uvi_lua_init");
    let attached = translated.attach_script(48000, sampler_uvi::script::Config::realtime());
    drop(span);
    let attached = match attached {
        Ok(attached) => attached,
        Err(error) => {
            assert!(
                error.to_string().contains("uvi_lua_init unsupported"),
                "{error}"
            );
            assert!(started.elapsed() < Duration::from_secs(90));
            eprintln!("ADMISSION unsupported {} ms", started.elapsed().as_millis());
            return;
        }
    };
    let span = sampler_kontakt::audit::Span::new("uvi_stream_prepare");
    let _streamed =
        sampler_uvi::assemble_translated_streamed(translated, 48000, &Default::default()).unwrap();
    drop(span);
    assert!(started.elapsed() < Duration::from_secs(90));
    drop(attached);
    eprintln!("ADMISSION loaded {} ms", started.elapsed().as_millis());
}
