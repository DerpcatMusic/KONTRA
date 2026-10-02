# Symphonia RIFF patch

Upstream: `pdeljanov/Symphonia`, tag `v0.5.5`, crate `symphonia-format-riff`0.5.5 (MPL-2.0). The crate sources/Cargo manifest/README were copied unchanged from the published0.5.5 crate before applying the following patch. Existing source copyright/license notices are retained. No proprietary instrument or audio fixture is included.

`src/wave/chunks.rs`: accept a complete extended PCM format descriptor (`fmt` length>=18), ignoring `cbSize` as required for WAVE_FORMAT_PCM and skipping only the remaining declared chunk bytes. The16-byte PCM descriptor stays supported; partial17-byte descriptors stay rejected. Validate PCM block alignment against channels and encoded sample width. Other format tags/codec selection are unchanged.

Primary specification: https://learn.microsoft.com/en-us/windows/win32/api/mmreg/ns-mmreg-waveformatex (PCM clients ignorecbSize; block alignment equals channels times bytes per sample). Original parser: https://github.com/pdeljanov/Symphonia/blob/v0.5.5/symphonia-format-riff/src/wave/chunks.rs.

Regression: `audio::tests::extended_pcm_wave_formats_decode_and_reject_incomplete_descriptors` tests authored bounded PCM descriptors and exact decoded samples, partial/truncated descriptors and invalid block alignment. No claim is made about an unavailable library's exactWAV header.
