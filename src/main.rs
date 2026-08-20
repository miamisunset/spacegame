//! Thin binary — `anyhow` at the edge, `DefaultPlugins` + `SimPlugin`/`RenderPlugin`/`UiPlugin`.
//!
//! Dev-only troubleshooting: `cargo run --features brp` adds
//! `bevy_brp_extras::BrpExtrasPlugin::default()` (HTTP 15702, env `BRP_EXTRAS_PORT` overrides).
//! Never use `BRP`/`MCP` from `crates/*/tests`.

use bevy::prelude::*;
use spacegame_render::RenderPlugin;
use spacegame_sim::SimPlugin;
use spacegame_ui::UiPlugin;

fn main() -> anyhow::Result<()> {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins);

    #[cfg(feature = "brp")]
    {
        app.add_plugins(bevy_brp_extras::BrpExtrasPlugin::default());
    }

    app.add_plugins((SimPlugin, RenderPlugin, UiPlugin));
    app.run();

    Ok(())
}
