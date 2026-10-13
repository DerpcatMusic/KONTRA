# Omitted Kontakt voice-group overrides

Source baseline: `ce0a2e61`. The load audit found assigned voice groups with
clear bits in the `0x32` override mask. The translator treated these as
undefined and left their runtime voice limit unbound. They are predefined
native groups with default settings, rather than missing authored groups.

## Contract and native evidence

The official [Kontakt manual, Classic view / Voice Groups](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view#voice-groups)
states that every instrument has 128 predefined voice groups. Assignments
share a voice-allocation pool; the maximum, stealing mode, released-voice
preference and fade time belong to the voice group, rather than each sample
group. Unassigned sample groups use the instrument-wide pool.

The existing Ghidra research in [NI_FILE_BINARY_RECORDS.md](NI_FILE_BINARY_RECORDS.md)
records writer `0x140d145f0`, pseudocode lines 4733813–4733818: only nondefault
group records are emitted. The historical export directory in the handoff is
absent on this machine; the committed research remains available. A fresh
read-only radare2 disassembly of the SHA-identical copy independently verifies
the branch and constants:

`/mnt/Windows11/DEV_WORKSPACE/re-binaries/Kontakt8.exe`, SHA-256
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.

| Native address | Omission comparison | Default |
|---|---|---|
| `0x140d14978` | 16-bit mode field | 1, oldest |
| `0x140d14983` | 32-bit voice maximum | 1 |
| `0x140d1498d` | preference byte | nonzero |
| `0x140d14997` | fade field | 10 milliseconds |
| `0x140d149a1` | exclusion field | -1, none |
| `0x140d149ab` | name length | 0 |

When all comparisons match, the branch reaches `0x140d149d8`, skipping the
mask-bit set at `0x140d149c9–0x140d149d6`. The next unrolled group repeats
these comparisons at `0x140d149df–0x140d14a12`. Mask indexing uses byte
`group >> 3` and bit `group & 7`, consistent with the existing 128-bit reader.
No proprietary pseudocode was copied into the implementation.

## Implementation boundary

Keep the raw override mask and its typed `None` entries intact. A clear mask
bit is an absent *override*. Materialize the verified runtime default only
when a sample group assigns that physical voice-group index; reuse it for
subsequent assignments. This preserves one shared polyphony domain and avoids
allocating 128 unused runtime limits. Explicit overrides retain their decoded
values. Zero assignment remains unassigned; an index outside the 128 physical
groups remains an invalid-value diagnostic.

The v1 importer at `0cb7a8a0:src/import.rs` also left absent overrides unbound;
its agreement is not native parity. This fix uses native serialization evidence
and the documented allocation contract. It changes admission of existing
voice-limit behavior, without changing the stealing or fade DSP laws.

The focused regression `omitted_voice_group_override_still_binds_its_native_shared_limit`
uses independently authored wire records. It checks a missing slot 127,
repeated assignments sharing one runtime limit, an explicit slot 64 override,
unassigned and out-of-range assignments, and unchanged source-mask holes.
Its constants are vectors derived from the verified binary comparisons; no
calibrated native audio recording is claimed.
