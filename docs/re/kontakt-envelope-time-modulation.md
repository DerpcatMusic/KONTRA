# Envelope-time modulation and the compiler's expression templates

Binary: Kontakt8.exe SHA256 `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
All addresses are virtual addresses in that image. Nothing here is copied
vendor code: the evidence is the instruction stream, the format strings the
compiler emits, and the property strings it compares.

## Why this note exists

The installed-library survey reports `modulation of a module parameter`
26,882 times across 916 instruments. 26,873 of those entries are
`ahdsr_attack` / `ahdsr_release` targets: a modulation route that addresses a
stage time of an AHDSR modulator. That is the largest single unimplemented
feature in the Kontakt corpus, and the compiler that would emit it is the same
one this repository already models in `sampler-core/src/voice_mod/laws.rs`.

This note records what the native compiler actually emits for those targets,
because the answer decides three things the current code guesses:

* whether an envelope time is scaled in the time domain or in the knob
  domain,
* whether the inner expression is cubed, and
* when the envelope conversion pair (`fconv_u2n_env_a`, `fconv_n2u_env_a`)
  applies at all.

## The compiler emits expression templates, not arithmetic

`FUN_140873040` builds a small expression per route and evaluates it. The
combination step is at `0x1408766bb..0x140876952`. Five format strings exist:

| Address | Template |
|---|---|
| `0x144f1da68` | `    $out := {}({} * {}({}) * $intensity)\n` |
| `0x144f1daa0` | `    $out := {}({} * {}(1.0 - (1.0 - ...\n         {}) * $intensity))\n` |
| `0x144f1daf0` | `    $out := 1.0 - (1.0 - $in) * ...\n            (1.0 -  {} * $intensity)\n` |
| `0x144f1db40` | `    $out := $in * {} * $intensity\n` |
| `0x144f1db68` | `    $out := $in + {} * $intensity\n` |
| `0x144f1db90` | `    $out := fcubic(1.0 - (1.0 - ...\n         {}) * $intensity)\n` |

The four `{}` slots of the first two templates are filled, in order, from the
descriptor at `[rbp + 0x290]`: `r11`, `r10`, `rcx`, `r8` (stored at
`0x140876768..0x140876776` and `0x1408767f5..0x140876803`). `r8` is the
prepared source expression. `r10` is `"$in"` (`0x144f1da08`) and `r11` is
the empty string (`0x14470787b`) unless the envelope conversion pair
replaces them. `rcx` is `"fcubic"` (`0x144f1da0c`) or the empty string.

So the two multi-function templates are

* `A(B * C(source) * $intensity)` and
* `A(B * C(1.0 - (1.0 - source) * $intensity))`

with `A`, `B`, `C` taken from the descriptor.

## Family H and the `fcubic` selection

`0x140875ac8..0x140875b3b` decides the family from the target id. The
compiler sets `r13` for the ids `2`, `0x44`, `0x71..=0x89`, `0xb8..=0xbb`,
`0xbd`, `0xbf`, `0xc0`, `0xc4..=0xcb` and `0x19` without route flag `0x04`.
`r13 != 0` selects the two-function templates above; the remaining ids use
`1 - (1 - x)(1 - e*i)`, `x * e * i`, `fcubic(inner)` or `x + e * i`.

`C` is chosen at `0x140876753`:

```
0x140876753  cmp byte [rbp - 0x5c], 0
0x140876757  cmove r9, rcx        ; rcx = "" when the flag is clear
0x14087675b  mov rcx, r9
```

`[rbp - 0x5c]` is set at `0x140873120` from the external source descriptor:
`setbe` after `lea eax, [rcx - 7]; cmp eax, 6`, so the flag is set for the
external source types 7..=13 (key position, velocity, release velocity, the
release trigger counter, constant, and the two random generators) and clear
for types 1..=4 and for every internal source. The internal branch at
`0x14087312f` never writes `[rbp - 0x5d]` or `[rbp - 0x5c]`.

Therefore, for a family H target:

* an internal source, or an external source of type 1..=4:
  `x * (1 - (1 - e) * i)`;
* an external source of type 7..=13: `x * fcubic(1 - (1 - e) * i)`.

The repository applied the cube only to the "direct" ids (`2`, `0x71..=0x89`)
and used the un-cubed product for every other family H id. `sampler-core/src/voice_mod/laws.rs`
now follows the compiler: the cube follows the source type, for the whole
family. The internal special source (`[rbp - 0x80]`, from the internal
descriptor at `+0x548`) selects the other template, `A(B * C(source) *
$intensity)`, where the cube applies to the source instead of the inner
expression.

## The envelope conversion pair and the K2 compatibility flag

`0x1408766dc` reads the flag that gates the pair:

```
0x1408766dc  cmp byte [rbp - 0x5d], 0
0x1408766e0  je 0x14087673d        ; no pair: the plain family H template
0x1408766e2  mov edx, dword [r14 + r15 + 8]   ; the target id
0x1408766e7  lea eax, [rdx - 0xb8]
0x1408766ed  cmp eax, 3
0x1408766f0  jbe 0x1408766fd      ; 0xb8..0xbb: attack, hold, decay, release
0x1408766f2  lea eax, [rdx - 0xbf]
0x1408766f8  cmp eax, 1
0x1408766fb  ja 0x14087673d       ; 0xbf, 0xc0: the DBD decays
0x1408766fd  mov rcx, r9
0x140876700  lea eax, [rdx - 0xb8]
0x140876706  cmp eax, 1
0x140876709  ja 0x140876719
0x14087670b  lea r10, [0x144f1da18]   ; "fconv_u2n_env_a($in)"
0x140876712  lea r11, [0x144f1da30]   ; "fconv_n2u_env_a"
0x140876719  lea eax, [rdx - 0xba]
0x14087671f  cmp eax, 1
0x140876722  jbe 0x14087672f
0x140876724  lea eax, [rdx - 0xbf]
0x14087672a  cmp eax, 1
0x14087672d  ja 0x14087673d
0x14087672f  lea r10, [0x144f1da40]   ; "fconv_u2n_env_d($in)"
0x140876736  lea r11, [0x144f1da58]   ; "fconv_n2u_env_d"
```

So `A`/`B` become the envelope conversions for the attack/hold pair (`A`,
the 15 s range) and the decay/release and DBD pair (`D`, the 25 s range). The
pair is the Kontakt 2 time-domain law: the time is scaled and converted back
to the knob.

`[rbp - 0x5d]` comes from the external source descriptor at `+0x53d`
(`0x14087310a..0x140873111`). The loader writes that byte at `0x1409254fc`:

```
0x1409254b8  lea rdx, [0x144f1e780]  ; "simulateK2EnvTimeModulation"
0x1409254bf  mov rcx, qword [rbx]
0x1409254c2  call strcmp
0x1409254c7  test eax, eax
0x1409254c9  jne 0x14092550c
0x1409254d1  mov rdx, qword [rbx + 8]  ; the property value
0x1409254d5  lea r8, [0x144f0f648]     ; "yes"
0x1409254e0  movzx ecx, byte [rdx + rax]   ; four-byte compare
0x1409254f9  sete al
0x1409254fc  mov byte [r14 + 0x53d], al
```

The byte is 1 exactly when the external source carries the property
`simulateK2EnvTimeModulation` with the value `yes`. It is a Kontakt 2
compatibility switch: with it, an envelope time is scaled in the time
domain; without it (every library in this corpus), the normalized knob is
scaled instead, and the envelope conversion pair is not emitted at all.

## What this fixes in the repository

* `laws::combine` now applies `fcubic` to the inner expression for the whole
  family H when the source type selects it, and to the source expression for
  the internal special template.
* The native law path in `sampler-core/src/voice_mod.rs` now carries the
  source type's cube selection instead of assuming the un-cubed product.

## What stays open

* The source *producer* for the external types 9..=13 is not extracted, so
  the release trigger counter's value domain stays unproved and its routes
  keep the generic source preparation.
* The property `simulateK2EnvTimeModulation` is not read from the saved
  external source record, so the K2 pair cannot be selected yet.
* The envelope stage's own time law and the moment a changed stage time
  becomes visible are separate questions: the compiler writes the target
  value, and the envelope core consumes it. Nothing here claims how a
  mid-stage change is applied.
