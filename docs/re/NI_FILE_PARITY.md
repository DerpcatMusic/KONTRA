# ni-file parity census

The metric for the ni-file parity lane: of every Kontakt file in the local
corpus, how much of what Kontakt reads KONTRA reads. Target: no partial or
unrecognised object that Kontakt reads, so 0 unaccounted bytes.

Tool: `crates/format/kontakt/examples/chunks.rs`.

```
build-slot cargo build --release -p kontra-format-kontakt --example chunks
nice -n 10 <target>/release/examples/chunks /data/main/Libraries/Kontakt > census.md
```

`KONTRA_CHUNK_DUMP=<dir>` keeps the chunks not read in full;
`KONTRA_CENSUS_EXAMPLES=1` names one file per class on stderr. Iterate on the
affected files and run the whole corpus once per PR.

How a chunk is judged:

- Every chunk of every chunk stream is walked: structured children and the
  slots a hand decoder holds (parameter arrays, snapshot groups).
- Presets open through the plugin's own load path (`kontra_format_kontakt::decode`),
  the code that plays installed libraries in KONTRA. Encrypted presets it opens
  are counted ("of them encrypted"); only counts and class names leave the
  run, and no chunk of an encrypted preset is dumped. What that path cannot
  open is **protected**. The census adds no decryption of its own.
- A chunk whose body starts with 1 or 0 is a serialized object (1: private,
  public, children; 0: version and one block) when its framing covers the
  chunk exactly (1) or Kontakt has a reader for that version (0). Unframed
  records (lists led by a u32 count) can start with either byte; they count
  as `raw`. Kontakt reads the public and
  private blocks with separate readers ([NI_FILE_BINARY_RECORDS.md](NI_FILE_BINARY_RECORDS.md) §2).
  On the corpus every class keeps its fields in one block, and the catalog
  reader (`native_schema`, Kontakt 8.13.1) decodes that block exactly.
- **full**: Kontakt's reader for the SerType and stored version accounts for
  every byte of each non-empty block, and nothing follows the children.
- **partial**: something reads it but bytes are not accounted for: the native
  reader is truncated, rejects the stream, reaches code not modelled, leaves
  trailing bytes, or only a hand reader with no byte accounting reads it
  (`hand reader only`; `raw` when the body is not object-framed).
- **unrecognised**: no reader at all.
- Objects nested in a block (`(rd O)` sites the reader tables in
  `native_schema/vm.rs` name) decode with their own reader. Bytes of a nested
  object whose class is not named, or that does not decode, are **opaque**:
  the parent is partial with `nested object of unknown class` and only those
  bytes are unaccounted.
- A parent's bytes include those of the objects nested in it, which are also
  counted on their own rows, so byte totals overlap.

## Baseline (2026-11-25, origin/main 2597cd0d2, encrypted presets not opened, `/data/main/Libraries/Kontakt`)

### Files

| Type | Files | Read | Protected | Unreadable |
| --- | ---: | ---: | ---: | ---: |
| .nicnt | 9 | 0 | 0 | 9 |
| .nkc | 241 | 0 | 0 | 241 |
| .nki | 682 | 57 | 625 | 0 |
| .nkm | 54 | 1 | 53 | 0 |
| .nkr | 12 | 12 | 0 | 0 |
| .nksn | 1139 | 1139 | 0 | 0 |
| .nkx | 229 | 229 | 0 | 0 |

Unreadable:

- 9 × .nicnt: no reader
- 241 × .nkc: no reader

### Objects

18429640 objects: 17016822 full, 1412818 partial, 0 unrecognised. Bytes 3668698267, unaccounted for 2091593731.

| Partial because | Objects |
| --- | ---: |
| Kontakt's reader rejects it | 234 |
| hand reader only | 1405125 |
| raw, hand reader only | 1310 |
| reader not modelled | 903 |
| truncated for Kontakt's reader | 5246 |

By SerType, stored version and class, most unaccounted bytes first. Hand: KONTRA's typed decoder succeeded / failed / names the id only / does not know it.

| SerType | Version | Class | Objects | Full | Partial | Unrecognised | Bytes | Unaccounted | Hand |
| --- | --- | --- | ---: | ---: | --- | ---: | ---: | ---: | --- |
| 0x004f | 0x1 | BSnapshot | 801 | 0 | 801 reader not modelled | 0 | 1808206084 | 1808206084 | 801/0/0/0 |
| 0x004f | 0x3 | BSnapshot | 338 | 237 | 101 reader not modelled | 0 | 182923102 | 178496971 | 338/0/0/0 |
| 0x0034 | raw | ZoneList | 53 | 0 | 53 raw, hand reader only | 0 | 44171830 | 44171830 | 53/0/0/0 |
| 0x004b | raw | FNTableImpl | 1197 | 0 | 1197 raw, hand reader only | 0 | 24197128 | 24197128 | 1197/0/0/0 |
| 0x0033 | raw | GroupList | 57 | 0 | 57 raw, hand reader only | 0 | 15086348 | 15086348 | 57/0/0/0 |
| 0x0037 | raw | SlotList | 1 | 0 | 1 raw, hand reader only | 0 | 10572781 | 10572781 | 1/0/0/0 |
| 0x0007 | 0x90 | BParEnv | 1389661 | 0 | 1389661 hand reader only | 0 | 5558644 | 5558644 | 1389661/0/0/0 |
| 0x003a | 0x12 | BParameterArraySer<struct BParFX, 8> | 15475 | 10728 | 4747 truncated for Kontakt's reader | 0 | 2602094 | 2516270 | 15475/0/0/0 |
| 0x0006 | 0x60 | BParScript | 290 | 56 | 234 Kontakt's reader rejects it | 0 | 4089595 | 1342601 | 290/0/0/0 |
| 0x0034 | 0x3 | ZoneList | 3 | 0 | 3 hand reader only | 0 | 549379 | 549379 | 3/0/0/0 |
| 0x0045 | 0x11 | BInsertBus | 13712 | 0 | 13712 hand reader only | 0 | 368510 | 368510 | 13712/0/0/0 |
| 0x0034 | 0x5 | ZoneList | 1 | 0 | 1 hand reader only | 0 | 289281 | 289281 | 1/0/0/0 |
| 0x003a | 0x13 | BParameterArraySer<struct BParFX, 8> | 1938 | 1439 | 499 truncated for Kontakt's reader | 0 | 179942 | 162674 | 1938/0/0/0 |
| 0x0045 | 0x12 | BInsertBus | 1632 | 0 | 1632 hand reader only | 0 | 47124 | 47124 | 1632/0/0/0 |
| 0x0028 | 0xae | Program | 49 | 0 | 49 hand reader only | 0 | 16639 | 16639 | 49/0/0/0 |
| 0x0032 | 0x60 | VoiceGroups | 57 | 0 | 57 hand reader only | 0 | 3363 | 3363 | 57/0/0/0 |
| 0x0f02 | 0x10 | SessionState | 1 | 0 | 1 reader not modelled | 0 | 2528 | 2528 | 0/0/1/0 |
| 0x0028 | 0xab | Program | 7 | 0 | 7 hand reader only | 0 | 1899 | 1899 | 7/0/0/0 |
| 0x0048 | raw | MultiSlots | 1 | 0 | 1 raw, hand reader only | 0 | 1536 | 1536 | 1/0/0/0 |
| 0x003e | 0x0 | BOutputConfiguration | 1 | 0 | 1 hand reader only | 0 | 1460 | 1460 | 1/0/0/0 |
| 0x0028 | 0xb5 | Program | 1 | 0 | 1 hand reader only | 0 | 364 | 364 | 1/0/0/0 |
| 0x0049 | raw | MidiPortMaps | 1 | 0 | 1 raw, hand reader only | 0 | 272 | 272 | 1/0/0/0 |
| 0x0003 | 0x73 | Bank | 1 | 0 | 1 hand reader only | 0 | 45 | 45 | 1/0/0/0 |
| 0x000c | 0x100 | BParExternalMod | 5951201 | 5951201 |  | 0 | 412980777 | 0 | 5951201/0/0/0 |
| 0x000d | 0x80 | BParInternalMod | 2704343 | 2704343 |  | 0 | 773429784 | 0 | 2704343/0/0/0 |
| 0x0025 | 0x50 | BParFX | 2018043 | 2018043 |  | 0 | 44396370 | 0 | 2018043/0/0/0 |
| 0x0008 | 0x71 | BParLFO | 1682400 | 1682400 |  | 0 | 112720800 | 0 | 1682400/0/0/0 |
| 0x003f | 0x11 | BParEnv_AHDSR | 1389661 | 1389661 |  | 0 | 107003897 | 0 | 1389661/0/0/0 |
| 0x000d | 0x81 | BParInternalMod | 732530 | 732530 |  | 0 | 55561905 | 0 | 732530/0/0/0 |
| 0x0018 | 0x92 | BParFXFilter | 693170 | 693170 |  | 0 | 12633703 | 0 | 693170/0/0/0 |
| 0x0008 | 0x73 | BParLFO | 364812 | 364812 |  | 0 | 24807216 | 0 | 364812/0/0/0 |
| 0x001d | 0x80 | BParFXShaper | 338783 | 338783 |  | 0 | 2710264 | 0 | 338783/0/0/0 |
| 0x001e | 0x60 | BParFXDistortion | 338584 | 338584 |  | 0 | 4063008 | 0 | 338584/0/0/0 |
| 0x0020 | 0x70 | BParFXLoFi | 338583 | 338583 |  | 0 | 5755911 | 0 | 338583/0/0/0 |
| 0x001a | 0x60 | BParFXInverter | 189140 | 189140 |  | 0 | 378280 | 0 | 189140/0/0/0 |
| 0x000c | 0x104 | BParExternalMod | 142107 | 142107 |  | 0 | 9281799 | 0 | 142107/0/0/0 |
| 0x0018 | 0x95 | BParFXFilter | 91414 | 91414 |  | 0 | 3109118 | 0 | 91414/0/0/0 |
| 0x0010 | 0x51 | BParFXDelay | 5608 | 5608 |  | 0 | 162632 | 0 | 5608/0/0/0 |
| 0x001f | 0x70 | BParFXStereoSpread | 4514 | 4514 |  | 0 | 40626 | 0 | 4514/0/0/0 |
| 0x0019 | 0x70 | BParFXCompressor | 3705 | 3705 |  | 0 | 77805 | 0 | 3705/0/0/0 |
| 0x0015 | 0x50 | BParFXReverb | 2804 | 2804 |  | 0 | 56080 | 0 | 2804/0/0/0 |
| 0x004d | 0x10 | BParFXJump | 2804 | 2804 |  | 0 | 72904 | 0 | 2804/0/0/0 |
| 0x0044 | 0x10 | BParFXSSLGEQ | 2228 | 2228 |  | 0 | 93576 | 0 | 2228/0/0/0 |
| 0x0016 | 0x70 | BParFXIRC | 1749 | 1749 |  | 0 | 211629 | 0 | 1749/0/0/0 |
| 0x0017 | 0x51 | BParFXSendLevels | 1265 | 1265 |  | 0 | 136620 | 0 | 1265/0/0/0 |
| 0x0051 | 0x1 | BSnapshotMetaData | 1139 | 1139 |  | 0 | 71510 | 0 | 1139/0/0/0 |
| 0x0011 | 0x51 | BParFXChorus | 701 | 701 |  | 0 | 17525 | 0 | 701/0/0/0 |
| 0x0014 | 0x51 | BParFXPhaser | 701 | 701 |  | 0 | 20329 | 0 | 701/0/0/0 |
| 0x001c | 0x60 | BParFXLimiter | 701 | 701 |  | 0 | 5608 | 0 | 701/0/0/0 |
| 0x0042 | 0x10 | BParFXTape | 509 | 509 |  | 0 | 6617 | 0 | 509/0/0/0 |
| 0x0013 | 0x50 | BParFXGainer | 464 | 464 |  | 0 | 1856 | 0 | 464/0/0/0 |
| 0x0021 | 0x50 | BParFXSkreamer | 200 | 200 |  | 0 | 4000 | 0 | 200/0/0/0 |
| 0x0043 | 0x10 | BParFXTrans | 100 | 100 |  | 0 | 1600 | 0 | 100/0/0/0 |
| 0x0046 | 0x12 | BParFXSSLGBusComp | 100 | 100 |  | 0 | 3000 | 0 | 100/0/0/0 |
| 0x004c | 0x10 | BParFXFBComp | 100 | 100 |  | 0 | 3100 | 0 | 100/0/0/0 |
| 0x0047 | 0x10 | SaveSettings | 58 | 58 |  | 0 | 870 | 0 | 58/0/0/0 |
| 0x004e | 0x1 | QuickBrowseData | 57 | 57 |  | 0 | 228 | 0 | 57/0/0/0 |
| 0x005a | 0x12 | BParFXReplikaDelay | 56 | 56 |  | 0 | 6384 | 0 | 56/0/0/0 |
| 0x005d | 0x51 | BParFXChoral | 12 | 12 |  | 0 | 408 | 0 | 12/0/0/0 |
| 0x005b | 0x51 | BParFXPhasis | 5 | 5 |  | 0 | 260 | 0 | 5/0/0/0 |
| 0x0072 | 0x1 | BParFXGater | 5 | 5 |  | 0 | 100 | 0 | 5/0/0/0 |
| 0x0056 | 0x10 | BParFXDStortion | 2 | 2 |  | 0 | 50 | 0 | 2/0/0/0 |
| 0x005c | 0x51 | BParFXFlair | 2 | 2 |  | 0 | 132 | 0 | 2/0/0/0 |
| 0x0074 | 0x1 | BParFXReverseGrain | 2 | 2 |  | 0 | 38 | 0 | 2/0/0/0 |

## Batch 2: load-path scope and snapshot nested objects

The census opens presets through the plugin's load path (above), so 625 .nki
and 53 .nkm that the baseline counted as protected are now read: 0 protected.
Before is the census with that scope and the exact-framing rule on e4dbdc662;
after adds the snapshot readers below.

| | Objects | Full | Partial | Bytes | Unaccounted |
| --- | ---: | ---: | ---: | ---: | ---: |
| Before | 18466552 | 17036402 | 1430150 | 14543257982 | 11239467243 |
| After | 18466552 | 17042634 | 1423918 | 14543257982 | 9398002934 |

| Class | Before unaccounted | After unaccounted | After verdict |
| --- | ---: | ---: | --- |
| 0x4F BSnapshot v1 | 1808206084 | 109307562 | 801 nested object of unknown class |
| 0x4F BSnapshot v3 | 178496971 | 38830120 | 101 nested object of unknown class, 237 full |
| 0x3A BParameterArraySer<BParFX, 8> v0x12 | 2736021 | 0 | full |
| 0x3A BParameterArraySer<BParFX, 8> v0x13 | 162915 | 0 | full |

What closed them (evidence in the `BODIES`, `OBJECTS` and `VIRTUAL` tables of
`vendor/ni-file/src/kontakt/native_schema/vm.rs`):

- BSnapshot nested FX racks (0x3A) and buses (0x45), and the
  group snapshots ([ref], 0x50 BGroupSnapshot) with their FX rack,
  source (0x0E), internal (0x3B) and external (0x3C) modulator arrays
, in the order objects/snapshot.rs and snapshot_group.rs read.
- BParSrcMode's source reader [ref] starts with a virtual call on the
  member at +0xC8. Its constructor installs vtable [ref]
  there; slot 0x10 is [ref], which reads one u32.
- A framed nested object decodes with its own host-state search, memoized
  per outermost decode, so a parent's search does not replay it: the full
  corpus runs in 9.6 min.

Left: the snapshots' opaque bytes are nested objects with no catalog reader
yet (0x45 BInsertBus is hand-only) or that Kontakt's reader rejects (scripts).
SessionState (0x0F02) now names its stop: an unframed nested object of
unknown class in [ref]. Next: unframed lists (ZoneList, FNTableImpl,
GroupList, SlotList), BParEnv 0x90, scripts (BParScript 0x60).

## Batch 3: BParEnv, and what snapshots leave unread

The census now splits the bytes of nested objects a reader consumes but does not
decode by parent, nested SerType and cause. On 0a34e0945 (before this batch) the 902 partial snapshots left 145914405 bytes in 0x07
BParEnv (no reader) and 2223277 in 0x45 BInsertBus (no reader); no script was
rejected.

BParEnv had no reader because the catalog took vtable [ref]'s private
reader (slot 11, [ref]), an empty stub. Its public reader (slot 10,
[ref]) accepts version 0x90 and reads one u32 envelope type, 0, 1 or 2.
It is the first entry of `native_schema/curated.rs`: readers programs.rs lacks,
each one function lowered by `tools/re/lower_curated.py` from the decompile
(programs.rs itself cannot be regenerated: its Ghidra project is gone and
another analysis does not reproduce it).

| | Full | Partial | Unaccounted bytes |
| --- | ---: | ---: | ---: |
| Before | 17042634 | 1423918 | 9398002934 |
| After | 18432295 | 34257 | 9246529885 |

- 0x07 BParEnv v0x90: 1389661 objects, 5558644 → 0 unaccounted, all full.
- 0x4F BSnapshot v1: 109307562 → 2003547; v3: 38830120 → 219730. What is left
  is 0x45 BInsertBus, nested with no reader.

Next: BInsertBus 0x45, then the unframed lists (ZoneList, FNTableImpl,
GroupList, SlotList).
