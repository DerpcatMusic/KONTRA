use mlua::Function;
use sampler_parity::Coverage;
use std::collections::BTreeMap;

struct Script {
    chunk: String,
    lines: Vec<u32>,
    functions: Vec<Function>,
    faults: Vec<(u32, String)>,
}
#[derive(Default)]
pub(crate) struct Inspection {
    scripts: BTreeMap<String, Script>,
}
impl Inspection {
    pub(crate) fn register(&mut self, id: String, chunk: String, source: &str, function: Function) {
        let script = self.scripts.entry(id).or_insert_with(|| Script {
            chunk,
            lines: std::iter::once(0)
                .chain(
                    source
                        .bytes()
                        .enumerate()
                        .filter(|(_, b)| *b == b'\n')
                        .map(|(i, _)| (i + 1) as u32),
                )
                .collect(),
            functions: Vec::new(),
            faults: Vec::new(),
        });
        script.functions.push(function);
    }
    pub(crate) fn fault(&mut self, error: &str) {
        // Tracebacks can mention several scripts. Attribute the fault to the
        // innermost authored location, rather than every caller in the trace.
        let location = self
            .scripts
            .iter()
            .filter_map(|(id, script)| {
                let quoted = format!("[string \"{}\"]:", script.chunk);
                let plain = format!("{}:", script.chunk);
                let (at, prefix) = error
                    .find(&quoted)
                    .map(|at| (at, quoted.len()))
                    .or_else(|| error.find(&plain).map(|at| (at, plain.len())))?;
                let rest = &error[at + prefix..];
                let line = rest.split(':').next()?.parse::<usize>().ok()?;
                Some((at, id.clone(), *script.lines.get(line.checked_sub(1)?)?))
            })
            .min_by_key(|(at, _, _)| *at);
        if let Some((_, id, offset)) = location {
            let faults = &mut self.scripts.get_mut(&id).unwrap().faults;
            if faults.len() < 256 {
                faults.push((offset, error.chars().take(1024).collect()));
            }
        }
    }
    pub(crate) fn snapshots(&self) -> Vec<Coverage> {
        self.scripts
            .iter()
            .map(|(id, script)| {
                let mut hits = BTreeMap::<u32, u64>::new();
                for function in &script.functions {
                    function.coverage(|info| {
                        for (line, count) in info.hits.into_iter().enumerate() {
                            if count > 0
                                && let Some(offset) = script.lines.get(line.saturating_sub(1))
                            {
                                let hit = hits.entry(*offset).or_default();
                                *hit = hit.saturating_add(count as u64);
                            }
                        }
                    });
                }
                Coverage {
                    script_id: id.clone(),
                    hits: hits.into_iter().collect(),
                    faults: script.faults.clone(),
                }
            })
            .collect()
    }
}
