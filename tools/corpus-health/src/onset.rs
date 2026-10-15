//! Thin wrapper; typed parsing and execution live in the shots root module.
use serde_json::json;

pub(super) fn run(args: &[String]) -> i32 {
    if args.len() != 1 {
        println!(
            "{}",
            json!({"schema":1,"status":"invalid_config","reason":"one_plan_path_required"})
        );
        return 2;
    }
    #[cfg(not(feature = "ui-render"))]
    {
        let _ = args;
        println!(
            "{}",
            json!({"schema":1,"status":"unavailable","reason":"ui_render_feature_required"})
        );
        2
    }
    #[cfg(feature = "ui-render")]
    {
        supervise(&args[0])
    }
}

#[cfg(feature = "ui-render")]
fn read_plan(input: impl std::io::Read) -> Result<serde_json::Value, &'static str> {
    use std::io::Read;
    let mut bytes = Vec::new();
    input
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| "plan_read_failed")?;
    if bytes.len() > 65536 {
        return Err("plan_exceeds_64k_limit");
    }
    let value = serde_json::from_slice(&bytes).map_err(|_| "invalid_json_plan")?;
    kontakto::onset_config::Config::parse(&value)?;
    Ok(value)
}

/// A distinct process bounds otherwise uninterruptible loader I/O/interpreters.
/// The child receives the validated in-memory plan; it never reopens the plan.
#[cfg(feature = "ui-render")]
fn supervise(path: &str) -> i32 {
    use std::{
        io::{Read, Write},
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    const MAX_JSON: usize = 8 * 1024 * 1024;
    let started = Instant::now();
    let result = (|| -> Result<(serde_json::Value, i32), &'static str> {
        if !std::fs::metadata(path)
            .map_err(|_| "plan_metadata_failed")?
            .is_file()
        {
            return Err("plan_must_be_a_regular_file");
        }
        let file = std::fs::File::open(path).map_err(|_| "plan_open_failed")?;
        let plan = read_plan(file)?;
        let total = kontakto::onset_config::Config::parse(&plan)?.total_deadline();
        let deadline = started + total.min(Duration::from_secs(120));
        let bytes = serde_json::to_vec(&plan).map_err(|_| "plan_encode_failed")?;
        let mut child =
            Command::new(std::env::current_exe().map_err(|_| "helper_executable_unavailable")?)
                .arg("onset-worker")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| "outer_process_spawn_failed")?;
        let mut output = child.stdout.take().ok_or("child_stdout_unavailable")?;
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let mut overflow = false;
            let mut chunk = [0u8; 8192];
            loop {
                let n = output.read(&mut chunk)?;
                if n == 0 {
                    break;
                }
                let remaining = MAX_JSON.saturating_sub(bytes.len());
                bytes.extend_from_slice(&chunk[..n.min(remaining)]);
                overflow |= n > remaining;
            }
            Ok::<_, std::io::Error>((bytes, overflow))
        });
        let sent = child
            .stdin
            .take()
            .ok_or("child_stdin_unavailable")?
            .write_all(&bytes);
        if sent.is_err() {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Err("child_plan_delivery_failed");
        }
        let mut timed_out = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    return Err("outer_process_wait_failed");
                }
            }
            if Instant::now() >= deadline {
                timed_out = true;
                child.kill().map_err(|_| "outer_process_kill_failed")?;
                break child.wait().map_err(|_| "outer_process_kill_failed")?;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let (bytes, overflow) = reader
            .join()
            .map_err(|_| "outer_report_reader_panicked")?
            .map_err(|_| "outer_report_read_failed")?;
        if timed_out {
            return Err("outer_process_deadline_exceeded");
        }
        if overflow {
            return Err("numeric_report_exceeds_8m_limit");
        }
        let mut report: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| "child_numeric_report_missing")?;
        report["outer_process_deadline_ms"] = json!(total.as_millis());
        report["outer_process_enforced"] = json!(true);
        Ok((report, if status.success() { 0 } else { 1 }))
    })();
    match result {
        Ok((report, code)) => {
            println!("{report}");
            code
        }
        Err(reason) => {
            println!(
                "{}",
                json!({"schema":1,"status":"partial","reason":reason,"outer_process_enforced":true})
            );
            1
        }
    }
}

pub(super) fn worker(args: &[String]) -> i32 {
    #[cfg(not(feature = "ui-render"))]
    {
        let _ = args;
        2
    }
    #[cfg(feature = "ui-render")]
    {
        let result = if args.is_empty() {
            read_plan(std::io::stdin().lock()).and_then(|p| kontakto::offline_kontakt_onset(&p))
        } else {
            Err("worker_requires_stdin_plan")
        };
        match result {
            Ok(report) => {
                let complete = report["status"] == "complete";
                println!("{report}");
                if complete { 0 } else { 1 }
            }
            Err(reason) => {
                println!("{}", json!({"schema":1,"status":"partial","reason":reason}));
                1
            }
        }
    }
}

#[cfg(all(test, feature = "ui-render"))]
mod tests {
    use super::*;
    #[test]
    fn bounded_input_and_invalid_config_never_reach_a_loader() {
        assert!(read_plan(&b"{}"[..]).is_err());
        assert!(read_plan(&vec![b' '; 65537][..]).is_err());
        assert!(read_plan(&b"{\"path\":\"does-not-exist.nki\"}"[..]).is_err());
    }
}
