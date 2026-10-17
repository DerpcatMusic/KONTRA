# Symphonia FLAC 0.5.5 checked decoder boundary

Upstream: [Project Symphonia](https://github.com/pdeljanov/Symphonia), package
`symphonia-bundle-flac` 0.5.5, MPL-2.0. The complete published crate was copied
before editing. Published archive SHA256:
`c91565e180aea25d9b80a910c546802526ffd0072d0b8974e3ebe59b686c9976`.
Its `.cargo_vcs_info.json` records upstream commit
`6d533f26150953a882a6a111ebd13f0abf7129d5`, path `symphonia-bundle-flac`.
The normalized Cargo manifest, original manifest, README, upstream lock and
all original source files are retained. Only `src/decoder.rs` and
`src/validate.rs` are modified;
this document and the full `LICENSE-MPL-2.0` are added. All original source
copyright/MPL headers are retained. This is a local path source override of
the existing version, not a registry-cache edit, replacement parser, alternate
decoder or third-party fixture bundle.

The original decoder SHA256 is
`2aa694a4669341a0733a2f00d2bdf936d685767010e604e9af95f175d7636e26`.
Changes in that file:

- Validate represented subframe width before bit reads. A 33-bit stereo side
  channel is explicitly unsupported by the existing i32 sample representation;
  this change does not implement or claim 32-bit decorrelated stereo parity.
- Bound the normal bit reader's unary wasted-bit read at the first impossible
  value, requiring a positive effective width before subtraction or shifting.
- Return ordinary DecodeError when fixed/LPC warm-up order exceeds the actual
  subframe block length, before slicing it.
- Keep each fixed/LPC prediction in i64, add the i64 residual, check the signed
  effective sample-width domain, then narrow. A valid wider prediction cancelled
  by the residual stays valid. Validate before using a reconstructed sample in
  subsequent prediction, and before restoring wasted zeros. No wrapped result
  is accepted as decoded audio.
- Bound Rice quotient using the same existing capped unary reader, consume the
  terminator at the boundary, and range-check the combined u64 word before
  narrowing. Valid signed residual extremes and parameter-zero cases remain
  represented. Out-of-domain residuals return DecodeError.
- Append four authored inline test functions covering malformed/positive
  warm-up and wasted widths, fixed/LPC orders/bit widths, both LPC loops, true
  wide-intermediate cancellation, predictor overflow and Rice domains. No
  proprietary source, audio, library or activation data is included.

`src/validate.rs` replaces numeric checksum byte-width dispatch and its two
unreachable branches with a typed four-variant width. `Validator::update`
returns the existing Result, refusing bps0/>32 and checked sample-byte geometry
before buffer or MD5 mutation; Decoder propagates that result. Two additional
inline tests cover all1..32 valid widths against independently interleaved bytes
and invalid-width state/capacity preservation before/after valid input. Original
validator SHA256:
`f3f515874c0c5aad6416da605fe830e807cc0e6dc6b22e54c6f203c83751a6f7`.

No new production allocation, decoder field, parser, registry, thread, codec
workspace buffer or DSP law is introduced. Existing ordinary Decoder::decode
clears its output on a returned error. The KONTRA checked owner additionally
permanently poisons itself on that returned error; no production unwind handler
is installed. Generic decoder entrypoint hardening outside these checks and
complete whole-decoder/stereo-33-bit certification are not claimed.

Validation identity belongs to the integrating root source/lock/configuration
and its actual receipts. The private author did not compile, format, execute
tests, run a decoder or produce a shipping source archive. Root must run the
inline tests, original checked-FLAC tests plus append-only ordinary-parser
causals, legal archive self-tests and affected product quality checks. Modified
preferred source must be distributed with the MPL source bundle, rather than
substituting the original crates.io archive.

The full MPL text was obtained from upstream's v0.5.5 LICENSE:
<https://raw.githubusercontent.com/pdeljanov/Symphonia/v0.5.5/LICENSE>, SHA256
`c76f740d1521b9bed9ca7a04ad526c310493c62621b1341d623b431736533b30`.
