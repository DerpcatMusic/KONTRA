//! Authored controller tests: no display or native window is created.
use super::*;
use std::sync::mpsc::{self, RecvTimeoutError};

fn controller(worker: JoinHandle<()>) -> (WindowThreadControl, calloop::EventLoop<'static, ()>) {
    let event_loop = calloop::EventLoop::try_new().unwrap();
    let (request_sender, requests) = calloop::channel::sync_channel(1);
    let (responses, response_receiver) = mpsc::channel();
    // A dead request protocol must not make close detach its still-live worker.
    drop(requests);
    drop(responses);
    let control = WindowThreadControl {
        shared: Arc::new(WindowThreadShared::new()),
        loop_signal: event_loop.get_signal(),
        event_loop_handle: Cell::new(Some(worker)),
        request_sender,
        response_receiver,
        can_detach: Cell::new(false),
        close_timeout: Cell::new(None),
    };
    (control, event_loop)
}

#[test]
fn no_host_controller_is_send_without_an_unsafe_assertion() {
    fn assert_send<T: Send>() {}
    assert_send::<WindowThreadControl>();
    assert_send::<crate::ThreadedWindow>();
}

#[test]
fn failed_request_still_joins_worker_before_returning() {
    let (release, waiting) = mpsc::channel();
    let worker = thread::spawn(move || waiting.recv().unwrap());
    let (control, _event_loop) = controller(worker);
    let (entered, entering) = mpsc::channel();
    let (finished, finishing) = mpsc::channel();
    let closer = thread::spawn(move || {
        entered.send(()).unwrap();
        let result = control.run_until_closed();
        finished.send(result.is_err()).unwrap();
    });
    entering.recv().unwrap();
    assert_eq!(finishing.recv_timeout(Duration::from_millis(30)), Err(RecvTimeoutError::Timeout));
    release.send(()).unwrap();
    assert!(finishing.recv_timeout(Duration::from_secs(2)).unwrap());
    closer.join().unwrap();
}

#[test]
fn controller_drop_waits_for_payload_drop_on_worker() {
    struct Payload(mpsc::Sender<thread::ThreadId>);
    impl Drop for Payload {
        fn drop(&mut self) {
            self.0.send(thread::current().id()).unwrap();
        }
    }
    let (release, waiting) = mpsc::channel();
    let (record, dropped) = mpsc::channel();
    let worker = thread::spawn(move || {
        let _payload = Payload(record);
        waiting.recv().unwrap();
    });
    let native_owner = worker.thread().id();
    let (control, _event_loop) = controller(worker);
    let (entered, entering) = mpsc::channel();
    let (finished, finishing) = mpsc::channel();
    let closer = thread::spawn(move || {
        entered.send(()).unwrap();
        drop(control);
        finished.send(()).unwrap();
    });
    entering.recv().unwrap();
    assert_eq!(finishing.recv_timeout(Duration::from_millis(30)), Err(RecvTimeoutError::Timeout));
    release.send(()).unwrap();
    finishing.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(dropped.recv().unwrap(), native_owner);
    closer.join().unwrap();
}

#[test]
fn self_close_requests_stop_without_joining_itself() {
    let (send, receive) = mpsc::channel::<WindowThreadControl>();
    let (result, finished) = mpsc::channel();
    let worker = thread::spawn(move || {
        let control = receive.recv().unwrap();
        let state = Arc::clone(&control.shared);
        let closed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(control)));
        result.send((closed.is_ok(), state.is_stop_host_requested())).unwrap();
    });
    let (control, _event_loop) = controller(worker);
    send.send(control).unwrap();
    assert_eq!(finished.recv_timeout(Duration::from_secs(2)).unwrap(), (true, true));
}

#[test]
fn stale_controller_cannot_stop_a_different_window_worker() {
    let (release_a, waiting_a) = mpsc::channel();
    let (release_b, waiting_b) = mpsc::channel();
    let (a, _loop_a) = controller(thread::spawn(move || waiting_a.recv().unwrap()));
    let (b, _loop_b) = controller(thread::spawn(move || waiting_b.recv().unwrap()));
    let state_b = Arc::clone(&b.shared);
    release_a.send(()).unwrap();
    thread::spawn(move || drop(a)).join().unwrap();
    assert!(!state_b.is_stop_host_requested());
    release_b.send(()).unwrap();
    drop(b);
    assert!(state_b.is_stop_host_requested());
}
