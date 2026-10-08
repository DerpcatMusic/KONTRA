//! Non-audio collection and privacy-safe run artifacts for the shared trace.
use crate::trace::{TraceReader, TraceRecord};
use serde::Serialize;
use std::{
    fs::File,
    io::{self, Seek, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock, mpsc},
    time::{Duration, Instant},
};
#[derive(Clone, Debug, Serialize)]
pub struct ReportSummary {
    pub json: PathBuf,
    pub chart: PathBuf,
    pub records: u64,
    pub dropped: u64,
    pub complete: bool,
    pub error: Option<String>,
}
static REPORTS: Mutex<Vec<ReportSummary>> = Mutex::new(Vec::new());
enum Command {
    Start(TraceReader, PathBuf),
    Flush(mpsc::Sender<()>),
}
static WORKER: OnceLock<mpsc::Sender<Command>> = OnceLock::new();
/// Paths and status for the plugin's Logs/Report tab. Never call on audio.
pub fn reports() -> Vec<ReportSummary> {
    REPORTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}
/// Complete pending drains before a non-RT CLI exits.
pub fn flush(timeout: Duration) -> bool {
    let Some(worker) = WORKER.get() else {
        return true;
    };
    let (tx, rx) = mpsc::channel();
    worker.send(Command::Flush(tx)).is_ok() && rx.recv_timeout(timeout).is_ok()
}
pub(crate) fn configure(plan: &mut crate::Prepared) -> Result<(), crate::Error> {
    if std::env::var_os("KONTRA_SIGNAL_TRACE").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return Ok(());
    }
    let Some(root) = std::env::var_os("KONTRA_REPORT_DIR").map(PathBuf::from) else {
        return Ok(());
    };
    if plan.signal_trace.is_none() {
        plan.enable_signal_trace(262144)?;
    }
    if let Some(reader) = plan.signal_trace_reader() {
        start(reader, root);
    }
    Ok(())
}
pub(crate) fn start(reader: TraceReader, root: PathBuf) {
    let worker = WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Command>();
        std::thread::Builder::new()
            .name("signal-trace-report".into())
            .spawn(move || {
                let mut sessions = Vec::<Session>::new();
                loop {
                    let command = rx.recv_timeout(Duration::from_millis(10));
                    let mut ack = None;
                    match command {
                        Ok(Command::Start(reader, root)) => match Session::new(reader, &root) {
                            Ok(session) => sessions.push(session),
                            Err(error) => REPORTS.lock().unwrap().push(ReportSummary {
                                json: root.join("signal-trace.json"),
                                chart: root.join("signal-trace.svg"),
                                records: 0,
                                dropped: 0,
                                complete: true,
                                error: Some(error.to_string()),
                            }),
                        },
                        Ok(Command::Flush(tx)) => ack = Some(tx),
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    sessions.retain_mut(|s| match s.drain(ack.is_some()) {
                        Ok(done) => !done,
                        Err(error) => {
                            let mut reports = REPORTS.lock().unwrap();
                            reports[s.index].error = Some(error.to_string());
                            reports[s.index].complete = true;
                            false
                        }
                    });
                    if let Some(tx) = ack {
                        let _ = tx.send(());
                    }
                }
            })
            .expect("signal trace worker");
        tx
    });
    let _ = worker.send(Command::Start(reader, root));
}
#[derive(Default)]
struct Level {
    input: f64,
    output: f64,
    frames: u64,
    row: Option<TraceRecord>,
}
struct Session {
    reader: TraceReader,
    file: File,
    tail: u64,
    index: usize,
    rows: u64,
    levels: Vec<Level>,
    chart_at: Instant,
}
impl Session {
    fn new(reader: TraceReader, root: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(root)?;
        let mut reports = REPORTS.lock().unwrap();
        let index = reports.len();
        let directory = if index == 0 {
            root.to_owned()
        } else {
            root.join(format!("signal-trace-{}", index + 1))
        };
        std::fs::create_dir_all(&directory)?;
        let json = directory.join("signal-trace.json");
        let chart = directory.join("signal-trace.svg");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&json)?;
        file.write_all(b"{\"schema\":1,\"graph\":")?;
        file.write_all(&serde_json::to_vec(&*reader.graph)?)?;
        file.write_all(b",\"records\":[\n")?;
        let tail = file.stream_position()?;
        file.write_all(b"],\"dropped\":0,\"complete\":false}\n")?;
        let levels = (0..reader.graph.nodes.len())
            .map(|_| Level::default())
            .collect();
        reports.push(ReportSummary {
            json,
            chart,
            records: 0,
            dropped: 0,
            complete: false,
            error: None,
        });
        Ok(Self {
            reader,
            file,
            tail,
            index,
            rows: 0,
            levels,
            chart_at: Instant::now() - Duration::from_secs(2),
        })
    }
    fn drain(&mut self, force: bool) -> io::Result<bool> {
        let rows = self.reader.drain();
        self.file.seek(io::SeekFrom::Start(self.tail))?;
        for row in rows {
            if self.rows > 0 {
                self.file.write_all(b",\n")?;
            }
            self.file.write_all(&serde_json::to_vec(&row)?)?;
            self.rows += 1;
            if row.contribution {
                continue;
            }
            let level = &mut self.levels[row.node];
            level.frames += row.frames as u64;
            level.input +=
                row.input.rms.iter().map(|v| v * v).sum::<f64>() * 0.5 * row.frames as f64;
            level.output +=
                row.output.rms.iter().map(|v| v * v).sum::<f64>() * 0.5 * row.frames as f64;
            level.row = Some(row);
        }
        self.tail = self.file.stream_position()?;
        let complete = self.reader.abandoned();
        write!(
            self.file,
            "],\"dropped\":{},\"complete\":{complete}}}\n",
            self.reader.dropped()
        )?;
        let end = self.file.stream_position()?;
        self.file.set_len(end)?;
        self.file.flush()?;
        let mut reports = REPORTS.lock().unwrap();
        let report = &mut reports[self.index];
        report.records = self.rows;
        report.dropped = self.reader.dropped();
        report.complete = complete;
        if force || complete || self.chart_at.elapsed() >= Duration::from_secs(1) {
            std::fs::write(&report.chart, self.svg())?;
            self.chart_at = Instant::now();
        }
        Ok(complete)
    }
    fn svg(&self) -> String {
        let active: Vec<_> = self
            .reader
            .graph
            .order
            .iter()
            .filter_map(|&id| self.levels[id].row.map(|_| (id, &self.levels[id])))
            .collect();
        let mut body = String::new();
        let mut y = 65;
        for (id, level) in &active {
            let node = &self.reader.graph.nodes[*id];
            let row = level.row.unwrap();
            let db = |p: f64| {
                if p > 0. {
                    format!("{:.2}", 10. * p.log10())
                } else {
                    "−inf".into()
                }
            };
            let input = level.input / level.frames.max(1) as f64;
            let output = level.output / level.frames.max(1) as f64;
            let parents: Vec<_> = self
                .reader
                .graph
                .edges
                .iter()
                .filter(|e| e.to == *id && self.levels[e.from].row.is_some())
                .map(|e| e.from)
                .collect();
            let header = format!(
                "#{id} {} / {} | parents {parents:?} | zone {:?}, group {:?}, bus {:?}",
                node.kind, node.processor, node.zone, node.group, node.bus
            );
            let metrics = format!(
                "{} → {} dBFS; Δ {} dB | {} [{:.6}, {:.6}] | latency {} samples | enabled {} | contributors {}",
                db(input),
                db(output),
                db(if input > 0. { output / input } else { 0. }),
                node.gain_measurement, row.gain[0], row.gain[1],
                row.latency_samples,
                row.enabled,
                row.contributors
            );
            let parameters = node
                .parameters
                .iter()
                .take(16)
                .enumerate()
                .map(|(i, p)| {
                    let gain = if p.name.contains("gain") && row.values[i] > 0. {
                        format!(" ({:+.2} dB)", 20. * row.values[i].log10())
                    } else {
                        String::new()
                    };
                    format!(
                        "{}={:.6} raw={:?}{gain}",
                        p.name, row.values[i], row.normalized[i]
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            for text in [header, metrics, parameters] {
                let mut rest = text.as_str();
                while !rest.is_empty() {
                    let limit = rest.char_indices().nth(165).map_or(rest.len(), |(i, _)| i);
                    let split = if limit < rest.len() {
                        rest[..limit].rfind(' ').filter(|i| *i > 0).unwrap_or(limit)
                    } else { limit };
                    let escaped = rest[..split].replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
                    body.push_str(&format!("<text x=\"24\" y=\"{y}\">{escaped}</text>"));
                    y += 22;
                    rest = rest[split..].trim_start();
                }
            }
            y += 22;
        }
        let height = y + 24;
        let mut svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1400\" height=\"{height}\" viewBox=\"0 0 1400 {height}\"><rect width=\"100%\" height=\"100%\" fill=\"#101722\"/><g fill=\"#e6edf3\" font-family=\"monospace\" font-size=\"13\"><text x=\"24\" y=\"28\">Signal chain — energy across recorded blocks; levels dBFS, deltas dB. Numeric identities only.</text>{body}"
        );
        svg.push_str("</g></svg>");
        svg
    }
}
