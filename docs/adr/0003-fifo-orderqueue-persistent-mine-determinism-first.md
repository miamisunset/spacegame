# FIFO OrderQueue with persistent Mine and determinism-first, save-deferred

Ships hold a FIFO VecDeque<Order> (FlyTo, Approach, Orbit, Mine) where only the front order ticks on FixedUpdate until completion then pops; Mine loops cycles (range-checked, fatigue-scaled) until cargo full or asteroid destroyed. We prove determinism first (10k FixedUpdate ticks with seeded WyRand give identical positions and ore counts) and defer postcard/DynamicWorld save-roundtrip to the next slice to avoid paying Reflect/serde tax before movement and mining are fun.

Considered options: single active order (rejected: can't script approach-then-mine) and save-ready now (deferred for scope).
