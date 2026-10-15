# ADR-0019: Audio approach — procedural synthesis

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M13

## Context

Needed: ambient soundscape varying with zoom, time of day, weather and on-screen content
(nature, traffic hum, industry, rain, wind, thunder); UI sounds; construction/demolition;
vehicle sounds near the camera; sirens; per-channel volumes; music optional. The brief asks
to decide between synthesized audio and a small set of royalty-free files.

## Options considered

1. **CC0/royalty-free sample files** — highest realism quickly; requires sourcing and
   license tracking, adds binary assets (against the project's "everything procedural"
   spirit), and I cannot audition files in this environment either.
2. **Real-time procedural synthesis on the audio thread** — maximal flexibility, but
   custom `rodio::Source` graphs per sound are complex to mix and debug.
3. **Procedural synthesis at startup into in-memory PCM buffers, played with `bevy_audio`
   (chosen)** — deterministic, asset-free, simple playback/mixing; ambient beds are seamless
   loops crossfaded by a director.
4. **bevy_kira_audio** (supports 0.20) — better mixer/tweens; extra dependency; revisit if
   `bevy_audio` mixing proves insufficient.

## Decision

- `cb_audio` (pure DSP): oscillators (sine, tri, saw, band-limited), noise (white/pink/
  brown), biquad filters, ADSR/envelopes, FM, simple Schroeder reverb, limiter; renders
  sounds to `Vec<i16>` and encodes in-memory WAV (no files on disk).
- Bank (synthesized on the async pool during Boot, seeded variations, ~25–40 MB PCM):
  ambient loops (birds day, crickets night, wind light/strong, rain light/heavy, traffic
  hum low/high, industry drone, city murmur, water lapping), one-shots (UI click/hover/
  confirm/error, placement, demolition, construction hammer/saw, thunder × 4, milestone
  chime), vehicle loops (engine idle/drive by size, tram hum, bus), sirens (police
  wail/yelp, fire horn, ambulance hi-lo).
- `bevy_audio` (rodio, already in Bevy; `wav` feature): `AudioPlayer` + `AudioSink` volumes
  driven by the **ambient director** (weights from zoom, camera position sampling of
  nearby content via spatial indices, time, weather) and channel volumes (master, music,
  effects, ambient, UI); spatial audio (`SpatialListener`) for vehicles and sirens near the
  camera, limited to 16 concurrent emitters.
- Music: optional generative pad (slow pentatonic chord progressions) as a stretch goal in
  M13; channel exists from the start.

## Consequences

- No assets, deterministic sounds, small repo. Quality depends on careful DSP tuning; a
  listening pass by the user is requested in M13 (RISKS R11). If insufficient, a
  superseding ADR may introduce a curated CC0 sample set.

## Amendments

- 2026-10-15 (plan review): the bank is synthesized lazily per category on first use and
  cached on disk in `AppPaths::cache` keyed by `(bank_version, seed)` (a runtime-generated
  cache, not an asset). Objective audio QA tests are defined in ADR-0058. With no audio device
  (CI, container) or `--no-audio`, the audio plugin is not added. Generative music is not part
  of M13 acceptance (brief: music optional).
