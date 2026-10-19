# The Flex envelope's loop jump at the sustain row

Binary: Kontakt8.exe SHA256 `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
Addresses are virtual addresses in that image. Nothing here is copied vendor
code: the evidence is the instruction stream and the saved record's own
fields.

## Why this note exists

The installed-library survey reported `flex loop jump at the sustain row
(+0x194)` 504 times over four instruments (Audio Imperia CHORUS). The
Flex envelope record carries a byte at +0x194 that asks the envelope to
leave its sustain row instead of holding there. The effect on the held
envelope was not proved, so the translator reported it and ran the envelope
without it.

## The saved record

`BEnvelopeFM7` (chunk `0x40`, versions `0x11`/`0x12`) is read by
`ni_file::kontakt::objects::EnvelopeFlex`:

* `points`: the point list, each `(delta_ms, level, curve)`;
* `sustain`: the index held while the key is down;
* `unknown_index`: an index into `points`, `sustain - 1` in every local
  preset;
* `unknown_tail`: the trailing bytes. Version `0x12` has a `u16` first
  (`unknown_version_word`), whose low byte is the saved loop-jump flag and
  whose high byte is the release-adjustment bypass.

The loader copies the tail into the envelope's runtime object, so the saved
+0x194 reaches the runtime byte +0x321 (and +0x195 reaches +0x322).

## The advance

The envelope's advance computes the next point index at
`0x140ae57dd..0x140ae5803`:

```
0x140ae57dd  cmp esi, dword [rbx + 0x2fc]   ; the current point against a stored row
0x140ae57e3  jne 0x140ae57f8
0x140ae57e5  cmp byte [rbx + 0x321], 0       ; the saved loop-jump flag
0x140ae57ec  je 0x140ae57f8
0x140ae57ee  mov eax, dword [rbx + 0x300]   ; the loop's start
0x140ae57f4  inc eax
0x140ae57f6  jmp 0x140ae57fb
0x140ae57f8  lea eax, [rsi + 1]             ; the ordinary next point
0x140ae57fb  ...
0x140ae5803  mov dword [rbx + 0x314], eax   ; the next point index
```

The point indices are one-based: the ordinary next point is `esi + 1` and
the function's entry compares the current index against the point count
(+0x304) to decide that the envelope is finished. The jump target is
therefore `+0x300` read as a zero-based index (the stored value plus one
in the runtime's one-based numbering).

The two stored fields are the saved record's own indices: `+0x2fc` is the
sustain row and `+0x300` is the `unknown_index` — the point before it. So
the law is:

> When the envelope reaches its sustain row and the saved loop-jump flag is
> set, the next point is the point before the sustain row, and the envelope
> cycles between the two instead of holding.

## What the implementation does

* The translator reads the flag from the saved `u16` and sets the IR's
  `Breakpoints::loop_range` to `(unknown_index, sustain)`.
* The core's segment state takes the loop's start as the next point when
  it reaches the loop's end, and that replaces the hold at the sustain row
  (the hold is only for an envelope without a loop).

The saved release-adjustment bypass (+0x195) has no proved effect on the
held envelope and stays reported; no local instrument sets it.

## What stays open

* The native envelope's own level law for the segment the loop returns to
  is the existing Flex curve law (`Breakpoints::flex_release` and
  `flex_ratio`); this note only covers the point order.
* The field identity of `+0x300` is read from the jump target's
  relationship to the sustain row, not from the loader's writes: the
  loader copies the saved record in bulk, so the assignment of the saved
  `unknown_index` to that runtime field is inferred, not read.
