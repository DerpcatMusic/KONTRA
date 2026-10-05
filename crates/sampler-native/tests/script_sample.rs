use std::{fs, process::Command, time::SystemTime};

fn pcm16(rate: u32, frames: u32, amplitude: i16) -> Vec<u8> {
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + frames * 2).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(frames * 2).to_le_bytes());
    for _ in 0..frames {
        wav.extend_from_slice(&amplitude.to_le_bytes());
    }
    wav
}

#[test]
fn authored_script_renders_owned_wav_at_its_rate_and_refuses_invalid_or_existing_outputs() {
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "sampler-native-source-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir(&directory).unwrap();
    let source = directory.join("delayed.ksp");
    fs::write(
        &source,
        include_str!("../../sampler-ksp/tests/fixtures/delayed-note.ksp"),
    )
    .unwrap();
    let release_source = directory.join("release.ksp");
    fs::write(
        &release_source,
        include_str!("../../sampler-ksp/tests/fixtures/polyphonic-release.ksp"),
    )
    .unwrap();
    let branch_source = directory.join("branch.ksp");
    fs::write(
        &branch_source,
        "on note ignore_event($EVENT_ID)
        wait(750000)
        if ($NOTE_HELD = 0)
            play_note($EVENT_NOTE, 96, 0, 125000)
            exit
        else
            play_note($EVENT_NOTE, 1, 0, 125000)
        end if
        play_note($EVENT_NOTE, 127, 0, 125000)
        end on",
    )
    .unwrap();
    let executable = env!("CARGO_BIN_EXE_sampler-native");
    let loop_source = directory.join("loop.ksp");
    fs::write(
        &loop_source,
        "on note ignore_event($EVENT_ID)
        while ($NOTE_HELD = 1)
            play_note($EVENT_NOTE, 96, 0, 125000)
            wait(300000)
        end while
        end on",
    )
    .unwrap();
    for rate in [44100u32, 48000, 96000] {
        let frames = rate / 100;
        // Authored 10 ms mono PCM16 constant; no decoder or engine serves as oracle.
        let wav = pcm16(rate, frames, 16384);
        let input = directory.join(format!("sample-{rate}.wav"));
        let output = directory.join(format!("output-{rate}.wav"));
        fs::write(&input, &wav).unwrap();
        let rendered = Command::new(executable)
            .arg("script")
            .args([&source, &input, &output])
            .output()
            .unwrap();
        assert!(
            rendered.status.success(),
            "{}",
            String::from_utf8_lossy(&rendered.stderr)
        );
        let result = fs::read(&output).unwrap();
        assert_eq!(&result[..4], b"RIFF");
        assert_eq!(&result[24..28], &rate.to_le_bytes());
        assert_eq!(&result[50..54], b"data");
        assert_eq!(result.len(), 58 + rate as usize * 2 * 8);
        let start = rate as usize * 5 / 4;
        let attack = (rate / 200) as usize;
        let decay = (rate / 10) as usize;
        for (i, frame) in result[58..].as_chunks::<8>().0.iter().enumerate() {
            let expected = if (start..start + frames as usize).contains(&i) {
                let t = i - start;
                let envelope = if t < attack {
                    t as f32 / attack as f32
                } else {
                    1. - 0.2 * (t - attack) as f32 / decay as f32
                };
                0.5 * (96. / 127.) * envelope
            } else {
                0.
            };
            for sample in frame.as_chunks::<4>().0 {
                let actual = f32::from_le_bytes(*sample);
                assert!(
                    (actual - expected).abs() < 2e-7,
                    "rate {rate}, frame {i}: {actual} != {expected}"
                );
            }
        }
        let release_output = directory.join(format!("release-{rate}.wav"));
        let release_render = Command::new(executable)
            .arg("script")
            .args([&release_source, &input, &release_output])
            .output()
            .unwrap();
        assert!(
            release_render.status.success(),
            "{}",
            String::from_utf8_lossy(&release_render.stderr)
        );
        let release_audio = fs::read(&release_output).unwrap();
        let start = rate as usize / 2 + (u64::from(rate) * 125).div_ceil(1_000_000) as usize;
        for (i, frame) in release_audio[58..].as_chunks::<8>().0.iter().enumerate() {
            let left = f32::from_le_bytes(frame[..4].try_into().unwrap());
            let right = f32::from_le_bytes(frame[4..].try_into().unwrap());
            assert_eq!(left, right);
            assert!(left.is_finite());
            assert_eq!(
                left > 0.,
                (start + 1..start + frames as usize).contains(&i),
                "release timing at rate {rate}, frame {i}"
            );
        }
        let branch_output = directory.join(format!("branch-{rate}.wav"));
        let branch_render = Command::new(executable)
            .arg("script")
            .args([&branch_source, &input, &branch_output])
            .output()
            .unwrap();
        assert!(
            branch_render.status.success(),
            "{}",
            String::from_utf8_lossy(&branch_render.stderr)
        );
        let branch_audio = fs::read(branch_output).unwrap();
        assert_eq!(branch_audio.len(), result.len());
        // The original independent expected waveform above is shifted 0.5 s.
        // Key-up was at 0.5 s, but sustain still holds the gate until 1 s.
        for (i, actual) in branch_audio[58..].as_chunks::<8>().0.iter().enumerate() {
            let expected = result[58..]
                .as_chunks::<8>()
                .0
                .get(i + rate as usize / 2)
                .copied()
                .unwrap_or([0; 8]);
            assert_eq!(
                *actual, expected,
                "conditional PCM at rate {rate}, frame {i}"
            );
        }
        let loop_output = directory.join(format!("loop-{rate}.wav"));
        let loop_render = Command::new(executable)
            .arg("script")
            .args([&loop_source, &input, &loop_output])
            .output()
            .unwrap();
        assert!(
            loop_render.status.success(),
            "{}",
            String::from_utf8_lossy(&loop_render.stderr)
        );
        let loop_audio = fs::read(loop_output).unwrap();
        assert_eq!(loop_audio.len(), result.len());
        for (i, actual) in loop_audio[58..].as_chunks::<8>().0.iter().enumerate() {
            let pulse_start = if i < rate as usize * 3 / 10 {
                0
            } else {
                rate as usize * 3 / 10
            };
            let expected = if i - pulse_start < frames as usize {
                result[58..].as_chunks::<8>().0[rate as usize * 5 / 4 + i - pulse_start]
            } else {
                [0; 8]
            };
            assert_eq!(*actual, expected, "loop PCM at rate {rate}, frame {i}");
        }
        let overwrite = Command::new(executable)
            .arg("script")
            .args([&source, &input, &output])
            .output()
            .unwrap();
        assert!(!overwrite.status.success());
        assert_eq!(fs::read(&output).unwrap(), result);
        let overwrite_input = Command::new(executable)
            .arg("script")
            .args([&source, &input, &input])
            .output()
            .unwrap();
        assert!(!overwrite_input.status.success());
        assert_eq!(fs::read(&input).unwrap(), wav);
    }
    let invalid = directory.join("invalid.ksp");
    fs::write(&invalid, "on note ignore_event($EVENT_ID) unknown() end on").unwrap();
    let bad_output = directory.join("bad.wav");
    let valid_input = directory.join("sample-48000.wav");
    let failed = Command::new(executable)
        .arg("script")
        .args([&invalid, &valid_input, &bad_output])
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(!bad_output.exists());
    let failed = Command::new(executable)
        .arg("script")
        .args([&source, &invalid, &bad_output])
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(!bad_output.exists());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn replacement_audition_overlaps_generations_and_releases_the_original_samples() {
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "sampler-native-replace-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir(&directory).unwrap();
    let executable = env!("CARGO_BIN_EXE_sampler-native");
    for rate in [44100u32, 48000, 96000] {
        let first = directory.join(format!("first-{rate}.wav"));
        let second = directory.join(format!("second-{rate}.wav"));
        let output = directory.join(format!("output-{rate}.wav"));
        fs::write(&first, pcm16(rate, rate * 2, 8192)).unwrap();
        fs::write(&second, pcm16(rate, rate * 2, 16384)).unwrap();
        let result = Command::new(executable)
            .arg("replace")
            .args([&first, &second, &output])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(String::from_utf8_lossy(&result.stdout).contains("2 terminals accepted"));
        let bytes = fs::read(&output).unwrap();
        assert_eq!(bytes.len(), 58 + rate as usize * 2 * 8);
        let envelope = |t: usize| {
            let attack = (rate / 200) as usize;
            let decay = (rate / 10) as usize;
            let release = (rate / 20) as usize;
            if t < attack {
                t as f32 / attack as f32
            } else if t < attack + decay {
                1. - 0.2 * (t - attack) as f32 / decay as f32
            } else if t < rate as usize {
                0.8
            } else if t < rate as usize + release {
                0.8 * (1. - (t - rate as usize) as f32 / release as f32)
            } else {
                0.
            }
        };
        for (i, frame) in bytes[58..].as_chunks::<8>().0.iter().enumerate() {
            let expected = 0.25 * envelope(i)
                + i.checked_sub(rate as usize / 2)
                    .map_or(0., |t| 0.5 * envelope(t));
            for sample in frame.as_chunks::<4>().0 {
                let actual = f32::from_le_bytes(*sample);
                assert!(
                    (actual - expected).abs() < 2e-7,
                    "rate {rate}, frame {i}: {actual} != {expected}"
                );
            }
        }
    }
    let first = directory.join("first-48000.wav");
    let wrong_rate = directory.join("second-44100.wav");
    let output = directory.join("mismatch.wav");
    let result = Command::new(executable)
        .arg("replace")
        .args([&first, &wrong_rate, &output])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("SampleRate"));
    assert!(!output.exists());
    let too_slow = directory.join("one-hz.wav");
    fs::write(&too_slow, pcm16(1, 2, 8192)).unwrap();
    let result = Command::new(executable)
        .arg("replace")
        .args([&too_slow, &too_slow, &output])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!output.exists());
    fs::remove_dir_all(directory).unwrap();
}
