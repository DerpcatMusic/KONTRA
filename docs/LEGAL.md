# Licensing and interoperability review

Reviewed 2026-10-02 for an Israel-based publisher. This is a source-linked
technical and licensing review, not a legal opinion or permission from
Native Instruments. It does not establish that every library, use or
distribution is lawful.

## Project purpose and license

KONTRA aims to let musicians use instruments they are entitled to use in
an independent sampler, including on Linux. Native Instruments currently
lists Windows and macOS for Kontakt; Linux is not listed as a supported
platform. [Kontakt system requirements](https://www.native-instruments.com/products/kontakt).

Keep the standard Apache License 2.0 in LICENSE for project-authored code
and documentation, with attribution in NOTICE. It permits reuse and
distribution subject to its conditions, including notice preservation;
it cannot grant another owner's rights. Adding a custom "legal use only"
clause would change that standard grant rather than resolve third-party
rights. [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0).

## What the repository actually does

| Area | Finding | Consequence |
|---|---|---|
| Default builds | `Cargo.toml` enables `library-access`; the nightly workflow explicitly includes it. | Public builds include encrypted-content access, not just unencrypted parsing. |
| Local access data | `src/access.rs::library_key` reads `.nicnt` metadata and constructs a symmetric keystream. Import, audio and resource readers use it. | This is decryption capability, not a purchase, account or activation check. Local access data is not evidence of a user's license. |
| Third-party parser | `vendor/ni-file` is a required dependency with no explicit redistribution license found at the pinned upstream commit. | Project-authored Apache-2.0 licensing does not clear redistribution of the parser or binaries containing it. |
| Content supplied | No commercial Kontakt instrument library is supplied; the tree does contain vendored NCW/WAV codec fixtures and an OFL font. | Avoid blanket claims that the repository contains no third-party samples or assets. Fixture recording provenance still needs verification. |
| Local copies and exports | `src/cache.rs` stores instrument scripts and decoded impulse responses. Creator/state exports can contain library-derived data. | A cache, conversion or saved state does not acquire a new content license. Do not share those files merely because KONTRA produced them. |
| Format references | THIRD_PARTY.md records algorithm/constant overlaps with GPL-3.0 references. | Protectable copied expression and applicable license obligations need a provenance review; matching format facts alone do not decide this. |
| Release notices | Nightlies stage LICENSE, NOTICE, THIRD_PARTY.md, this review, OFL, generated dependency texts, unchanged MOOSE/MUI licenses and exact MPL source archives. | Notices address distribution obligations; they do not establish missing permission or lawful decryption. |

The `license_info.rs` file in the parser contains comments about format
properties; it is not an ownership verification implementation.
Disabling `library-access` does not remove the required `ni-file` parser
or clear its provenance.

Installed-library discovery reads Native Access-related local paths;
discovery is not entitlement verification. The access path is shared by
preset import, sample playback and resource loading. Resource loading
can reproduce library-authored graphics as well as parse format metadata.
Creator output copies supplied WAV/AIFF files into newly created library
folders. That is distinct from distributing KONTRA's own sampler code.

The maintainer reports no separate written parser permission or development
provenance records. The available Ma5onic fork history was checked for
license-file additions and license/permission references in its manifest
and README; no explicit license grant was found. This review records
existing history and source comparisons, not a retrospective clean-room
claim. No permission request has been sent on the maintainer's behalf.

## Native Instruments' published position

NI's EULA reviewed here is dated 2026-07-01:

- Sections 2 and 3.1 address activation and prohibit reverse engineering.
- Section 3.7 permits music/audio production but restricts sound-library
  reuse and standalone sample redistribution.
- Sections 4 and 5 address third-party rights and retaining identification.
- Section 7.3 chooses German law while recognizing mandatory consumer law.

These are NI's contractual terms, not a ruling on statutory exceptions.
Third-party library vendors may impose their own terms, and the agreement
accepted for a particular purchase may differ. Review the actual product
licenses. [NI EULA](https://www.native-instruments.com/pages/end-user-license-agreement).

NI requested removal of `monomadic/ni-file` in April 2024, alleging EULA,
copyright and trade-secret violations. GitHub's API currently reports it
blocked for DMCA reasons. The vendored Ma5onic fork derives from that
upstream. A takedown is not a court judgment, but the existing dispute is
directly relevant to this dependency. Permission from parser authors and
NI's objections are separate questions.
[Published NI notice](https://github.com/github/dmca/blob/master/2024/04/2024-04-04-native-instruments.md).

## Israel and international distribution

### Israeli copyright and trade secrets

The WIPO English text runs through June 2026; its section 29 amendment
takes effect on 2027-05-20. It replaces the 2011 reference. Hebrew controls.

- Section 5 distinguishes ideas, methods, mathematics and facts from
  protected expression.
- Section 19 permits conditional fair use, including research/education;
  purpose, nature, extent and market effects matter.
- Section 24(c) permits necessary software copying/adaptation by an
  authorized-copy holder for intended use, interoperability and obtaining
  information for independently developed software. Sections 24(d–e)
  limit information use/disclosure and define an authorized copy.
- Section 26 addresses transient copying for lawful use where the copy
  has no significant independent economic value.
- Section 48A addresses knowing or negligent facilitation of access to
  publicly infringing works in business for profit. Section 53A addresses
  access-restriction orders; section 56 allows damages without proof of loss.

[Copyright Act, June 2026 translation and source record](https://www.wipo.int/wipolex/en/legislation/details/23843).
Confirm applicability and contractual effect against the
[official Knesset record](https://main.knesset.gov.il/Activity/Legislation/Laws/Pages/LawPrimary.aspx?lawitemid=2000641&t=lawlaws).

For this implementation, record which authorized software copies were
studied, what information was needed, what was already available, and
how each resulting implementation was written. A sample recording,
library script or graphic is different from a format fact. The persisted
cache includes decoded impulse responses and scripts; it is not merely
transient playback memory. Whether a particular operation qualifies
requires those facts, the actual product agreement and legal analysis.

Israel's Commercial Torts Law section 6(c) says reverse engineering is
not, by itself, an improper acquisition method under section 6(b)(1).
That supports lawful analysis, but sections 6(b)(2–3) separately address
contractual/fiduciary breaches and knowingly receiving improperly
transferred secrets. Section 5 requires secrecy, commercial advantage
and reasonable protective steps; publicly known format facts differ
from a qualifying secret. Public availability after an alleged leak
does not automatically settle lawful provenance.
[Commercial Torts Law, sections 5–6](https://www.wipo.int/wipolex/en/legislation/details/2375).

### Relevant Israeli judgment, with its limits

In **Telran Communications v. Charlton**, CA 5097/11 (2013), the Supreme
Court rejected a copyright claim based on selling decoder cards.
Paragraphs 20–25 explained that Israel had no separate copyright
prohibition on technological-measure circumvention at that time.
Paragraphs 27–29 required actual underlying infringement, knowledge and
substantial contribution for contributory infringement. The court
remanded the separate unjust-enrichment question rather than ending
every possible claim. It concerned broadcasts and viewers, not this
sampler's reproduction of recordings, graphics or scripts.
[Full judgment, Cardozo translation](https://versa.cardozo.yu.edu/opinions/telran-communications-ltd-v-charlton-ltd).

This is not a ruling on KONTRA. The June 2026 text does not add a standalone
DMCA-style circumvention provision. That is a limited observation about
this statute, not clearance under every Israeli law or foreign jurisdiction.
Later online-infringement provisions and separate claims still require
assessment. Do not turn the 2013 judgment into a promise of immunity.

### Germany, the EU and US hosting

German law separately permits authorized observation/testing under
section 69d(3) and limited interoperability decompilation under section
69e. Section 69g(2) makes conflicting contractual terms void for specified
exceptions; section 69g(1) preserves other legal regimes. NI's general
prohibition therefore does not alone settle every interoperability case.
[UrhG 69d](https://www.gesetze-im-internet.de/urhg/__69d.html),
[69e](https://www.gesetze-im-internet.de/urhg/__69e.html),
[69g](https://www.gesetze-im-internet.de/urhg/__69g.html).

The distinction between compatibility and copying is also addressed in
**SAS Institute v. World Programming**, C-406/10: the CJEU distinguished
functionality, programming language and data-file formats from protected
program expression, while retaining possible protection for copied
manual expression. That supports examining KONTRA's format work on its
facts; it does not supply rights in library recordings or third-party code.
[CJEU judgment](https://eur-lex.europa.eu/legal-content/EN/TXT/PDF/?uri=CELEX:62010CJ0406).
The US Fourth Circuit later upheld reverse-engineering-related contract
liability in the SAS dispute under the law it applied, while dismissing
the software-copyright issue as moot. A favorable compatibility principle
in one jurisdiction does not settle a separate contract claim elsewhere.
[Fourth Circuit judgment, 2017](https://www.eff.org/files/2021/10/14/sas_v._wpl_ca4-opinion_10-24-2017.pdf).

For US distribution and hosting, 17 USC 1201 restricts unauthorized
circumvention and certain circumvention tools. Its subsection (f) has a
conditional interoperability exception, including conditions on sharing
information and means. Its applicability to an alternative sampler's
access to protected library content needs specific analysis.
[US Copyright Office, section 1201](https://www.copyright.gov/title17/92chap12.html).
GitHub also operates its own [DMCA process](https://docs.github.com/en/site-policy/content-removal-policies/dmca-takedown-policy).
Publishing from Israel does not by itself resolve foreign distribution
or hosting questions.

The US interoperability exception concerns independently created
computer programs and has conditions on access, necessity and sharing.
Encrypted recordings and library resources need a separate analysis;
it should not be assumed that a right to study software covers every
encrypted data file or the public distribution of its access tooling.

## Claims and evidence to keep separate

| Question | Evidence relevant to KONTRA | What remains unresolved |
|---|---|---|
| Copyright in NI software | Independent sampler source, format observations, no NI executable supplied. | Whether any protectable code/manual expression was copied; origins of reference-derived paths. |
| Copyright in parser authors' code | Exact Ma5onic pin and local patch history are retained. | An affirmative redistribution grant covering relevant authors/contributions. |
| Library content rights | Commercial instruments are supplied locally by users; caches and creator outputs can contain copied content. | Rights for alternate-sampler access and for each reproduction/export; terms differ by vendor. |
| Contracts | NI's published EULA and any product agreement actually accepted. | Parties bound, purchase-time wording, governing law, mandatory exceptions and development conduct. |
| Trade secrets | Public references, observed format facts and any independently recorded work. | Whether information qualifies as secret and was acquired/used lawfully; no complete provenance records supplied. |
| Circumvention/tool distribution | Default access implementation reads local metadata and decrypts content without checking entitlement. | Statutory exceptions, whether a measure qualifies, authorization, and foreign distribution. |
| Branding and assets | Independent KONTRA name and explicit non-affiliation. | Rights for specific published library screenshots/artwork, and fixture recordings. |

These are issues to resolve, not findings that any particular claim
would succeed. Neither NI's allegation nor the maintainer's stated
lawful purpose proves the facts needed for a claim or defense. Apache's
contributor patent grant covers only its defined scope; it is not a
third-party patent clearance. No patent or worldwide trademark search
has been completed.

## GitHub notices and response preparation

GitHub's DMCA policy addresses copyright complaints; other complaints
use other processes. A prior upstream notice does not automatically
remove every fork. For technical circumvention allegations, GitHub
requests details and performs additional review; its policy separately
addresses some license-check bypass tools. That process is not a court
decision about this implementation.

If a notice arrives, retain its exact text, dates, affected revision,
files and release identity. Identify the specific allegation before
responding: copied expression, unauthorized content, contract, secrets
or circumvention can require different evidence. Preserve development
and permission records; do not invent clean-room history or delete
provenance to disguise an origin. Consider counsel before any
counter-notice: the procedure includes sworn statements and jurisdiction
consequences, and restoration depends on the process and court-action
timing. No automatic counter-notice or litigation threat is authorized.
[GitHub DMCA policy](https://docs.github.com/en/site-policy/content-removal-policies/dmca-takedown-policy).

## Liability and educational purpose

KONTRA's development includes education, file-format research and lawful
interoperability. These are statements of purpose, not findings that all
acts are permitted. Applicable exceptions depend on their legal
conditions and the actual conduct; an educational label alone does not
satisfy those conditions.

Apache-2.0 Sections 7 and 8 disclaim warranties and limit contributors'
liability to recipients, subject to applicable law and written agreements.
They do not grant third-party intellectual-property rights or establish
immunity for a publisher against a rights holder's claims.
[Apache License 2.0, Sections 6–8](https://www.apache.org/licenses/LICENSE-2.0).

A README cannot prevent a rights holder from filing a claim or a hosting
provider from processing a takedown notice. GitHub has notice and
counter-notice procedures; removal is not itself a court finding, and
responding to a notice can have legal consequences. No advance promise
of a successful defense is made here.
[GitHub DMCA policy](https://docs.github.com/en/site-policy/content-removal-policies/dmca-takedown-policy).

The practical next steps are to establish the parser's redistribution
rights and obtain an Israeli lawyer's assessment of the actual encrypted
library-access implementation and intended distribution. Documentation
cannot resolve those outstanding facts or replace that assessment.

## Remaining decisions

1. Establish redistribution rights for the pinned parser and relevant
   contributions, through a documented grant or a replacement with clear
   provenance. Public GitHub availability is not that grant.
2. Review the GPL reference overlaps and codec recording provenance;
   retain authorship evidence, permissions and any required source offers.
3. Obtain advice on the actual access implementation, applicable Israeli
   law, product agreements and NI's existing upstream complaint. Separate
   permission to study a program from rights to access recordings,
   reproduce library artwork/scripts and distribute tools.
4. Regenerate and review the notice/source bundle for each release and
   every dependency change, including embedded/native components.
   Current nightlies bundle recognized dependency texts and exact MPL
   source archives. Inventory and packaging checks are not clearance.

The maintainer chose to retain current encrypted-library playback in
default and nightly builds with these questions documented. No activation
or purchase check was invented, and no NI/parser permission was inferred
from that choice. The standard Apache-2.0 LICENSE remains unchanged.

Project documentation should state the interoperability purpose, user
content responsibility, implementation behavior and unresolved issues.
It should not claim that a purchase, an educational label, a disclaimer
or Apache-2.0 automatically makes every use lawful.
