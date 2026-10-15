# ADR-0057: Goods economy and delivery trips

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M5 (model, macro deliveries), M9 (truck agents)

## Context

The brief requires "delivery trucks between industry and commercial" and "trips generated from
… the goods economy" (review findings A14, G-B3). The plan had no production, stock, matching,
import/export or truck-trip generation model.

## Options considered

1. **Abstract goods index** feeding demand only — no trucks, misses the brief.
2. **Per-building stocks with hourly matching and truck trips (chosen)**.
3. Full supply chains (raw → processed → retail) — out of scope for v1.

## Decision

- `GoodsMarket` resource + per-building stocks: industry produces
  `0.5 units × filled_jobs` per game hour into `goods_stock` (capacity 40 units × level);
  commercial consumes `0.02 units × customers` into sales.
- Each game hour (phase slot), every commercial building with stock < 50 % requests
  `ceil(deficit / 10)` deliveries. Matching: nearest industry by network ETA (cached per graph
  version) with stock ≥ 10; otherwise import via the nearest outside connection at +30 % goods
  cost. Industry with stock > 80 % exports surplus via an outside connection (export income).
- Each delivery is a `Truck` trip carrying 10 units (agent or macro per ADR-0047 admission).
  Stock moves on arrival; trips in flight are saved.
- Business profit per game hour = sales − wages − goods cost (+ export income), taxed at the
  zone rate; feeds leveling (`profit` term) and demand (`goods_availability`,
  `production`, `export_access` terms — GAME_DESIGN §15).
- Constants live in `economy.ron`; tests: conservation of units (produced = sold + stocked +
  exported + in transit), deliveries prefer nearest supplier, import fallback when no supplier.

## Consequences

- Visible, meaningful truck traffic and industry–commerce coupling at modest cost.
