use sampler_ir::AssetResidency;
use std::path::PathBuf;
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "kontra-residency-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        for name in ["a.wav", "b.wav"] {
            let mut bytes = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0".to_vec();
            bytes.extend(48000u32.to_le_bytes());
            bytes.extend(96000u32.to_le_bytes());
            bytes.extend(2u16.to_le_bytes());
            bytes.extend(16u16.to_le_bytes());
            bytes.extend(b"data");
            bytes.extend(32u32.to_le_bytes());
            for _ in 0..16 {
                bytes.extend(8192i16.to_le_bytes());
            }
            let len = (bytes.len() - 8) as u32;
            bytes[4..8].copy_from_slice(&len.to_le_bytes());
            std::fs::write(path.join(name), bytes).unwrap();
        }
        Self(path)
    }
    fn program(&self, program: &str, players: &str) -> PathBuf {
        let path = self.0.join("instrument.uvip");
        std::fs::write(&path,format!("<UVI4><Program {program}><Layers><Layer><Keygroups><Keygroup LowKey='60' HighKey='60'><Oscillators>{players}</Oscillators></Keygroup></Keygroups></Layer></Layers></Program></UVI4>")).unwrap();
        path
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn policy(lazy: bool, budget: usize) -> sampler_kontakt::StreamPolicy {
    sampler_kontakt::StreamPolicy {
        lazy,
        head_budget: budget,
        voices: 4,
        ..Default::default()
    }
}

#[test]
fn any_authored_resident_user_dominates_a_shared_source_even_when_bypassed() {
    let dir = Scratch::new();
    for players in [
        "<SamplePlayer SamplePath='a.wav' AllowStreaming='1'/><SamplePlayer SamplePath='a.wav' AllowStreaming='0'/>",
        "<SamplePlayer SamplePath='a.wav' AllowStreaming='0'/><SamplePlayer SamplePath='a.wav' AllowStreaming='1'/>",
        "<SamplePlayer SamplePath='a.wav' AllowStreaming='1'/><SamplePlayer SamplePath='a.wav' Bypass='1' AllowStreaming='0'/>",
    ] {
        let path = dir.program("", players);
        let translated = sampler_uvi::translate_path(&path).unwrap();
        assert_eq!(translated.instrument.assets.len(), 1);
        assert_eq!(
            translated.instrument.assets[0].residency,
            AssetResidency::Resident
        );
        let streamed =
            sampler_uvi::assemble_translated_streamed(translated, 48000, &policy(true, 128))
                .unwrap();
        assert_eq!(streamed.report.required_resident_assets, 1);
        assert_eq!(streamed.report.required_resident_bytes, 128);
        assert_eq!(streamed.report.head_bytes, 0);
        assert!(
            streamed
                .streamer
                .head_readiness(&streamed.assets)
                .complete()
        );
        assert_eq!(streamed.streamer.head_budget_status().held_bytes, 128);
        assert_eq!(
            &*streamed.assets[0].resident_frames().unwrap(),
            &[[0.25; 2]; 16]
        );
        let budget = streamed.streamer.physical_budget();
        assert_eq!(
            streamed.assets[0].resident_non_head_bytes_outside(&budget)
                + budget.status().held_bytes,
            128
        );
    }
}

#[test]
fn mixed_full_and_packed_storage_share_cap_and_fallback_never_streams_a_required_source() {
    let dir = Scratch::new();
    let path = dir.program(
        "",
        "<SamplePlayer SamplePath='a.wav' AllowStreaming='0'/><SamplePlayer SamplePath='b.wav'/>",
    );
    for (lazy, cap, fallback, success, packed) in [
        (true, 128, false, true, 0),
        (false, 160, false, true, 32),
        (false, 159, false, false, 0),
        (false, 128, true, true, 0),
        (false, 127, true, false, 0),
    ] {
        let mut policy = policy(lazy, cap);
        policy.allow_cold_fallback = fallback;
        let translated = sampler_uvi::translate_path(&path).unwrap();
        let result = sampler_uvi::assemble_translated_streamed(translated, 48000, &policy);
        assert_eq!(
            result.is_ok(),
            success,
            "lazy={lazy} cap={cap} fallback={fallback}"
        );
        if let Ok(streamed) = result {
            assert_eq!(
                &*streamed.assets[0].resident_frames().unwrap(),
                &[[0.25; 2]; 16]
            );
            assert!(streamed.assets[1].resident_frames().is_none());
            assert_eq!(streamed.report.required_resident_bytes, 128);
            assert_eq!(streamed.report.head_bytes, packed);
            assert_eq!(
                streamed.streamer.head_budget_status().held_bytes,
                128 + packed
            );
            assert_eq!(streamed.report.head_fallback.is_some(), fallback);
            assert_eq!(
                streamed
                    .streamer
                    .head_readiness(&streamed.assets)
                    .complete(),
                packed == 32
            );
        }
    }
}

#[test]
fn program_memory_policy_covers_all_sources_and_cancellation_keeps_its_typed_identity() {
    let dir = Scratch::new();
    let path = dir.program(
        "Streaming='0'",
        "<SamplePlayer SamplePath='a.wav'/><SamplePlayer SamplePath='b.wav' AllowStreaming='1'/>",
    );
    let translated = sampler_uvi::translate_path(&path).unwrap();
    assert!(
        translated
            .instrument
            .assets
            .iter()
            .all(|a| a.residency == AssetResidency::Resident)
    );
    let streamed =
        sampler_uvi::assemble_translated_streamed(translated, 48000, &policy(true, 256)).unwrap();
    assert_eq!(streamed.report.required_resident_bytes, 256);
    assert_eq!(streamed.report.head_bytes, 0);
    let translated = sampler_uvi::translate_path(&path).unwrap();
    let error = sampler_uvi::assemble_translated_streamed_cancelable(
        translated,
        48000,
        &policy(true, 256),
        &|| true,
    )
    .err()
    .unwrap();
    assert_eq!(
        error
            .downcast_ref::<sampler_kontakt::LoadError>()
            .unwrap()
            .kind(),
        sampler_kontakt::Kind::Canceled
    );
}
