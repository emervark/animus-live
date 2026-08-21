# Animus Live — Business Plan

**Version:** 1.0
**Date:** 2026-08-21
**Prepared for:** the founder, prospective grant bodies, and prospective collaborators
**Planning horizon:** three years (Sep 2026 – Aug 2029)
**Currency:** EUR

> **Read this first.** Every market size, adoption number and price in this plan is a
> **model**, not a citation. There is no published market research covering "live puppet
> animation software" — the category is too small to be surveyed — so the sizing in §3 is
> built bottom-up from stated, checkable assumptions, and every assumption is listed in
> Appendix A where it can be attacked and replaced. Treat the arithmetic as sound and the
> inputs as debatable. That is the honest state of the evidence, and pretending otherwise
> would make the plan less useful, not more.

---

## 1. Executive summary

### 1.1 The business in one paragraph

Animus Live is a real-time puppet animation tool for live performance: import a drawing,
get a physically-simulated cutout puppet, drive it live from a mouse, a MIDI controller, an
OSC stream or a camera, and put it on a projector — all at 60 fps, in one Rust application.
It is an open-source, clean-room spiritual successor to **Animata** (Kitchen Budapest, 2007),
which has been unmaintained since roughly 2010 and whose users have had no replacement since.
The application is free and dual MIT/Apache-2.0 licensed; the on-disk format is published
CC0. Revenue comes from a paid **Studio** tier aimed at professional and institutional users,
a bundle with the founder's sibling product **Showmesh**, training and integration services,
and public funding for open-source infrastructure.

### 1.2 Why this, why now

Three things are simultaneously true, and they have not been simultaneously true before:

1. **The category is vacant.** Animata proved the idea in 2007 and died. Nothing since
   combines physics-driven cutout puppetry with the protocols a live-visuals rack actually
   speaks (Spout, NDI, OSC, MIDI). Adobe Character Animator went to streaming; Live2D went to
   VTubers; TouchDesigner and Notch make you build it yourself.
2. **The technology finally cooperates.** Bevy 0.19 ships GPU skinning with a flat joint
   array (no parenting required), built-in GPU readback, and multi-window borderless
   fullscreen with explicit monitor selection. These are exactly the four primitives this tool
   needs, and all four were verified working in the M0 spikes.
3. **The build cost has collapsed.** The codebase is AI-authored under a human-reviewed spec
   and plan. Roughly 16,000 lines across six crates, M0 and effectively all of M1 delivered
   inside a two-week window. A product that would have been an 18-month funded effort is a
   part-time solo effort with a €25k working-capital buffer.

### 1.3 Current status

M0 (four de-risking spikes) is complete. M1 — the 2D vertical slice — is **14 of 15 tasks
done**: image import, silhouette extraction, constrained-Delaunay meshing, joint/bone rigging,
attachment weights, the Verlet + Gauss-Seidel solver, the editor shell, the viewport,
dragging in both edit and live modes, the inspector and undo spine, the projector window, the
binary, and a full save/load round-trip under test. The application has been launched for
real: two windows, zero errors. **The product is weeks from a first public release, not
quarters.**

### 1.4 The financial shape

| | Year 1 | Year 2 | Year 3 |
|---|---:|---:|---:|
| Revenue | €40,735 | €117,925 | €234,300 |
| Costs | €40,921 | €90,086 | €161,510 |
| **Net** | **−€186** | **€27,839** | **€72,790** |
| Net margin | −0.5% | 23.6% | 31.1% |
| Cumulative net | −€186 | €27,653 | €100,443 |

Year 1 is break-even **because founder compensation is set at €18,000 gross**, not because
the business is profitable at that scale — that is the honest reading, and §7.5 states what
it would take to pay a market salary sooner. Peak cash requirement is €11,700 in Q1 of Year 1
on plan, rising to roughly €18,000 if the Year 1 grants slip. **Recommended buffer: €25,000**,
founder-funded or grant-funded. No equity investment is sought, and §9.4 explains why taking
venture money into this market would be a mistake for both sides.

### 1.5 What would make this fail, in one line each

- Nobody actually wants live puppetry badly enough to pay for it — **tested cheaply in Q2 of
  Year 1**, before any spend commits.
- The founder is the single point of failure — mitigated by the Bevy-free core, the CC0
  format spec, and a deliberate contributor funnel, none of which are aspirational: all three
  already exist in the repository.
- The MIT/Apache licence means anyone can rebuild the paid tier from source — true, and §5.4
  explains why that is a smaller problem than it looks and how the model is drawn around it.

---

## 2. The product

### 2.1 What it does

The core loop, which is what a demo video shows in twenty seconds:

1. **Import** a PNG with an alpha channel.
2. The alpha silhouette is traced (marching squares), simplified (Ramer–Douglas–Peucker) and
   triangulated into a textured mesh (constrained Delaunay, with Poisson-disc interior points).
3. **Rig** it: click to place joints, drag to make bones. The skeleton is a *graph of springs*,
   not a parent-child hierarchy — this is the design decision the whole feel of the tool rests on.
4. Bones attach to mesh vertices by radius falloff, and bake to four GPU influences per vertex.
5. **Drag a joint** and the mass-spring solver deforms the mesh organically — cloth-like, with
   follow-through, not rigid.
6. **Project it** borderless-fullscreen on a chosen monitor, with the editor's gizmos invisible
   on the output.
7. **Save and reopen** it byte-identically.

Everything after that is additive: live inputs, clips, 3D models, and the output matrix.

### 2.2 The three decisions that make it different

**Springs, not keyframes.** A clip in Animus Live moves *targets*; the physics moves the puppet.
A one-frame MIDI trigger therefore produces a whip with follow-through for free, and
retriggering mid-motion re-aims rather than snapping. Keyframing the mesh directly would throw
away the only thing the tool uniquely has.

**Recording, not authoring.** Because a performed drag already writes into the same target map
that live bindings write into, recording a take is capturing that map over time. *Arm → perform
the motion by hand → stop → it is a clip, and the clip loops.* Asking a VJ to keyframe a walk
cycle is asking them to be an animator; asking them to puppeteer it once and loop it is asking
them to do what they already do. This is a positioning decision disguised as an implementation
detail.

**A hand always beats a loop.** When a running clip and a live controller target the same joint
in the same tick, the live binding wins. That rule is fine to get wrong in a studio and
unusable to get wrong at 2 a.m. with an audience watching. Shipping it correct from the first
release is a *trust* feature.

### 2.3 Roadmap and what each milestone unlocks commercially

| Milestone | Effort | Status | Commercial unlock |
|---|---|---|---|
| **M0** — spikes | 2–3 wk | **done** | Technical risk retired before any spend |
| **M1** — 2D vertical slice | 6–8 wk | **14/15 done** | First public release; the demo video; the whole top of the funnel |
| **M2** — signal bus, OSC, MIDI | 4 wk | next | Enters the VJ rack. TouchDesigner/Resolume users can now use it *with* what they own |
| **M2.5** — clips, loops, triggers | 3–4 wk | | The Ableton story: a kick drum fires a puppet. This is the demo that travels |
| **M3** — 3D glTF in the unified scene | 4 wk | | Mixamo/Sketchfab reach; a 2D puppet and a 3D character in one scene is not available elsewhere |
| **M4** — Spout, NDI, video export | 4 wk | | Professional output; **the first credible Studio-tier boundary** |
| **M5** — live inputs + show hardening | 5 wk | | The 4-hour soak test. This is what makes it sellable to a *venue* rather than a hobbyist |
| **M6** — 1.0 | 6 wk | | Installer, docs site, manual mesh editing, templates, published CC0 reference reader |

**Total remaining: roughly 26–29 weeks of engineering.** M1 alone is a complete, shippable,
useful tool — which means the business can start before the roadmap finishes, and every
milestone after M1 is a marketing event with a release attached.

### 2.4 Technical moat, honestly assessed

There is no algorithmic moat. Constrained Delaunay triangulation and Verlet integration are
textbook. The defensible assets are:

- **The integration surface.** 2D puppets + 3D glTF + four input classes + four output classes,
  all in one 60 fps application that does not crash at a venue. Any one part is easy; the
  combination is a year of work and a soak-test discipline.
- **The show-hardening discipline.** Per-subsystem `catch_unwind`, a NaN guard that resets one
  puppet rather than propagating, a zero-allocation frame path verified by a counting
  allocator, autosave with rolling backups, a panic hotkey, and a manual on-hardware checklist
  per release. Competitors in this niche generally do not have this, and it is the difference
  between a toy and a tool someone will risk a paying audience on.
- **The category name.** Being the obvious answer to "what replaced Animata?" is worth more in
  a niche this small than any patent would be.

---

## 3. Market analysis

### 3.1 Who the customer actually is

Four segments, in descending order of fit:

**A. VJs and live-visual performers.** Own Resolume, MadMapper, TouchDesigner or Notch already.
Work in clubs, at festivals, on tour. Buy tools that plug into an existing rack — which is why
Spout/NDI/OSC/MIDI support is not a feature list item, it is the price of entry. Price-tolerant
(they already spend €400–800 per tool) but ruthless about reliability. **Primary segment.**

**B. Theatre, dance and opera visual designers.** Longer sales cycles, institutional budgets,
higher willingness to pay for support, and a genuine appetite for *character* on stage rather
than abstract visuals — which is exactly what a puppet is. Animata's original users came
disproportionately from here. **Highest-value segment per customer; slowest to close.**

**C. Media-art education and institutions.** Art academies, hackerspaces, museums, youth
media labs. Value open source *as such* — it is often a procurement requirement, and a free
tool that students can keep after graduating is a real argument. Buy site licences and
workshops rather than seats. **Best margin per hour of effort, via training.**

**D. VTubers and streamers.** Enormous and adjacent. Live2D and Warudo own the workflow; the
puppet model overlaps but the output path (virtual camera, not projector) does not. **Explicitly
an expansion market, not a launch market** — chasing it in Year 1 would distort the product
away from A and B.

### 3.2 Sizing (bottom-up, modelled)

| | Population (modelled) | Annual software spend | Value |
|---|---:|---:|---:|
| A. VJs / live-visual performers owning paid tools | 120,000 | €120 | €14.4M |
| B. Theatre / dance visual designers (EU + NA) | 25,000 | €200 | €5.0M |
| D. Media-art institutions & education | 8,000 | €300 | €2.4M |
| **TAM — annual spend on tools of this class** | | | **≈ €22M / yr** |

**SAM** — Windows users with a live-driven *character/puppet* need, reachable in English and
in the EU, and not locked into a competing pipeline: roughly 18% of TAM ≈ **€4.0M / yr**.

**SOM** — realistically attainable in three years: **€234k / yr**, which is 5.9% of SAM and
about 1.1% of TAM. That is a defensible share for the only tool in a vacant sub-category, and
it is deliberately not a hockey stick.

**The honest conclusion from this table: this is a small market.** It will support one to three
people well. It will not support a venture-scale company, and §9.4 treats that as a strategic
choice rather than a disappointment.

### 3.3 Competitive landscape

| Product | Price | Live-driven | Mesh/puppet deform | Spout/NDI/OSC | Open | Gap Animus Live fills |
|---|---|---|---|---|---|---|
| **Animata** (2007) | Free (GPL) | Yes | Yes (springs) | OSC only | Yes | **Dead since ~2010.** No 3D, no modern IO, no maintenance |
| **Adobe Character Animator** | Subscription (CC) | Yes | Yes | Partial | No | Streaming/broadcast oriented; not a projector tool; not in a VJ rack; subscription-only |
| **Live2D Cubism** | Subscription | Via runtime | Yes | No | No | VTuber pipeline; no live-projection path; no physics-first authoring |
| **TouchDesigner** | Free / $600 / $2,200 | Yes | Build it yourself | Yes | No | Generalist. Can do anything, gives you nothing. Steep learning curve |
| **Notch** | High subscription | Yes | Partial | Yes | No | Broadcast-tier pricing; overkill and unaffordable for segments A/B/C |
| **Resolume Arena** | ~€800 | Playback | No | Yes | No | Clip playback and mapping; no character rigging at all |
| **Isadora** | ~€350 | Yes | No | Yes | No | Theatre media control; no mesh puppetry |
| **MadMapper** | ~€450 | Yes | No | Yes | No | Projection mapping; adjacent, not competing |
| **Blender / Grease Pencil** | Free | No | Yes | No | Yes | Offline authoring; not a performance instrument |
| **Warudo / VSeeFace** | Free | Yes | Yes (3D) | Partial | Mixed | VTuber-shaped; not a stage tool |

**Positioning statement:** *Animus Live is the only open-source tool where a hand-drawn cutout
becomes a physically-simulated puppet that a MIDI controller can perform, live, onto a
projector — in the same rack as everything else you already own.*

**Complement, don't compete.** Resolume, TouchDesigner and MadMapper users are not people to
take share from — they are people to *send frames to* over Spout and NDI. That framing turns
the three biggest tools in the category from competitors into distribution.

### 3.4 The Animata inheritance

An unmaintained tool with a real user base is an unusual asset: a named audience that already
understands the value proposition and has been unserved for fifteen years. Concretely this is
worth pursuing through the media-art mailing lists and festival networks that carried the
original, and through the "awesome-vj"-style lists where Animata is still listed as dead.
`docs/heritage.md` credits Kitchen Budapest and the original authors by name, and the clean-room
policy in `CONTRIBUTING.md` is enforced rather than claimed — which means the inheritance can
be *stated publicly* without a legal problem. That is the whole point of having done the
clean-room work properly.

---

## 4. Customers, and how they are reached

### 4.1 The funnel

| | Year 1 | Year 2 | Year 3 |
|---|---:|---:|---:|
| Downloads (in year) | 6,000 | 22,000 | 55,000 |
| Cumulative downloads | 6,000 | 28,000 | 83,000 |
| Monthly active users | 480 | 2,800 | 9,130 |
| New paying customers | 55 | 235 | 550 |
| Conversion (active → paid) | 11.5% | 8.4% | 6.0% |

The declining conversion rate is intentional in the model: early adopters of a revived cult
tool convert far above normal, and the rate should be expected to fall toward a typical
open-core 3–6% as the audience broadens. A model that held 11% flat would be a model built to
flatter.

### 4.2 Go-to-market, in four phases

**Phase 0 — the artefact (Months 1–2).** One twenty-second video: a drawing is imported, an arm
is rigged in four clicks, it waves, it is on a projector. No narration, no roadmap. This
product's marketing asset is the product running, and that asset is nearly free to produce.
Launch on Hacker News, r/vjing, r/creativecoding, the Bevy showcase, This Week in Rust, and
the media-art lists that carried Animata.

**Phase 1 — the inheritance (Months 2–6).** The "what replaced Animata" narrative. Direct
outreach to Kitchen Budapest alumni and to the artists whose Animata work is still online.
Get listed everywhere Animata is listed. Publish the CC0 format spec with a reference reader —
this is a credibility artefact aimed at exactly the people who care most.

**Phase 2 — the rack (Months 6–14, gated on M2/M4).** Once OSC, MIDI, Spout and NDI ship, the
message changes from "a new tool" to "a new tool *for the rig you already have*." Tutorials
built explicitly as integrations: *Animus Live → Resolume*, *TouchDesigner → Animus Live*,
*Ableton fires a puppet*. Each of these is a video that finds an audience that already exists.

**Phase 3 — the stage (Months 12–36).** Festival and conference presence: Resonate, MUTEK,
Sonar+D, Ars Electronica, and on the theatre side the Prague Quadrennial. Locally: Tallinn
Music Week, Plektrum. The objective at each is not booth leads — it is *one artist using it in
a real show*, because in this market a credited show in a festival programme outperforms any
amount of advertising.

**Always-on: the contributor funnel.** `animus-core` is deliberately Bevy-free, plain Rust,
plain `cargo test`, no GPU required. A contributor can improve the triangulator or the OSC
parser without learning Bevy or owning a projector. Keeping `good-first-issue` stocked from the
core crates is the single highest-leverage community activity available, and it doubles as the
mitigation for key-person risk.

### 4.3 Channel economics

Blended customer acquisition cost at Year 3 is **€21.82** (marketing spend ÷ new paying
customers). Lifetime value, at a €199 licence net of a 5% merchant fee plus an average 1.6
maintenance renewals, is **€309**. **LTV/CAC ≈ 14×.**

That ratio is high because the product *is* the marketing — a puppet moving on screen is
inherently shareable, and the incremental cost of the content is a screen recording. The risk
in this line is not the ratio, it is the absolute ceiling: cheap acquisition in a small market
still reaches the end of the market. Growth beyond Year 3 comes from segment D (VTubers) or
from adjacent products, not from spending more on the same channels.

---

## 5. Business model

### 5.1 The constraint that shapes everything

The application is **MIT OR Apache-2.0**. The file format spec is **CC0**. Both are already
committed and both are the right decisions for reasons that have nothing to do with revenue:
MIT/Apache is required for the NDI path (whose runtime is proprietary and non-redistributable),
it matches the entire Rust ecosystem, and it maximises an already-tiny contributor pool.

The consequence is unavoidable and must be stated plainly: **anyone can compile the free
application from source, and nothing paid can be built by removing features from it.** Any
revenue model that depends on withholding what has already been promised would be both
unenforceable and a betrayal of the project's stated character.

### 5.2 Where the line is drawn instead

**Everything specified in the design spec through 1.0 stays free, forever.** That includes
Spout, NDI, video export, the signal bus, clips and triggers, and 3D glTF support. Publishing
this as a written promise is worth more than the revenue any of it could be paywalled for.

The paid **Animus Live Studio** tier is built from things that are *outside* that promise and
that are genuinely hard to self-serve:

| Studio component | Why someone pays rather than builds |
|---|---|
| Signed, notarised installers with auto-update | Building and code-signing your own is an afternoon you do not have before a show |
| Show-control layer: ordered cue lists, multi-machine sync, remote trigger | Not in the 1.0 spec; genuinely new engineering; only matters to professionals |
| Cloud project sync + versioned show backup | A service, not a binary — cannot be forked |
| Asset library service (Sketchfab/Mixamo brokering, curated puppet packs) | A service plus licensed content |
| Priority support with a named response window | A person, not a feature |
| Commercial-use assurance and third-party licence attestation | What a venue's procurement department actually asks for |

**Pricing: €199 perpetual, including 12 months of updates; €79/year thereafter to continue
receiving them.** Perpetual-with-maintenance rather than subscription is chosen deliberately —
this audience has been burned by Adobe, and "your show still opens in five years" is a selling
point against every closed competitor in §3.3.

### 5.3 The full revenue portfolio

| Line | Price | Nature | Y3 share |
|---|---|---|---:|
| Studio licences | €199 | One-time | 36% |
| Maintenance renewals | €79/yr | Recurring | 5% |
| Showmesh + Animus bundle | €349 | One-time | 19% |
| Venue / institution site licence | €1,200/yr | Recurring | 7% |
| Training & workshops | €900/day | Services | 8% |
| Integration & commissioned work | €600/day | Services | 6% |
| Puppet & template packs | €29 | One-time | 9% |
| Grants | — | Non-recurring | 6% |
| Sponsorship | — | Semi-recurring | 3% |

Product revenue rises from 40% of the total in Year 1 to 76% in Year 3, while grants and
sponsorship fall from 32% to 9%. **That trajectory is the plan's central financial claim:**
public funding buys the runway to build the thing, and the thing then pays for itself. A plan
where grants are still a third of revenue in Year 3 would be a plan for a permanently
grant-dependent project.

### 5.4 The Showmesh relationship

Showmesh is the founder's existing live-visuals application. It already sends Spout and speaks
OSC and MIDI, and Animus Live has adopted its design system wholesale rather than inventing a
second visual language — *the same operator, the same rack, the same evening*. This is a
material commercial asset and the plan uses it three ways: a **bundle** at €349 (modelled as
19% of Year 3 revenue), a **cross-sell audience** that is already qualified, and a **shared
support and release process** that keeps the cost of serving two products closer to the cost of
serving one.

> **Assumption flagged for the founder to correct:** this plan models Showmesh as
> commercially available at roughly €249 standalone. If Showmesh is not yet sold, or is priced
> differently, the bundle line moves to Year 2 and Year 1 revenue falls by €5,235 — which
> deepens the Year 1 cash trough to about €17,000 but does not change the shape of the plan.

### 5.5 Why forking is a smaller threat than it looks

Someone could fork the free application, add their own cue list, and sell it. In practice this
requires the fork's author to also carry the Bevy churn, the egui version lockstep, the venue
soak-testing and the support load — which is the actual work. The realistic risk is not a
commercial fork; it is a *community* fork caused by the project mishandling its own open-source
obligations. The mitigation is governance, not licensing: keep the promise in §5.2 in writing,
develop in the open, and never move a previously-free capability behind the Studio line.

---

## 6. Operations

### 6.1 Legal and administrative

Estonian **OÜ**, which is the natural vehicle: low administrative overhead, e-Residency-compatible
tooling, and — importantly for a business whose profits are reinvested — corporate income tax is
charged on distribution rather than on accrual, so retained earnings that fund development are
not taxed while they stay in the company.

Sales run through a **merchant of record** (Paddle or Lemon Squeezy) rather than direct card
processing. The all-in fee of roughly 5% is more than a payment processor charges, and it buys
away the entire EU VAT-OSS, US sales-tax and invoicing problem — which for a solo operator is
worth considerably more than the spread.

**Trademark:** register "Animus Live" as an EU trademark (EUIPO) in Year 1, budgeted at €1,500.
The name is the category position; leaving it unregistered while promoting it is the cheap
mistake in this plan.

### 6.2 Third-party obligations, tracked

Already enforced in the repository by `cargo deny check licenses` on every commit, with
`THIRD-PARTY-NOTICES.md` generated in CI:

- **spout2-rs** — BSD-2-Clause, © Lynn Jarvis; notice reproduced; static linking of the vendored SDK permitted.
- **NDI** — attribution required; "NDI® is a registered trademark of Vizrt NDI AB" in About and README; runtime **not** bundled.
- **ffmpeg** — subprocess only, so no licence obligation attaches; the user supplies it.
- **Fonts** (Inter, JetBrains Mono) — both OFL; licence files included.
- **Ableton Link** — GPLv2-or-commercial, and therefore **off the table** as a linked
  dependency. MIDI clock at 24 ppqn supplies tempo and phase instead, at the cost of a counter.

### 6.3 Release and support

Releases on a milestone cadence, each with a **manual on-hardware checklist** — a second
display, a real projector, a real MIDI controller — because the failure mode this product must
never have is a black projector in front of an audience. Support is community-first (GitHub
Issues, Discord) for the free tier, with a named response window for Studio and a same-day
target for venue site licences during a production week.

### 6.4 Team and hiring

Year 1 is the founder plus AI-assisted development. Year 2 adds roughly €15,000 of contract
work — documentation, tutorial video, and a second pair of hands on the Bevy half. Year 3
raises that to €45,000, the point at which a part-time contractor becomes a plausible first
permanent hire.

**The bottleneck to watch is not engineering capacity — it is support and community
management.** In open-source products of this shape, that is what saturates the founder first,
and it should be the first thing contracted out rather than the last.

---

## 7. Financial plan

### 7.1 Revenue detail

| Line | Y1 | Y2 | Y3 |
|---|---:|---:|---:|
| Studio licences (40 / 180 / 420 @ €199) | 7,960 | 35,820 | 83,580 |
| Maintenance renewals (0 / 30 / 150 @ €79) | 0 | 2,370 | 11,850 |
| Showmesh + Animus bundle (15 / 55 / 130 @ €349) | 5,235 | 19,195 | 45,370 |
| Venue site licences (1 / 5 / 14 @ €1,200) | 1,200 | 6,000 | 16,800 |
| Training & workshops (6 / 14 / 22 days @ €900) | 5,400 | 12,600 | 19,800 |
| Integration & commissioned work (10 / 18 / 24 days @ €600) | 6,000 | 10,800 | 14,400 |
| Puppet / template packs (60 / 260 / 700 @ €29) | 1,740 | 7,540 | 20,300 |
| Grants | 12,000 | 20,000 | 15,000 |
| Sponsorship | 1,200 | 3,600 | 7,200 |
| **Total revenue** | **40,735** | **117,925** | **234,300** |

### 7.2 Cost detail

| Line | Y1 | Y2 | Y3 |
|---|---:|---:|---:|
| Founder compensation (gross €18k/36k/54k × 1.338 employer tax) | 24,084 | 48,168 | 72,252 |
| Contractors (development, docs, video) | 0 | 15,000 | 45,000 |
| Hardware (workstation, displays, projector, controllers) | 3,500 | 2,000 | 2,500 |
| AI-assisted development tooling | 2,400 | 3,600 | 3,600 |
| Code signing (EV) + Apple Developer | 499 | 499 | 499 |
| CI, hosting, docs site, domains | 600 | 1,200 | 1,800 |
| Accounting, legal, OÜ administration | 1,500 | 2,400 | 3,000 |
| Trademark registration (EUIPO) | 1,500 | 0 | 0 |
| Marketing, festivals, travel | 3,000 | 7,000 | 12,000 |
| Payment processing (5% of product revenue) | 807 | 3,546 | 8,895 |
| Contingency (8%) | 3,031 | 6,673 | 11,964 |
| **Total costs** | **40,921** | **90,086** | **161,510** |

### 7.3 Profit and cash

| | Y1 | Y2 | Y3 |
|---|---:|---:|---:|
| Revenue | 40,735 | 117,925 | 234,300 |
| Costs | 40,921 | 90,086 | 161,510 |
| **Net result** | **−186** | **27,839** | **72,790** |
| Net margin | −0.5% | 23.6% | 31.1% |
| Cumulative | −186 | 27,653 | 100,443 |

**Year 1 quarterly cash** (the store opens in Q2; grants modelled as landing in Q2 and Q4):

| Quarter | Revenue | Costs | Net | Cumulative |
|---|---:|---:|---:|---:|
| Q1 | 720 | 12,391 | −11,671 | **−11,671** |
| Q2 | 10,407 | 10,406 | 0 | −11,670 |
| Q3 | 9,717 | 9,083 | +634 | −11,037 |
| Q4 | 19,892 | 9,819 | +10,072 | −964 |

**Peak cash requirement: €11,671 in Q1.** If both Year 1 grants slip out of the year, the
trough deepens to roughly €18,000. **A €25,000 buffer covers the plan with margin**, and is the
only external capital the business needs.

### 7.4 Unit economics

| Metric | Value |
|---|---:|
| Studio licence price | €199 |
| Net of merchant-of-record fee (5%) | €189.05 |
| Marginal cost of goods | ≈ €0 |
| Blended CAC (Y3 marketing ÷ new paying customers) | €21.82 |
| LTV (licence + 1.6 average renewals, net of fees) | €309.13 |
| **LTV / CAC** | **14.2×** |

### 7.5 Break-even

At the Year 2 cost base of €90,086, and with services, grants and sponsorship performing to
plan (€47,000 combined), **228 Studio licences** cover the year. Without any services or
grants at all, it takes **477 licences**. Both numbers are within the modelled funnel, which
means the plan does not depend on grants to survive — only to be comfortable.

The Year 1 near-break-even is a function of the €18,000 founder salary. Paying a market
Estonian developer salary (roughly €48,000 gross, €64,200 fully loaded) in Year 1 would open a
€40,000 gap. **Closing that gap a year early requires either a €40,000 grant or roughly 210
additional Studio licences in Year 1** — and of the two, the grant is by far the more likely,
which is why §8 treats grant applications as a Q1 priority rather than a nice-to-have.

### 7.6 Scenarios (Year 3)

| Scenario | Assumption | Revenue | Costs | Net |
|---|---|---:|---:|---:|
| **Bear** | Product revenue at 45% of plan; services and grants halved; costs cut 28% | 108,255 | 116,287 | **−8,032** |
| **Base** | As modelled | 234,300 | 161,510 | **+72,790** |
| **Bull** | Product revenue at 1.9× plan (a festival show goes viral, or segment D opens early); costs +25% | 411,330 | 201,887 | **+209,443** |

The bear case is a **survivable** loss, not a fatal one — the cost base is dominated by founder
compensation and contractors, both of which are discretionary. That asymmetry (bounded downside,
unbounded-ish upside from a single viral show) is the strongest structural feature of this
business, and it is a direct consequence of keeping fixed costs near zero.

---

## 8. Funding

### 8.1 What is needed

**€25,000 of working capital** to cover the Year 1 cash trough with margin against grant
slippage. Nothing beyond that.

### 8.2 Sources, in preference order

1. **Founder capital / retained Showmesh revenue.** Cheapest, fastest, no dilution, no reporting.
2. **NLnet NGI Zero (Commons Fund / Entrust).** Grants typically €5,000–50,000 for open-source
   internet and digital-commons infrastructure. Animus Live fits well: MIT/Apache application,
   CC0 file format, published reference implementation, an explicit anti-lock-in story. **Apply
   in Q1 of Year 1** — this is the single highest-expected-value administrative action available.
3. **Eesti Kultuurkapital** (Estonian Cultural Endowment), audiovisual and design fields —
   suited to funding the *artistic* side: commissioned shows, festival presence, documentation.
4. **Creative Europe** — culture-sector digital tooling; larger, slower, better as a Year 2
   consortium application with a theatre partner than as a solo Year 1 attempt.
5. **GitHub Sponsors / Open Collective** — small but compounding, and valuable as a public
   signal of a user base.

### 8.3 What the money buys

Runway to reach M2 (signal bus, OSC, MIDI) and M4 (Spout, NDI, export), which together convert
the product from *interesting* to *installable in a professional rack*. That is the point at
which the Studio tier has a credible boundary and revenue stops depending on grants.

### 8.4 Why not venture capital

A €22M annual TAM cannot return a venture fund. Taking institutional equity would force one of
two distortions: pivoting into the VTuber market (segment D) at the cost of the theatre and VJ
users who are the actual reason the product is good, or relicensing away from MIT/Apache — which
would break the NDI path, gut the contributor pool, and destroy the open-source credibility
that makes the grant funding available in the first place. **The correct shape for this business
is a profitable, independent, one-to-three-person company that funds itself,** and the plan is
built to reach that rather than to be fundable by someone who wants something else.

---

## 9. Risks

Ranked by expected damage. Technical risks are carried from the design spec §19 in condensed
form; commercial risks are new to this document.

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| 1 | **A live show fails on stage** — crash, freeze, black projector, NaN explosion | Medium | **Catastrophic** | `--perform` mode with no editor systems; per-subsystem `catch_unwind`; NaN guard resets one puppet; panic hotkey; manual window-geometry override; autosave + rolling backups; zero-allocation frame path verified by a counting allocator; 4-hour soak test as an M5 gate; manual on-hardware checklist per release |
| 2 | **Nobody will pay** — the free tier is enough for everyone | Medium | High | Studio boundary drawn at *services and hard engineering*, not withheld features; tested in Q2 Y1 with a real store before marketing spend commits; services and grants carry Y1 regardless |
| 3 | **Key-person dependency** | **High** | High | Bevy-free `animus-core` as a contributor on-ramp (plain Rust, plain `cargo test`, no GPU); CC0 format spec so the *format* survives even if the app does not; reference reader published to crates.io; `good-first-issue` kept stocked |
| 4 | **egui reads as a debug tool to artists** | **High** | High | Showmesh's design system adopted wholesale from M1, not "later"; hand-written inspectors; inspector-egui confined to a hidden Dev tab; the output window contains no egui at all. Residual accepted: compensate with density and speed |
| 5 | **Market is too small to support the plan** | Medium | High | Segment D (VTubers) is a real expansion valve; services scale independently of licence count; cost base is discretionary, so the bear case is survivable |
| 6 | **Bevy 0.x breaking changes / egui version lockstep** | Certain | Medium | Exact-pinned versions with the verified set recorded in the manifest; "bump the UI stack" treated as its own scheduled task; willingness to sit on an old egui for a release; the Bevy-free core is where most development happens |
| 7 | **A competitor adds live puppetry** | Low-Medium | High | Speed and category ownership; the integration surface is a year of work; open source makes displacement-by-feature-copy less effective than usual |
| 8 | **Spout zero-copy unreachable through wgpu** | Medium-High | Medium | Readback path implemented first and shipped; `FrameSink` makes zero-copy a swap; honest documentation of latency |
| 9 | **Grants do not land** | Medium | Medium | Multiple independent bodies applied to in parallel; break-even in §7.5 computed *without* grants; €25k buffer sized for total slippage |
| 10 | **`spout2-rs` / NDI binding abandonment** | Medium | Medium | BSD-2 with vendored SDK makes forking cheap; both wrapped behind our own sink traits; NDI feature-gated and runtime-detected |
| 11 | **Scope** — 2D + 3D + four input classes + four output classes | High | Medium | Milestone ordering is the mitigation: M1 alone is complete and shippable, and every later milestone is optional |
| 12 | **Support load saturates the founder** | Medium | Medium | First function to contract out in Y2; community-first support for the free tier; named response windows only for paying tiers |

---

## 10. Key performance indicators

Reviewed quarterly. Each has a stated action if it misses, because a KPI without a
consequence is a decoration.

| KPI | Y1 target | Y2 | Y3 | If missed |
|---|---:|---:|---:|---|
| Downloads (cumulative) | 6,000 | 28,000 | 83,000 | Re-cut the demo video; the artefact is wrong, not the market |
| Monthly active users | 480 | 2,800 | 9,130 | Onboarding problem — re-run the M1 "ten minutes, no instructions" test on strangers |
| Conversion (active → paid) | 11.5% | 8.4% | 6.0% | Studio boundary is in the wrong place; move it, do not paywall existing features |
| Studio licences (new, in year) | 40 | 180 | 420 | Check whether services should become the primary line |
| Venue site licences | 1 | 5 | 14 | Segment B outreach is failing — this is a relationship channel, not a marketing one |
| External contributors with a merged PR | 3 | 12 | 30 | Key-person risk is not being mitigated; escalate `good-first-issue` work |
| Public shows credited to the tool | 2 | 10 | 30 | **The most important number in this table.** A show is worth more than a thousand downloads |
| Grant funding secured | €12,000 | €20,000 | €15,000 | Extend runway by cutting contractors before cutting marketing |

**If only one number is watched, watch "public shows credited to the tool."** Downloads measure
curiosity; a credited show measures whether someone trusted the product in front of an
audience, which is the only thing this category actually sells.

---

## 11. Ninety-day plan

| Weeks | Action | Output |
|---|---|---|
| 1–2 | Finish M1 Task 6 (fonts, theme) and Task 15 (the done-when test) | M1 complete |
| 2–3 | Cut the twenty-second demo video; publish the CC0 format spec with a reference reader | The launch artefact |
| 3–4 | First public release: signed installer, docs site, GitHub release, Discord | v0.1 shipped |
| 4 | Launch posts: HN, r/vjing, r/creativecoding, Bevy showcase, This Week in Rust, media-art lists | Top-of-funnel |
| 4–5 | **Submit the NLnet NGI Zero application**; open the Kultuurkapital application | Funding in flight |
| 5–6 | Register "Animus Live" with EUIPO; incorporate/confirm the OÜ; set up the merchant of record | Legal and commercial base |
| 6–10 | **M2: signal bus, OSC, MIDI** | Enters the VJ rack |
| 8 | Open the store with the Studio tier — **before** any marketing spend, to test §9 risk 2 cheaply | The willingness-to-pay test |
| 10–12 | First integration tutorials (Resolume, TouchDesigner); direct outreach to Animata-era artists | Phase 1 and 2 in motion |
| 12 | Quarterly review against §10 | Go / adjust decision |

---

## Appendix A — Assumptions

Every number in §7 traces to one of these. They are listed so they can be replaced.

**Market**
- 120,000 people worldwide own paid live-visual software, spending ~€120/yr each.
- 25,000 theatre/dance visual designers in EU + NA, spending ~€200/yr.
- 8,000 media-art institutions and education programmes, spending ~€300/yr.
- 18% of that total is realistically serviceable by a Windows-first, English-language, puppet-focused tool.

**Adoption**
- 6,000 / 22,000 / 55,000 downloads per year.
- 8% / 10% / 11% of cumulative downloads become monthly active users.
- 11.5% / 8.4% / 6.0% of active users become paying customers.

**Pricing**
- Studio €199 perpetual with 12 months of updates; €79/yr maintenance thereafter.
- Showmesh + Animus bundle €349 (assumes Showmesh at roughly €249 standalone — **to be confirmed**).
- Venue site licence €1,200/yr. Training €900/day. Integration €600/day. Asset packs €29.
- Maintenance renewal rate averaging 1.6 renewals per licence over its life.

**Costs**
- Founder gross salary €18k / €36k / €54k; Estonian employer cost multiplier 1.338 (33% social tax + 0.8% unemployment insurance).
- Merchant of record all-in fee 5% of product revenue.
- 8% contingency on all costs.
- Contractors €0 / €15,000 / €45,000.

**Funding**
- Year 1 grants of €12,000 landing in Q2 and Q4. Modelled at zero in the break-even test in §7.5.

---

## Appendix B — Source material

This plan was built by analysing the repository, not from external market research:

- `docs/superpowers/specs/2026-08-16-animus-live-design.md` — product scope, architecture, milestones (§18), risks (§19), licensing (§20)
- `docs/superpowers/plans/2026-08-17-m1-2d-vertical-slice.md` — M1 status, 14 of 15 tasks complete
- `docs/heritage.md`, `CONTRIBUTING.md` — the Animata relationship and the clean-room policy
- `spec/animus-project-format-v1.md`, `spec/LICENSE` — the CC0 format specification
- `docs/spikes/m0-*.md` — the four de-risking spikes and their measured outcomes
- `Cargo.toml`, `crates/` — six crates, ~16,000 lines, the dependency pins and licence constraints

The financial model is reproducible: `docs/business/model.py` prints every table in §7.
