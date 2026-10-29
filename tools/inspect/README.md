# kontra-inspect

`kontra-inspect load <instrument> --json <report.json>` reads and prepares a
Kontakt instrument or a UVI program (`bank.ufs/path/program.uvip`, or the bank
for its first program) using the v3 host. It prints a concise summary and writes
a versioned JSON inventory when `--json` is supplied. Exit 2 means a usage,
read, store or output error.

The report contains the translated samples, groups, zones, modulation routes,
shapers, effects and scripts, the prepared-zone count and per-zone `prepared`
flag, initialized UI models, and translator/prepare diagnostics with impact.
A zone not prepared is not necessarily an error: for example, a saved purged
group can defer sample loading. See diagnostics and group `purged` state.
Effect and some model details use Rust debug text; they are descriptive, not a
stable machine-readable schema. The versioned envelope reserves `null` for
unavailable data. Translator law IDs and provenance, script callback inventory,
original-to-runtime script slot mapping, and asset existence are not yet exposed by this path and are explicitly listed
as unavailable. Script inventory slots are original file slots; UI slots are runtime slots, which can differ after a compile failure. Slot laws and route curves that are already in the IR are shown.

`kontra-inspect` does not run the existing v2 `kontra-probe` contracts.

`kontra-inspect play <instrument> --scenario user --json <report.json>` runs
legato, chords and repeats with slow and fast CC1/CC11 sweeps, then 20 seconds
past the last note-off. A file scenario is `{"events":[{"frame":0,
"type":"note_on","key":60},{"frame":28800,"type":"note_off","key":60}]}`;
`cc` events use `cc` and `value` fields. Frames are at 48 kHz and note-on
velocity is 100. Reports include inputs, block-boundary EQ route/voice snapshots,
peak, clipped samples, maximum adjacent sample jump and the stereo second-
difference energy in a ±5 ms window around each CC event. This energy is a
measurement, not by itself a click verdict. The per-voice peak is the last
voice render span before group effects; it is not the level of the final mix.
Unavailable stages are listed in JSON. The inspector enables a compile-time
voice trace feature; the normal feature-off voice renderer has no trace state
or per-frame instrumentation.
