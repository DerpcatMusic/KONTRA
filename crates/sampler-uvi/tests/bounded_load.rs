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
                load_work: 1000,
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
    let bank = PathBuf::from(std::env::var("UVI_LOAD_BANK").unwrap_or_else(|_| "/mnt/MAIN_STORAGE/Libraries/UVI/UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs".into()));
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

#[test]
#[ignore = "largest installed AO graph; simulate slow resource preload through kontakto-heavy"]
fn large_program_slow_preload_does_not_spend_lua_init_budget() {
    let path = PathBuf::from("/mnt/MAIN_STORAGE/Libraries/UVI/UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs/Presets/00 Orchestra/01 Strings/V Strings Bartok.uvip");
    let mut translated = sampler_uvi::translate_path(&path).unwrap();
    // Production has read the XML and module bytes before arming the Lua deadline.
    std::thread::sleep(Duration::from_secs(21));
    let started = Instant::now();
    let attached = translated.attach_script(48000, sampler_uvi::script::Config::realtime())
        .unwrap().expect("installed preset has a script");
    assert!(started.elapsed() < Duration::from_secs(20));
    assert!(attached.interface.widgets.len() > 5000);
    eprintln!("SLOW_PRELOAD init_ready_ms={} zones={} widgets={}", started.elapsed().as_millis(), translated.instrument.zones.len(), attached.interface.widgets.len());
}

#[test]
fn zero_wait_cannot_refill_a_virtual_clock_work_allowance() {
    let xml = "<UVI4><Program><EventProcessors><ScriptProcessor><script>function onNote(e) while true do wait(0) end end</script></ScriptProcessor></EventProcessors></Program></UVI4>";
    let mut h = sampler_uvi::script::ScriptHost::new(xml, (), sampler_uvi::script::Config { callback_work: 1000, ..Default::default() }).unwrap();
    h.note_on(1,60,64,0);
    h.advance(0.);
    assert_eq!(h.next_due(),None);
    assert_eq!(h.fault_counts().runtime[&sampler_uvi::script::FaultCategory::Budget],1);
}
