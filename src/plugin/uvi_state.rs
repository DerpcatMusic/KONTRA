//! Native persistence stays on the controller lane. Only a scalar minimum PCM
//! boundary crosses audio; Lua/onSave, encoding and owned bytes stay off audio.
use super::*;
use crate::uvi::worker::Stamp;
use std::time::Duration;

fn installed(params: &SamplerParams, slot: usize, activation: &uvi_load::Activation) -> bool {
    activation.current(params, slot)
        && params.shared.part(slot).is_some_and(|part| {
            part.uvi_generation.load(Ordering::Acquire) == activation.generation
                && part.uvi_part_generation.load(Ordering::Acquire) == activation.part_generation
                && !part.uvi_failed.load(Ordering::Acquire)
        })
}

fn minimum(params: &SamplerParams, slot: usize, activation: &uvi_load::Activation) -> Stamp {
    Stamp {
        epoch: activation.epoch,
        generation: activation.generation,
        frame: params
            .shared
            .part(slot)
            .unwrap()
            .uvi_state_frame
            .load(Ordering::Acquire),
    }
}

fn commit(params: &SamplerParams, values: &[(usize, uvi_load::Activation, NativeState)]) -> bool {
    // Same lock order as Load. The baseline and rack bytes change together, so
    // capture never looks like a host restore of the same bank/member.
    let mut view = params.shared.view.lock().unwrap();
    let mut selection = params.selection.write().unwrap();
    if values.iter().any(|(slot, activation, _)| {
        !activation.context_matches(params, *slot, &selection)
            || !view
                .parts
                .get(*slot)
                .and_then(|part| part.uvi_activation.as_ref())
                .is_some_and(|current| {
                    (current.epoch, current.generation) == (activation.epoch, activation.generation)
                })
    }) {
        return false;
    }
    for (slot, _, bytes) in values {
        selection.parts[*slot].uvi_state = bytes.clone();
        view.parts[*slot]
            .uvi_activation
            .as_mut()
            .unwrap()
            .saved_state = bytes.clone();
    }
    true
}

/// Drain only explicitly requested replies on Load. Authored onSave is never
/// called by ordinary playback or editor polling.
pub(super) fn poll(params: &SamplerParams) {
    let Ok(_capture) = params.shared.uvi_state_capture.try_lock() else {
        return;
    };
    let activations: Vec<_> = params
        .shared
        .view
        .lock()
        .unwrap()
        .parts
        .iter()
        .enumerate()
        .filter_map(|(slot, part)| part.uvi_activation.clone().map(|a| (slot, a)))
        .collect();
    for (slot, activation) in activations {
        if !installed(params, slot, &activation) {
            continue;
        }
        let reply = params
            .shared
            .uvi_controls
            .lock()
            .unwrap()
            .poll_state(activation.epoch, activation.generation);
        if let Some(Ok((_, bytes))) = reply {
            if !commit(params, &[(slot, activation.clone(), bytes.into())]) {
                continue;
            }
        }
    }
}

/// Explicit rack/host save: never report success with an older native control
/// value. A stopped host with an unsealed PCM packet cannot settle it; a bounded
/// failure retains the last good state, and rack Save can report that failure.
pub(super) fn capture(params: &SamplerParams, requested: &mut Selection) -> anyhow::Result<()> {
    if !requested.parts.iter().any(|part| part.uvi.is_some()) {
        return Ok(());
    }
    let _capture = params.shared.uvi_state_capture.lock().unwrap();
    // The UI may have copied the rack just before the loader published newer
    // bytes. Refresh only matching source state before capturing.
    {
        let current = params.selection.read().unwrap();
        for (slot, part) in requested.parts.iter_mut().enumerate() {
            if let Some(now) = current.parts.get(slot).filter(|now| now.uvi == part.uvi) {
                part.uvi_state = now.uvi_state.clone();
            }
        }
    }
    anyhow::ensure!(
        params.shared.uvi_edits.is_empty(),
        "UVI controls are still applying; save again after playback processes them"
    );
    let targets: Vec<_> = {
        let view = params.shared.view.lock().unwrap();
        requested
            .parts
            .iter()
            .enumerate()
            .filter(|(_, part)| part.uvi.is_some())
            .map(|(slot, part)| {
                let activation = view
                    .parts
                    .get(slot)
                    .and_then(|v| v.uvi_activation.clone())
                    .ok_or_else(|| anyhow::anyhow!("UVI instrument is still loading"))?;
                anyhow::ensure!(
                    part.uvi.as_ref() == Some(&activation.source)
                        && part.uvi_state == activation.saved_state,
                    "UVI instrument changed while saving"
                );
                Ok((slot, activation))
            })
            .collect::<anyhow::Result<_>>()?
    };
    for (slot, activation) in &targets {
        anyhow::ensure!(
            installed(params, *slot, activation),
            "UVI instrument is not ready to save"
        );
        params
            .shared
            .uvi_controls
            .lock()
            .unwrap()
            .request_state(minimum(params, *slot, activation), true)
            .map_err(|_| anyhow::anyhow!("UVI state capture could not start"))?;
    }
    let started = Instant::now();
    let mut values = Vec::with_capacity(targets.len());
    let mut pending = targets;
    while !pending.is_empty() {
        for at in (0..pending.len()).rev() {
            let (slot, activation) = &pending[at];
            anyhow::ensure!(
                installed(params, *slot, activation),
                "UVI instrument changed or failed while saving"
            );
            let reply = params
                .shared
                .uvi_controls
                .lock()
                .unwrap()
                .poll_state(activation.epoch, activation.generation);
            if let Some(reply) = reply {
                let (_, bytes) = reply.map_err(|_| {
                    anyhow::anyhow!("UVI onSave failed; previous state was retained")
                })?;
                let (slot, activation) = pending.swap_remove(at);
                values.push((slot, activation, bytes.into()));
            }
        }
        if !pending.is_empty() {
            anyhow::ensure!(
                started.elapsed() < Duration::from_millis(500),
                "UVI state is waiting for a processed audio boundary; resume playback and save again"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    anyhow::ensure!(
        commit(params, &values),
        "UVI instrument changed while saving"
    );
    for (slot, _, bytes) in values {
        requested.parts[slot].uvi_state = bytes;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_commit_updates_baseline_without_reload_and_rejects_same_source_restore() {
        let params = SamplerParams::default();
        params.shared.ensure_parts(1);
        params
            .shared
            .rate
            .store(48000f64.to_bits(), Ordering::Release);
        params.shared.uvi_max_host_frames.store(256, Ordering::Release);
        let source = library::UviSource {
            bank: "state-test.ufs".into(),
            bank_uuid: [0; 16],
            member: "preset.uvip".into(),
        };
        params.selection.write().unwrap().parts = vec![Part {
            uvi: Some(source.clone()),
            uvi_state: vec![1].into(),
            ..Default::default()
        }];
        let activation = uvi_load::Activation {
            source,
            saved_state: vec![1].into(),
            epoch: params.shared.uvi_epoch.load(Ordering::Acquire),
            generation: 2,
            part_generation: params
                .shared
                .part(0)
                .unwrap()
                .generation
                .load(Ordering::Acquire),
            rate: 48000,
            max_host_frames: params.shared.uvi_max_host_frames.load(Ordering::Acquire),
            published: true,
        };
        params.shared.view.lock().unwrap().parts[0].uvi_activation = Some(activation.clone());
        assert!(commit(&params, &[(0, activation.clone(), vec![2].into())]));
        let updated = params.shared.view.lock().unwrap().parts[0]
            .uvi_activation
            .clone()
            .unwrap();
        assert!(
            updated.current(&params, 0),
            "captured bytes advance the baseline without a replacement"
        );
        assert!(
            Arc::ptr_eq(
                &params.selection.read().unwrap().parts[0].uvi_state.0,
                &updated.saved_state.0
            ),
            "rack bytes and activation baseline share one allocation"
        );
        assert!(
            !commit(&params, &[(0, activation, vec![3].into())]),
            "an old reply cannot overwrite a newer baseline"
        );
        params.selection.write().unwrap().parts[0].uvi_state = vec![4].into();
        assert!(
            !updated.current(&params, 0),
            "a same-source host restore needs a fresh native activation"
        );
        assert!(!commit(&params, &[(0, updated, vec![5].into())]));
        assert_eq!(
            params.selection.read().unwrap().parts[0].uvi_state,
            NativeState::from(vec![4])
        );
    }
    fn live_fixture(script: &str) -> (SamplerParams, uvi_control::Audio, std::path::PathBuf) {
        let params = SamplerParams::default();
        params.shared.ensure_parts(1);
        params.shared.uvi_max_host_frames.store(256, Ordering::Release);
        params
            .shared
            .rate
            .store(48000f64.to_bits(), Ordering::Release);
        let (mut config, _) = crate::uvi::worker::tests::authored_bank_with_script(script);
        config.expected_bank_uuid = Some([0; 16]);
        let source = library::UviSource {
            bank: config.bank.clone(),
            bank_uuid: [0; 16],
            member: config.member.clone(),
        };
        let path = source.bank.clone();
        let activation = uvi_load::Activation {
            source: source.clone(),
            saved_state: NativeState::default(),
            epoch: params.shared.uvi_epoch.load(Ordering::Acquire),
            generation: 2,
            part_generation: params
                .shared
                .part(0)
                .unwrap()
                .generation
                .load(Ordering::Acquire),
            rate: 48000,
            max_host_frames: params.shared.uvi_max_host_frames.load(Ordering::Acquire),
            published: true,
        };
        params.selection.write().unwrap().parts = vec![Part {
            uvi: Some(source.clone()),
            ..Default::default()
        }];
        params.shared.view.lock().unwrap().parts[0].uvi_activation = Some(activation.clone());
        params
            .shared
            .uvi_controls
            .lock()
            .unwrap()
            .prepare(
                config,
                source,
                activation.epoch,
                2,
                activation.part_generation,
                0,
                activation.max_host_frames,
                1,
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let audio = loop {
            match params
                .shared
                .uvi_controls
                .lock()
                .unwrap()
                .take_ready(activation.epoch, 2)
            {
                Ok(Some(audio)) => break audio,
                Ok(None) => {}
                Err(error) => panic!("{error:?}"),
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        params
            .shared
            .part(0)
            .unwrap()
            .uvi_generation
            .store(2, Ordering::Release);
        params
            .shared
            .part(0)
            .unwrap()
            .uvi_part_generation
            .store(activation.part_generation, Ordering::Release);
        (params, audio, path)
    }

    #[test]
    fn explicit_save_waits_for_pcm_and_retains_previous_bytes_on_timeout_or_on_save_failure() {
        let (params, audio, path) =
            live_fixture("n=Knob('n',0.25,0,1);function onSave()return {saved=true}end");
        let mut selection = params.selection.read().unwrap().clone();
        capture(&params, &mut selection).unwrap();
        assert!(!selection.parts[0].uvi_state.is_empty());
        let previous = selection.parts[0].uvi_state.clone();
        params
            .shared
            .part(0)
            .unwrap()
            .uvi_state_frame
            .store(256, Ordering::Release);
        assert!(
            capture(&params, &mut selection).is_err(),
            "no audio packet was processed at the requested boundary"
        );
        assert_eq!(
            params.selection.read().unwrap().parts[0].uvi_state,
            previous
        );
        drop(audio);
        drop(params);
        std::fs::remove_file(path).unwrap();
        let (params, audio, path) =
            live_fixture("function onSave()error('authored save failure')end");
        let mut selection = params.selection.read().unwrap().clone();
        assert!(capture(&params, &mut selection).is_err());
        assert!(
            params.selection.read().unwrap().parts[0]
                .uvi_state
                .is_empty()
        );
        drop(audio);
        drop(params);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn playback_and_loader_poll_never_call_authored_on_save() {
        use crate::uvi::host::UiValue;
        let (params, mut audio, path) =
            live_fixture("n=Knob('saves',0,0,100);function onSave()n.value=n.value+1;return {}end");
        let epoch = params.shared.uvi_epoch.load(Ordering::Acquire);
        for _ in 0..4 {
            audio
                .slot_mut()
                .process_mode(&mut [0.; 256], &mut [0.; 256], true)
                .unwrap();
            poll(&params);
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(ui) = params.shared.uvi_controls.lock().unwrap().poll_ui(epoch, 2)
                && let Some(widget) = ui.snapshots.iter().flat_map(|s| &s.widgets).next()
            {
                assert!(
                    widget.value == Some(UiValue::Number(0.)),
                    "onSave must stay dormant during playback"
                );
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        let mut selection = params.selection.read().unwrap().clone();
        capture(&params, &mut selection).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(ui) = params.shared.uvi_controls.lock().unwrap().poll_ui(epoch, 2)
                && let Some(widget) = ui.snapshots.iter().flat_map(|s| &s.widgets).next()
                && widget.value == Some(UiValue::Number(1.))
            {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        drop(audio);
        drop(params);
        std::fs::remove_file(path).unwrap();
    }
}
