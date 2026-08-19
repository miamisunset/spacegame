# Spacegame

Persistent single-player space tycoon — EVE Online depth with X4 Foundations empire building — offline, deterministic, time-accelerated. The player never has an avatar; all action is through crews and queued orders.

## Language

### Empire & Actors

**Empire**: The player's persistent organization that owns ships, crews, stations and credits.
_Avoid_: Player, Corporation, Account

**Crew**: A named entity with a role (e.g. Miner) and skill/fatigue that staffs a ship and scales its module performance.
_Avoid_: Pilot, Character, Avatar, Captain

**Ship**: A mobile entity with a Transform, cargo inventory and modules, always operated by assigned crew.
_Avoid_: Vessel, Unit, Actor

**Station**: A fixed entity in a system where ships can dock and transfer wares or credits. Not in slice 1.
_Avoid_: Base, Outpost

**Faction**: An NPC organization with standing toward the empire. Distinct from Empire.
_Avoid_: Corporation (when meaning NPC)

### Simulation

**System**: A bounded, seeded volume of space (a Grid in EVE terms) containing ships, stations and asteroids. One System is the entire world for slice 1.
_Avoid_: Sector, Zone, Map, Solar System (until AU/warp lands)

**Ware**: A tradable good type with volume, e.g. Ore. Defined in RON, instantiated as inventory counts.
_Avoid_: Item, Resource, Commodity, Good

**Asteroid**: A depletable source entity containing a finite amount of a ware; destroyed at zero and respawned after a timed queue.
_Avoid_: Rock, Node, Field

**Inventory**: The ware counts held by a ship or station, constrained by cargo capacity and ware volume.
_Avoid_: Cargo (use only for capacity), Storage

### Orders & Movement

**Order**: A single CEO-issued instruction to a ship: FlyTo (point), Approach (entity), Orbit (entity + range) or Mine (entity).
_Avoid_: Command, Task, Job

**OrderQueue**: FIFO queue of Orders on a ship; the front order ticks to completion on FixedUpdate before popping. Mine loops until cargo full or asteroid destroyed.
_Avoid_: Queue alone, OrderList, CommandBuffer

**Module**: A functional component on a ship, e.g. MiningLaser, that executes an Order with cycle time and yield.
_Avoid_: Fitting, Slot, Upgrade, Equipment

**Orbit**: A steering behavior that maintains tangential velocity at a fixed range around a target entity.
_Avoid_: Circle, Hold
