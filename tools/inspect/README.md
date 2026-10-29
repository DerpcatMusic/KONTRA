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
