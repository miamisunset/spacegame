# AGENTS.md

This document provides operational context, technical constraints, coding standards, and workflow contracts for AI coding agents, subagents, and automated assistants interacting with this Bevy Engine game repository.

## Project Overview

Single-player space sim — a cross between **EVE Online** (deep player-driven economy, large-scale combat, faction standing, market depth) and **X4: Foundations** (empire building, fleet management, station construction, order queues) — offline, persistent universe. No networking. Time runs real-time with X4-style SETA/time acceleration, so the simulation must be deterministic and tick on `FixedUpdate`.

## Agent Capabilities

Always check if there is an MCP tool or skill available before performing operations manually, deploying resources, setting up a new crate, generating code, and other common workflows.

**BRP / MCP (dev-only troubleshooting):** `bevy_brp_extras` is a dev-only, opt-in troubleshooting terminal. Only use the `brp` subagent (`brp_list_agent_tools` / `brp_execute`, `brp_extras/screenshot`, `click_mouse`, `send_keys`) after the app is running via `cargo run --features brp` (HTTP 15702, env `BRP_EXTRAS_PORT` overrides). Never use `BRP`/`MCP` from `crates/*/tests` — integration tests inspect `World` directly (`app.world().get::<T>()`, `query::<(&Transform,&Asteroid)>`). CI enforces this with `grep -R "BrpExtrasPlugin\|RemoteHttpPlugin\|brp_extras\|bevy_brp" crates/*/tests`.

**PR Content Rule:** When creating pull requests via the MCP tool, the PR body **must** list the exact files modified and any relevant Bevy system dependencies impacted (schedules / `SystemSet`s / `FixedUpdate` vs `Update`). If the PR is related to an issue, reference it using `Closes #<issue-number>` in the PR body.

## Recommended Actions

AI agents can assist with:

1. **Code Generation**
   - Writing new Rust code following Bevy's Entity Component System (ECS) architecture.
   - Generating `bsn!` and `bsn_list!` proc-macros for declarative UI (Feathers) and hierarchical scenes.
   - Generating unit tests (using `#[cfg(test)]` modules and Bevy `App` setups).
   - Running `cargo fmt` and `cargo clippy` on all modified crates (see [Linting and Formatting](#linting-and-formatting)).

2. **Code Review**
   - Identifying bugs, safety issues, or borrow-checker conflicts.
   - Suggesting improvements for idiomatic Rust and Bevy ECS patterns (e.g., avoiding "God systems," optimizing queries, or fixing scheduler conflicts).
   - Reviewing query filters and ensuring `Mut<T>` vs `&T` are used correctly for change detection; flag broad-query conflicts with resources.

3. **Documentation**
   - Improving inline documentation (using `///` doc comments).
   - Updating README files (use ` ```rust no_run ` for examples that require Bevy app loops).
   - Documenting Bevy Systems, Components, and Resources effectively.

4. **Refactoring**
   - Applying clippy suggestions.
   - Migrating imperative UI `commands.spawn(Node{..}).with_children(...)` to `bsn!` where the UI uses Feathers.
   - Updating dependencies in `Cargo.toml` (workspace `Cargo.toml` is source of truth).
   - Consolidating imports (e.g., `use bevy::prelude::*;` over individual imports when appropriate).

## Persona

You are an expert Rust programmer specializing in game development with the Bevy Engine. You write safe, highly parallel, efficient, and well-tested ECS code.

- Use an informal tone.
- Do not be overly apologetic; focus on clear, actionable guidance.
- If you cannot confidently generate code or other content, do not generate anything and ask for clarification.

## Agent skills

### Issue tracker

Issues live in GitHub Issues (gh CLI). See `docs/agents/issue-tracker.md`.

### Triage labels

Five canonical labels as-is (needs-triage, needs-info, ready-for-agent, ready-for-human, wontfix). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` + `docs/adr/` at repo root. See `docs/agents/domain.md`.

## Project Structure

Single crate today (`spacegame` at workspace root, `src/main.rs`), migrating to this workspace as soon as the first simulation system lands. Do not create ad-hoc `src/foo.rs` sprawl — follow the target layout so agents share seams:

```text
spacegame/                          # Cargo workspace root
├── Cargo.toml                      # [workspace] + [workspace.dependencies]
├── AGENTS.md
├── .config/nextest.toml            # nextest profiles [profile.default] / [profile.ci]
├── assets/                         # Bevy AssetServer root (see Asset Layout)
│   ├── data/                       # RON templates (ships, stations, wares, factions)
│   ├── textures/                   # ktx2/png atlases, ship/station textures, ui/icons
│   ├── shaders/                    # WGSL
│   ├── models/                     # glTF
│   └── ui/                         # reserved; BSN UI kept inline in crates/ui until .bsn loader ships
├── crates/
│   ├── spacegame_sim/              # deterministic headless sim — no bevy_render/bevy_pbr dep
│   │   └── tests/
│   │       ├── common/mod.rs       # shared headless harness (headless_app, world_hash, etc.)
│   │       └── headless_mining_loop.rs
│   ├── spacegame_render/           # Bevy render plugins, meshes, materials, lighting
│   ├── spacegame_ui/               # Feathers + bsn! UI (Button, Inventory, Map)
│   ├── spacegame_persist/          # save/load (postcard + DynamicWorld)
│   └── spacegame_data/             # typed RON loaders + registries
└── src/
    └── main.rs                     # thin binary: anyhow at the edge, DefaultPlugins + Sim/Ui plugins, optional BrpExtrasPlugin
```

Rules:

- `spacegame_sim` must `cargo nextest run -p spacegame_sim --all-features` **without** render features (`default-features = false`, `bevy = { features = ["bevy_asset","bevy_state","bevy_scene"] }`).
- `spacegame_data` owns `thiserror` typed parse errors for RON; `spacegame_persist` owns save versioning.
- Workspace `Cargo.toml` is the only place to add `bevy` / `bevy_brp_extras` / `thiserror` / `ron` / `postcard` / `serde` versions. `bevy_brp_extras = "0.22.3"` is pinned there (Bevy 0.19 row). Only `spacegame` binary opts into `bevy` `png` and `bevy_brp_extras` via `[features] brp = ["bevy_brp_extras","bevy/png"]` — no other crate enables `png`/`brp`.

## Asset Layout

- **Templates (RON):** `assets/data/ships/*.ron`, `assets/data/stations/*.ron`, `assets/data/wares.ron`, `assets/data/factions/*.ron` — attributes like `speed`, `mass`, `shield_resistance`, `armor`, `cargo_capacity`. Loaded via `ron::de` or an `AssetLoader`, `#[derive(Serialize, Deserialize, Reflect)]`. Never hand-code stats in Rust.
- **Textures / Icons:** `assets/textures/**` and `assets/textures/ui/icons/*.png` (256×256, premultiplied, `Handle<Image>`). Prefer `ktx2` for PBR; `png` for UI.
- **Shaders:** `assets/shaders/*.wgsl` via `bevy_shader`. `Hdr` lives in `bevy_camera`, `Skybox` image is `Option<Handle<Image>>`, `Atmosphere` is an entity in `bevy_light`, `bevy_material` split from `bevy_pbr` — import accordingly.
- **Models:** `assets/models/**/*.gltf` spawned via `WorldAssetRoot(handle)` (`bevy_world_serialization`), not `SceneRoot`. glTF via `bevy_gltf` still uses the world-serialization path in 0.19.
- **UI (BSN):** No `.bsn` asset file loader ships in 0.19. Keep BSN **inline** in `crates/spacegame_ui/src/**/*.rs` as `bsn!{ ... }` / `bsn_list!{ ... }` with `on(|e: On<Pointer<Click>>| {...})` observers. Do not create `assets/ui/*.bsn` expecting the asset server to load it.
- **Saves:** `saves/*.save` (outside `assets/`) — binary `postcard` snapshots, not hot-reloaded. History ring buffers (`VecDeque<PriceSample>` capped ~1k) live in ECS, not RON.

## Coding Conventions

### Bevy 0.19 Standards

- **Scenes & UI — `bsn!`/`bsn_list!` proc-macros:** Use `bsn!`/`bsn_list!` from `bevy_scene` (`bevy::scene::prelude::*`) for **declarative UI (Feathers) and scene hierarchies**, e.g. `bsn!{ Entity { Children [ Entity { Name("child") } ] } }`. Hierarchy maps to `Relationship` / `ChildOf` / `Children`. Spawn via `Commands::spawn_scene` / `World::spawn_scene`. Keeping BSN inline in `spacegame_ui`; do not assume a `.bsn` asset loader — it does not ship in 0.19. Imperative `commands.spawn(...).with_children(...)` remains valid for non-UI / non-scene code and quick sim factories. Old `bevy_scene` is now `bevy_world_serialization` (`Scene` → `WorldAsset`, `SceneRoot` → `WorldAssetRoot`, `DynamicScene` → `DynamicWorld`, builder now needs `&TypeRegistry`).

- **Resources are Components:** In Bevy 0.19 `Resource: Component` (`#[derive(Resource)]` implements both). **Never** `#[derive(Component, Resource)]` — it is a hard compile error; split into `MyDataComp` / `MyDataRes`. Inserting a `T: Resource` as a component despawns other copies (uniqueness). Resources appear in `Query<&T>` — broad queries `Query<Entity>`, `Query<EntityMut>`, `Query<()>`, `Query<Option<&T>>` now **conflict** with `Res<T>` / `NonSend<T>`; fix with `Without<IsResource>` or `Without<T>`. `#[reflect(Resource)]` is a ZST — use `ReflectComponent`. `MapEntities` is auto-implemented for `Resource`. `World::clear_entities` now also clears resources (`clear_all` clears NonSend too); `remove_resource_by_id` returns `bool`.

- **Required Components:** Use `#[require(...)]` for flat automatic dependencies, e.g. `#[derive(Component)] #[require(Transform, GlobalTransform)] struct Ship;` or `#[require(Inventory::default())]` / `#[require(Faction = init_faction())]`. Also available at runtime via `World::register_required_components::<A, B>()`. Reserve `bsn!` for UI/scene hierarchies; use `#[require]` for entity-level defaults.

- **Events & Observers:** Use Bevy Observers `Trigger<E>` / `On<E>` with `on(|e: On<Pointer<Press>>| {...})` **inside** `bsn!` for callback-style UI/collision behavior (observer run conditions and lifecycle `on_discard` — ex `on_replace` — are available in 0.19). Use buffered `MessageReader`/`MessageWriter` (`Message` is the 0.19 successor to `Event` for streams) for continuous data like market ticks or combat logs. See migration guide `0-18-to-0-19` sections on lifecycle observers.

- **Math:** Utilize `glam` via Bevy (`bevy::prelude::*` / `bevy_math`, `pub use glam::*`, version 0.32 — e.g. `Vec2`, `Vec3`, `Quat`, `Mat4`). Avoid custom math types.

- **Cargo Features (0.19):** `bevy` features are now granular — `audio` is **not** implied by `2d`/`3d`/`ui`, `ui` not implied by `2d`/`3d`, `bevy_picking` no longer pulls `bevy_input_focus`, `bevy_window`/`custom_cursor` moved to alternate collections, `bevy_material` split from `bevy_pbr`, `bevy_light` owns `Skybox`/`Atmosphere`. Headless `spacegame_sim` must be `bevy = { version = "0.19", default-features = false, features = ["bevy_asset","bevy_state","bevy_scene"] }` (add `multi-threaded` as needed); client/binary adds `["2d","3d","bevy_pbr","bevy_ui","audio","bevy_gltf","bevy_feathers","png"]` explicitly. `bevy_brp_extras` is `0.22.3` ↔ Bevy `0.19` per upstream matrix; only `spacegame` binary enables it via `brp` feature.

### Simulation Conventions (EVE × X4)

- `FixedUpdate` for deterministic economy/orders/AI/faction standing; `Update` only for render/input. Define `SystemSet`s `EconomySet`, `AiSet`, `MovementSet`, `CombatSet` with explicit ordering, gated by `in_state(GameState::Simulating)`.
- Deterministic RNG — seeded `WyRand` (or `rand` 0.9 as re-exported by Bevy 0.19), never `thread_rng` inside `spacegame_sim`.
- Data-driven: wares/ships/stations from `assets/data/**/*.ron`; no hardcoded stats.
- Avoid `&mut World` exclusive systems; batch `Commands` where hot (thousands of ships/stations); prefer `Changed<T>` / `Added<T>` with change lists and contiguous query access for performance. Use `Delayed Commands` / `Remote Entity Reservation` for deferred cross-frame spawns when needed.

### Naming

- Types, Components, Resources, Events, and Traits are `PascalCase`.
- Constants and statics are `UPPER_SNAKE_CASE`.
- Systems, system sets, functions, fields, and variables are `snake_case`.
- Crate and module names are `snake_case`.

### Imports

- Keep `use` directives at the top of the module.
- Prefer `use bevy::prelude::*;` for core engine types.
- Inside test modules, it is preferred to import APIs from `super`.
- Prefer merging new `use` directives into existing ones rather than creating new `use` blocks.

### Error Handling

- Domain crates (`spacegame_sim`, `spacegame_data`, `spacegame_persist`) use `thiserror` typed errors: `#[derive(Debug, thiserror::Error)]` with `#[error("...")]`, propagated via `?`.
- The binary / app edge (`src/main.rs`, integration code) uses `anyhow` for context (`anyhow::Context`), mapping `thiserror` via `?` and `map_err`.
- In Bevy systems, handle fallible operations gracefully. Use Bevy's logging macros (`bevy::log::error!`, `warn!`, `info!`) rather than unwrapping/panicking, to prevent the engine loop from crashing.

### Documentation

- Document all public APIs (Systems, Components, Resources) using a concise summary.
- Use Rust's document comment syntax (`///`) with Markdown.

### General

- Write idiomatic Rust code (e.g., implementing `From`, `Default`, and `Display`).
- Derive `Reflect` on custom Components and Resources when they need to be inspected, serialized, or used within scenes.
- Prioritize ECS safety, avoid unnecessary exclusive systems (`&mut World`), and respect Rust's borrowing rules inside queries.
- When finding references to a symbol for code changes, prefer using LSP (e.g., `findReferences`, `incomingCalls`) over text search for compiler-verified results. Use `workspaceSymbol` or `goToDefinition` to locate the symbol first.

## Building

```bash
# Build a specific crate
cargo build -p {crate-name}

# Build entire workspace
cargo build --workspace

# Verify dev-only BRP extras compile (no separate binary)
cargo check --features brp
```

Workspace dependencies (source of truth in root `Cargo.toml` `[workspace.dependencies]`):

```toml
[workspace.dependencies]
bevy = { version = "0.19", default-features = false, features = ["bevy_asset", "bevy_state", "bevy_scene"] }
bevy_brp_extras = "0.22.3"  # pinned to Bevy 0.19 row; only `spacegame` binary enables via `brp` feature
thiserror = "2"
anyhow = "1"
ron = "0.8"
postcard = { version = "1", features = ["use-std"] }
serde = { version = "1", features = ["derive"] }
```

Binary/client adds `bevy` with `features = ["2d","3d","bevy_pbr","bevy_ui","audio","bevy_gltf","bevy_feathers","png"]` plus `bevy_brp_extras` via `[features] brp = ["bevy_brp_extras","bevy/png"]`. No other crate enables `png` or `brp` — this keeps `spacegame_sim` headless (`default-features = false`) and avoids paying `bevy_render` cost in sim tests.

Troubleshooting (dev-only): `cargo run --features brp` adds `BrpExtrasPlugin::default()` (listens `127.0.0.1:15702`, env `BRP_EXTRAS_PORT` overrides). `brp_list_agent_tools` / `brp_execute` (`brp_extras/screenshot`, `click_mouse`, `send_keys`) work only after this plugin is added.

## Testing

`cargo nextest` is the sole runner — `cargo test` is forbidden (docs and CI use `nextest` exclusively to avoid runner-specific isolation drift). Each `crates/*/tests/*.rs` file runs as a separate binary in parallel, so adding tests doesn't linearly slow the suite.

```bash
# Headless sim (determinism, order-queue, perf budgets) — P0, no window
cargo nextest run -p spacegame_sim --all-features

# Full workspace (sim + data + render + ui)
cargo nextest run --workspace --all-features

# CI profile (retries, junit, 60s slow-timeout) — canonical CI command
cargo nextest run --profile ci --workspace --all-features
```

Profiles live in `.config/nextest.toml`:

```toml
[profile.default]
retries = 1
slow-timeout = { period = "60s", terminate-after = 5 }

[profile.ci]
retries = 2
slow-timeout = { period = "60s", terminate-after = 5 }

[profile.ci.junit]
path = "junit.xml"
```

### Headless harness & fixtures

- **Location:** `crates/spacegame_sim/tests/common/mod.rs` — shared helpers extracted from the mining loop: `headless_app()`, `wyrand_next`, `wyrand_vec3`, `world_hash`, `tick_n`, `miner_template()`/`miner_stats()`, `seeded_asteroid_positions`, `spawn_seeded_asteroids`.
- **Seam:** `headless_app()` = `App::new().add_plugins((MinimalPlugins, SimPlugin)) + TimeUpdateStrategy::ManualDuration(Time::<Fixed>::default().timestep())`. No `bevy_render`/`bevy_pbr`, no window/GPU, gated by `in_state(GameState::Simulating)`. `SimPlugin` wires `FixedUpdate` `EconomySet→AiSet→MovementSet→MiningSet→CombatSet` via `StatesPlugin` idempotent install.
- **Per-feature files:** One `crates/spacegame_sim/tests/*.rs` file per feature (determinism, economy invariants, order-queue, etc.) — not one monolithic file.
- **BRP/MCP forbid:** Integration tests never import `bevy_brp_extras`, `RemoteHttpPlugin`, or `brp_extras`. Tests inspect `World` directly (`app.world().get::<T>()`, `query::<(&Transform,&Asteroid)>`). `brp_extras` mouse/keyboard/screenshot remain MCP troubleshooting-only. CI fails on `grep -R "BrpExtrasPlugin\|RemoteHttpPlugin\|brp_extras\|bevy_brp" crates/*/tests`.
- **Full-app P1 (future):** `App::new().add_plugins((MinimalPlugins, SimPlugin, spacegame_render, spacegame_ui))` headless, synthetic `app.world_mut().trigger(...)` / observer `On<Pointer<Click>>` inside `bsn!` to verify `Camera3d` order 0 / `Camera2d` order 1 `ZIndex(100)` / `Pickable::IGNORE` / `HoverMap` occlusion — no `RemoteHttpPlugin`/`RemotePlugin`, no real window/GPU.

### Test Generation

- Tests should be generated in a tests module conditioned on `#[cfg(test)]`.
- Place the tests module at the bottom of the file being tested, or in a dedicated tests.rs or integration_test.rs.
- The tests module should always import APIs from `super`.
- Test functions do not need to be public.

### Simulation Testing (add for this project)

- Determinism: seeded `WyRand` run for 10k `FixedUpdate` ticks produces identical `world_hash` (paired `(Transform, Asteroid)` + ship transforms, sorted tuples) for a given seed — SETA scaling stays deterministic.
- Invariants: closed-economy `total_credits + ware_value` constant, no negative wares.
- Headless: `cargo nextest run -p spacegame_sim --all-features` must pass without a window; `cargo bench -p spacegame_sim` budgets market tick < 2ms at 1k stations (background, not PR-gating).
- Perf budgets are per-test asserts (`<2ms market tick @1k stations`, `<0.1ms headless tick 1 ship+2 Asteroids`) — 2× regressions fail before `nextest` 60s slow-timeout hang backstop.

## Git Workflow

### 1. Branching Strategy
- **Rule:** Never modify or commit code directly to the `main` branch. 
- **Workflow:** For every new feature or refactor request, use local git tools to create a short-lived branch named: `feature/short-description` or `fix/issue-name`.

### 2. Commit Standards (Conventional Commits)
- **Format:** All commit messages must strictly adhere to the Conventional Commits specification: `<type>(<scope>): <short description>`.
- **Allowed Types:** `feat` (new features), `fix` (bug fixes), `refactor` (code restructuring), `test` (adding/updating tests).
- **Example:** `feat(ui): implement spaceship docking`
- **Constraint:** Keep descriptions present-tense, imperative, and under 72 characters.

### 3. Github

Prefer Github MCP over `gh` command.

## Persistence & Data

- **Templates vs live state:** `RON` in `assets/data/**` for templates (ship/station/ware/faction attributes); live market (orders, prices, inventories, standing) is ECS `Component`/`Resource` in `spacegame_sim`, not RON.
- **Saves:** binary `postcard` snapshots (`serde`) in `saves/*.save` via `bevy_world_serialization` (`DynamicWorldBuilder::from_world(world, &registry)` needing `&TypeRegistry`) + `postcard::to_stdvec`; `IsResource` entities included via the same path. No SQL/DB in MVP.
- **History:** `VecDeque<PriceSample>` capped ~1k per `(Station, Ware)` in ECS. A persistent DB is **deferred** — if later needed for SQL analytics/external tooling, use `redb` (pure-Rust KV, fastest point writes) or `rusqlite` `bundled` (SQLite, full SQL `JOIN/GROUP/WINDOW` for `best-price`/`trade-route` profits). Do not add either until history beyond 1k or SQL queries are required.

## Linting and Formatting

Always run `cargo fmt` and `cargo clippy` when any `.rs` file has been modified. Wait until all code changes are complete before running these commands. This must be done before committing, opening a pull request, or presenting changes. Fix all warnings and errors before proceeding.

```bash
# Format code
cargo fmt -p {crate-name}

# Lint code
cargo clippy -p {crate-name}

# Auto-fix some issues
cargo clippy --fix -p {crate-name}
```

CI gate is `cargo nextest run --profile ci --workspace --all-features` plus the BRP grep gate (`grep -R "BrpExtrasPlugin\|RemoteHttpPlugin\|brp_extras\|bevy_brp" crates/*/tests` must be empty) and `cargo fmt --check` / `cargo clippy --workspace --all-features`. `cargo test` is not used.

Migration reference: `https://bevy.org/learn/migration-guides/0-18-to-0-19/` (note `0-18` hyphen, not `0.18`).
