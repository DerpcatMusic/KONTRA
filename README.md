# KONTRA

**One free, open-source sampler for the libraries you paid for.**

KONTRA is a plugin that plays Kontakt and UVI / Falcon sample libraries, aiming to sound and behave like the originals and then to do more.

<!-- derpcat-support -->
<p align="center">
  <a href="https://www.patreon.com/derpcatmusic">
    <img src=".github/support-derpcat.svg" alt="Donate on Patreon" width="480">
  </a>
</p>
<!-- /derpcat-support -->

[Website](https://derpcatmusic.github.io/KONTRA/) · [Progress](https://derpcatmusic.github.io/KONTRA/progress.html) · [Releases](https://github.com/DerpcatMusic/KONTRA/releases) · [Roadmap](ROADMAP.md)

## Downloads

[![Download for Windows x64](https://img.shields.io/badge/Download-Windows%20x64-0078D4?style=for-the-badge)](https://github.com/DerpcatMusic/KONTRA/releases/latest/download/KONTRA-nightly-windows-x86_64.zip)
[![Download for macOS, universal](https://img.shields.io/badge/Download-macOS%20Universal-222222?style=for-the-badge&logo=apple&logoColor=white)](https://github.com/DerpcatMusic/KONTRA/releases/latest/download/KONTRA-nightly-macos-universal.pkg)
[![Download for Linux x64](https://img.shields.io/badge/Download-Linux%20x64-E8A317?style=for-the-badge&logo=linux&logoColor=white)](https://github.com/DerpcatMusic/KONTRA/releases/latest/download/KONTRA-nightly-linux-x86_64.zip)

A new release most days · CLAP and VST3 · [All releases and what changed in each](https://github.com/DerpcatMusic/KONTRA/releases)

**Alpha software.** It loads and plays many Kontakt and UVI libraries, some of them closely, none of them perfectly yet. Expect wrong sounds, dead controls and changes between releases; it is not yet a replacement for Kontakt or Falcon.

## Where it stands

**[Progress](https://derpcatmusic.github.io/KONTRA/progress.html)** shows it with numbers: for each test library, how many of the same notes, chords and phrases sound the same in KONTRA as in Kontakt, and what differs in the ones that do not. The same page lists the open problems and the latest releases.

| | Today |
| :--- | :--- |
| Kontakt libraries | Load and play; sound is measured against Kontakt per library on the Progress page |
| Instrument panels and scripts | Each library's own interface is drawn and scripted; not every control works yet |
| Effects | Many Kontakt effects are modelled; some are still passed through unprocessed |
| UVI / Falcon libraries | Sample playback, scripts and interfaces work in part; sound is not yet measured |
| Plugin | CLAP and VST3 on Windows, macOS and Linux |

Feature by feature: [compatibility table](https://derpcatmusic.github.io/KONTRA/features.html).

## Something is wrong?

[Open an issue](https://github.com/DerpcatMusic/KONTRA/issues/new/choose) with the library, what you did and what you heard or saw. Questions and ideas go in [Discussions](https://github.com/DerpcatMusic/KONTRA/discussions).

## How it is built

One engine plays everything; each format is translated into the same description of an instrument. [docs/ENGINE.md](docs/ENGINE.md) explains the design and [docs/architecture-v3/VISION.md](docs/architecture-v3/VISION.md) why.

Every change arrives as a pull request whose title says what changed in plain words; a release is cut most days, numbered `3.MINOR.PATCH`, with notes that credit each change to its pull request and author. Details in [CONTRIBUTING.md](CONTRIBUTING.md#pull-requests-commits-and-releases).

[Contribute](CONTRIBUTING.md)

[Apache-2.0](LICENSE). Bring your own legally usable libraries; purchase alone may not authorize third-party playback. Parser redistribution rights remain unresolved. Independent of NI/UVI. [Legal details](docs/LEGAL.md).
