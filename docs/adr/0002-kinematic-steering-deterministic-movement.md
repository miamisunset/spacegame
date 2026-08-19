# Kinematic steering for ship movement, not Newtonian physics

Ship Approach and Orbit are implemented as kinematic seek/arrive plus tangential orbit velocity — no thrust/mass integration or inertia — ticked deterministically on FixedUpdate with stats (speed, ranges) from RON. We chose this over Newtonian thrust/mass because it matches EVE's actual model, stays deterministic under SETA time acceleration, and avoids tuning overshoot/braking before the tycoon loop is fun; Newtonian physics can be revisited as an optional module later.

Considered options: Newtonian thrust/mass integration (rejected: breaks determinism at high SETA, heavier tuning) and spline interpolation (rejected: no steering feel).
