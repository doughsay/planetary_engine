use bevy::prelude::*;
use crate::terrain::TerrainConfig;

#[derive(Component, Debug)]
pub struct Planet {
    pub config: TerrainConfig,
}

pub struct PlanetPlugin;

impl Plugin for PlanetPlugin {
    fn build(&self, _app: &mut App) {
        // Planet-specific systems can go here if needed.
    }
}
