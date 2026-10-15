//! Off-audio UVI observations bound to the exact installed owner and epoch.
//! These APIs do not service/render the core, alter inputs, or infer audibility.
use super::V2Core;
use sampler_uvi::scripted::{
    DiagnosticPoll, DiagnosticProgress, DiagnosticRequest, DiagnosticRequestError, UiBridge,
};
use std::{sync::Arc, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UviDiagnosticError {
    PartUnavailable,
    StaleEpoch,
    DriverUnavailable,
    StaleOwner,
    Request(DiagnosticRequestError),
}

pub struct UviDiagnosticRequest {
    slot: usize,
    epoch: u64,
    owner: Arc<UiBridge>,
    request: DiagnosticRequest,
}
impl UviDiagnosticRequest {
    pub fn slot(&self) -> usize {
        self.slot
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn id(&self) -> u64 {
        self.request.id()
    }
    pub fn cancel(&mut self) -> DiagnosticPoll {
        self.request.cancel()
    }
}

pub struct UviDiagnosticObservation {
    pub handles_notes: bool,
    pub progress: Option<DiagnosticProgress>,
    #[cfg(feature = "shots")]
    pub owner_audit: Option<sampler_uvi::script::diagnostics::OwnerAuditSnapshot>,
}

pub struct UviDiagnosticReply {
    pub poll: DiagnosticPoll,
    /// Ordered marker consumed by the ordinary command drain. Audio witnesses
    /// and successful sample commits must still be assessed separately.
    pub commands_drained: bool,
    pub observation: UviDiagnosticObservation,
}

impl V2Core {
    fn uvi_diagnostic_owner(
        &self,
        slot: usize,
        epoch: u64,
        owner: &Arc<UiBridge>,
    ) -> Result<&super::ScriptDriver, UviDiagnosticError> {
        let part = self
            .parts
            .get(slot)
            .and_then(Option::as_ref)
            .ok_or(UviDiagnosticError::PartUnavailable)?;
        if part.epoch != epoch {
            return Err(UviDiagnosticError::StaleEpoch);
        }
        let driver = part
            .script
            .as_deref()
            .ok_or(UviDiagnosticError::DriverUnavailable)?;
        if !Arc::ptr_eq(driver.ui(), owner) {
            return Err(UviDiagnosticError::StaleOwner);
        }
        Ok(driver)
    }

    /// Off audio only. Enable before enqueueing the note/controller to count it.
    pub fn enable_uvi_diagnostics(
        &self,
        slot: usize,
        epoch: u64,
        owner: &Arc<UiBridge>,
    ) -> Result<DiagnosticProgress, UviDiagnosticError> {
        Ok(self
            .uvi_diagnostic_owner(slot, epoch, owner)?
            .ui()
            .enable_diagnostics())
    }

    /// Numeric snapshots never block behind the owner's Lua or UI export.
    pub fn uvi_diagnostic_progress(
        &self,
        slot: usize,
        epoch: u64,
        owner: &Arc<UiBridge>,
    ) -> Result<UviDiagnosticObservation, UviDiagnosticError> {
        let driver = self.uvi_diagnostic_owner(slot, epoch, owner)?;
        Ok(UviDiagnosticObservation {
            handles_notes: driver.handles_notes(),
            progress: driver.ui().diagnostic_progress(),
            #[cfg(feature = "shots")]
            owner_audit: driver.ui().owner_audit(),
        })
    }

    /// Request one ordinary owner turn. `timeout` is wall time, independently
    /// of any audio frame/service budget enforced by the inspector caller.
    pub fn request_uvi_diagnostics(
        &self,
        slot: usize,
        epoch: u64,
        owner: &Arc<UiBridge>,
        timeout: Duration,
    ) -> Result<UviDiagnosticRequest, UviDiagnosticError> {
        let driver = self.uvi_diagnostic_owner(slot, epoch, owner)?;
        let request = driver
            .ui()
            .request_diagnostics(timeout)
            .map_err(UviDiagnosticError::Request)?;
        Ok(UviDiagnosticRequest {
            slot,
            epoch,
            owner: owner.clone(),
            request,
        })
    }

    /// Validate the current installed owner on EVERY poll, including terminal
    /// replies. A retired or staged bridge cannot satisfy a replacement's wait.
    pub fn poll_uvi_diagnostics(
        &self,
        token: &mut UviDiagnosticRequest,
    ) -> Result<UviDiagnosticReply, UviDiagnosticError> {
        let observation = match self.uvi_diagnostic_progress(token.slot, token.epoch, &token.owner)
        {
            Ok(observation) => observation,
            Err(error) => {
                token.cancel();
                return Err(error);
            }
        };
        Ok(UviDiagnosticReply {
            poll: token.request.poll(),
            commands_drained: token.request.commands_drained(),
            observation,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{BlockInfo, Core, Event, MixTree, Part};
    use super::*;
    use sampler_core::{Envelope, Limits, Pcm, Playback, Prepared, Region, Runtime};
    use sampler_uvi::{
        OscGroup,
        script::{Config, Files},
        scripted::{Driver, ScriptThread},
    };
    use std::{sync::mpsc, time::Instant};

    fn part(files: impl Files + Send + 'static) -> (Box<Part>, Arc<UiBridge>) {
        let plan = Prepared::new(
            48000,
            vec![Pcm::new(48000, vec![[0.5; 2]; 16000].into()).unwrap()],
            vec![Region {
                sample: 0,
                key_low: 60,
                key_high: 60,
                root_key: Some(60),
                velocity_low: 0.,
                velocity_high: 1.,
                gain: 1.,
                envelope: Envelope::new(0, 0, 0, 1., 1).unwrap(),
                playback: Playback::default(),
            }],
            1,
        )
        .unwrap()
        .with_groups(1, vec![Some(0)])
        .unwrap();
        let limits = Limits::for_plan(&plan, 8, 8);
        let mut part = Part::new(
            Runtime::new(plan, limits).unwrap(),
            MixTree::instrument("authored"),
        )
        .unwrap();
        let xml = "<UVI4><Program><Layers><Layer/></Layers><EventProcessors><ScriptProcessor><script>function onNote(e) require('hold'); playNote(e.note,e.velocity) end</script></ScriptProcessor></EventProcessors></Program></UVI4>";
        let (thread, loaded) = ScriptThread::spawn(xml.into(), files, Config::default()).unwrap();
        part.script = Some(Box::new(Driver::new(
            thread,
            vec![OscGroup {
                layer: 1,
                osc: 1,
                group: 0,
                keygroup: 0,
                oscillator: 0,
            }],
            48000,
        )));
        part.epoch = 77;
        (Box::new(part), loaded.ui)
    }
    fn until(mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !done() {
            assert!(
                Instant::now() < deadline,
                "bounded core diagnostic fixture timeout"
            );
            std::thread::yield_now();
        }
    }
    struct Held {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }
    impl Files for Held {
        fn script(&self, _: &str) -> Option<String> {
            self.entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(3)).unwrap();
            Some("return {}".into())
        }
    }
    struct Module;
    impl Files for Module {
        fn script(&self, _: &str) -> Option<String> {
            Some("return {}".into())
        }
    }

    #[test]
    fn fast_silent_window_is_unverified_until_real_owner_and_core_acknowledge() {
        let (entered, waiting) = mpsc::channel();
        let (release, held) = mpsc::channel();
        let (part, owner) = part(Held {
            entered,
            release: held,
        });
        let mut core = V2Core::default();
        core.reset(48000.);
        core.install(0, Some(part));
        core.enable_uvi_diagnostics(0, 77, &owner).unwrap();
        assert!(
            !core.scripts_running(0),
            "empty KSP ledger cannot attest to Lua execution"
        );
        core.begin_block(&BlockInfo {
            frames: 128,
            offline: true,
            ..Default::default()
        });
        core.play(0, Event::midi1(0x90, 60, 64));
        waiting.recv_timeout(Duration::from_secs(3)).unwrap();
        let mut token = core
            .request_uvi_diagnostics(0, 77, &owner, Duration::from_secs(3))
            .unwrap();
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                for frames in [128; 37].into_iter().chain([64]) {
                    let audio = core.render(frames);
                    assert!(
                        audio.buses[0]
                            .iter()
                            .all(|c| c[..frames].iter().all(|s| *s == 0.))
                    );
                }
            }),
            0,
            "opt-in monitoring cannot allocate on audio"
        );
        assert_eq!(core.voices().active, 0);
        let busy = core.poll_uvi_diagnostics(&mut token).unwrap();
        assert_eq!(busy.poll, DiagnosticPoll::Pending);
        assert!(!busy.commands_drained);
        assert_eq!(busy.observation.progress.unwrap().events_executed, 0);
        release.send(()).unwrap();
        until(|| {
            matches!(
                core.poll_uvi_diagnostics(&mut token).unwrap().poll,
                DiagnosticPoll::Complete(_)
            )
        });
        assert!(
            !core
                .poll_uvi_diagnostics(&mut token)
                .unwrap()
                .commands_drained
        );
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                core.begin_block(&BlockInfo {
                    frames: 128,
                    offline: true,
                    ..Default::default()
                });
                let output = core.render(128);
                assert!(
                    output.buses[0]
                        .iter()
                        .any(|c| c[..128].iter().any(|s| s.abs() > 0.01))
                );
            }),
            0,
            "ordered diagnostic markers cannot allocate on audio"
        );
        assert!(core.voices().active > 0);
        let completed = core.poll_uvi_diagnostics(&mut token).unwrap();
        assert!(completed.commands_drained);
        let DiagnosticPoll::Complete(ack) = completed.poll else {
            panic!("owner acknowledgement")
        };
        assert!(ack.sample_counts_conserved());
    }

    #[test]
    fn staged_replaced_or_stale_epoch_owner_cannot_satisfy_a_scoped_token() {
        let (active, owner) = part(Module);
        let (staged, candidate) = part(Module);
        let mut core = V2Core::default();
        core.install(0, Some(active));
        assert!(matches!(
            core.request_uvi_diagnostics(0, 78, &owner, Duration::from_secs(3)),
            Err(UviDiagnosticError::StaleEpoch)
        ));
        assert!(matches!(
            core.request_uvi_diagnostics(0, 77, &candidate, Duration::from_secs(3)),
            Err(UviDiagnosticError::StaleOwner)
        ));
        assert!(matches!(
            core.uvi_diagnostic_progress(1, 77, &owner),
            Err(UviDiagnosticError::PartUnavailable)
        ));
        let mut token = core
            .request_uvi_diagnostics(0, 77, &owner, Duration::from_secs(3))
            .unwrap();
        until(|| {
            matches!(
                core.poll_uvi_diagnostics(&mut token).unwrap().poll,
                DiagnosticPoll::Complete(_)
            )
        });
        let retired = core.install(0, Some(staged));
        assert!(matches!(
            core.poll_uvi_diagnostics(&mut token),
            Err(UviDiagnosticError::StaleOwner)
        ));
        assert!(matches!(
            core.uvi_diagnostic_progress(0, 77, &owner),
            Err(UviDiagnosticError::StaleOwner)
        ));
        let mut replacement = core
            .request_uvi_diagnostics(0, 77, &candidate, Duration::from_secs(3))
            .unwrap();
        core.parts[0].as_mut().unwrap().epoch = 78;
        assert!(matches!(
            core.poll_uvi_diagnostics(&mut replacement),
            Err(UviDiagnosticError::StaleEpoch)
        ));
        drop(retired);
    }
}
