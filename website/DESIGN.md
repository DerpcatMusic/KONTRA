---
name: KONTRA Public Website
description: Public home, downloads, support evidence, and roadmap for the KONTRA player.
colors:
  muted-pink-accent: "#e6b6c7"
  donation-hover: "#f5cad7"
  discord-blue: "#5865f2"
  discord-hover: "#4752c4"
  action-ink: "#171719"
  action-white: "#fff"
  download-border: "#535353"
  download-hover: "#3b3b3b"
  detected-metadata: "#454549"
  table-row-hover: "#222"
  graphite-background: "#1b1b1b"
  graphite-surface: "#252525"
  graphite-field: "#101010"
  graphite-hover: "#303030"
  text-primary: "#e8e8e8"
  text-muted: "#b1b1b1"
  reading-copy: "#c7c7c7"
  hairline: "#414141"
  status-implemented: "#c7d8c7"
  scrollbar-thumb: "#686868"
typography:
  display:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "clamp(36px, 4vw, 52px)"
    fontWeight: 400
    lineHeight: 1.2
    letterSpacing: "-0.03em"
  page-title:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "clamp(36px, 4vw, 52px)"
    fontWeight: 400
    lineHeight: 1.2
    letterSpacing: "-0.03em"
  headline:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "26px"
    fontWeight: 400
    lineHeight: 1.2
    letterSpacing: "-0.02em"
  title:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "18px"
    fontWeight: 400
    lineHeight: 1.2
  body:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "16px"
    lineHeight: 1.65
  reading:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "15px"
  navigation:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "13px"
  download:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "15px"
  small:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "12px"
  metadata:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "13px"
  sampler-band-heading:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "24px"
    fontWeight: 400
    lineHeight: 1.2
    letterSpacing: "-0.02em"
  table-cell:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "13px"
    lineHeight: 1.6
  table-header:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "12px"
    fontWeight: 400
  technical:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "14px"
    lineHeight: 1.65
rounded:
  square: "0px"
spacing:
  action-gap: "10px"
  control-inline: "16px"
  phone-gutter: "20px"
  tablet-gutter: "24px"
  desktop-gutter: "40px"
  filter-gap: "12px"
  table-cell-block: "16px"
  table-cell-inline: "14px"
components:
  community-donate:
    backgroundColor: "{colors.muted-pink-accent}"
    textColor: "{colors.action-ink}"
    rounded: "{rounded.square}"
    padding: "10px 16px"
  community-donate-hover:
    backgroundColor: "{colors.donation-hover}"
  community-discord:
    backgroundColor: "{colors.discord-blue}"
    textColor: "{colors.action-white}"
    rounded: "{rounded.square}"
    padding: "10px 16px"
  community-discord-hover:
    backgroundColor: "{colors.discord-hover}"
  platform-download:
    backgroundColor: "{colors.graphite-hover}"
    textColor: "{colors.text-primary}"
    typography: "{typography.download}"
    rounded: "{rounded.square}"
    padding: "14px 16px"
  platform-download-hover:
    backgroundColor: "{colors.download-hover}"
  platform-download-detected:
    backgroundColor: "{colors.text-primary}"
    textColor: "{colors.action-ink}"
  platform-download-detected-hover:
    backgroundColor: "{colors.action-white}"
  current-navigation:
    textColor: "{colors.text-primary}"
    typography: "{typography.navigation}"
    rounded: "{rounded.square}"
  technical-disclosure:
    textColor: "{colors.text-muted}"
    typography: "{typography.metadata}"
    rounded: "{rounded.square}"
  quiet-button:
    backgroundColor: "{colors.graphite-surface}"
    textColor: "{colors.text-primary}"
    typography: "{typography.metadata}"
    rounded: "{rounded.square}"
    padding: "8px 14px"
  quiet-button-hover:
    backgroundColor: "{colors.graphite-hover}"
  quiet-button-disabled:
    backgroundColor: "{colors.graphite-background}"
    textColor: "{colors.text-muted}"
  filter-field:
    backgroundColor: "{colors.graphite-field}"
    textColor: "{colors.text-primary}"
    typography: "{typography.metadata}"
    rounded: "{rounded.square}"
    padding: "8px 10px"
    height: "44px"
  feature-table-header:
    backgroundColor: "{colors.graphite-surface}"
    textColor: "{colors.text-muted}"
    typography: "{typography.table-header}"
    rounded: "{rounded.square}"
  feature-table-row-hover:
    backgroundColor: "{colors.table-row-hover}"
  sampler-row:
    textColor: "{colors.text-primary}"
    rounded: "{rounded.square}"
    padding: "20px 0"
  footer-legal:
    textColor: "{colors.text-muted}"
    typography: "{typography.small}"
    rounded: "{rounded.square}"
---

# Design System: KONTRA Public Website

## Overview

**Creative North Star: "A Native Instrument's Public Home"**

This website extends the native graphite and Noto Sans system recorded in the root [DESIGN.md](../DESIGN.md). The native editor remains the authority for the application; this document records the web reading and download surface. Graphite fills and square edges carry through, while the website uses muted pink for interaction and donations, recognizable Discord blue for community access, and a light primary download for the detected desktop platform.

The overview keeps the message short and puts the release identity and three icon-led downloads before an uncropped image of the actual player. Shared header and footer actions make donations and community access prominent. Reading pages retain essential instructions and evidence. Compatibility uses a source-built feature table with visible status and implementation notes; the roadmap groups parallel sampler work and planned expansion into quiet, unnumbered bands.

**Key Characteristics:**
- Dark graphite surfaces with thin dividers and quiet neutral text.
- Purposeful pink donation actions, Discord blue, and light detected downloads on graphite.
- A bundled Noto Sans face and square page controls.
- The real player screenshot as product proof, concise reading, and expandable engineering scope.

**The Native System Extension Rule.** Preserve the native editor's graphite, Noto Sans, square-edge identity; apply website-specific type scale and responsive reading layout only to this web surface.

## Colors

### Primary
- **Muted Pink Accent:** marks link hover, active navigation, visible focus, partial support, and the solid Patreon donation action. Donation Hover brightens that action.

### Secondary
- **Discord Blue and Discord Hover:** identify the solid community action; Action White supplies its text and icon contrast.

### Neutral
- **Graphite Background, Surface, Field, and Hover:** distinguish the page, download cards, inline code, and interactive card state.
- **Primary Text, Muted Text, and Reading Copy:** keep the hero and navigation clear while long compatibility notes remain easy to scan.
- **Hairline and Scrollbar Thumb:** give page bands, tables, and scroll regions a quiet boundary.
- **Action Ink and Primary Text:** form the dark-on-light detected download; Detected Metadata keeps its architecture note readable. Action White is its hover fill.
- **Download Border and Download Hover:** distinguish alternative OS actions. Table Row Hover provides a restrained reading highlight.

### Status
- **Implemented Status:** a soft green marks implemented support. Partial support shares the pink accent; Not implemented stays muted. Partial rows show incomplete, unverified or mismatched mappings in explicit text, including a smaller muted proof note when available.

**The Pink Signal Rule.** Use pink for donation actions, interaction, focus, and partial support; keep the page foundation graphite.

**The Graphite Continuity Rule.** Keep the shared graphite palette as the page foundation and let the actual application image supply its own color.

## Typography

Noto Sans is bundled at `assets/noto-sans.woff2` and carries the wordmark, headings, navigation, actions, and reading copy. The frontmatter records the implemented type roles. Titles use the fluid display scale; section headings use Headline, while sampler band headings use their own 24px role at every width. Body copy reads at 16px/1.65; articles use Reading, expanded sampler scope uses Technical, and revision lines, filters and row metadata use Metadata. The table uses 13px/1.6 cells, 12px headers and proof notes, and tabular numerals for counts, result totals and pagination. Inline `code` keeps the browser's monospace face on a dark inset fill.

**The One Face Rule.** Keep the bundled Noto Sans for the site's normal text and avoid adding a second display face.

## Layout

The content frame stops at 1240px. Side gutters step from 40px to 24px below 760px and 20px below 480px. The desktop header keeps the wordmark left, page navigation in the middle, and Patreon and Discord at the far right. Below 1050px the page navigation moves to a second row; on small phones the two community actions stack at the top right. The same actions repeat at the right of the shared footer on all four pages. They remain at least 44px tall.

The overview pairs one short introduction with a compact download list, followed immediately by the actual player image. Three bordered, square icon buttons name the platform and architecture; the detected desktop button gets a light fill while alternatives stay graphite. The format line appears once, beneath the downloads. A concise alpha caveat stays beside the introduction. On smaller screens, introduction and downloads stack.

Documentation uses a 190px table of contents beside the article with a 64px gap. Below 760px the table of contents wraps above the article. General wide tables remain horizontally scrollable.

Compatibility uses the full content frame. Its filter bar has five desktop columns: a search field with a 220px minimum and 1.5fr share, sampler (1fr), category (1.2fr), status (1fr), and a clear action. At 1050px it becomes three equal columns with search spanning two; at 760px it becomes two equal columns and search fills the row. The gap is 12px, becoming 16px vertically and 10px horizontally below 480px. Fields and quiet buttons remain at least 44px tall.

The six-column semantic feature table retains a 1040px minimum width inside a keyboard-focusable horizontal scroll region. Feature, Status, Sampler, Category, Implementation notes and Source occupy 22%, 15%, 8%, 15%, 33% and 7% on desktop. Below 760px, Feature becomes 17% and Category 20%, keeping Feature and Status at the left edge of the initial mobile view; the remaining evidence stays available by scrolling. Cells use 16px × 14px padding, with 48px minimum header buttons. Preserve table semantics at every width rather than converting rows into cards. Counts precede the controls, while result totals and the JSON download sit above the table and pagination sits below it.

The roadmap uses two unnumbered bands: Current work and Next samplers. On desktop, each band places a 210px heading column beside a row list with a 48px gap; rows use flexible sampler names, a 150px status column and a 110px link column, separated by 24px. At 1050px the band heading becomes 160px with a 32px gap, rows use a 130px status column, and links move beneath the name. Below 760px the band stacks with a 20px gap; below 480px the status column becomes 120px. Hairlines bound the list and divide rows, with 20px vertical row padding and 44px between desktop bands (32px on mobile).

The overview fetches the latest GitHub release tag and published date when available; the direct release and asset links work without JavaScript, with a release-page fallback if the lookup fails. Desktop Windows, macOS, and Linux detection gives one download button the light primary treatment and an assistive “Your platform” label. Mobile browsers, including iPad/iOS and Android, and unknown platforms remain unmarked. With JavaScript disabled, all three download links remain available, community links and SVG masks still render, and native disclosures remain operable.

**The Download First Rule.** Keep the release identity and all three platform choices ahead of the large player image.

**The Semantic Evidence Rule.** Keep all six compatibility columns in one semantic table at every width, with Feature and Status first and the complete notes and source reachable by horizontal scrolling.

## Elevation & Depth

The site is flat. Graphite fills, solid action colors, light selected downloads, and hairlines distinguish hierarchy without shadows. Reading sections use space rather than repeated frames. The product screenshot remains still; action state transitions last 150ms with ease and are removed for reduced motion, which also disables smooth scrolling.

**The Flat Plane Rule.** Use tonal fills, spacing, and hairlines for hierarchy; keep cards on the same plane as the page.

## Shapes

Page surfaces, download buttons, community actions and disclosures have square corners. Links, summaries and focusable table regions use a two-pixel pink focus outline with a five-pixel offset. Buttons, search fields and selects use the same outline with a three-pixel offset. Sort chevrons use two one-pixel borders rather than a text glyph. The detected platform uses a light fill rather than a pink rule.

**The Square Edge Rule.** Keep controls and containers rectangular, with an accent outline for keyboard focus.

## Components

### Header and Navigation
- A compact wordmark, four page links and the two community actions repeat on every page. “Compatibility” names the audited feature table; installation and troubleshooting live in Documentation. The current page uses primary text and a thin pink underline; other links use muted text.
- On mobile, page links wrap below the wordmark and right-aligned community actions. Source, releases and contributor links stay in the shared footer.

### Community Actions and Icons
- Donate uses the real Patreon mark on solid pink; Join Discord uses the real Discord mark on its blue fill. Icons are 20px; controls are at least 46px tall, with 10px × 16px padding and a 9px icon-to-label gap. On small-phone headers they become 44px tall with 9px × 12px padding.
- Repeat the same actions at the top right and footer right on every page. Below 480px the header actions stack; footer actions take a full-width row while keeping right alignment.
- The six unmodified Font Awesome Free 7.3.1 SVGs are pinned to commit `14c65a3747d0f3b751f15831fc719236aea8729d`. CSS masks use current text color. Keep local assets, attribution comments, `FONTAWESOME-LICENSE.txt`, and `assets/PROVENANCE.md`; do not substitute generated or text glyph icons.

### Platform Downloads
- Three square, bordered OS links cover Windows x64, macOS Apple Silicon + Intel, and Linux x64. Each has a 26px OS mark at the left and an 18px download icon at the right, with a 66px minimum height and 14px × 16px padding.
- Alternatives use Graphite Hover and Download Border; hover brightens the fill and border. Desktop detection gives one option Primary Text fill and Action Ink content, with a white hover state. The short recommendation is available to assistive technology rather than added visual copy.
- Keep release identity above the group and format text once beneath it. Mobile and unknown platforms keep all options equally available.

### Product Screenshot
- Show the supplied player screenshot wide and uncropped directly after the download area, linked to the full image. Keep surrounding chrome quiet.
- The screenshot remains still. It supplies product detail without an entrance animation.

### Documentation and Disclosures
- Concise headings and a compact in-page contents column lead installation, library use, and recovery guidance. Preserve practical caveats; remove redundant inferred explanation.
- Native `details` / `summary` provide installation, troubleshooting and developer references. Windows installation starts open. Summaries retain browser disclosure markers, a minimum 44px hit target and visible keyboard focus.

### Compatibility Table and Controls
- The generated page identifies the exact source commit and commit date above overall counts. Its native “How status is tracked” disclosure explains the claims without displacing the table. On every main commit, the generator reevaluates committed code probes, implementation ledgers and recorded verification without a private catalog cache. Code links are pinned to the same full source SHA; Download data exposes the generated JSON.
- Status labels remain explicit: Implemented requires a proved mapping; Partial includes incomplete, unverified or mismatched mappings; Not implemented denotes an absent implementation. These describe individual features rather than whole-library compatibility. Use soft green, pink and muted text respectively, with muted proof notes beneath Partial when available.
- Search covers feature text and notes; Sampler, Category and Status selects constrain the table. Quiet square fields use Graphite Field, a Scrollbar Thumb colored border, 12px labels and 13px values. Clear filters, Previous and Next use square Graphite Surface buttons; hover uses Graphite Hover and disabled buttons return to Graphite Background with muted text and hairline borders.
- The first four column headers contain full-width sort buttons with small geometric chevrons. The active column exposes `aria-sort`; inactive headers remain neutral. Feature is a row header, and a visually hidden caption names the table. Result totals use a polite live status; empty results provide a Clear filters action.
- The initial HTML contains every row and the JSON download remains a direct link without JavaScript. JavaScript reveals the controls, enables sorting, shows 25 rows per page and persists search and filter selections in the URL. Preserve the full static table as the baseline.

### Sampler Roadmap
- Current work lists Kontakt and UVI / Falcon as parallel development tracks, each with an In development label and a link to its filtered feature table. Next samplers lists Opus / EastWest, HALion, HISE and Other samplers as Planned. The bands express current focus and expansion without a fixed order, promised dates, completion charts or progress percentages.
- A compact native “Implementation scope” disclosure follows current sampler rows. Its muted 13px summary expands to 14px prose and source links. Keep the main sampler names and statuses visible before expansion.
- Quiet headings sit left of the hairline row lists on desktop and above them on mobile. The roadmap links beneath the bands lead to the implementation table, roadmap source and recent commits.

### Shared Footer and Legal
- Repeat the brand and strong community actions above source, releases, issues, contribution and license links. Hairlines and spacing define the footer rather than a panel.
- “Legal” is a right-aligned native disclosure with 12px text, no border, and a 44px summary target. Its full independence, third-party ownership, library-rights and provenance text stays available without JavaScript.

**The Real Player Rule.** Let the actual application screenshot carry the product's visual detail; keep the surrounding web chrome quiet.

**The Claim and Evidence Rule.** Keep each feature’s explicit status, implementation notes and source in the same row, and pin source links to the revision shown above the table.

## Do's and Don'ts

### Do:
- **Do** keep the website an extension of the native graphite and Noto Sans system.
- **Do** lead the overview with the current release and all three platform downloads.
- **Do** use the real, uncropped player screenshot as product proof.
- **Do** preserve complete table evidence and keyboard-native scope disclosures.
- **Do** use pinned real service and OS SVGs with text labels and working static links.
- **Do** retain all six evidence columns, explicit status labels and source links at every screen size.

### Don't:
- **Don't** create a separate palette, rounded-card system, or shadow-heavy layer.
- **Don't** replace the player screenshot with stock artwork or a fabricated interface.
- **Don't** spread action colors into decorative page backgrounds or use pink as a compatibility success color.
- **Don't** replace the mobile feature table with cards or hide its evidence behind hover-only controls.
- **Don't** turn sampler focus bands into a numbered sequence, completion chart or promised release schedule.
