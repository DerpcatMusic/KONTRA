# KONTRA public site

Static HTML/CSS/JavaScript, published to GitHub Pages by `.github/workflows/pages.yml` on **every push to main**. The public pages need no browser API token.

Build and preview from the checked-out revision (choose an output directory on the dev drive):

```sh
python3 website/build_site.py /mnt/Windows11/DEV_WORKSPACE/RunScratch/kontra-site
# Activate emsdk 6.0.12 and install the wasm32-unknown-emscripten Rust target first.
python3 -m http.server 4173 --directory /mnt/Windows11/DEV_WORKSPACE/RunScratch/kontra-site
```

The build copies public assets and renders `features.html` with the recorded compatibility observations and embedded data. JavaScript adds search, sampler/category/status filters, semantic status sorting and 25-row pagination. Without JavaScript, the recorded observations and source links remain available. Filtered views can be linked with `?sampler=Kontakt` or `?sampler=UVI`, plus `category`, `status` and `q`. The same data is published as `compatibility.json`. The old `/tracker/` URL redirects to the table.

## Compatibility data

`build_site.py` reads the committed `tools/coverage/v3-facts.json` snapshot: the v3 load census, KSP census and Kontakt/UVI law audit. The snapshot records its source and scope. Results are partial, dated 29 October 2026 and may be stale; they do not establish whole-library compatibility. No private library paths or program names are published.

`ROADMAP.md` and `roadmap.html` record the current sampler priorities: Kontakt and UVI/Falcon in parallel; Opus/EastWest, HALion, HISE and other sampler frontends next. Keep those two files synchronized. Historical architecture checkpoints do not override these priorities.

## Validation

```sh
python3 website/check.py
python3 website/test_build_site.py
node website/test-compatibility.cjs
node website/test-releases.cjs
python3 website/check.py /mnt/Windows11/DEV_WORKSPACE/RunScratch/kontra-site
```

Only public HTML, CSS, JavaScript, generated data and assets are deployed. Design context, checks and build scripts stay out of the artifact. Font and icon provenance/license files remain alongside assets.

The homepage queries GitHub's latest published release for its version/date and highlights the detected desktop platform. Direct downloads and release-history links work without JavaScript. API failure leaves the release-page link available. Update download asset names if release packaging changes.
