# Focused runtime and widget probes

These are bounded inspector observations, not native parity certification or an
installed update. The installed revision remains `5044359961c1`.

The initial helper SHA-256 is
`dec5b0d0eecf69329521ca48b15a9d273a73e55054bb5cc0f2cc7076cc406016`.
It runs real V2 preparation/install, retained script activation, callbacks,
read-only UI effects, CPU asset projection and discarded PCM without a plugin,
DAW or audio device. Exact source hashes and private numeric reports live under
`PluginArtifacts/kontra-diagnostic-fixes/`.

| Selection / action | Observation | Remaining boundary |
| --- | --- | --- |
| Una Corda Felt, tab then reverb menu | Tab expands 7 visible widgets to 41; all 23 requested assets decode. Visible/enabled reverb menu changes 2000 to 1500 through its real callback. Audio continues with no runtime fault, refusal or underrun in this window. | No post-action image was retained by this helper; native pixels and all other controls are unverified. |
| Analog Gates, header FX tab | Visible widgets increase 118 to 135. The requested filter control is still hidden. | Direct hidden-control admission is **not** a reachable-menu pass; the authored per-layer gates need further actions. |
| Punch → Gates → Harmonics → Punch | All four fresh-note windows produce nonzero PCM without stream underruns or voice-capacity refusals in this run. | Offline fast rendering is not a paced streaming or KONTRA CPU benchmark. This is full runtime replacement, not snapshot hot application. |
| Evolved First Steps → Float → Four Winds → First Steps | Repeated First Steps UI and control states match exactly. Four Winds produces PCM. | First Steps remains silent with one active voice; Float selects no voice. Both remain silent in a longer 96,000-frame window at key 60, velocity 64. This does not establish silence for every key. |
| UVI Pizzicato 2 → Bartok → Pizzicato 2 | All three preparations publish; no Lua fault is reported. | Audio verdict **unverified**: KSP-only activation reports zero frames/callbacks, and a 4,800-frame note renders in under 1 ms while Lua runs asynchronously. The probe needs explicit UVI owner readiness and paced worker time. Zero PCM is not evidence that the plugin cannot play these programs. |

Float's `SilentNote` breakdown reports 901 condition rejections and 32 script
group rejections, with no source-unavailable/pitch/velocity rejection. The
condition category includes mute, residency and native start criteria; it must
not be relabelled as controller failure without further evidence.

Separate paced DFD helpers confirm all three Analog selections sound after real
activation, but retain eight Harmonics first-tail underruns in one run and one
in a repeat. Saved constant start modulation and tuning shorten the usable
resident runway. Worker audits also show repeated source-open latency and
off-CPU delay. Those failures remain open; the shorter-head quota fallback is
not a complete streaming solution. See [streaming evidence](../architecture-v2/STREAM_PRELOAD_EVIDENCE.md).

Private report basenames are `una-felt-widget-current.json`,
`analog-gates-widget-current.json`, `analog-aba-current.json`,
`evolved-aba-current.json`, `evolved-long-attack-current.json` and
`uvi-pizzicato-aba-current.json` in `preset-switch-20261015/`. They preserve the
exact executable/plan identity, activation status, callback state, numeric
assets and faults. No commercial scripts, artwork or audio are in this document.

## Later guarded UI and residency evidence

These observations use separate executable cohorts; they do not retroactively
change the initial reports above.

The guarded Analog route passes each visible/enabled gate: FX, Layer FX, filter
tab, filter on, then filter type. The menu changes to semantic value `1` through
the real callback. Six private post-action images were retained with helper
`ea40a4be3933d8f5d89284ac8866870433637299b1e15989a17f1b3fad491a6e`.
Earlier hidden-control attempts remain failed reachability evidence. Pointer,
popup and native DSP parity are separate checks.

Helper `1f5f622bfaa797de3b7a79e7e52cf0674468ab41192ce258435ab2a25d769f7f`
contains the wallpaper viewport repair and opt-in residency settlement. Its
Una tab/reverb recapture restores the complete white Workbench background,
confirmed by visual inspection. The fabric picture remains compressed into
the label's text bounds; intrinsic picture sizing is a separate open defect.
All three captures finish with zero pending assets. This is a fresh diagnostic
asset owner, not a production warm-cache timing measurement.

For both First Steps and Float, a 10-second residency allowance immediately
reports `settled_with_failures`: 1,024 accepted requests, 933 committed, 91 stale,
848 queue refusals, zero pending jobs and zero worker failures. Exactly 32 of
933 groups remain loaded. The first rejected request is group 88, purge.
Fresh 96,000-frame note windows remain silent. This rules out merely waiting
for the currently accepted jobs; it exposes lost startup residency admission.
It does not justify forcing all groups loaded or increasing the queue without
preserving authored ordering and callback semantics.

The newer private reports are `analog-gates-layer-filter-enabled-png-current.json`,
`una-felt-wallpaper-fixed-current.json` and
`evolved-residency-settled-current.json`. Every report retains its own executable
and plan hashes. The installed plugin is unchanged.
