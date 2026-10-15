# Assumptions

Assumptions the plan makes that the user has not confirmed (from the independent review, section
B, plus later additions). **DS** = design-shaping (architecture changes if wrong), **T** =
tunable. "Default" is what will be implemented unless the user decides otherwise. Items marked
**❓ needs your input** are the ones worth a decision before or early in implementation; none
blocks M1.

| ID | Assumption | Tag | Default (current plan) | Status |
|---|---|---|---|---|
| B1 | Time model: 1 game minute = 2 sim s; vehicles move at nominal speed in sim time, i.e. visibly **2× real speed** at the default 24-minute day ("diorama pace"); commutes span ~1–2 game hours on the clock; UI shows perceived minutes | DS | ADR-0042, validated by spike S-Time | **confirmed by user** (2026-10-15) |
| B2 | Day length options change only pacing, not results | DS | ADR-0042 (16/24/32/48 min) | resolved by design |
| B3 | "Lock time of day" freezes lighting only; the calendar and schedules continue | DS | presentation-only | **confirmed by user**: lighting only, clock keeps running |
| B4 | Right-hand traffic, right-before-left at equal priority | DS | RHT; `DrivingSide` kept as a catalog constant so LHT can be added | ❓ confirm RHT |
| B5 | Per-approach stop/yield signs plus all-way stops and lights | DS | ADR-0045 | resolved |
| B6 | Road geometry can be edited (move node, drag control point) | DS | `EditRoadGeometry` (ADR-0055) | resolved |
| B7 | Utilities run implicitly along every road; no power lines | DS | ADR-0026 | ❓ confirm (disconnected road networks need their own plants/pumps) |
| B8 | One vehicle trip represents 6 citizens' trips (ADR-0062); beyond `agent_cap` trips are statistically sampled (camera-independent) rather than "aggregate when off-screen" | DS | ADR-0062/0047; at 50k all trips are agents (≈ 3 100 at peak) | ❓ confirm (S-Time showed 1 car per citizen cannot work with the approved pace) |
| B9 | Routing on a segment-direction link graph with local lane choice (lane plans), not a lane-level graph | DS | ADR-0048 | default |
| B10 | Pedestrians are visual near the camera, spawned from stop queues and access points | T | ADR-0047 | default |
| B11 | Reference PC | DS | Ryzen 5 7500F, GTX 1660 Ti, 16 GB DDR5-6000 single-channel = minimum spec (PERFORMANCE §1) | **confirmed by user** |
| B12 | Mac = MacBook Air 13" M4, built-in display | DS | PERFORMANCE §1 | assumed (user's answer contained the template text); ❓ please confirm |
| B13 | You run occasional real-hardware checks (smoke, screenshots, benchmark, trackpad, listening) — non-blocking | DS | User verification queue | ❓ confirm you're OK with occasional batched requests |
| B14 | 60 FPS also at 4× | T | yes (PERFORMANCE §1) | resolved |
| B15 | "60 FPS" means 1080p High on PC, Auto render scale Medium on M4 | T | PERFORMANCE §1 | default |
| B16 | Agent cap is a new-game sim option, not a graphics preset | T | ADR-0047 | resolved |
| B17 | Minimum OS: Windows 10 22H2, glibc 2.35 (Ubuntu 22.04), macOS 14 | T | ADR-0037 | default |
| B18 | macOS bundle ID `org.citybuilder.CityBuilder`; no notarization (needs an Apple Developer ID) | T | ADR-0037 | ❓ provide a bundle ID / Developer ID if you want notarized builds |
| B19 | Repository licence | T | proprietary, all rights reserved (ADR-0059) | **confirmed by user** |
| B20 | macOS CI cadence | T | macOS on main, nightly and one dispatch per milestone (ADR-0060) | **decided by user** |
| B21 | Currency label `$` (en) / `€` (de) for the same in-game money | T | ADR-0021 | default |
| B22 | 24 h clock in both locales; calendar "Year 3, Spring, Day 2" | T | ADR-0021 | default |
| B23 | Art direction: Townscaper/Islanders softness with central-European proportions | DS | ART_DIRECTION; M2 screenshots are the checkpoint | default (you review `docs/screenshots/M2/`) |
| B24 | UI font | DS | Inter 4.001 (SIL OFL) Regular + SemiBold (ADR-0059) | **decided by user** |
| B25 | No hard game over | T | GAME_DESIGN §1 | default |
| B26 | Start: 2–3 pre-built outside road stubs, town hall auto-placed (movable once) | T | GAME_DESIGN §5 | default |
| B27 | Progression thresholds and unlock contents | T | GAME_DESIGN §8 | default |
| B28 | Undo depth 100, 1-game-day window, refund decaying 100 → 25 % | T | ADR-0055 | default |
| B29 | Demolish refund 25 % within one game day | T | GAME_DESIGN §9 | default |
| B30 | Junction limits: min angle 25°, no junctions on bridges, min node spacing table | DS | ADR-0045 | default |
| B31 | Grade separation only via manual elevation (≥ 5.5 m) with an "auto overpass" quick fix; no interchange ramps tool; no tunnels | T | ALGORITHMS §1 | default |
| B32 | Water bodies immutable (no land reclamation or dams) | DS | ADR-0046 | default |
| B33 | Terrain resolution 2 m | DS | ADR-0046 (+ carve/shoulder rules) | default |
| B34 | Zoned buildings seeded by LotId (+ level, regrowth) | T | ADR-0043 | resolved |
| B35 | Household sizes, cars by wealth, lifecycle rates | T | GAME_DESIGN §10 | default |
| B36 | Logit mode-choice form and β | T | ALGORITHMS §16 | default |
| B37 | Shift distributions (commercial σ 75, industry σ 45 game min) | T | ADR-0062 (S-Time) | default |
| B38 | Walking thresholds (800 / 600 / 300 / 400 m) | T | GAME_DESIGN §10 | default |
| B39 | Commercial opening hours 07–21 (L1–L2), extended for L3 | T | ADR-0018 | default |
| B40 | Weather Markov tables and effect magnitudes | T | GAME_DESIGN §11 | default |
| B41 | Weather off = always clear, seasons still tint visuals | T | GAME_DESIGN §11 | default |
| B42 | Q/E: tap snaps 90°, hold rotates continuously | T | ADR-0052 | resolved |
| B43 | Drag precedence: tools own drags on targets; Space+drag always pans; right-drag rotates, right-click cancels | T | ADR-0052 | default |
| B44 | Fire spreads within 6 m; untreated building ruined after ≈ 130 sim s | T | ALGORITHMS §17 | default |
| B45 | Autosave every 10 real minutes, 3 rotating slots | T | ADR-0044 | default |
| B46 | Saves are portable between your Windows PC and Mac | DS | yes: base terrain saved + cross-OS determinism (ADR-0043/0044) | resolved |
| B47 | Agent self-review of lavapipe screenshots + objective metrics suffices to pass visual gates; your review is welcome but not blocking | DS | ADR-0058 | ❓ confirm (you'll find every suite in `docs/screenshots/`) |
| B48 | The agent can read CI results through the GitHub tools available in the session | T | milestone-completion skill step 9 | default (fallback: "CI-pending") |
| B49 | No hard city-size cap; beyond targets performance degrades gracefully (sampling, LOD) | T | PERFORMANCE | default |
| B50 | Sandbox = unlimited money + all unlocked | T | GAME_DESIGN §1 | default |
| B51 | Seasons last 4 game days; year = 16 days | T | GAME_DESIGN §2 | default |
| B52 | Effort ≈ 150 agent-days total (estimates in PROGRESS.md), scope-cut ladder applies | DS | PROGRESS.md | default |
| B53 | English and German only; German written alongside English from M1 | T | ADR-0021 | default |
