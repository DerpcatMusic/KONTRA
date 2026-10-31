# oracle-mcp — a Kontakt oracle an agent can call

A stdio MCP server that drives **real Kontakt 8.14.1 standalone** on a private Xvfb display
and measures what it does: render audio, load an instrument, screenshot the instrument
page, click a control, read a script, read memory. It exists so that parity with KONTRA can
be tested on demand, in one call, instead of through bespoke scripts with pixel constants.

Everything below is measured on this machine, not assumed. The rig (DXVK on lavapipe,
Xvfb `:77`, openbox) comes from `docs/re/kontakt-headless-gui.md` and
`tools/kontakt-oracle/gui-probe.sh` on `spike/headless-gui`; this server builds on that
configuration and does not re-derive it.

```
tools/oracle-mcp/
  server.py      the MCP server (python3, no build step)
  client.py      a small stdio MCP client; runs a JSONL script of tool calls
  ksp_text.lua   the Lua probe oracle_dump_script uses to return KSP text
  smoke.sh       up -> load -> shot -> click -> render -> down, with checks
  parity.py      one parity datum: the same note through Kontakt and through KONTRA
  README.md      this file
```

## Session model

One long-lived Kontakt **per server process**. `oracle_up` launches it (~5 s measured:
`Xvfb` 0.4 s, Kontakt window 4–5 s); every other tool reuses it; `oracle_down` stops only
what that process started. A call never pays the launch cost twice.

Calls are serialised: one GUI, one session, so a long `oracle_render` blocks the next call
rather than running beside it. That is deliberate.

If the server process dies without cleaning up (SIGKILL), it leaves `session.json` with the
PIDs it started; the next server start **reaps those exact PIDs** after checking each
process's own environment still names this display / prefix, and clears a stale X lock whose
PID is dead.

## Running it

```bash
# as an MCP server on stdio (what an agent host does)
python3 tools/oracle-mcp/server.py

# drive it yourself
python3 tools/oracle-mcp/client.py --tools                     # handshake + tool list
python3 tools/oracle-mcp/client.py '{"tool":"oracle_up","arguments":{}}'
python3 tools/oracle-mcp/client.py --script tools/oracle-mcp/steps-example.jsonl
```

`client.py` keeps one server process alive for the whole sequence, which is the only way to
share a session. Each step prints the tool's JSON verbatim; `"expect":"error"` inverts the
pass condition and `"checks"` asserts on returned fields (`{"frames":{"min":48000}}`).

Register it like any other stdio server, e.g. in `opencode.jsonc`:

```jsonc
{ "mcp": { "kontakt-oracle": {
    "type": "local",
    "command": ["python3", "/path/to/repo/tools/oracle-mcp/server.py"],
    "enabled": true } } }
```

## Tools and their exact return shape

All results are JSON in `content[0].text`. A tool that cannot measure something returns
`isError: true` with `{"error": "..."}` — never a plausible default.

### `oracle_up(nki?: str)`
Starts the display, the window manager, the null sink check and Kontakt.

```json
{"state":"up","display":":77","prefix":"/home/dracula/Armory/.oracle-spike/prefix-dxvk",
 "exe":".../Kontakt 8/x64/Kontakt 8.exe",
 "wine":{"path":"/opt/wine-staging/bin/wine","version":"wine-11.16 (Staging)"},
 "config":{"icd":"/usr/share/vulkan/icd.d/lvp_icd.x86_64.json",
           "dll_overrides":"d3d11,d3d10core,dxgi,d3d9=n","wm":"openbox",
           "xvfb_args":"-screen 0 1600x1000x24 +extension GLX +extension RANDR +render -noreset -nolisten tcp",
           "sink":"kontra_ref","midi_port":14,"midi_device":"Midi Through Port-0"},
 "renderer":{"dxvk":"DXVK: v3.0.2-21-g3a4c6fa3cb1548d0",
             "device":"Found device: llvmpipe (LLVM 22.1.8, 256 bits)"},
 "pids":{"xvfb":2455327,"openbox":2455336,"wine":2455342,"kontakt":[2455342]},
 "window":{"window":12582917,"x":3,"y":29,"w":1010,"h":666},
 "launch_s":5.03,"sink":{"sink":"kontra_ref","state":"IDLE","reloaded":false},
 "settings_changed":[],"notes":["prefix present: ...","display :77 free (xdpyinfo: no server answering)"],
 "page":{"client":{"sd":0.169495,"colours":5709,"w":1010,"h":666},
         "keyboard_strip":{"sd":0.023239,"colours":2,"region":[380,620,600,60]},
         "ui_rendered":true,"instrument_page":false,"mode":"browser"},
 "instrument":null,"loaded":null,"shot":".../run-.../up.png",
 "work_dir":"...","log":"/tmp/opencode/oracle-mcp/server.log"}
```

A fresh launch shows the **browser**, not an instrument page: `mode` is `browser` and
`instrument_page` is `false` (measured: keyboard strip `sd=0.023`, 2 colours). `oracle_load`
is what turns that into `mode: instrument` (keyboard strip `sd≈0.32`, ≈200 colours).

### `oracle_down()`
```json
{"state":"down","display":":77","display_gone":true,
 "stopped":[{"name":"wine","pid":2455342,"pgid":2455342,"signal":"SIGTERM"},
            {"name":"openbox","pid":2455336,"pgid":2455336,"signal":"SIGTERM"},
            {"name":"xvfb","pid":2455327,"pgid":2455327,"signal":"SIGTERM"}],
 "kontakt_leftovers":[]}
```
Signals are sent to the **process group of the PID this server started**, so Kontakt (which
inherits that group) goes with it and nothing outside is touched. Calling it when nothing is
up returns `{"state":"down","stopped":[],"note":"nothing was up"}`.

### `oracle_load(nki)`
Ctrl+O, then the Windows path, then wait until the instrument page paints (up to 60 s).

```json
{"nki":"...Electric Mint.nki","win_path":"Z:\\home\\...\\Electric Mint.nki",
 "window":{"window":12582917,"x":3,"y":29,"w":1010,"h":698},
 "page":{"ui_rendered":true,"instrument_page":true,"mode":"instrument"},
 "shot":".../load-4.png","strip_region":[380,620,600,60],
 "renderer":{"dxvk":"DXVK: v3.0.2-...","device":"Found device: llvmpipe (LLVM 22.1.8, 256 bits)"},
 "load_s":4.8}
```

### `oracle_shot()`
Root capture of the private display, cropped to the Kontakt window.

```json
{"path":".../shot-001.png","root":".../shot-001-root.png",
 "window":{"window":12582917,"x":3,"y":29,"w":1010,"h":698},
 "bytes":288633,"sd":0.257729,"colours":56178,
 "keyboard_strip":{"sd":0.316852,"colours":216,"region":[380,620,600,60]},
 "ui_rendered":true,"instrument_page":true,"mode":"instrument",
 "sha256":"fbcbc9c4..."}
```
A blank window is an error, and the raw root capture path is included in the message.

### `oracle_click(x, y, settle=2.5)`
Window-relative coordinates; the server adds the window origin itself.

```json
{"x":722,"y":570,"abs":[725,599],"changed":44627,
 "window":{"window":12582917,"x":3,"y":29,"w":1010,"h":698},
 "settle_s":2.5,"before":".../click-001-before.png","after":".../click-001-after.png"}
```
`changed` is the validated mean-absolute-difference × pixel count between the client area
before and after (idle 4 s measures **0**, so 0 means dead input). `changed == 0` returns
`isError: true`. Coordinates outside the window are refused with
`"... is outside the 1010x698 Kontakt window — nothing was clicked, changed=0"`.

Measured controls at a 1010×698 client (Electric Mint): **Amps & FX** tab `(722, 570)`
→ 44 627 px; the `D1` keyswitch row `(430, 512)` → ≈1 700 px.

### `oracle_render(midi? | notes?, seconds?, tail=0.7)`
Plays the MIDI into Kontakt with `aplaymidi -p 14` (ALSA `Midi Through Port-0`, Kontakt's
`Port A`) and records the monitor of the `kontra_ref` null sink with `pw-record`; Kontakt's
outputs are re-linked to that sink first, and every other link from them is destroyed, so the
user's speakers are never in the path.

```json
{"wav":".../render-001.wav","raw":".../render-001-raw.wav",
 "raw_duration_s":4.458667,"raw_onset_s":0.265354,
 "trim":{"start_frame":10337,"frames":129600,"rate":48000},
 "timing":{"record_spawn_to_stop_s":4.479,"play_spawn_to_stop_s":4.228},
 "midi":".../smoke.mid","midi_duration_s":2.0,"notes":3,
 "requested_seconds":2.0,"truncated":false,
 "sink":{"sink":"kontra_ref","state":"RUNNING","reloaded":false},
 "routed":{"ports":2,"sink_node":132,"new_links":0,"already_linked":[51,210]},
 "midi_port":14,"instrument":"...Electric Mint.nki",
 "latency_note":"the WAV starts 50 ms before the measured onset; raw_onset_s is ...",
 "sample_rate":48000,"channels":2,"frames":129600,"duration_s":2.7,
 "peak":0.286427,"peak_dbfs":-10.87,"rms_dbfs":-25.1,"onset_s":0.05,
 "sha256":"229966b8...","bytes":1036844}
```
`notes` is a list of `{start_s, length_s, key, velocity, channel?}`, written to a MIDI file
at 480 PPQ / 120 bpm (`ORACLE_MCP_BPM`). A silent render (peak 0) is an error that names the
sink, routing, port, MIDI file and instrument.

**What `duration_s` means.** The raw capture is longer than the note (pw-record starts
asynchronously and aplaymidi is slow to begin under wine: `record_spawn_to_stop_s` 4.48 s for
a 1.5 s note, `play_spawn_to_stop_s` 4.23 s). So the returned `wav` is *cut relative to the
measured onset*, not to the wall clock: it starts 50 ms before the first sample above
−60 dBFS and runs `seconds` (or the MIDI's own length) plus `tail`. `raw` is kept uncut, and
`raw_onset_s` is the measured MIDI+sink+Kontakt latency on this machine (0.26–0.36 s).

### `oracle_dump_script(nki, probe="ksp"|"dump", timeout=240)`
Runs a Lua probe inside a **disposable** Kontakt on a second private display (your session
stays up) using dump.py's contract — `ORACLE_INPUTS`/`ORACLE_OUT`, one JSON record per line,
last line `{"done":true}`.

`probe: "ksp"` (default) returns the KSP text per slot:

```json
{"nki":"...","probe":"ksp","probe_file":".../ksp_text.lua",
 "records":[{"input":"Z:\\...","instrument":{"index":0,"name":"Electric Mint",
   "scripts":[{"slot":1,"name":"ElMint","protected":true,
               "source_error":"get_instrument_script_source failed. script is protected"}]}}],
 "seconds":5.79,"run_dir":"...","display":":78",
 "scripts":[{"slot":0,"name":"Strat","protected":false,"source":"\n"}],
 "ksp_chars":2}
```

`probe: "dump"` runs the repo's `tools/kontakt-oracle/dump.lua` (groups, zones, loops,
hashed script sources). An instrument whose scripts are all protected returns an error
saying so rather than pretending.

### `oracle_mem(addr, length=64, pid?)`
Reads `/proc/<kontakt-pid>/mem` (this session's prefix only). `addr` is `0x...`, decimal, or
`"exe"` for the Kontakt module base.

```json
{"pid":2460275,"addr":"0x140000000","length":32,
 "hex":"4d5a90000300000004000000ffff0000b8000000000000004000000000000000",
 "u32":[9460301,3,4,65535,184,0,64,0],"f32":[0.0,0.0,...],
 "mapping":{"range":"140000000-140001000","perms":"r--p",
            "path":".../Kontakt 8/x64/Kontakt 8.exe"}}
```
`4d5a` = `MZ`: the PE header, read from the live process. An unmapped address, a short read
or a PID that is not this session's is an error.

### `oracle_status()`
Session state: `up`, `display`, `window`, `instrument`, `pids`, `window_geometry`,
`work_dir`, `log`, `kontakt_pids_in_prefix`.

## The rig, as measured here

| Item | Value |
|---|---|
| Display | `:77`, else the first free of `:94…:97` — chosen by the server, never by the caller |
| Screen | `1600x1000x24 +extension GLX +extension RANDR +render -noreset -nolisten tcp` |
| WM | openbox (`_NET_SUPPORTING_WM_CHECK` verified before Kontakt launches) |
| Wine | `/opt/wine-staging/bin/wine` = wine-11.16 (Staging) |
| Prefix | `/home/dracula/Armory/.oracle-spike/prefix-dxvk` (reflinked copy; never `Armory/WINE`, where REAPER runs) |
| DLLs | `WINEDLLOVERRIDES=d3d11,d3d10core,dxgi,d3d9=n` (native DXVK) |
| Vulkan | `VK_ICD_FILENAMES=…/lvp_icd.x86_64.json` (lavapipe) |
| Renderer line | `DXVK: v3.0.2-21-g3a4c6fa3cb1548d0`, `Found device: llvmpipe (LLVM 22.1.8, 256 bits)` |
| Kontakt | `Armory/VSTs/Kontakt 8.14.1 Portable/Kontakt 8/x64/Kontakt 8.exe` |
| Audio | PipeWire null sink `kontra_ref` (`node.always-process=true`), recorded with `pw-record --target … -P '{ stream.capture.sink=true }'` |
| MIDI | ALSA sequencer `Midi Through Port-0` (client 14) → Kontakt `Port A` |
| Window | client at `(3,29)`; `1010x666` with the browser, `1010x698` with an instrument page |

Two things in Kontakt's own `UserData/Settings.cfg` make this deterministic, and the server
writes them if they are missing (once, with a `.pre-oracle-mcp` backup):

```
AB2 AudioDevice=sz:WASAPI (Shared Mode) kontra_ref
AB2 MidiDevice0 Name=sz:Midi Through Port-0
AB2 MidiDevice0 UID=sz:WinMidi Input Midi Through Port-0
AB2 MidiDevice0 PortID=dw:0
```

Measured consequence: after the second line exists, no Options-dialog clicking is needed for
MIDI — a launch listens on Port A immediately, and `aplaymidi -p 14` reaches the instrument.

## Safety rules, enforced in code

- **Displays**: only `:77`, `:78` and `:94…:99` (`ALLOWED_DISPLAYS`), and only a display
  this server started (checked with `xdpyinfo`, not by looking at the lock file). `:0` is
  refused by name in `guard_display()`, on every command path. The caller cannot pass a
  display.
- **Screenshots**: `import -window root` of the private display, cropped to the Kontakt
  window. Nothing else is ever captured.
- **Killing**: only the process groups of PIDs this server started, plus each process this
  session's prefix owns (verified through `/proc/<pid>/environ`). No `pkill -f` anywhere —
  that pattern has killed a caller's shell on this machine before.
- **Audio**: the null sink is the only destination; existing links from Kontakt to real
  devices are destroyed and re-made to the sink before a render.

## Verification

`tools/oracle-mcp/smoke.sh` runs, from a clean state and printing every measured value:

1. `oracle_shot` before `oracle_up` → **must fail** (a tool without a session never invents
   a frame);
2. `oracle_up` (checks `page.mode == browser`, ≥1 Kontakt PID);
3. `oracle_load` Electric Mint (checks `page.instrument_page == true`);
4. `oracle_shot` (checks `mode == instrument`);
5. `oracle_click(722, 570)` on the Amps & FX tab (checks `changed ≥ 1000`);
6. `oracle_click(5000, 5000)` → **must fail** with `changed=0`, not success;
7. `oracle_render(midi, seconds=2)` (checks `frames ≥ 48000`, `peak > 1e-4`);
8. `oracle_down` (checks `state == down`).

Exit status is 0 only when every step produced the expected outcome.

`tools/oracle-mcp/parity.py` renders one note through Kontakt (via this server) and through
KONTRA (this repo's `kontra-host --example render`), prints both sha256s and then uses
`tools/oracle/compare.py`'s own alignment/band functions on the pair. Measured 2026-10-30,
Electric Mint, key 60 velocity 100 held 1.5 s, 2 s window:

| | Kontakt | KONTRA |
|---|---|---|
| sha256 | `c3250e52d67adc5996f0dcae9ef7602665d11b3b1ec118a5610db31725f635b5` | `05bf6d2eba950d124406d5f67ef557b97bdc3c127f7bd16fa6e532e4d60b9901` |
| peak | −11.17 dBFS | −15.27 dBFS |

`compare.py`: envelope lag **0.0 ms** (`env_r` 0.979, onsets both 0.000 s), level
**+3.10 dB** (Kontakt louder), timbre residual **7.08 dB**, waveform `r` **0.71** at
−12.75 ms, and the 1/3-octave band list rising from 0.0 dB at the bottom to **+19.5 dB**
around 12–14 kHz and back to +13 dB at the top — i.e. KONTRA is missing high-frequency
content in this instrument's amp/FX chain, which is exactly the kind of difference the
oracle exists to find. (The KONTRA hash is reproducible run to run; the Kontakt hash is not,
because it is a live capture.)

`tools/test_oracle_mcp.py` covers the pure parts (MIDI writer/reader, WAV read/trim/stats,
the change metric, the display guard, the page verdict) with no Kontakt and no display.

## What this cannot do

- **`kontra-inspect play` cannot produce an audio file**, so parity is hashed against
  `crates/app/host/examples/render.rs` instead: `play` reports playback observations, and
  only `play --oracle` writes a WAV — and that path shells out to `capture.py` inside
  `oracle-slot`, which needs `~/.wine-oracle` slot prefixes and a VST3 Kontakt in `~/.wine`.
  Neither exists on this machine (checked), so `--oracle` fails; `parity.py` states this and
  uses the repo's actual WAV renderer with the same note.
- **`tools/kontakt-oracle/dump.py` cannot run as-is**: its default EXE is
  `~/.wine/drive_c/…/Portapotty/Kontakt 8/x64/Kontakt 8.exe` (not present), and it forces
  `WINEDLLOVERRIDES=…=b`, which on this wine install finds DXVK PEs where it expects wine
  builtins (`c0000135`). `oracle_dump_script` keeps its *contract* and runs the same probes
  with the working DXVK launch instead.
- **`tools/kontakt-oracle/memprobe.py` does not read addresses**: it is the load/RSS oracle.
  `oracle_mem` reads `/proc/<pid>/mem` directly (`ptrace_scope=0` here, so a same-user read
  works).
- **Coordinates are fixed**, not resolution-independent: the tab row moves with the window,
  so a click is only meaningful at the geometry `oracle_shot` reports.
- **One session per server, one GUI per session.** Two `oracle_up` calls fail on purpose.
- **RADV is not usable** (deterministic `XIO` crash); lavapipe only, so frames are slow
  (≈35 s for load + shot cycles, 5 s to launch).
- **Audio latency is not calibrated**: `onset_s` is measured from the start of the recording
  (≈0.26–0.36 s here, sink + wine + Kontakt). `compare.py` aligns before comparing.

## Logging

Every call, its arguments, its duration and every failure go to
`/tmp/opencode/oracle-mcp/server.log` (or `ORACLE_MCP_LOG`), one line per event, flushed
immediately — so the log can be read while the server runs and a crash still leaves its
trace. It rotates at 4 MB (`server.log.1`). `ORACLE_MCP_VERBOSE=1` adds debug lines. The path
is announced on stderr at startup and returned by `oracle_status`.

## Environment variables

| Variable | Default | Meaning |
|---|---|---|
| `ORACLE_MCP_WORK` | `/tmp/opencode/oracle-mcp` | artifacts, state file, default log dir |
| `ORACLE_MCP_LOG` | `$WORK/server.log` | log file |
| `ORACLE_MCP_VERBOSE` | unset | extra log lines on stderr |
| `ORACLE_MCP_PREFIX` | `~/Armory/.oracle-spike/prefix-dxvk` | private wine prefix |
| `ORACLE_MCP_PREFIX_SOURCE` | `~/Armory/WINE` | reflink source when the prefix must be created |
| `ORACLE_MCP_WINE` | `/opt/wine-staging/bin/wine` | wine binary |
| `ORACLE_MCP_PORTABLE` | `~/Armory/VSTs/Kontakt 8.14.1 Portable` | portable Kontakt |
| `ORACLE_MCP_SINK` | `kontra_ref` | PipeWire null sink |
| `ORACLE_MCP_BPM` | `120` | tempo for note-list renders |
| `ORACLE_MCP_NKI` | Electric Mint | instrument used by `smoke.sh` |
