//! Explicit runtime diagnostic; no plugin entry point or host is launched.

pub(super) fn run(args: &[String]) -> i32 {
    let [plan] = args else {
        eprintln!(
            "preset-switch PLAN.json (requires ui-render; see tools/corpus-health/PRESET_SWITCH.md)"
        );
        return 2;
    };
    #[cfg(not(feature = "ui-render"))]
    {
        let _ = plan;
        eprintln!("preset-switch requires the ui-render diagnostic helper feature");
        2
    }
    #[cfg(feature = "ui-render")]
    {
        use std::io::Read;
        let mut bytes = Vec::new();
        let read = std::fs::File::open(plan)
            .and_then(|file| file.take((1 << 20) + 1).read_to_end(&mut bytes));
        if read.is_err() || bytes.len() > 1 << 20 {
            eprintln!("preset-switch plan could not be read within the 1 MiB limit");
            return 2;
        }
        let plan: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(plan) => plan,
            Err(_) => {
                eprintln!("preset-switch plan is not valid JSON");
                return 2;
            }
        };
        match kontakto::OfflineUiLifecycle::preset_switch_probe(&plan) {
            Ok(report) => {
                println!("{report}");
                0
            }
            Err(error) => {
                eprintln!("preset-switch: {error}");
                1
            }
        }
    }
}
