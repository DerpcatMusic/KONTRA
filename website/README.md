# KONTRA public site

Buildless HTML/CSS, published by `.github/workflows/pages.yml` to GitHub Pages. No package installation is required.

Preview: `python3 -m http.server 4173 --directory website`.

Check: `python3 website/check.py`.

`ROADMAP.md` is the public planning document. Keep the corresponding prose in `website/roadmap.html` synchronized when changing it. Downloads target GitHub's latest release asset names. Update the links if release packaging changes. Technical documentation links resolve to the main branch; inspect the linked review dates before updating compatibility claims.

Only HTML, CSS and assets are deployed. Design briefs, checks and project context are excluded. Asset origin and font license are retained alongside public assets.

The homepage queries GitHub's latest published release for its version/date and highlights the detected desktop platform; all three downloads and the release-history link work without JavaScript. API failure leaves a direct release link. Test this with `node website/test-releases.cjs`.

`features.html` mirrors all 147 rows in the repository's dated `docs/FEATURES.md` audit. Update the audit date, pinned evidence and page together; do not label it live certification.
