# Observed UFS2 archive layout

`tools/inspect_uvi.py --scan --reader <official-UVI-Workstation-PE> <bank.ufs>`
walks the UFS2 layout observed in UVI Workstation 4.0.9 and 25 installed VWinds
banks. It decodes record names using the supplied reader binary; the reader's
format namespace and derived keys are kept in memory and are never printed or
written to the repository. The namespace is located by testing printable
36-byte reader strings against the encrypted root name `Root`.

The 328-byte header overlaps the first record length at offset 320. The observed
record stream is a sequence of little-endian `u64 length` followed by that many
payload bytes. Header `u64@32` matches the computed chain end, including the
missing final 12 bytes seen in both the free Starter bank and EBClarinet. The
scanner reports physical and computed ends separately, so this common short
tail is not mistaken for a record length or an audio boundary.

Observed payload tags and fields:

| Tag | Payload size | Observed fields |
| --- | ---: | --- |
| `3236ba2f` | 272 | 256-byte encrypted name at `+4`; absolute child-descriptor payload offset at `+260`; 4-byte footer at `+268` |
| `e4505867` | 289 | 256-byte encrypted name at `+4`; exact member byte size at `+260`; absolute member data offset at `+268`; mode byte at `+276`; remaining footer bytes are opaque |
| `98b34718` | 34 | Absolute index-node, first-leaf, and last-leaf payload offsets at `+4`, `+12`, and `+20` |
| `af6aa83c` | usually 16,932 | Child count at `+4`; then `count` 264-byte entries, each with a 256-byte encrypted child name and a clear absolute metadata-payload pointer |
| `ab829c4f` | variable | Child index-node record; contents remain opaque |

Directory pointers resolve as directory node → `98b34718` descriptor →
index root plus first/last linked leaves. For a single-leaf directory, all three
descriptor offsets are equal. Each table entry's filename stream starts at its
own physical name offset, `leaf_payload + 8 + index * 264`, and its pointer
resolves to the child node payload offset. The leaf footer stores previous and
next leaf offsets immediately after its local entries; `u64::MAX` marks either
end of the linked list. Names agree with referenced node names across the
inspected Starter and EBClarinet banks, yielding full paths. The index-node
body remains opaque; linked leaves provide the complete child lists.
The Starter root points to table payload offset 1992 (count 10); EBClarinet root
points to payload offset 91072 (count 7).

File size and data offset fields bound each member exactly, even when the
surrounding record contains trailing bytes. For example, the EBClarinet member
at data offset 24,890,992 has an exact length of 36,735 bytes; the outer record
contains two additional bytes. That bounded span passes `flac -t`; treating the
entire outer record as FLAC fails. Similar measured suffixes vary, so callers
must use the metadata size, never infer the stream end from the next record.

Mode 0 members can be extracted without altering the bank:

```sh
python3 tools/inspect_uvi.py --reader UVIWorkstationx64.exe \
  --extract-one 24890992 --output sample.flac bank.ufs
```

Extraction requires an exact `data_offset`, refuses to overwrite an existing
output, checks the declared span against the bank, and refuses nonzero modes.
Some mode-0 resources named `.wav` contain FLAC streams; the extension is not a
codec guarantee. Other modes are reported numerically and left untouched.

The XOR-stream primitive is also used for the observed filename codec. It does
not itself establish the key for protected file payloads. `--recover-content-key`
selects a mode-2 PNG by its decoded member name, derives a candidate from the
standard PNG signature and IHDR prefix with the generic C state search, then
validates the complete PNG chunk CRCs using the 512-byte content-block reset
rule. It writes only the verified 8-byte key to the explicitly requested new
owner-only file and never prints the key. If no candidate passes CRC validation,
the command fails without publishing a key.

```sh
python3 tools/inspect_uvi.py --recover-content-key bank.ufs \
  --reader UVIWorkstationx64.exe --key-output /tmp/uvi-content-key.bin
```

The helper compiles in a temporary directory and uses OpenMP when available.
It bounds a candidate PNG at 64 MiB before reading it into memory.
The free Starter bank has no mode-2 PNG candidates; its inspected file nodes
are mode 0 except for one mode-1 manifest.

Run `python3 tools/inspect_uvi.py --self-test-recovery` to compile the helper
and check it against an authored PNG-header/cipher fixture without any bank
data or key material.

The scanner is a structural inspector and clear-member extractor. It does not
decode index-node bodies, protected program payloads, or DSP/module semantics,
and it is not a sampler runtime.

### Image wavetable resampling boundary

UVI's [custom wavetable documentation](https://support.uvi.net/hc/en-us/articles/360001260738-Falcon-Loading-Custom-Wavetables) defines one cycle per image row but leaves large-image resizing or cropping unspecified. Targeted inspection of the verified official Workstation 4.0.9 reader resolves that geometry: importer `0x141528b50` rescales the entire source image to 2048 columns and `min(source_height, 128)` rows. PNG and JPG extension branches share that importer. The shared rescaler `0x1409c5020` uses an affine scale in both dimensions with numeric quality argument 1. [JUCE's public enum](https://docs.juce.com/master/classjuce_1_1Graphics.html) names quality 1 medium resampling; this API correspondence is supporting evidence, not proof of the exact native interpolation or quantization.

These static observations establish a resize rather than a crop. Existing authored native PNG comparisons establish conversion only for bounded images up to 2048 columns and 128 rows. The importer therefore rejects wider or taller images before pixel decoding with the exact missing resampling behavior named in the error. JPEG decoding remains gated. New authored 129/256-row fixtures were prepared privately, but no successful fresh native render was obtained: the official VST failed initialization with Windows error 1114 in the isolated Wine prefix. Its packaged support runtime first rejected the prefix's Windows 7 setting; retrying with Windows 10 did not establish a working reference host. No activation checks were modified, and no account state was copied. This is a remaining oracle limitation, not a completed numerical comparison.
