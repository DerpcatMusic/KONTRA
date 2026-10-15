//! Caller-side progress relay while the existing load worker owns preparation.
use crate::sound::Progress;
use std::{
    sync::mpsc::{Receiver, RecvTimeoutError},
    time::{Duration, Instant},
};

pub(super) const SERVICE_PERIOD: Duration = Duration::from_millis(20);

pub(crate) fn relay(
    receiver: Receiver<Progress>,
    progress: &mut dyn FnMut(Progress),
    mut service: Option<&mut dyn FnMut()>,
) {
    let Some(service) = service.as_mut() else {
        for value in receiver {
            progress(value);
        }
        return;
    };
    let mut next = Instant::now() + SERVICE_PERIOD;
    loop {
        match receiver.recv_timeout(next.saturating_duration_since(Instant::now())) {
            Ok(value) => progress(value),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        // Busy progress delivery must not starve service either. Continue until
        // the worker disconnects, including its cancellation/unwind interval.
        if Instant::now() >= next {
            service();
            next = Instant::now() + SERVICE_PERIOD;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_owner_is_serviced_before_a_worker_with_no_progress_can_complete() {
        let (progress_tx, progress_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let caller = std::thread::current().id();
        let mut observed = Vec::new();
        let mut services = 0;
        std::thread::scope(|scope| {
            scope.spawn(move || {
                release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                progress_tx.send(Progress(750)).unwrap();
                progress_tx.send(Progress::DONE).unwrap();
            });
            relay(
                progress_rx,
                &mut |p| observed.push(p.0),
                Some(&mut || {
                    assert_eq!(std::thread::current().id(), caller);
                    services += 1;
                    let _ = release_tx.try_send(());
                }),
            );
        });
        assert!(services > 0);
        assert_eq!(observed, [750, Progress::DONE.0]);
    }

    #[test]
    fn continuous_progress_does_not_starve_service_or_drop_progress() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Progress(100)).unwrap();
        tx.send(Progress(200)).unwrap();
        drop(tx);
        let mut observed = Vec::new();
        let mut services = 0;
        relay(
            rx,
            &mut |p| {
                observed.push(p.0);
                // Model a slow progress consumer while the channel remains ready.
                std::thread::sleep(SERVICE_PERIOD);
            },
            Some(&mut || services += 1),
        );
        assert_eq!(observed, [100, 200]);
        assert_eq!(services, 2);
    }
}
