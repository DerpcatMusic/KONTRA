# KONTRA

**One free, open-source sampler for the libraries you paid for.**

## Downloads

[![Download for Windows x64](https://img.shields.io/badge/Download-Windows%20x64-0078D4?style=for-the-badge)](https://github.com/DerpcatMusic/KONTRA/releases/latest/download/KONTRA-nightly-windows-x86_64.zip)
[![Download for macOS, Apple Silicon and Intel](https://img.shields.io/badge/Download-macOS%20Universal-222222?style=for-the-badge&logo=apple&logoColor=white)](https://github.com/DerpcatMusic/KONTRA/releases/latest/download/KONTRA-nightly-macos-universal.pkg)
[![Download for Linux x64](https://img.shields.io/badge/Download-Linux%20x64-E8A317?style=for-the-badge&logo=linux&logoColor=white)](https://github.com/DerpcatMusic/KONTRA/releases/latest/download/KONTRA-nightly-linux-x86_64.zip)

Nightly alpha builds · CLAP, VST3 and standalone · [All downloads & release notes](https://github.com/DerpcatMusic/KONTRA/releases/latest)

<details>
<summary>Portable macOS downloads</summary>

[Apple Silicon ZIP](https://github.com/DerpcatMusic/KONTRA/releases/latest/download/KONTRA-nightly-macos-arm64.zip) · [Intel ZIP](https://github.com/DerpcatMusic/KONTRA/releases/latest/download/KONTRA-nightly-macos-x86_64.zip)

</details>

**Alpha software, under heavy development.** Features, UI, compatibility and saved-state formats may change substantially between builds. Expect incomplete behavior and regressions; this is not yet a dependable replacement for Kontakt or Falcon.

Rust. Custom UI framework. Windows, macOS, Linux. CLAP, VST3, standalone.

| Feature | Works | Missing |
| :--- | :--- | :--- |
| Kontakt | Experimental playback | Full compatibility |
| Scripts, UI & effects | Partial support | Full coverage |
| Falcon / UFS | Metadata inspection | Playback |

## Feature status

See the **[full feature inventory](docs/FEATURES.md)** for ✓ implemented, ◐ partial and ✗ missing functionality across DSP, filters, scripting, UI, formats and routing. Each entry includes implementation evidence and remaining gaps, cross-referenced with official Kontakt, KSP, Falcon and UVIScript documentation.

[Contribute](CONTRIBUTING.md) · [2.0 architecture plan](docs/architecture-v2/README.md)

[Apache-2.0](LICENSE). Bring your own legally usable libraries; purchase alone may not authorize third-party playback. Parser redistribution rights remain unresolved. Independent of NI/UVI. [Legal details](docs/LEGAL.md).
