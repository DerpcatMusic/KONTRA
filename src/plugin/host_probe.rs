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
    let loaded_parts = s.view.lock().unwrap().parts.iter()
        .filter(|v| !v.loading && v.instrument.is_some()).count() as u64;
    Perf {
        busy_ns: s.busy_ns.load(Ordering::Relaxed),
        span_ns: s.span_ns.load(Ordering::Relaxed),
        voices: s.voices.load(Ordering::Relaxed),
        audible: s.audible.load(Ordering::Relaxed),
        dropouts: s.dropouts.load(Ordering::Relaxed), memory, freed,
        disk_read: sampler_kontakt::DISK_READ.load(Ordering::Relaxed),
        underruns, loaded_parts, blocks: s.blocks.load(Ordering::Relaxed),
    }
}

/// # Safety
/// `plugin` is a live CLAP instance created by this library; `out` is writable.
/// Call only on the host main thread, before destroying that instance.
#[cfg(feature = "clap")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __kontra_clap_perf(
    plugin: *const clap_sys::plugin::clap_plugin, out: *mut Perf,
) -> bool {
    if plugin.is_null() || out.is_null() { return false; }
    let result = std::panic::catch_unwind(|| unsafe {
        moose_clap::with_plugin_params::<Plugin, _>(plugin, snapshot)
    });
    match result {
        Ok(perf) => { unsafe { out.write(perf); } true }
        Err(_) => false,
    }
}

/// Port from v1 0cb7a8a0:src/project_migration.rs, using only v2 rack state.
pub fn export_multi_state(multi: &Path, destination: &Path) -> anyhow::Result<()> {
    use std::io::Write;
    use moose::core::{export::PluginExport, state};
    let saved = SavedMulti::read(multi)?;
    anyhow::ensure!(!saved.parts.is_empty(), "multi contains no parts");
    let selection = Selection {
        order: (0..saved.parts.len() as u32).collect(), parts: saved.parts,
        ..Default::default()
    };
    let plugin = Plugin::create();
    *plugin.params().selection.write().unwrap() = selection.clone();
    let bytes = state::snapshot_plugin(&plugin);
    let mut restored = Plugin::create();
    state::restore_plugin(&mut restored, &bytes).map_err(|e| anyhow::anyhow!("{e}"))?;
    anyhow::ensure!(*restored.params().selection.read().unwrap() == selection, "state round-trip changed rack");
    std::fs::OpenOptions::new().write(true).create_new(true).open(destination)?.write_all(&bytes)?;
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
        assert_eq!((perf.busy_ns, perf.span_ns, perf.voices, perf.audible, perf.dropouts), (100, 400, 7, 3, 2));
        assert_eq!(perf.loaded_parts, 0);
    }
    #[test]
    fn native_host_state_export_rejects_v1_and_preserves_existing_files() {
        let tmp = tempfile::tempdir().unwrap();
        let multi = tmp.path().join("x.kontra-multi");
        let state = tmp.path().join("x.state");
        std::fs::write(&multi, r#"{"format":"kontra-multi","version":1,"parts":[{"path":"test.nki"}]}"#).unwrap();
        assert!(export_multi_state(&multi, &state).is_err());
        std::fs::write(&multi, r#"{"format":"kontra-multi","version":2,"parts":[{"path":"test.nki"}]}"#).unwrap();
        export_multi_state(&multi, &state).unwrap();
        let before = std::fs::read(&state).unwrap();
        assert!(export_multi_state(&multi, &state).is_err());
        assert_eq!(std::fs::read(&state).unwrap(), before);
    }
}
