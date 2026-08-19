//! Save/load via bevy_world_serialization + postcard, versioned.
//! Uses DynamicWorldBuilder::from_world(world, &registry) + postcard::to_stdvec.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PersistError {
    #[error("postcard error: {0}")]
    Postcard(#[from] postcard::Error),
}
