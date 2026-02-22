use bevy::prelude::*;
use noise::{NoiseFn, Seedable, SuperSimplex};

/// Total number of noise octaves. Covers detail from continental (~1000 km)
/// down to ~10m scale at radius 6360 with noise_scale 4.0.
pub const TOTAL_OCTAVES: usize = 20;

/// Number of octaves used at depth 0 (coarsest LOD).
pub const BASE_OCTAVES: usize = 6;

/// Central configuration for terrain generation.
/// Uses a manual Fbm implementation so we can cap octaves per LOD level.
#[derive(Clone, Debug)]
pub struct TerrainConfig {
    sources: Vec<SuperSimplex>,
    frequency: f64,
    lacunarity: f64,
    persistence: f64,
    /// Pre-computed amplitude sum for TOTAL_OCTAVES, used as a fixed divisor
    /// so that changing octave count adds detail without rescaling earlier frequencies.
    full_amp_sum: f64,
    pub radius: f32,
    pub noise_scale: f32,
    pub amplitude: f32,
}

impl TerrainConfig {
    pub fn new(radius: f32, noise_scale: f32, amplitude: f32, seed: u32) -> Self {
        let mut sources = Vec::with_capacity(TOTAL_OCTAVES);
        for i in 0..TOTAL_OCTAVES {
            sources.push(SuperSimplex::default().set_seed(seed + i as u32));
        }

        let persistence = 0.5f64;
        let mut full_amp_sum = 0.0f64;
        let mut amp = 1.0f64;
        for _ in 0..TOTAL_OCTAVES {
            full_amp_sum += amp;
            amp *= persistence;
        }

        Self {
            sources,
            frequency: 1.0,
            lacunarity: 2.0,
            persistence,
            full_amp_sum,
            radius,
            noise_scale,
            amplitude,
        }
    }

    /// Evaluate Fbm noise using up to `max_octaves` octaves.
    fn sample_noise(&self, dir: Vec3, max_octaves: usize) -> f32 {
        let octaves = max_octaves.min(self.sources.len());
        let mut point = [
            dir.x as f64 * self.noise_scale as f64,
            dir.y as f64 * self.noise_scale as f64,
            dir.z as f64 * self.noise_scale as f64,
        ];

        point[0] *= self.frequency;
        point[1] *= self.frequency;
        point[2] *= self.frequency;

        let mut result = 0.0f64;
        let mut amplitude = 1.0f64;

        for i in 0..octaves {
            let signal = self.sources[i].get(point);
            result += signal * amplitude;

            amplitude *= self.persistence;
            point[0] *= self.lacunarity;
            point[1] *= self.lacunarity;
            point[2] *= self.lacunarity;
        }

        // Divide by the fixed full-octave amplitude sum so that adding octaves
        // adds fine detail without rescaling earlier frequencies.
        (result / self.full_amp_sum) as f32
    }

    /// Returns the displaced position using a limited number of octaves.
    /// Use this for vertex positions — coarse LOD chunks skip fine octaves.
    pub fn get_displaced_position_lod(&self, normalized_dir: Vec3, max_octaves: usize) -> Vec3 {
        let noise_value = self.sample_noise(normalized_dir, max_octaves);
        let elevation = self.radius + (noise_value * self.amplitude);
        normalized_dir * elevation
    }
}
