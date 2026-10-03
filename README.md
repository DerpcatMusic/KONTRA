# KONTRA

**One free, open-source sampler for the libraries you paid for.**

Written in Rust with a custom UI framework. The goal: play Kontakt and UVI Falcon (UFS) libraries in one sampler, on Windows, macOS and Linux.

Runs as **CLAP, VST3 or standalone**. Experimental: compatibility varies by library.

## What works, what’s next

| Feature | ✓ Implemented | ◇ Missing or incomplete |
| :--- | :--- | :--- |
| **Kontakt** | NKI instruments, NKM multis, supported snapshots | Full library compatibility and exact sound matching |
| **Samples & playback** | WAV, AIFF, NCW, disk streaming, multiple outputs | Some protected libraries and sample formats |
| **KSP scripting** | Core note, controller and UI callbacks | Complete KSP API and file-loading support |
| **Instrument UI** | Browser, mapping, mixer, imported artwork and controls | Some widgets, live meters and Komplete UI |
| **Effects** | Selected filters, EQ, reverb, convolution and modulation | Full effect coverage and exact Kontakt behavior |
| **MIDI & expression** | Pedals, articulations, channel routing, both MPE zones | Full MPE negotiation and broader library testing |
| **Create & save** | Sample mapping, KONTRA/SFZ export, limited NKI writing | Arbitrary imported-preset editing and NKM creation |
| **REAPER migration** | Explicit instrument mappings into a project copy | Kontakt state and automation translation |
| **Falcon / UFS** | Header and metadata inspection | Playback, sample access, scripts, UI and effects |

Implemented does not mean complete or verified in every DAW.

## Download

Experimental nightlies: [Windows](https://github.com/DerpcatMusic/KONTRA/releases/latest/download/KONTRA-nightly-windows-x86_64.zip) · [macOS](https://github.com/DerpcatMusic/KONTRA/releases/latest/download/KONTRA-nightly-macos-universal.pkg) · [Linux](https://github.com/DerpcatMusic/KONTRA/releases/latest/download/KONTRA-nightly-linux-x86_64.zip)

Windows/Linux x64 plug-ins require AVX2, FMA and BMI2.

## Legal

Bring your own libraries; none are included. Use them only as their licenses and local law allow. Buying a library does not automatically permit using its protected content in another sampler.

Project code is [Apache-2.0](LICENSE). Third-party components have their own terms; redistribution permission for the required `ni-file` parser remains unresolved. See [third-party notices](THIRD_PARTY.md) and the [legal review](docs/LEGAL.md).

KONTRA is independent of Native Instruments and UVI. Their trademarks belong to their owners. Provided as is, without warranty, under the license.

[Build & contribute](CONTRIBUTING.md) · [Changes](CHANGELOG.md) · [Report a bug](https://github.com/DerpcatMusic/KONTRA/issues)
