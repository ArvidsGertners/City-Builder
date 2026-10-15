# Phase S — spike evidence

## S-Render (2026-10-15)

lavapipe (llvmpipe, Vulkan, CPU), 1600 × 900, spike `spikes/s_render`. Not an art-direction
checkpoint (lighting is uncalibrated; that is M2).

| Image | What it proves | Metrics |
|---|---|---|
| `S-Render-01-lit.png` | `WorldMaterial` (ExtendedMaterial + WESL) with the ADR-0049 20-byte vertex layout; faceted shading from screen-space derivatives; CSM shadows cast by the custom-layout meshes; SSAO contact shading; baked vertex AO (darker wall bases); per-slot tints from a `ShaderBuffer`; SMAA, bloom, TonyMcMapface | 0 % pure black, 0 magenta, 0 shader errors |
| `S-Render-02-prepass-normals.png` | The normal prepass (what SSAO reads) receives the derivative normals: one constant colour per face | within-face normal variance 0.00019 (gate ≤ 0.02) |
| `S-Render-03-composite-2_3.png` | Render scale 2/3 via offscreen target + composite camera (softer, correctly framed) | mean abs. difference to native 0.47/255 |
| `S-Render-04-focus-fog.png` | Focus-distance fog `1 − exp(−d·max(0,|p−focus|−r0))` at an exaggerated density (0.04/m, r0 10 m): the house at the focus keeps its colour, others haze | — |

`MainPassResolutionOverride` at 2/3 rendered **bit-identical** to native: it is a render-world
hook for upscalers (DLSS) and needs an upscale node; the composite path is used (ADR-0053
amendment).
