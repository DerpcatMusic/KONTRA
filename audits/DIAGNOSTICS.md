# Load and runtime diagnostics

Open the selected part's **Info** tab for its load status, stage timings, missing resources and script errors. A load with known issues is **partial**, even if it produces sound. **Loaded** only means the implemented checks found no issue; it does not certify Kontakt parity.

The Info tab shows the actual JSON Lines log path. By default logs live in the OS cache directory under `kontra/logs` (`~/.cache/kontra/logs` on Linux). Set `KONTRA_LOG_DIR` before launching the DAW or CLI to choose another location. Each process has a separate session file. At 8 MiB, it rotates to one `*.previous.jsonl` file. Older sessions remain available; they can be removed manually.

Each load has a unique `load_id`, path, program, rack part, package version, importer source hash, OS and architecture. `stage_started` is written before import, scripts, artwork, samples and effects. If loading stops, the last event identifies where it stopped; a timeout alone does not prove a deadlock. Finished reports include counts of playable/skipped zones, loaded/streamed samples, memory use and issues with their original reasons. RAM fills and script restores have separate reports. Runtime faults include script slot, source line and repeat count; repeated locations update the Info report without flooding disk.

Logging, formatting and file operations run off the audio thread. The audio callback only copies bounded fault records into the existing preallocated snapshot buffers. Fault snapshots are polled once per second when the editor is closed. Logging errors appear in the load report and stderr.

The sample bank retains eight failure examples, alongside the total skipped-zone count. Missing references from the importer are listed individually. Runtime fault locations are capped at 256 and explicitly report overflow. The Info tab shows the first 64 load issues; the journal contains the full recorded report. Logs contain local paths and error text, so review them before sharing. They do not contain sample audio, artwork bytes or script source.

To isolate slow presets, use the existing audit CLI through this standard-library runner:

```sh
python3 tools/audit_presets.py --bin /path/to/kontakto --out artifacts/preset-audit --timeout 120 /path/to/library
python3 tools/audit_presets.py --bin /path/to/kontakto --mode playback --out artifacts/playback-audit /path/to/instrument.nki
```

Each preset runs in its own process. `results.jsonl` records completion, failure or timeout and the last diagnostic event. Its evidence folder retains the CLI report, process output and journals. UI audits check layout and resources; playback audits also load samples and render notes. Neither replaces a paired Kontakt reference or a DAW check. `completed` describes process completion; inspect the report for partial compatibility.
