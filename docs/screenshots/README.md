# Screenshots

Review images for every milestone with visual output, produced by the
`visual-verification` skill (`.claude/skills/visual-verification/SKILL.md`) and checked by
the developer before the milestone is marked done.

Layout:

```
docs/screenshots/
├── README.md            this file
├── M2/
│   ├── index.md         shot table (camera, time, weather, purpose, verdict), issues found/fixed, git hash
│   ├── capture.json     render settings, adapter, timings
│   └── M2-01-overview.png …
├── M3/ …
└── S/                   spike evidence images (terrain/road blending, bridges, junctions)
```

Conventions:
- 1600 × 900 PNG, scale factor 1.0, deterministic seed and scripted scenario, UI hidden
  unless the shot says otherwise; ≤ 1.5 MB per image.
- File names `<suite>-<nn>-<id>.png`; the shot list for each suite is defined in
  `crates/cb_game/src/debug/screenshot_suites.ron` and mirrored in the milestone's acceptance
  criteria in `docs/PROGRESS.md`.
- Software-rendered (lavapipe) in the container; real-GPU confirmation images from the
  user's machines can be added as `<suite>-<nn>-<id>.<platform>.png`.
