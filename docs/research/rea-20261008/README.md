# REA continuation: native host parameter boundaries

Date: 2026-10-08. This pass continues the existing Kontakt/Falcon compatibility
research. It does not establish complete engine reconstruction or audio parity.

## Pinned targets

Kontakt's **audio payload** is `Kontakt 8.vst3plugin`, version 8.13.1,
178,689,024 bytes, SHA-256
`8ed90c4b9dd2bb2c5cc45b64dea144f6cca245c706457de4aa72c643a8acc09d`.
It is distinct from the standalone EXE and the small portable VST3 loader.
Their addresses and decompilation coverage cannot be transferred to this image.

The installed UVI Workstation VST3 is version 4.0.9, 55,250,272 bytes,
SHA-256 `5215dd15ac95038483378925bc55380f48f991aed597ad6c20a0debfe64018b5`.
Workstation evidence does not establish Falcon's implementation.

A filename scan of the Wine prefix, mounted Windows/storage volumes, `/opt`,
system/user VST directories, and the Codex cache found no Falcon executable or
plugin. This is a scoped absence observation, not proof that Falcon exists
nowhere. The actual Falcon binary remains required for native analysis.

## Newly executed checks

Run `python3 docs/research/rea-20261008/verify_host_parameters.py`.
The existing installed `pefile` and `unicorn` packages are sufficient.
The script verifies both whole-image hashes and executes unchanged original
instructions. It does not launch Wine, initialize a plugin, or write a prefix.

All **124 cases pass**, 62 per image. The verified entries are:

- Kontakt `getParameterCount`: `0x180d6acf0`.
- Kontakt `getParameterInfo`: `0x180d6ad10`, using original index helper
  `0x180d6fdb0`.
- Workstation `getParameterCount`: `0x1809397e0`.
- Workstation `getParameterInfo`: `0x180939800`, with its index checks inlined.

Both count methods read the parameter container at controller-interface
pointer `+0x20`. A null container yields zero. Otherwise they compute
`(end - begin) / 8` from the container's pointer vector. Authored vector sizes
0, 1, 2047, 2048, 2049 and 2050 are returned unchanged. These getters do not
impose a fixed 2048-slot capacity.

Both information methods reject negative indices, indices greater than or
equal to the current vector size, null containers and null parameter entries
with return value 1. A valid entry returns 0 and copies 792 bytes of parameter
information from its virtual accessor. Tests verify every copied byte and the
unchanged 32-byte guards on either side. Invalid-index cases leave the output
untouched.

The accessor is an explicitly **authored virtual method** returning a patterned
record. Count calculations, index checks and copy routines are original code;
their helpers are not substituted. A record whose first u32 is `0xf1234567`
survives the copy unchanged. Enumeration index and returned parameter ID are
separate values. This does not prove the set of IDs created by either vendor.

Actual initialized counts, registration order, vendor parameter units/ranges,
ID-to-BAO mapping and setter semantics remain unverified by these tests.

## Standalone automation evidence rechecked

The existing W7 check in
`/home/derpcat/.t3/worktrees/KONTAKTO/re-w7-vectors-20261008/docs/research/w7-native-20261008/verify.py`
passes again: 32 pinned PE ranges, 12 native-reader vectors with stated I/O
substitutions, and 16 original-code address-bound vectors.

For the standalone EXE hash
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`:

- BAO v0x71 reader `0x140cfec70` stores the source u16 unchanged, including
  65535. Mode 1 means MIDI controller; mode 2 means host parameter.
- Creation gate `0x1408f3479..0x1408f348a` rejects unsigned addresses at least
  `0x801` (2049), permitting 0..2048 at this gate.
- Program/group remap `0x1409ab730` clamps shifted eligible mode-2 addresses
  to `[0, global_limit - 1]`. The PE initializer at `0x144fd4c80` is 2049.

These are three different boundaries: wire preservation, native creation and
runtime remapping. They do not establish a public VST3 parameter-ID maximum,
nor justify clamping all serialized u16 fields to 2047. The significance of
address 2048 and runtime changes to the global limit remain open.

## REA session and retained gaps

REA MCP 3.2.1 opened and pinned the Kontakt payload with Ghidra 12.1.4.
The first `analyze_function` request for `0x180d6ad10` timed out at the MCP
300-second boundary during whole-image analysis. The subsequent request for
`0x180d6acf0` returned a provider execution failure. Neither request produced
an authenticated function dossier. Root artifact inspection succeeded; its
evidence proves the file identity and inventory, not native semantics.

The local evidence bundle is
`artifacts/rea-2026-10-08/kontakt-evidence.json`; the provider-neutral session
snapshot is `kontakt-session.json` in that directory. Its export reported zero
snapshot entries. Exact errors and CLI diagnostics are retained there too.
With the live server's Ghidra/Java paths supplied, the diagnostic recognizes
Ghidra 12.1.4 and Java 21 as available but flags CachyOS as an unsupported host.
This diagnostic does not prove the cause of the analysis failure.

REA records the unresolved host-registration question as
`unk_380ee303868e9f7bd8d61a3cd1600d6f1b9e5f293379ac6226a26b022fef0371`,
linked to the pinned payload's inventory evidence. The UVI Workstation
`analyze_function` request for `0x180939800` also hit the 300-second MCP limit
during whole-image analysis. Its identity/inventory evidence and session
snapshot are retained as `uvi-evidence.json` and `uvi-session.json` in the
same artifact directory. The byte-level tests above are a separate evidence
source; they are not REA Ghidra dossiers.

The smaller portable VST3 loader **did complete REA native analysis**. It is
2,484,224 bytes, SHA-256
`799120e1611318b60d2ef267cb21ad2df2753012c262c3ca1fc734ba25d40b46`.
Four authenticated dossiers are retained in
`artifacts/rea-2026-10-08/portable-loader-evidence.json`, alongside the
overview and a session snapshot. The compact tracked index is
[rea-loader-summary.json](rea-loader-summary.json).

- `GetPluginFactory`, `0x1800f3820`, evidence
  `ev_2786cf220a25c40b69da388f7f87d45d0f66db82f63ecaa9421bc81831609e91`.
- `InitDll`, `0x1800f9c90`, evidence
  `ev_d2cce1dc66d4dbea031338f76654f34178a3fc44d71c810d18decf35cdced5ef`.
- `ExitDll`, `0x1800f9c70`, evidence
  `ev_b4e5d8e07816d4d7df1abf7085fd5bb349e0e0e395e6844735c90078997e7216`.
- Native loader `0x1800f2400`, evidence
  `ev_4c7ec0ef5e6f63c28fba075c291e4afa6fcc54e7e2680782514dbbfecab155cc`.

The loader's static code resolves the installation path, loads the auxiliary
`ktp8131.dll` and payload, obtains the payload's `GetPluginFactory`, retains a
creation callback through a private factory layout, and resolves its module
lifecycle exports. The wrapper factory has callback-present/absent
registration branches and a compatibility class. These confirm the separate
loader/payload boundary; the small wrapper is not the DSP engine.

The two lifecycle exports maintain a shared initialization counter. Init
increments it and calls its initialization helpers when it becomes one;
Exit decrements it and calls its teardown helpers when it becomes zero.
Helper execution and actual module loading were not tested here.

REA's completed overview observes 11,116 procedure records and 2,260 defined
strings for this wrapper. Provider-discovered records are not unique original
function counts or proof of complete decompilation. The factory's value-flow
record is truncated; pseudocode also contains warnings. No
exhaustive callgraph or runtime reconstruction claim follows from these four
dossiers. All three REA sessions were saved and closed.

## Existing broad coverage and remaining reconstruction

The previous [system inventory](../../DSP_SYSTEM_INVENTORY.md) and
[format/DSP specification](../../DSP_FORMAT_SPECIFICATION.md) retain the
module catalogs, type/vtable maps, complete candidate ledgers and isolated
machine-code results. The previous Kontakt EXE/portable-loader pass has
264,775 primary plus validated supplemental pseudocode outputs; 468 primary
failures and four unrecovered EXE candidates remain. These totals exclude a
complete decompilation of the separate audio payload used in this pass.

The earlier UVI Workstation standalone study has 88,170 distinct pseudocode
outputs and 83 remaining initial failures. The Falcon documentation catalog
has 167 module types and 2,877 parameter records. Documentation counts are not
counts of recovered Falcon native implementations.

Complete reconstruction still requires version-pinned serialized-state to
parameter mappings, processing kernels and numeric behavior, voice/channel
ownership, streaming/resource lookup, script/event scheduling, modulation
cadence, reset/restore lifecycle, tails/latency and full native render
comparisons. Pseudocode output and isolated passing checks close only their
stated boundaries. Falcon's native path additionally depends on locating its
actual binary.
