use bevy::prelude::*;
use bevy::math::DVec3;
use big_space::prelude::*;

#[derive(Component, Debug, Clone)]
pub struct Orbit {
    pub semi_major_axis: f64,
    pub eccentricity: f64,
    pub inclination: f32,
    pub longitude_of_ascending_node: f32,
    pub argument_of_periapsis: f32,
    pub period: f64,
    pub initial_mean_anomaly: f64,
    pub parent: Option<Entity>,
}

#[derive(Resource)]
pub struct OrbitalTime {
    pub speed: f64, 
    pub elapsed: f64,
    pub start_time: Option<f64>,
}

impl Default for OrbitalTime {
    fn default() -> Self {
        Self {
            speed: 1.0,
            elapsed: 0.0,
            start_time: None,
        }
    }
}

pub struct OrbitPlugin;

impl Plugin for OrbitPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(OrbitalTime::default())
            .add_systems(Update, update_orbits);
    }
}

fn update_orbits(
    time: Res<Time>,
    mut orbital_time: ResMut<OrbitalTime>,
    grid_q: Query<&Grid, With<BigSpace>>,
    mut params: ParamSet<(
        Query<(Entity, &Orbit)>,
        Query<(Entity, &mut CellCoord, &mut Transform)>,
    )>,
) {
    let Ok(grid) = grid_q.single() else { return };
    
    // Sync to absolute engine time to avoid accumulation drift
    let current_time = time.elapsed_secs_f64();
    if orbital_time.start_time.is_none() {
        orbital_time.start_time = Some(current_time);
    }
    
    orbital_time.elapsed = (current_time - orbital_time.start_time.unwrap()) * orbital_time.speed;
    let t = orbital_time.elapsed;

    // 1. Calculate local orbital positions for all bodies
    let mut orbit_data: Vec<(Entity, DVec3, Option<Entity>)> = Vec::new();
    {
        let q = params.p0();
        for (entity, orbit) in &q {
            let local_pos = calculate_keplerian_position(orbit, t);
            orbit_data.push((entity, local_pos, orbit.parent));
        }
    }

    // 2. Resolve parent positions from the same timestep (no 1-frame lag).
    //    Build a lookup so children can find their parent's orbital position.
    let pos_map: std::collections::HashMap<Entity, DVec3> = orbit_data
        .iter()
        .map(|(e, p, _)| (*e, *p))
        .collect();

    let mut updates: Vec<(Entity, DVec3)> = Vec::new();
    for (entity, local_pos, parent_opt) in &orbit_data {
        let mut world_pos = *local_pos;
        if let Some(parent_entity) = parent_opt {
            if let Some(&parent_pos) = pos_map.get(parent_entity) {
                world_pos += parent_pos;
            }
        }
        updates.push((*entity, world_pos));
    }

    // 3. Apply updates using Grid::translation_to_grid for proper cell conversion.
    //    Grid::default() has cell_edge_length=2000, so raw floor() on world positions
    //    would place entities 2000x too far apart.
    {
        let mut q_apply = params.p1();
        for (entity, world_pos) in updates {
            if let Ok((_, mut cell, mut transform)) = q_apply.get_mut(entity) {
                let (new_cell, offset) = grid.translation_to_grid(world_pos);
                *cell = new_cell;
                transform.translation = offset;
            }
        }
    }
}

fn calculate_keplerian_position(orbit: &Orbit, time: f64) -> DVec3 {
    let mean_anomaly = orbit.initial_mean_anomaly
        + (2.0 * std::f64::consts::PI / orbit.period) * time;

    let mut e_anom = mean_anomaly;
    for _ in 0..10 {
        let delta = (e_anom - orbit.eccentricity * e_anom.sin() - mean_anomaly) 
                  / (1.0 - orbit.eccentricity * e_anom.cos());
        e_anom -= delta;
        if delta.abs() < 1e-10 { break; }
    }

    let cos_e = e_anom.cos();
    let sin_e = e_anom.sin();
    
    let x_orb = orbit.semi_major_axis * (cos_e - orbit.eccentricity);
    let y_orb = orbit.semi_major_axis * (1.0 - orbit.eccentricity * orbit.eccentricity).sqrt() * sin_e;

    let cos_inc = (orbit.inclination as f64).cos();
    let sin_inc = (orbit.inclination as f64).sin();
    let cos_lan = (orbit.longitude_of_ascending_node as f64).cos();
    let sin_lan = (orbit.longitude_of_ascending_node as f64).sin();
    let cos_arg = (orbit.argument_of_periapsis as f64).cos();
    let sin_arg = (orbit.argument_of_periapsis as f64).sin();

    let x_node = x_orb * cos_arg - y_orb * sin_arg;
    let y_node = x_orb * sin_arg + y_orb * cos_arg;

    DVec3::new(
        x_node * cos_lan - y_node * sin_lan * cos_inc,
        x_node * sin_lan + y_node * cos_lan * cos_inc,
        y_node * sin_inc
    )
}
