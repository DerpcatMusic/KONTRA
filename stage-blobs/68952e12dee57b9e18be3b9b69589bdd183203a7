# Kontakt performance UI health

Owner: `v2/gpt-kontakt-ui`. This report covers Kontakt stock KSP, bitmap KSP,
Creator Tools GUI Designer performance views, and Komplete UI. UVI is excluded.
The measured corpus is the supplied `~/.cache/kontakto-corpus/items.tsv`: 781
Kontakt instruments and 53 Kontakt multis. Every program in a multi is surveyed.

## Reproduction and record contract

Build the existing native CPU frontend once:

```sh
/home/derpcat/.cache/kontakto-heavy cargo build --profile ci --example ui_health --features shots
```

Run the binary with `tools/corpus-health/ui-run.sh BINARY OUT.jsonl CACHE_DIR
--shots SHOTS_DIR`. `--only SUBSTRING` selects a library or instrument. The runner
releases its heavy slot after approximately four minutes, caches each completed
instrument atomically, and resumes with the next uncached instrument. A single
instrument finishes before releasing the slot. Cache namespaces include the
binary hash; item/input identities include source and resource metadata. Cached
files contain diagnostic records and hashes; script and resource bytes stay in
memory. Screenshots are rendered performance views, never extracted resources.

`tools/corpus-health/ui-summary.py OUT.jsonl` reports distinct-instrument
percentages and ranks failure classes by affected instruments. An instrument
with several script slots or multi programs counts once per frontend; all its
views must pass for that frontend to pass. A frontend absent from the corpus is
unmeasured, rather than 100% covered.

The `ui` section is emitted by a separate native-render example because the
audio-only corpus-health crate deliberately does not depend on the editor. It
calls the same Kontakt frontend compiler, picture loader, UI view, and CPU
renderer as the plugin. Each page renders in bitmap and vector presentations.
Records include authored/expected frontend, successful slots, visible widget
count, missing pictures/fonts, resource locations, unsupported widget/property
names, layout errors, binding failures, idle value changes, and renderer errors.

“Loads” means the authored frontend was compiled. “Usable” additionally requires
successful renders, bindings for interactive scalar controls, no unsupported
interactive widgets, and no idle mutation of scalar values. “Complete” also
requires resources, geometry, and properties to pass. These are mechanical
checks; they do not claim that every callback's musical behavior matches native
Kontakt. Focused synthetic tests verify callback routing and recall separately.

## Binding and recall repairs

Native Kontakt menu persistence and host control persistence use different
representations. Native saved state uses an item position; KONTRA's control
service carries the selected item value. The loader now accepts semantic host
values separately, restores them before `persistence_changed`, and preserves
the native position conversion. Regression coverage includes menu values 4,
−3, and 0, including values that themselves are valid positions.

Plugin state and KONTRA multis capture scalar control values with exact 128-bit
control identities. Same-source host recall forces fresh script initialization;
pending recall cannot be overwritten by the old running control mirror.

Idle menu drawing no longer selects the first visible item's value. Widget IDs
include the rack part and script slot so identical controls in different parts
do not share interaction state. Auto-sizing fills only an unauthored axis;
authored widths and heights survive. Script-driven scalar changes publish a
control revision to wake the editor, and queued edits carry their load generation.

## Measurement status

The optimized, resumable before/after censuses are in progress. The original
partial baseline covered 40 instruments, including Afflatus and Analog Strings:
39/40 usable (97.5%), 40/40 bitmap frontends loaded, 27/40 complete (67.5%). These
are **partial** results, not whole-corpus coverage. The idle mutation occurred
in Analog Strings. Twelve Afflatus records referenced `articulation_list`;
the read-only resource probe confirms the PNG and TXT companion are absent
from the installed library. Native factory fallback remains unverified.

## Resource evidence and remaining investigation

`Samples/Afflatus_Brass.nkr` uses directory version `0x111` and indexes 89
members. Name lookup is case insensitive and normalizes backslashes. It has
`Resources/pictures/switch_articulation.PNG` and its `.txt` companion, but no
literal `articulation_list` image or TXT companion. Its NICNT indexes 15
branding/database members and neither name. A recursive scan found no loose
`articulation_list` file, including Resources/Data folders. The probe checked
case variants through both `Resources` and `ResourceContainer`. A `.txt` describes frame/stretch layout; it is not image pixels. Factory
resource resolution also requires native reference evidence. No alias to a different picture is assumed.

Existing renderer gaps include array/table, XY, text/file/mouse interaction,
waveform/wavetable display, meter sources, and custom bitmap fonts. Their corpus
impact will be ranked by the completed census. Script compilation and linked
resource failures are reported to their runtime/format owners; this branch does
not broaden KSP execution semantics.

The official [KSP widget manual](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-widgets)
and [UI command manual](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands)
define declaration grid sizes and bitmap font layouts. These are the reference
for subsequent geometry/font checks; fallback drawings are not counted as full
widget support.

UI resource lookup now uses the existing bounded `ResourceContainer` reader for
both NKR and NICNT, preserving loose-file precedence and normalized path/case
lookup. Lookup stops at the corpus Kontakt boundary instead of borrowing a
sibling library’s resources. `locations()` retains paths after opening them. This also serves the
format owner's linked-script port, which calls `Resources::read`.

The official [control parameter documentation](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/control-parameters)
specifies user/factory picture fallback for NKIs without a resource container.
A TXT companion describes layout rather than replacing a missing image. Do not
alias `articulation_list` to `switch_articulation`: their meanings are distinct.
