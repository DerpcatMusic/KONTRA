# KONTRA public site

Static HTML/CSS/JavaScript, published to GitHub Pages by `.github/workflows/pages.yml` on **every push to main**. The public pages need no browser API token.

Build and preview from the checked-out revision (choose an output directory on the dev drive):

```sh
python3 website/build_site.py /mnt/Windows11/DEV_WORKSPACE/RunScratch/kontra-site
# Activate emsdk 6.0.12 and install the wasm32-unknown-emscripten Rust target first.
python3 -m http.server 4173 --directory /mnt/Windows11/DEV_WORKSPACE/RunScratch/kontra-site
```

The build copies public assets and renders `features.html` (Compatibility) from the parity board: `tools/parity-board.py` writes `docs/PARITY.html`, and its board fragment goes into the site's chrome. It shows headline numbers, collapsible categories per sampler, each feature's status, its change badges and proof, with search and sampler and status filters. The board is the one compatibility list; there is no other. The old `/tracker/` and `parity.html` URLs redirect to it.

`ROADMAP.md` and `roadmap.html` record the current sampler priorities: Kontakt and UVI/Falcon in parallel; Opus/EastWest, HALion, HISE and other sampler frontends next. Keep those two files synchronized. Historical architecture checkpoints do not override these priorities.

## Validation

```sh
python3 website/check.py
python3 website/test_build_site.py
node website/test-releases.cjs
python3 website/check.py /mnt/Windows11/DEV_WORKSPACE/RunScratch/kontra-site
```

Only public HTML, CSS, JavaScript, generated data and assets are deployed. Design context, checks and build scripts stay out of the artifact. Font and icon provenance/license files remain alongside assets.

The homepage queries GitHub's latest published release for its version/date and highlights the detected desktop platform. Direct downloads and release-history links work without JavaScript. API failure leaves the release-page link available. Update download asset names if release packaging changes.
