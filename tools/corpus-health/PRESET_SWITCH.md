# Scripted preset-switch diagnostic

`corpus-health preset-switch PLAN.json` executes sequential real `V2Loader`
preparations, full `Core::install` replacements, deferred activation callbacks,
the shared read-only KSP UI effect drain and fresh host notes. This is an explicit
runtime audit, distinct from static IR inspection. It uses an existing helper
with `ui-render`; it launches no plugin, DAW, window or audio device.

Run the bounded launcher with the exact existing executable selected for the
diagnosis. All output/scratch/cache paths should be private diagnostic paths:

```sh
export TMPDIR=/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts/kontra-diagnostic-fixes/test-scratch
export XDG_CACHE_HOME=/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts/kontra-diagnostic-fixes/test-cache
export KONTRA_DISABLE_NETWORK=1
python3 tools/corpus-health/preset_switch_probe.py PRIVATE_PLAN.json \
  --binary EXISTING_CORPUS_HEALTH --output PRIVATE_NEW_EVIDENCE.json --timeout 180
```

The launcher never builds or installs anything. A helper predating this command
produces `probe_report: unavailable`, with its exact binary hash, rather than
static evidence being reported as a switch run. The output must be a new file;
existing evidence is never overwritten. Raw stderr, artwork and PCM are omitted.
Reports contain PCM fingerprints and numeric signal measurements, not audio.

A small plan can exercise A–B–A and the old-owner failure cases:

```json
{
  "schema": 1,
  "render_ui": false,
  "key": 60,
  "velocity": 64,
  "note_frames": 4800,
  "steps": [
    {"op": "load", "path": "/owned/A.nksn", "base": "/owned/Base.nki"},
    {"op": "load", "path": "/owned/B.nksn", "base": "/owned/Base.nki"},
    {"op": "load", "path": "/owned/A.nksn", "base": "/owned/Base.nki"},
    {"op": "load", "path": "/owned/absent.nki"},
    {"op": "load", "path": "/owned/B.nksn", "base": "/owned/Base.nki", "cancel_at_progress": 750},
    {"op": "prepare", "path": "/owned/B.nksn", "base": "/owned/Base.nki"},
    {"op": "load", "path": "/owned/A.nksn", "base": "/owned/Base.nki"},
    {"op": "publish", "request": 6}
  ]
}
```

Each `load`/`prepare` increments the request number, including failed or canceled
ones. `load` publishes a successful uncanceled preparation; `prepare` retains it
without publication. `publish` completes a retained preparation. At most two
preparations may be staged. A completion older than the latest requested load is
discarded by this **diagnostic host's** gate. This scripted ordering is not a
test of concurrent plugin workers or the plugin's publication gate.

`cancel_at_progress` sets the real loader's atomic cancellation predicate when
its progress reaches 0..1000 (zero starts canceled). Progress is relayed from a
worker; the worker may complete before the caller sets the predicate. The report
distinguishes an actual loader cancellation from `canceled_before_publish` and
does not infer that work before the cancellation checkpoint was avoided.

`{"op":"ui_edit","generation":1,"control":"0x0000000000000000000000000000007b","value":30}` submits through
the retained production `ControlIngress` belonging to that generation, then
steps only the current owner. Choose an actual scalar control ID from the
initialized inspector; convert its decimal binding ID to an exact 32-hex-digit
string with the `0x` prefix. This preserves all 128 bits without JSON number
truncation. The report records admission, whether that owner is stale
and the active control's before/after value. A missing ingress/control is reported;
it is not evidence of successful isolation. Synthetic tests cover queued and
late old edits with the same menu identity, plus current-owner edits and a
replacement with a different structure. UVI Lua setter ingress is not exercised
by this KSP queue test.

To exercise an authored tab/menu callback, use one bounded typed widget action:

```json
{"op":"widget_edit","generation":1,"slot":0,"source_id":123,"index":0,
 "require_visible_enabled":true,"value":{"type":"integer","value":1}}
```

The source ID must resolve to exactly one widget in that KSP slot, and the value
type is explicitly `integer` (signed 32-bit) or finite `real`. The normal
`submit_ui_widgets` / `InvokeWidget` path owns validation and callback dispatch.
The report retains target count, admission, current/stale generation, widget
flags, visible-widget counts and bounded callback-settled state. Ambiguous or
absent targets are not replaced with another control. No text, file selection,
drop, script execution or source-file write operation is accepted. With
`render_ui: true`, the resulting panel is also projected through the CPU painter.

Set `require_visible_enabled: true` for user-reachable tab/menu flows. Before
submission, the exact target must be on a declared page, pass the interface's
effective visibility check (including hidden ancestors), and be enabled.
`target_effective_visible`, `target_enabled` and `blocked_reason` report this
decision. A blocked action is never queued, and no alternative target is used.
The optional field defaults to `false`, preserving deliberate direct/stale-owner
audit semantics. This precondition does not establish pointer hit testing,
occlusion or native-host visibility.

Every publication reports the resolved resource owner, actual audio generation,
menu item values/visibility and caption hashes, complete UI/control fingerprints,
control identity/order comparison and exact repeated-preset state comparisons.
The identities are observations, never a runtime/resource reuse key. Authored
randomness and listener timing can legitimately produce different repeated state.
The source hashes identify the compiled witness and core source, while the build
identity and launcher identify the actual executable. Working-tree changes are
not evidence that a frozen executable contains those changes.

The existing `KONTRA_AUDIT_LOAD` spans are grouped by explicit step/phase markers.
Counts distinguish container reads/decodes, base object parses, script init,
sample header/preload batches and head-worker jobs. They count observed span
executions, including resolver/identity/cache work; they do not infer individual
file-open counts. Span durations nest and must not be summed as exclusive time.
Malformed, unassigned or omitted records make exact counts unknown. Worker
sample identifiers/probes and authored log text are discarded.

Set `render_ui: true` only for a scheduled focused UI run. It executes the
existing production layout, asset worker and CPU painter and retains only
numeric job outcomes/timings. The painter creates a fresh asset owner per
interface; this measures that diagnostic path, not plugin asset-cache reuse.
At most 16 pages per interface and both existing presentations are observed;
renderer preparation bounds remain in force. Decode jobs, failures and pending
shots stay distinct. No encoded images enter the switch report.

For private visual inspection, the helper also accepts an explicit artifact sink:

```sh
EXISTING_CORPUS_HEALTH preset-switch PRIVATE_PLAN.json \
  --ui-artifacts /mnt/Windows11/DEV_WORKSPACE/PluginArtifacts/PRIVATE_NEW_DIRECTORY \
  --ui-slot 2 --ui-page 0
```

The plan must set `render_ui: true`. All three capture options are required
together. The absolute output directory must be new, with an existing parent;
existing directories and files are never overwritten. The bounded Python
launcher above does not currently forward these options; use the selected helper
directly or an explicitly bounded private runner. With no options, the existing
probe API and its lack of artifact writes are unchanged.

Capture selects exactly one authored **KSP script slot**, an explicit page and
the Bitmap presentation. A slot is not an interface-array index. It saves the
PNG already rendered after each publication and each `widget_edit` callback
drain, without another script initialization. Filenames contain only numeric
identity, for example
`step-002-generation-000001-slot-02-page-000-bitmap.png`. At most 24 image-write
attempts and 64 MiB of admitted PNG data are allowed. A partial or refused write
still consumes its reserved budget. Missing/ambiguous slots, unavailable shots,
malformed pixel data and exhausted budgets have explicit capture statuses.

KSP capture keeps its explicit physical `--ui-slot`. For a UVI face, use
`--ui-source uvi --ui-page 0` with the new private `--ui-artifacts` directory
and omit `--ui-slot`; Creator Tools uses `--ui-source performance-view`.
A missing or ambiguous source is refused, never replaced by a different face.

Artwork is written only to the requested private directory. JSON retains
capture status, dimensions, byte counts and SHA-256, without pixels, captions or
artwork paths. A partial render can be captured, while its render status and
pending-asset count remain distinct from a successful file write. This is the
current post-action interface/control projection through the shared CPU painter;
it does not capture an open popup, pointer state, a native Kontakt host or the
plugin's retained asset-cache lifecycle.

Full runtime replacement can cut an already-held note. The report therefore
separates its 512-frame post-publish transition from the new note played **after**
bounded activation, including nonzero frames/blocks, peak, RMS, runtime faults
and signal hash. An unfinished activation or a silent fresh note remains visible.
Controllers stay at loader defaults, and the chosen key/velocity can be unmapped
or silent by design. KSP activation uses the existing 96,000-frame/five-second
bound; musical windows use their own five-second threshold between blocks.
Native comparison and all owned libraries remain unverified. Full `load` actions
still replace the owner; the explicit `snapshot` subset below separately tests
retained-owner application. No overall universal compatibility verdict is emitted.

`residency_at_note` records the generation-checked group eligibility and pending
purge/reload work at the actual note boundary, including retained queue
rejections and failed/stale completions. KSP callback completion alone does not
mean these worker jobs finished. To test a fresh note after loading settles,
explicitly set `"residency_wait_ms": 5000` at plan level (valid range 1..10000).
The diagnostic admits real completions through zero-frame renders and services
finite callbacks within separate bounds. It never forces purged groups loaded.
Timeout, admission refusal and failure remain in `residency_settlement`; a
previously rejected attack is not replayed when loading finishes. Use an
independent load for the comparison rather than treating a longer held note as
a retry. Group rows are capped at 4096, with an explicit omitted count.

The physical residency queue and retained startup-completion spool are distinct.
`startup_completions_pending` and `pending_startup_work` must also clear before
settlement, including unadmitted startup programs. `stream_after_activation`,
`stream_at_note` and `stream_after_note` snapshot current physical heads and
selected hint coverage; the original load report is historical and can differ
after authored purge/reload. These metadata queries do not decode samples.

UVI runs on an independent asynchronous Lua owner. The KSP-only activation pass
therefore reports `uvi_owner_not_observed`, even if its callback list is empty.
An empty fast-rendered PCM window without owner acknowledgement and sufficient
worker time is not an instrument-silence verdict. UVI acknowledgement, command
consumption, pending sample work and later authored wait/listener work must be
observed separately. Controller values are never silently widened by this probe.

For an explicit paced UVI note condition, add this plan-level object:

```json
"uvi": {"phase_timeout_ms": 1000, "total_timeout_ms": 10000,
        "pace_ms": 2, "controllers": []}
```

The config is validated before loading and is refused for a non-UVI owner.
It installs monitoring before inputs, requires a real unseeded owner
acknowledgement and ordered core command drain, then separately observes
sample transactions and bounded paced PCM. It owns the single fresh NoteOn;
the ordinary fast note is not also sent. Explicit controller entries, when
supplied, must be acknowledged before the note. The empty list changes no
controller defaults. Reports distinguish the script note route from direct
sampler forwarding without `onNote`.

Acknowledgement does not complete future waits/listeners or certify native
parity. Per-phase traces also retain a bounded first observed command-drain
increase, active voices and owner phase. This is an off-audio polling bound,
not an exact command timestamp or native/host onset measurement.
Timeouts and unresolved/failed sample work retain partial results;
all-zero PCM alone does not establish incompatibility. The PCM fingerprint is
a labelled indexed chain of per-block digests, not concatenated audio. See the
[UVI probe contract](../../docs/re/uvi-inspector-note-probe.md) for exact bounds,
sample conservation and input/owner evidence.

Plans are limited to 1 MiB, 24 actions and 128..96,000 fresh-note frames per
publication. The launcher caps each pipe at 16 MiB, numeric audit records at
4096 and total process time at its explicit timeout. These bounds describe the
probe; they do not establish real-time host performance.


Loader spans now distinguish elapsed wall time from caller-thread CPU when
supported. `thread_cpu_calls` records actual clock coverage; missing/moved-thread
or legacy clocks remain unknown. Nested spans overlap and cannot be summed into
a total. A wall/CPU gap includes scheduling and waiting, without proving I/O.
`KONTRA_AUDIT_BEGIN=1` optionally adds numeric span IDs before work; the launcher
enables it, bounds begins and completions separately to 4096 each, and retains
up to 16 unfinished code-owned stages on a bounded or
failed process. Matching completion records stay assigned to the phase where
they began, even if another thread emits them later. Legacy end-only helpers
remain supported. Truncated/unfinished coverage keeps work counts unknown.

### Explicit same-owner snapshot action

After a `load`, `{"op":"snapshot","path":"/absolute/Preset.nksn","release_held":true}`
exercises the product's prepared saved-state mapper and queued native transaction
without `Core::install`. It reports refusal instead of silently reloading.
`release_held` releases only the probe's held host note and renders 4096 frames;
callbacks, longer releases or other native events can still refuse admission.
The current subset requires an unchanged complete setup/resource authority,
no incoming native group/effect payload and no initialization. Persisted menus
remain canonical-loader cases until their live position/value roster is fenced.
See `docs/architecture-v2/SNAPSHOT_REUSE.md` for exact scope and bounds.
