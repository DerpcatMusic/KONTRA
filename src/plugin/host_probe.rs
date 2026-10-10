//! Main-thread numeric readback from the same counters the performance header uses.
use super::*;

#[repr(C)]
#[derive(Default)]
pub struct Perf {
    pub busy_ns: u64,
    pub span_ns: u64,
    pub voices: u64,
    pub audible: u64,
    pub dropouts: u64,
    pub memory: u64,
    pub freed: u64,
    pub disk_read: u64,
    pub underruns: u64,
    pub loaded_parts: u64,
    pub blocks: u64,
}

fn snapshot(p: &SamplerParams) -> Perf {
    let s = &p.shared;
    let (memory, freed) = s.memory_snapshot();
    let underruns = s.with_parts(|parts| parts.iter().map(|p| p.problems().underruns).sum());
    let loaded_parts = s
        .view
        .lock_unpoisoned()
        .parts
        .iter()
        .filter(|v| !v.loading && v.instrument.is_some())
        .count() as u64;
    Perf {
        busy_ns: s.busy_ns.load(Ordering::Relaxed),
        span_ns: s.span_ns.load(Ordering::Relaxed),
        voices: s.voices.load(Ordering::Relaxed),
        audible: s.audible.load(Ordering::Relaxed),
        dropouts: s.dropouts.load(Ordering::Relaxed),
        memory,
        freed,
        disk_read: sampler_kontakt::DISK_READ.load(Ordering::Relaxed),
        underruns,
        loaded_parts,
        blocks: s.blocks.load(Ordering::Relaxed),
    }
}

/// # Safety
/// `plugin` is a live CLAP instance created by this library; `out` is writable.
/// Call only on the host main thread, before destroying that instance.
#[cfg(feature = "clap")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __kontra_clap_perf(
    plugin: *const clap_sys::plugin::clap_plugin,
    out: *mut Perf,
) -> bool {
    if plugin.is_null() || out.is_null() {
        return false;
    }
    let result = std::panic::catch_unwind(|| unsafe {
        moose_clap::with_plugin_params::<Plugin, _>(plugin, snapshot)
    });
    match result {
        Ok(perf) => {
            unsafe {
                out.write(perf);
            }
            true
        }
        Err(_) => false,
    }
}

fn ui_activity_snapshot(activity: &ui_activity::Activity, version: u64, out: &mut [u64]) -> bool {
    if version != ui_activity::VERSION || out.len() != ui_activity::FIELDS.len() {
        return false;
    }
    let Some(counts) = activity.snapshot() else {
        return false;
    };
    out.copy_from_slice(&counts);
    true
}

/// Read the versioned UI activity counters without changing the legacy Perf ABI.
/// These independent cumulative counters do not prove renderer readiness.
///
/// # Safety
/// `plugin` is a live CLAP instance created by this library. `out` points to
/// `count` writable, aligned u64 values. Call only on the host main thread,
/// before destroying the instance. On failure, the output is unchanged.
#[cfg(feature = "clap")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __kontra_clap_ui_activity(
    plugin: *const clap_sys::plugin::clap_plugin,
    version: u64,
    out: *mut u64,
    count: usize,
) -> bool {
    if plugin.is_null()
        || out.is_null()
        || version != ui_activity::VERSION
        || count != ui_activity::FIELDS.len()
    {
        return false;
    }
    std::panic::catch_unwind(|| unsafe {
        moose_clap::with_plugin_params::<Plugin, _>(plugin, |p| {
            ui_activity_snapshot(
                &p.shared.ui_activity,
                version,
                std::slice::from_raw_parts_mut(out, count),
            )
        })
    })
    .unwrap_or(false)
}

/// Port from v1 0cb7a8a0:src/project_migration.rs, using only v2 rack state.
pub fn export_multi_state(multi: &Path, destination: &Path) -> anyhow::Result<()> {
    use moose::core::{export::PluginExport, state};
    use std::io::Write;
    let saved = SavedMulti::read(multi)?;
    anyhow::ensure!(!saved.parts.is_empty(), "multi contains no parts");
    let selection = Selection {
        order: (0..saved.parts.len() as u32).collect(),
        parts: saved.parts,
        ..Default::default()
    };
    let plugin = Plugin::create();
    *plugin.params().selection.write_unpoisoned() = selection.clone();
    let bytes = state::snapshot_plugin(&plugin);
    let mut restored = Plugin::create();
    state::restore_plugin(&mut restored, &bytes).map_err(|e| anyhow::anyhow!("{e}"))?;
    anyhow::ensure!(
        *restored.params().selection.read_unpoisoned() == selection,
        "state round-trip changed rack"
    );
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?
        .write_all(&bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_host_readback_uses_performance_header_counters() {
        let p = SamplerParams::new();
        p.shared.busy_ns.store(100, Ordering::Relaxed);
        p.shared.span_ns.store(400, Ordering::Relaxed);
        p.shared.voices.store(7, Ordering::Relaxed);
        p.shared.audible.store(3, Ordering::Relaxed);
        p.shared.dropouts.store(2, Ordering::Relaxed);
        let perf = snapshot(&p);
        assert_eq!(
            (
                perf.busy_ns,
                perf.span_ns,
                perf.voices,
                perf.audible,
                perf.dropouts
            ),
            (100, 400, 7, 3, 2)
        );
        assert_eq!(perf.loaded_parts, 0);
    }
    #[test]
    fn ui_activity_export_preserves_output_for_disabled_or_mismatched_contracts() {
        let enabled = ui_activity::Activity::new(true);
        let disabled = ui_activity::Activity::new(false);
        for (activity, version, size) in [
            (&enabled, ui_activity::VERSION + 1, ui_activity::FIELDS.len()),
            (&enabled, ui_activity::VERSION, ui_activity::FIELDS.len() - 1),
            (&enabled, ui_activity::VERSION, ui_activity::FIELDS.len() + 1),
            (&disabled, ui_activity::VERSION, ui_activity::FIELDS.len()),
        ] {
            let mut out = vec![73; size];
            assert!(!ui_activity_snapshot(activity, version, &mut out));
            assert!(out.iter().all(|&value| value == 73));
        }
    }

    #[test]
    fn ui_activity_export_copies_current_instance_counters_in_schema_order() {
        let activity = ui_activity::Activity::new(true);
        activity.add(ui_activity::Count::ReadbackCalls, 3);
        activity.add(ui_activity::Count::WatchWakes, 2);
        let mut out = [73; ui_activity::FIELDS.len()];
        assert!(ui_activity_snapshot(&activity, ui_activity::VERSION, &mut out));
        assert_eq!(out, activity.snapshot().unwrap());
        assert_eq!(out[ui_activity::Count::ReadbackCalls as usize], 3);
        assert_eq!(out[ui_activity::Count::WatchWakes as usize], 2);
        assert_eq!(out.iter().sum::<u64>(), 5);
    }

    #[cfg(feature = "clap")]
    #[test]
    fn ui_activity_export_rejects_null_plugin_without_writing_output() {
        let mut out = [73; ui_activity::FIELDS.len()];
        assert!(!unsafe {
            __kontra_clap_ui_activity(
                std::ptr::null(),
                ui_activity::VERSION,
                out.as_mut_ptr(),
                out.len(),
            )
        });
        assert_eq!(out, [73; ui_activity::FIELDS.len()]);
    }

    #[test]
    fn native_host_state_export_rejects_v1_and_preserves_existing_files() {
        let tmp = tempfile::tempdir().unwrap();
        let multi = tmp.path().join("x.kontra-multi");
        let state = tmp.path().join("x.state");
        std::fs::write(
            &multi,
            r#"{"format":"kontra-multi","version":1,"name":"Probe","parts":[{"path":"test.nki"}]}"#,
        )
        .unwrap();
        assert!(export_multi_state(&multi, &state).is_err());
        std::fs::write(
            &multi,
            r#"{"format":"kontra-multi","version":2,"name":"Probe","parts":[{"path":"test.nki"}]}"#,
        )
        .unwrap();
        export_multi_state(&multi, &state).unwrap();
        let before = std::fs::read(&state).unwrap();
        assert!(export_multi_state(&multi, &state).is_err());
        assert_eq!(std::fs::read(&state).unwrap(), before);
    }
}
