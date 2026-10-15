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
  phase-marker: "#818181"
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
  phase-heading-mobile:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "24px"
    fontWeight: 400
    lineHeight: 1.2
    letterSpacing: "-0.02em"
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
  footer-legal:
    textColor: "{colors.text-muted}"
    typography: "{typography.small}"
    rounded: "{rounded.square}"
---

# Design System: KONTRA Public Website

## Overview

**Creative North Star: "A Native Instrument's Public Home"**

This website extends the native graphite and Noto Sans system recorded in the root [DESIGN.md](../DESIGN.md). The native editor remains the authority for the application; this document records the web reading and download surface. Graphite fills and square edges carry through, while the website uses muted pink for interaction and donations, recognizable Discord blue for community access, and a light primary download for the detected desktop platform.

The overview keeps the message short and puts the release identity and three icon-led downloads before an uncropped image of the actual player. Shared header and footer actions make donations and community access prominent. Reading pages retain essential instructions and evidence; the roadmap uses a quiet connected phase sequence with complete implementation detail in native disclosures.

**Key Characteristics:**
- Dark graphite surfaces with thin dividers and quiet neutral text.
- Purposeful pink donation actions, Discord blue, and light detected downloads on graphite.
- A bundled Noto Sans face and square page controls.
- The real player screenshot as product proof, concise reading, and expandable engineering scope.

**The Native System Extension Rule.** Preserve the native editor's graphite, Noto Sans, square-edge identity; apply website-specific type scale and responsive reading layout only to this web surface.

## Colors

### Primary
- **Muted Pink Accent:** marks link hover, active navigation, visible focus, the first roadmap phase, partial support, and the solid Patreon donation action. Donation Hover brightens that action.

### Secondary
- **Discord Blue and Discord Hover:** identify the solid community action; Action White supplies its text and icon contrast.

### Neutral
- **Graphite Background, Surface, Field, and Hover:** distinguish the page, download cards, inline code, and interactive card state.
- **Primary Text, Muted Text, and Reading Copy:** keep the hero and navigation clear while long compatibility notes remain easy to scan.
- **Hairline and Scrollbar Thumb:** give page bands, tables, and scroll regions a quiet boundary.
- **Action Ink and Primary Text:** form the dark-on-light detected download; Detected Metadata keeps its architecture note readable. Action White is its hover fill.
- **Download Border and Download Hover:** distinguish alternative OS actions; Phase Marker outlines later priority nodes.

### Status
- **Implemented Status:** a soft green marks implemented support. Partial support shares the pink accent; missing and unverified entries stay muted.

**The Pink Signal Rule.** Use pink for donation actions, interaction, focus, first-phase priority, and partial support; keep the page foundation graphite.

**The Graphite Continuity Rule.** Keep the shared graphite palette as the page foundation and let the actual application image supply its own color.

## Typography

Noto Sans is bundled at `assets/noto-sans.woff2` and carries the wordmark, headings, navigation, actions, and reading copy. The frontmatter records the implemented type roles. Titles use the fluid display scale; section headings use Headline, with phase headings stepping to the mobile role below 760px. Body copy reads at 16px/1.65; articles use Reading, roadmap implementation detail uses Technical, and concise goals and metadata use Metadata. Inline `code` keeps the browser's monospace face on a dark inset fill.

**The One Face Rule.** Keep the bundled Noto Sans for the site's normal text and avoid adding a second display face.

## Layout

The content frame stops at 1240px. Side gutters step from 40px to 24px below 760px and 20px below 480px. The desktop header keeps the wordmark left, page navigation in the middle, and Patreon and Discord at the far right. Below 1050px the page navigation moves to a second row; on small phones the two community actions stack at the top right. The same actions repeat at the right of the shared footer on all four pages. They remain at least 44px tall.

The overview pairs one short introduction with a compact download list, followed immediately by the actual player image. Three bordered, square icon buttons name the platform and architecture; the detected desktop button gets a light fill while alternatives stay graphite. The format line appears once, beneath the downloads. A concise alpha caveat stays beside the introduction. On smaller screens, introduction and downloads stack.

Documentation uses a 190px table of contents beside the article with a 64px gap. Below 760px the table of contents wraps above the article. General wide tables remain horizontally scrollable. Compatibility rows instead reflow below 760px: feature and status share the first line, while scope and gaps span the full row below. Keep those caveats visible when a support group is expanded.

The roadmap is a centered 1000px connected sequence. Its 110px order column and 36px gap reduce to 86px and 24px below 760px. Below 480px, the order moves above the title and goals within one column; the fine connector stays at the left. Phase goals stack below 760px. The first node uses pink; later nodes are outlined. These indicate priority order, not completion or dates.

The overview fetches the latest GitHub release tag and published date when available; the direct release and asset links work without JavaScript, with a release-page fallback if the lookup fails. Desktop Windows, macOS, and Linux detection gives one download button the light primary treatment and an assistive “Your platform” label. Mobile browsers, including iPad/iOS and Android, and unknown platforms remain unmarked. With JavaScript disabled, all three download links remain available, community links and SVG masks still render, and native disclosures remain operable.

**The Download First Rule.** Keep the release identity and all three platform choices ahead of the large player image.

**The Visible Scope Rule.** Preserve the support table's feature, status, and full scope text in its mobile layout.

## Elevation & Depth

The site is flat. Graphite fills, solid action colors, light selected downloads, and hairlines distinguish hierarchy without shadows. The roadmap connector is a one-pixel line. Reading sections use space rather than repeated frames. The product screenshot remains still; action state transitions last 150ms with ease and are removed for reduced motion, which also disables smooth scrolling.

**The Flat Plane Rule.** Use tonal fills, spacing, and hairlines for hierarchy; keep cards on the same plane as the page.

## Shapes

Page surfaces, download buttons, community actions and disclosures have square corners. The roadmap’s small circular sequence nodes are a specific exception, not rounded controls. Links, summaries and focusable table regions use a two-pixel pink focus outline with a five-pixel offset. The detected platform uses a light fill rather than a pink rule.

**The Square Edge Rule.** Keep controls and containers rectangular; use circular markers only for the roadmap sequence and an accent outline for keyboard focus.

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

### Documentation and Support
- Concise headings and a compact in-page contents column lead installation, library use, and recovery guidance. Preserve practical caveats; remove redundant inferred explanation.
- Native `details` / `summary` provide installation, troubleshooting, developer references and grouped compatibility evidence. Windows installation starts open. Summaries retain browser disclosure markers, a minimum 44px hit target and visible keyboard focus.
- Compatibility uses explicit implemented, partial, missing and unverified text statuses. On mobile, feature and status share the first line, with complete scope and gaps below; opening a group exposes the same evidence at every width.

### Phase Roadmap
- A quiet connected sequence presents First, Next, Then and Integration, each with a short heading and three goals. It communicates priority, without percentages, completion badges or invented dates.
- Each phase keeps full implementation prose, source links and exit evidence under a native “Technical scope” disclosure. Planning scope, current position, release criteria and deferred work remain in disclosures beneath the sequence; the planning snapshot and engineering roadmap link remain visible.
- Phase headings use Headline on desktop and the mobile phase role below 760px. Technical disclosures use muted 13px summaries and 14px reading prose. No disclosure animation or custom JavaScript is required.

### Shared Footer and Legal
- Repeat the brand and strong community actions above source, releases, issues, contribution and license links. Hairlines and spacing define the footer rather than a panel.
- “Legal” is a right-aligned native disclosure with 12px text, no border, and a 44px summary target. Its full independence, third-party ownership, library-rights and provenance text stays available without JavaScript.

**The Real Player Rule.** Let the actual application screenshot carry the product's visual detail; keep the surrounding web chrome quiet.

**The Evidence Stays Open Rule.** Within expanded support groups, keep status and its full scope visible together in each row, including at mobile widths.

## Do's and Don'ts

### Do:
- **Do** keep the website an extension of the native graphite and Noto Sans system.
- **Do** lead the overview with the current release and all three platform downloads.
- **Do** use the real, uncropped player screenshot as product proof.
- **Do** preserve full evidence and phase implementation scope in keyboard-native disclosures.
- **Do** use pinned real service and OS SVGs with text labels and working static links.
- **Do** retain the visible scope and gaps beside support statuses on every screen size.

### Don't:
- **Don't** create a separate palette, rounded-card system, or shadow-heavy layer.
- **Don't** replace the player screenshot with stock artwork or a fabricated interface.
- **Don't** spread action colors into decorative page backgrounds or use pink as a compatibility success color.
- **Don't** hide support gaps behind hover, tooltips, or mobile-only horizontal scrolling.
- **Don't** turn the priority sequence into a completion chart or a promised release schedule.
