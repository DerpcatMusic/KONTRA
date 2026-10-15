---
name: KONTRA Public Website
description: Public home, downloads, support evidence, and roadmap for the KONTRA player.
colors:
  muted-pink-accent: "#e6b6c7"
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
    lineHeight: 1.15
    letterSpacing: "-0.035em"
  page-title:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "clamp(36px, 4vw, 52px)"
    fontWeight: 400
    lineHeight: 1.15
    letterSpacing: "-0.035em"
  headline:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "26px"
    fontWeight: 400
    lineHeight: 1.15
    letterSpacing: "-0.025em"
  title:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "18px"
    fontWeight: 400
    lineHeight: 1.15
  body:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "16px"
    lineHeight: 1.65
  reading:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "15px"
  navigation:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "14px"
  download:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "15px"
  small:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "12px"
  metadata:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "13px"
rounded:
  square: "0px"
components:
  platform-download:
    backgroundColor: "{colors.graphite-background}"
    textColor: "{colors.text-primary}"
    typography: "{typography.download}"
    rounded: "{rounded.square}"
    padding: "14px 12px"
  platform-download-hover:
    backgroundColor: "{colors.graphite-hover}"
  current-navigation:
    textColor: "{colors.text-primary}"
    typography: "{typography.navigation}"
    rounded: "{rounded.square}"
  documentation-notice:
    textColor: "{colors.text-muted}"
    typography: "{typography.metadata}"
---

# Design System: KONTRA Public Website

## Overview

**Creative North Star: "A Native Instrument's Public Home"**

This website extends the native graphite and Noto Sans system recorded in the root [DESIGN.md](../DESIGN.md). The native editor remains the authority for the application; this document records the web reading and download surface. Graphite fills and square edges carry through, while the website uses a muted pink accent for links, focus, active navigation, and platform detection.

The overview keeps the message short, puts the current release identity and three platform downloads before an uncropped image of the actual player, and then points to installation, support evidence, and the roadmap. The documentation pages use the same wordmark, navigation, and footer so technical detail stays part of one site.

**Key Characteristics:**
- Dark graphite surfaces with thin dividers and quiet neutral text.
- A restrained pink accent that marks interaction and partial support.
- A bundled Noto Sans face and square page controls.
- The real player screenshot as the main product proof, followed by practical reference pages.

**The Native System Extension Rule.** Preserve the native editor's graphite, Noto Sans, square-edge identity; apply website-specific type scale and responsive reading layout only to this web surface.

## Colors

### Primary
- **Muted Pink Accent:** marks link hover, active navigation, visible focus, detected desktop downloads, and partial support.

### Neutral
- **Graphite Background, Surface, Field, and Hover:** distinguish the page, download cards, inline code, and interactive card state.
- **Primary Text, Muted Text, and Reading Copy:** keep the hero and navigation clear while long compatibility notes remain easy to scan.
- **Hairline and Scrollbar Thumb:** give page bands, tables, and scroll regions a quiet boundary.

### Status
- **Implemented Status:** a soft green marks implemented support. Partial support shares the pink accent; missing and unverified entries stay muted.

**The Pink Signal Rule.** Use the website's pink accent for interaction, active state, focus, and partial support; do not turn it into a decorative wash.

**The Graphite Continuity Rule.** Keep the shared graphite palette as the page foundation and let the actual application image supply its own color.

## Typography

Noto Sans is bundled at `assets/noto-sans.woff2` and carries the wordmark, headings, navigation, download links, and reading copy. Overview and reading pages share one restrained title scale. Redundant page subtitles are omitted. Workspace terms use a compact definition list instead of separate subheadings; roadmap headings are short and keep their full planning prose. Article paragraphs and table details use a denser reading size, with a small reduction on narrow screens. Inline `code` keeps the browser's monospace face on a dark inset fill.

**The One Face Rule.** Keep the bundled Noto Sans for the site's normal text and avoid adding a second display face.

## Layout

The content frame stops at 1240px. Side gutters step from 40px to 24px below 760px and 20px below 480px. The desktop header keeps the wordmark left, page navigation in the middle, and Patreon and Discord at the far right. Below 1050px the page navigation moves to a second row; on small phones the two community actions stack at the top right. The same actions repeat at the right of the shared footer on all four pages. They remain at least 44px tall.

The overview pairs one short introduction with a compact download list, followed immediately by the actual player image. Three flat download rows name the platform and architecture; avoid large download tiles. The format line appears once, beneath the downloads. A concise alpha caveat stays beside the introduction. On smaller screens, introduction and downloads stack.

Documentation and roadmap pages use a 190px table of contents beside the article with a 64px gap. Below 760px the table of contents wraps above the article. General wide tables remain horizontally scrollable. Compatibility rows instead reflow below 760px: feature and status share the first line, while scope and gaps span the full row below. Keep those caveats visible.

The overview fetches the latest GitHub release tag and published date when available; the direct release and asset links work without JavaScript, with a release-page fallback if the lookup fails. Desktop Windows, macOS, and Linux detection marks one download card. Mobile browsers, including iPad/iOS and Android, remain unmarked.

**The Download First Rule.** Keep the release identity and all three platform choices ahead of the large player image.

**The Visible Scope Rule.** Preserve the support table's feature, status, and full scope text in its mobile layout.

## Elevation & Depth

The site is flat. Graphite fills distinguish the page and inline code; hairlines divide the header, footer, download rows and compatibility rows. Reading sections use space rather than repeated rules. There is no shadow vocabulary.

**The Flat Plane Rule.** Use tonal fills, spacing, and hairlines for hierarchy; keep cards on the same plane as the page.

## Shapes

Page surfaces, download rows, community actions and disclosures have square corners. The detected platform uses a fine pink bottom rule, and links and summaries use a visible offset focus outline. No rounded control style is present in the website CSS.

**The Square Edge Rule.** Keep controls and containers rectangular; reserve the accent outline for an actual selected or focused state.

## Components

### Header and Navigation
- A compact wordmark, four page links and the two community actions repeat on every page. “Compatibility” names the audited feature table; installation and troubleshooting live in Documentation. The current page uses primary text and a thin pink underline; other links use muted text.
- On mobile, page links wrap below the wordmark and right-aligned community actions. Source, releases and contributor links stay in the shared footer.

### Platform Downloads
- Three text-led links cover Windows, macOS, and Linux. Each row names the platform and package architecture; the macOS note identifies both processor families.
- Hover changes the row fill. Desktop platform detection adds a thin accent rule and a short “Your platform” label. Unknown and mobile platforms keep all three links equally available.

### Product Screenshot
- Show the supplied player screenshot wide and uncropped directly after the download area. Keep its caption beneath it; do not place the image inside a decorative frame.
- The screenshot remains still. It supplies product detail without an entrance animation.

### Documentation and Support
- Long instructions use concise headings, a compact in-page contents column, plain tables, a workspace definition list, and native `details` / `summary` disclosures for installation steps, troubleshooting, and grouped support evidence.
- Support groups use text status labels for implemented, partial, missing, and unverified behavior. On mobile, keep the feature and status together and the scope/gap text on its own line.
- The roadmap is a long-form page with a clear heading hierarchy and complete prose sections; it is not presented as a closed or abbreviated accordion.

### Notices
- The documentation warning is compact text above installation, without a separate panel. Inline code uses a darker inset fill and a small monospace face.

**The Real Player Rule.** Let the actual application screenshot carry the product's visual detail; keep the surrounding web chrome quiet.

**The Evidence Stays Open Rule.** Keep support status and its scope visible in each row, including at mobile widths.

## Do's and Don'ts

### Do:
- **Do** keep the website an extension of the native graphite and Noto Sans system.
- **Do** lead the overview with the current release and all three platform downloads.
- **Do** use the real, uncropped player screenshot as product proof.
- **Do** keep installation guidance, support detail, and roadmap sections available through the shared page shell.
- **Do** retain the visible scope and gaps beside support statuses on every screen size.

### Don't:
- **Don't** create a separate palette, rounded-card system, or shadow-heavy layer.
- **Don't** replace the player screenshot with stock artwork or a fabricated interface.
- **Don't** use the pink accent as a large background or as a compatibility success color.
- **Don't** hide support gaps behind hover, tooltips, or mobile-only horizontal scrolling.
- **Don't** describe roadmap sections as collapsible; the shipped page presents them as headings and prose.
