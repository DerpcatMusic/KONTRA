# Offline library inspector

The inspector traces local library source to Kontra's static script bindings,
converted instrument structure and optional initialized UI model. It does not
open the plugin or audition audio. Use it before a runtime or native comparison
to identify where information is missing and which implementation owns it.

Native Kontakt and Falcon/Workstation behavior and their official scripting
contracts are the reference. Agreement between Kontra v1 and v2 is useful
regression evidence and can still preserve the same mistake. The
[Kontakt native contract audit](audit-2026-10-12/kontakt-native-contract.md) and
[Falcon native contract audit](audit-2026-10-12/falcon-native-contract.md) distinguish
documented rules, local findings and the native captures still needed.

## Start the workbench

Build through the repository's heavy-job wrapper:

```sh
/home/derpcat/.cache/kontakto-heavy cargo build --locked --profile ci -p corpus-health
python3 tools/library-inspector.py '/path/to/Piano.nki' --source
```

For a program inside a UFS bank:

```sh
python3 tools/library-inspector.py '/path/to/Bank.ufs::Presets/Piano.uvip' --source
```

Add `--initialize` to inspect actual product conversion and the initialized UI
model. For an NKM, `--program 2` selects the third embedded program for conversion;
the source inspection still includes all physical programs and script slots.
The default selected program is zero. Multiple input paths open together in the
workbench. `--binary` selects another corpus-health build; the default locates this
worktree's CI binary on the configured cargo-target volume.

The Problems view lists recorded unsupported features, stubs, approximations,
faults and conversion gaps. Selecting a script problem opens its coordinates
alongside the frontend's lowering records and implementation/spec references.
Original code appears when `--source` was requested or the opened report includes it.
Converted structure exposes groups, zones, processors, modulators, routes,
controls and physical ownership. UI projection exposes model widget coordinates,
bindings and omissions; it does not substitute a generated skin for native pixels.

## Evidence boundaries

| Path | What is inspected | What remains unverified |
| --- | --- | --- |
| Kontakt default | Decoded chunk structure, every physical script slot, frontend reports and NCKP resource/field gaps | Product initialization, IR admission, sample binding, audio and live callbacks |
| Falcon default | Program XML hierarchy, script bindings, actual compiler-only Luau syntax checks and production XML-to-IR translation | Native Lua 5.1 semantics, script initialization, sample binding, audio and live callbacks |
| Optional initialization | Product initializer and UI model; Falcon IR includes `onLoad` parameter overlays through the shared scripted-loader path; Kontakt also attempts binding against zero-payload assets with deliberately fictitious sample metadata | Real sample admission, musical callbacks, rendered pixels, gestures and native equivalence |

An available binding is not evidence that it executed. A successful stage means
that particular inspection completed, not that the whole library is compatible.
Unclassified fields remain unverified. Summary reports omit detailed coordinates
and lowering records; open a full per-file report for line inspection. Partial
program conversion does not establish complete NKM compatibility.

Kontakt UI projection reuses the translated initializer once. Positioned KSP
faults retain physical program/script-slot ownership and fail initialization;
compiler rejection fails the frontend stage. A rejected KONTRA frontend does not
establish that the native script is malformed. Omitted NCKP fields and controls
make UI projection partial. Default frontends retain categorized resource/field
gaps; optional source inspection adds categorized skipped counts in resource
metadata, the original NCKP JSON and its path.

Kontakt product conversion can read sample headers and decode convolution impulse
resources. It does not audition samples or produce PCM output. The source-only
path avoids that initializer entirely. Runtime and audio evidence are always
separate, and native equivalence remains unknown in these reports.

Initialized Falcon inspection promotes the decoded source, initializes one host,
applies its parameter overlays and translates that resulting XML through the same
helper as the scripted bank loaders. The returned IR therefore reflects `onLoad`
changes; samples are not bound or decoded. Live/asynchronous changes require
separate runtime evidence.

## Source and file handling

The launcher captures reports in memory and serves them only on loopback, with
an unguessable URL and `no-store` responses. It has no analytics or uploads.
`--source` explicitly includes original code/XML; source is absent by default.
Clear memory removes loaded reports from the view; closing the launcher releases
its report data and removes its owned RAM workspace.

On this Linux environment, initialization requires bubblewrap. The launcher mounts
the filesystem read-only, permitting writes only in its owned `/dev/shm` directory.
Authored external writes consequently fail and remain part of the inspection;
library files are preserved. Scan mode also bypasses Kontakt product-cache lookup
and storage. Direct CLI initialization requires the launcher's read-only inspection
marker and scan mode. Use the launcher to establish the actual sandbox.

## Scriptable reports

The `corpus-health` binary also exposes:

```text
corpus-health inspect INPUT [--program INDEX|BANK_MEMBER]
    [--initialize] [--ephemeral-source] [--summary]
corpus-health inspect-list MANIFEST
```

The manifest is the existing `corpus-health list` format: `kind<TAB>id`, with
bank program IDs written as `BANK.ufs::MEMBER.uvip`. `inspect-list` emits compact,
source-free JSONL and reuses open banks within that process. Its source inspection
does not run initialization. Invalid manifests and failed stages return a nonzero
exit status while valid records remain individually inspectable.

Full reports use `format: kontra-library-inspector`, `schema: 1`. They contain
separate `stages`, `scripts`, `converted`, `ui`, `diagnostics` and `evidence`
sections. Script reports reuse `sampler-parity` coordinates, construct status,
owners and lowering records. A source-free report can be opened through the
workbench's Open reports action or `--report-json FILE`.

## Validation

Authored backend fixtures cover physical empty/bypassed multi slots, source
opt-in, malformed typed input, initialization refusal, compact evidence and
bank-member identities. Five launcher tests cover read-only mounts, unsafe
initialization refusal, report schema, memory-only serving and manifest IDs.
The shared browser exercised problem navigation, lowering, source escaping,
conversion records, UI omissions and clearing. Final desktop and narrow previews
had no page overflow or console errors.

The related [Kontakt audit](audit-2026-10-12/kontakt-parity.md) records exact v1
NCKP fields that v2 omits, with source references and retained diagnostics.
