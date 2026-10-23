# IKMPAK: Modo Drum `.pak` sample packs

Status: container and payload fully decoded from the installed files;
reader in `crates/sampler-ikmpak` (read-only, no DSP, no decryption). Evidence
is black-box: the three installed packs under `/data/main/Libraries/Modo Drum`
(checked 2026-10-23). No IK binary was analysed.

| Pack | Size | Entries | Payload kinds |
| --- | ---: | ---: | --- |
| `MODO DRUM Brit Custom.pak` | 1 431 554 607 B | 1450 | `Cymbals` 886, `Kits` 564 |
| `MODO DRUM Metal.pak` | 1 781 206 773 B | 1510 | `Cymbals` 1076, `Kits` 434 |
| `MODO DRUM Silver.pak` | 2 079 665 613 B | 1774 | `Cymbals` 998, `Kits` 776 |

Total 4734 members, 5.3 GB (4.9 GiB) of audio. The payloads are not compressed
or encrypted: each one is a complete RIFF/WAVE file.

## Header (bytes 0–13, little-endian)

| Offset | Size | Field | Value seen |
| ---: | ---: | --- | --- |
| 0 | 6 | magic | `IKMPAK` |
| 6 | u32 | version | `1` (all three packs) |
| 10 | u32 | entry count | 1450 / 1510 / 1774 |

## Table of contents (from offset 14)

`count` records, back to back, no padding and no alignment:

| Field | Size | Meaning |
| --- | --- | --- |
| path | n bytes + NUL | member path, UTF-8 (all ASCII in the three packs), `/` separated, relative to the pack |
| offset | u32 | absolute byte offset of the payload in the pack file |
| size | u32 | payload length in bytes |

The records are not fixed size because the paths vary. The TOC ends where the
first payload starts. Checked on all 4734 records:

- The first payload starts at the end of the TOC: 137 625 (Brit), 169 577 (Metal), 190 911 (Silver).
- Payloads tile the file: `offset[i+1] == offset[i] + size[i]`, and the last
  payload ends at EOF. No gaps, no overlaps, no shared payloads.
- Offsets and sizes fit in u32 (the largest offset is 2 078 329 891).

The reader needs only the TOC. It reads it from a stream and never maps the
5 GB file.

## Payload codec

Every payload is a plain RIFF/WAVE file:

- chunks are exactly `fmt ` and `data`, in that order;
- `RIFF` size field equals payload size − 8 for all 4734 payloads;
- format tag 1 (PCM), 24 bits per sample, 1 or 2 channels, 48 000 or 96 000 Hz.

Measured mix, by mic:

| Mic (file suffix) | Count | Channels | Rate |
| --- | ---: | --- | --- |
| `DRY` | 2456 | mono 2288, stereo 168 | 48 kHz (2446), 96 kHz (10, Silver only) |
| `WET` | 2278 | stereo, all | 48 kHz (2268), 96 kHz (10, Silver only, the same 5 pairs as DRY) |

Stereo `DRY` (168) have no `WET` partner; the other 2278 `DRY` have one. In
every pair checked, the WET payload is exactly twice the byte size of its DRY
payload. Since WET is stereo and DRY is mono, both have the same frame count.
So WET is a same-length stereo layer, not a longer tail.

## Member naming (articulation, velocity, round robin, mic)

Path shape: `<top>/<folder>/…/<leaf>.wav`. The top level is `Cymbals` or `Kits`.
Folder depth below the top is 2 to 4 (the file is the last segment). The
innermost folder names the articulation, the one above it the piece. Examples:

- `Cymbals/China 2 - China 2 Custom/China Cymbal Choke/E2_v127_rr1 China Cymbal Choke-DRY.wav`
- `Kits/Metal/Toms/Rack 2 Rimshot Rattle Off/C2_v127_rr10_Floor Tom 1 Rimshot Rattle Off-WET.wav`
- `Cymbals/Hi-Hats/HH Custom - Custom/HH Foot Open/_V127_rr1 Pearl-Hit Hat Foot Open-DRY.wav`

Leaf grammar, as implemented in `member::parse_member` (every entry in all
three packs matches; keys are unique per folder, note, velocity, round robin and mic):

```
leaf   = [ ' ' | '_' ]* [ NOTE '_' ] ['_'] 'v' VEL ['_'] 'rr' RR [ ' ' | '_' ] NAME '-' MIC '.wav'
NOTE   = letter A–G, optional '#' or 'b', optional '-', octave digits
VEL    = decimal, observed 10…127 (Metal 20…127)
RR     = decimal, observed 1…23 (round-robin index within a layer)
MIC    = 'DRY' | 'WET'
```

The `NAME` part can contain `-` (for example `Pearl-Hit Hat`), so the mic
tag is taken from the last `-` before `.wav`.

Mapping for the engine (what the reader provides; the playback mapping is not done yet):

- **Articulation**: innermost folder (`Hit`, `Choke`, `Closed Shank`,
  `Extra Short`, `Floor Tom 1 Rimshot Rattle Off`, …). Not all members in a
  folder share one articulation, so the folder list is kept whole.
- **Key**: `NOTE` where present (C2, E2, C#1, …). 658 (Brit), 794 (Metal) and 960 (Silver)
  members have no note (`_v…` leaves), so their key comes from the folder alone. Note
  numbers use C3 = 60 (`member::note_number`). **Assumption**: the packs
  carry no key map, so this octave convention is the Kontakt/IK one and is
  not confirmed by the product.
- **Velocity layer**: the `v` value, a discrete layer. The packs do not store
  the bounds of each layer. Proposed rule for the mapping step: a layer covers
  the velocities above the previous layer's value up to its own value, and the
  top layer runs to 127. Not verified against the product.
- **Round robin**: `rr` index inside one (folder, note, velocity) cell.
- **Mic**: `DRY` = close mic (mono for most hits); `WET` = stereo room layer.
  Roles follow the suffix names. Confirm the mic mix against the product before
  routing.

## What the reader does

`crates/sampler-ikmpak`:

- `Pak::open(reader)` validates magic and version, reads the TOC, and checks
  each entry: NUL-terminated UTF-8 path, payload not inside the TOC, payload
  inside the file. Errors name the entry index.
- `Pak::payload(i)` copies one payload. `Pak::wav_format(i)` reads only the
  `fmt ` and `data` headers.
- `member::parse_member(path)` implements the leaf grammar above, and
  `note_number` converts key names.

Tests (`cargo test -p sampler-ikmpak`) open the three installed packs (skipped
if absent), parse every member, check the WAV format mix above, check the
key-uniqueness rule, and compare one payload's RIFF size to its TOC size.

## Open items

- No program or kit file is in the pack. Which members make one instrument,
  its key/velocity ranges and mic mix are in the product, which is not
  installed here. Folder grouping is the only structure in the pack.
- Engine integration: members need an asset location that reads a byte range
  of the pack. `sampler-ir` has `AssetLocation::{Path, KontaktFile}`, so a
  `Pak { container, index }` variant and a loader arm are the next step. Not
  done here: the change touches the shared IR and loader, and the playback
  mapping needs the open items above.
- Note-number octave convention and velocity-layer bounds: confirm against the
  product (or its MIDI map) before any parity claim.
