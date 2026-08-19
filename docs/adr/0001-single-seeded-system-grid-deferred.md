# Single seeded System (Grid) now, EVE-scale AU/Warp/Grid sharding deferred

We model the world as one bounded System (100–500 km box) deterministically seeded with WyRand for asteroid placement, using continuous f32 coordinates and no warp or gate jump. EVE stores true AU distances in its DB but runs gameplay inside 500 km Grids with warp as a teleport state and gate jumps as system switches — we defer that sharding, keep a SystemId component for future multi-system, and avoid AU-scale precision or streaming until mining and order queues prove fun.
