<!-- PR Content Rule (AGENTS.md): list exact files modified + Bevy system dependencies impacted. -->

## Summary
<!-- One-line: what player/dev behavior changes? -->

## Files Modified
<!-- Exact paths, one per line — required for agent review: -->
<!-- - crates/spacegame_sim/src/market.rs -->
<!-- - assets/data/wares.ron -->

## Bevy System Dependencies
<!-- Required: schedules / SystemSets / FixedUpdate vs Update gating: -->
<!-- - FixedUpdate: EconomySet -> MovementSet (ordered after AiSet), gated by in_state(GameState::Simulating) -->
<!-- - Resources: Res<Market> / Query<&mut OrderQueue, Without<IsResource>> — note any Without<IsResource> filters -->

## Related Issue
<!-- If this PR closes an issue, use: Closes #<issue-number> -->
<!-- e.g. Closes #42 -->

## Spec / Ticket
<!-- Link to spec or wayfinder ticket if applicable: -->
<!-- - Spec: #<spec-issue> -->
<!-- - Tickets: Blocked by #<n>, Blocks #<n> -->

## Testing
<!-- How verified: -->
<!-- - [ ] cargo fmt -p {crate} -->
<!-- - [ ] cargo clippy -p {crate} --all-features (0 warnings) -->
<!-- - [ ] cargo test -p {crate} --all-features -->
<!-- - [ ] Determinism hash seed 42 / invariant checks (if Sim) -->

## Screenshots / Saves
<!-- UI change: bsn! snippet + screenshot. Sim change: postcard save hash if relevant. -->

## Out of Scope
<!-- What this PR explicitly does NOT address -->

## Checklist
- [ ] Used `thiserror` in domain crates, `anyhow` only at app edge
- [ ] No hardcoded stats — RON templates updated if attributes changed
- [ ] No `.bsn` asset files — UI kept inline as `bsn!` in `crates/spacegame_ui`
- [ ] Deterministic RNG (seeded WyRand) — no `thread_rng` in Sim
