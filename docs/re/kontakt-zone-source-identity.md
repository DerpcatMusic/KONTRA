# The native zone source identity

`native source zone identity` reported that a saved source zone had no readable
identity, so no ordinal alias was created. The identity is the zone's terminal
private field; the reader used to gate it on one exact private layout, which
left every other saved layout without an identity. This record is the layout
law, taken from `Kontakt8.exe` SHA256
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.

## The reader

`0x140d0c760` is the zone private reader. Its first act is a version dispatch:

```
movzx eax, r8w
add eax, 0xffffff6e          ; version - 0x92
cmp eax, 0xa
ja  0x140d0cd0d              ; throw
jmp [0x140000000 + rax*4 + 0xd0cd2c]
```

The table at `0xd0cd2c` has eleven entries:

| version | branch |
|---|---|
| `0x92` | `0x140d0c7a1` |
| `0x93`, `0x94` | `0x140d0c8c5` |
| `0x95`, `0x97`, `0x98` | `0x140d0ca03` |
| `0x96` | `0x140d0cd0d` (throws) |
| `0x99` .. `0x9c` | `0x140d0cb71` |

Every branch except `0x96` ends by jumping to `0x140d0cce5`, and that tail is
where the identity is read:

```
0x140d0cce5  mov  rcx, rdi
0x140d0cce8  call 0x142a78270          ; read_u32
0x140d0cced  cmp  dword [rbx+0x5c], 1  ; aux count, read only by 0x99..0x9c
0x140d0ccf1  mov  [rbx+0xa4], eax      ; the identity
0x140d0ccf7  jg   0x140d0ccfd
0x140d0ccf9  mov  byte [rbx+0x45], 0
0x140d0ccfd  ... ret
```

`0x99..0x9c` read one extra `u32` into `+0x5c` immediately before the identity
(`0x140d0ccdd`); `0x92..0x98` jump straight to the identity read. Nothing is
read after `[rbx+0xa4]`.

## Why the private lengths differ

The stream between the version field and the identity is not fixed length. Two
callees consume variable amounts:

* `0x142a78400` returns a blob whose length is in the stream and whose pointer
  lands in `+0xa08`;
* `0x140cf1330` reads the object at `+0x510`.

So the saved privates differ in length — 83 bytes (`0x98`), 87 (`0x99`,
`0x9a`), 140..176 (`0x9a`) — while the identity stays the terminal `i32`.
That also explains the byte-56 correlation the old gate used: `private[56..58]`
is a field of one layout, not a flag. In the longer layouts the variable
sub-records shift everything after them, so the same byte reads `1`.

The accepted 87-byte layout (`0x9a..=0x9c`, id at 83..87) is one instance of
this law: `87 - 4` is the identity, and `83..87` is also where the terminal
`i32` sits.

## The mode guard

The private's first four bytes are read into `+0xa8`, then `0x140cf0360` reads
the mode at `private[4..8]` and throws unless it is 0, 1 or 2:

```
call 0x142a78270     ; mode
test eax, eax        ; 0 -> 0
...
cmp eax, 1           ; 1 -> 1 (0x140cf0388)
jne 0x140cf03a0      ; else throw
mov dword [rbx], 2   ; 2 -> 2
```

The guard is kept: it is fixed-position at every version, because it is the
first field after the initial `u32`.

## What the translator does

`vendor/ni-file/src/kontakt/objects/zone_data.rs::source_id` now admits every
version the native reader dispatches except `0x96`, requires at least eight
private bytes, keeps the mode guard, and takes the terminal `i32` as the
identity: `-1` means none, a negative value below `-1` is an error.

`crates/sampler-kontakt/src/library.rs::raw_zone` no longer gates on the exact
layout; it asks the decoder and keeps `None` when the private does not decode.
An undecodable private therefore still leaves the zone without an identity and
is counted in the `native source zone identity` report with its layout, which
is the behaviour the report was written for — but no correct private is
rejected because its length or byte 56 differs.

## Verification

* `vendor/ni-file` — `every_private_length_takes_its_terminal_i32_as_identity`,
  `version_branches_that_share_the_identity_tail_are_admitted_and_0x96_is_not`,
  `bad_mode_and_negative_ids_are_errors_not_identities`.
* `crates/sampler-kontakt` — `empty_zone_tests::every_native_private_layout_ends_with_its_identity`
  builds a zone list with 87-, 140-, 152- and 176-byte privates and reads all
  four identities through the catalog.
* Corpus re-survey (2026-10-20, `fx_survey` on the mounted library tree):
  see `docs/diagnostics/20261019-installed-gap-inventory.md` for the counts.
