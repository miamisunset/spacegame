//! Feathers + bsn! UI — keep BSN inline here.
//! Use `bsn!{ Entity { Children [ ... ] } }` with `on(|e: On<Pointer<Click>>| {...})`
//! No .bsn asset file loader in Bevy 0.19.
use bevy::prelude::*;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, _app: &mut App) {}
}
