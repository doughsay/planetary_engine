# Technical Design Details

## 1. Density Field Design

### Trait Definition

```rust
/// A density field defines the implicit surface of a planet.
/// Negative values = inside solid, Positive = outside (air), Zero = surface.
pub trait DensityField: Send + Sync {
    /// Sample the density at a world-space position (relative to planet center).
    fn sample(&self, pos: Vec3) -> f32;

    /// Compute the gradient (for normals).
    /// Implementors should prefer analytical gradients where possible.
    /// The `eps` parameter is the finite-difference step size, scaled by the
    /// caller to match the current LOD (node_size / resolution).
    fn gradient(&self, pos: Vec3, eps: f32) -> Vec3;
}
```

### Spherical Planet Density

```rust
pub struct SphericalPlanetDensity {
    pub radius: f32,
    pub noise: Fbm<SuperSimplex>,
    pub amplitude: f32,
    pub frequency: f64,
    // Additional noise layers for caves, overhangs, etc.
    pub cave_noise: Option<Fbm<SuperSimplex>>,
    pub cave_threshold: f32,
}

impl DensityField for SphericalPlanetDensity {
    fn sample(&self, pos: Vec3) -> f32 {
        let dist = pos.length();
        let dir = pos / dist;

        // Base sphere: negative inside, positive outside
        let mut density = dist - self.radius;

        // Surface displacement (same noise as current TerrainConfig)
        let noise_val = self.noise.get([
            (dir.x as f64) * self.frequency,
            (dir.y as f64) * self.frequency,
            (dir.z as f64) * self.frequency,
        ]) as f32;
        density -= noise_val * self.amplitude;

        // Optional cave carving
        if let Some(ref cave_noise) = self.cave_noise {
            let cave_val = cave_noise.get([
                pos.x as f64 * 0.01,
                pos.y as f64 * 0.01,
                pos.z as f64 * 0.01,
            ]) as f32;
            if cave_val > self.cave_threshold {
                density = density.max(cave_val - self.cave_threshold);
            }
        }

        density
    }

    fn gradient(&self, pos: Vec3, eps: f32) -> Vec3 {
        let dist = pos.length();
        let sphere_normal = pos / dist; // analytical gradient of base sphere

        // For the noise displacement, use finite differences for now.
        // TODO: Replace with analytical simplex gradient when available.
        // The eps is pre-scaled by the caller to match LOD level.
        let fd = finite_difference_gradient(self, pos, eps);

        // When far from surface (dist >> radius ± amplitude), the sphere
        // gradient dominates and finite differences on noise are stable.
        // When close to surface, the LOD-scaled eps keeps it accurate.
        fd
    }
}
```

### Gradient Strategy: Analytical vs Finite Differences

**Prefer analytical gradients.** The density function is composed of known mathematical operations — each has a known derivative:

1. **Base sphere**: `density = length(pos) - radius`
   - Gradient: `normalize(pos)` (trivially analytical)

2. **Noise displacement**: The `noise` crate's `SuperSimplex` doesn't expose analytical derivatives, but the math is well-defined. Options:
   - **Fork or wrap the noise crate** to compute derivatives alongside values (Simplex-type noise has cheap analytical gradients — the same permutation table lookups that produce the value also produce partial derivatives with minimal extra work)
   - **Use a noise library with built-in gradients** — evaluate during Phase 2 whether alternatives like `fastnoise-lite` or a custom Simplex implementation provide this
   - **Fall back to finite differences** only for noise layers where analytical gradients aren't available

3. **Cave carving (max/min operations)**: `density = max(sphere_density, cave_density)` — gradient is the gradient of whichever operand "wins" (i.e., has the larger value). This is piecewise analytical.

**Finite differences as fallback:**

When analytical gradients aren't available, finite differences work but the epsilon must be scaled to the LOD level. The caller (surface extraction) passes `eps = node_size / resolution` — this matches the sampling grid spacing, giving normals that are consistent with the mesh geometry:

```rust
/// Fallback finite-difference gradient. Costs 6 extra density samples.
fn finite_difference_gradient(field: &dyn DensityField, pos: Vec3, eps: f32) -> Vec3 {
    let dx = field.sample(pos + Vec3::X * eps) - field.sample(pos - Vec3::X * eps);
    let dy = field.sample(pos + Vec3::Y * eps) - field.sample(pos - Vec3::Y * eps);
    let dz = field.sample(pos + Vec3::Z * eps) - field.sample(pos - Vec3::Z * eps);
    Vec3::new(dx, dy, dz).normalize()
}
```

**Why this matters at planet scale:** The octree spans ~5 orders of magnitude (6360 km root node down to ~1m leaf). A fixed epsilon that works at the surface (eps=0.1 km) produces completely wrong normals at depth 15 (where features are sub-meter). Conversely, eps=0.001 at orbit distance would sample two nearly-identical density values and produce jittery normals from floating-point noise. Scaling eps to node size avoids both failure modes.

**Recommended approach for Phase 2:** Start with finite differences using scaled eps (it works, it's simple). Investigate analytical noise gradients as a performance/quality optimization once the pipeline is functional. The trait signature accepts `eps` either way — analytical implementations simply ignore it.

---

## 2. Octree Design

### Node Identification

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct OctreeNodeId {
    pub depth: u8,
    pub x: u32,
    pub y: u32,
    pub z: u32,
}
```

At depth `d`, coordinates range from `0` to `2^d - 1` in each axis. The root node is `(0, 0, 0, 0)`.

### Spatial Bounds

```rust
impl OctreeNodeId {
    /// Returns the AABB of this node in planet-local space.
    /// The root node spans [-radius, +radius] on all axes.
    pub fn bounds(&self, planet_radius: f32) -> (Vec3, Vec3) {
        let size = (planet_radius * 2.0) / (1 << self.depth) as f32;
        let origin = Vec3::new(
            -planet_radius + self.x as f32 * size,
            -planet_radius + self.y as f32 * size,
            -planet_radius + self.z as f32 * size,
        );
        (origin, origin + Vec3::splat(size))
    }

    pub fn center(&self, planet_radius: f32) -> Vec3 {
        let (min, max) = self.bounds(planet_radius);
        (min + max) * 0.5
    }

    pub fn size(&self, planet_radius: f32) -> f32 {
        (planet_radius * 2.0) / (1 << self.depth) as f32
    }
}
```

### Sparse Storage

Like the current `FaceQuadtree`, we use a HashMap for sparse storage:

```rust
pub enum OctreeState {
    Leaf,
    Split,
}

pub struct SparseOctree {
    nodes: HashMap<OctreeNodeId, OctreeState>,
    max_depth: u8,
}
```

Only nodes that contain the surface (density sign change within their bounds) need to exist in the tree. Fully-inside or fully-outside nodes are skipped entirely.

### Surface Detection

To check if a node contains the surface, sample density at the 8 corners of its AABB. If any two corners have different signs, the surface crosses this node. For robustness, also sample the center and edge midpoints.

### Children

```rust
impl OctreeNodeId {
    pub fn children(&self) -> [OctreeNodeId; 8] {
        let d = self.depth + 1;
        let x = self.x * 2;
        let y = self.y * 2;
        let z = self.z * 2;
        [
            OctreeNodeId { depth: d, x,     y,     z     },
            OctreeNodeId { depth: d, x + 1, y,     z     },
            OctreeNodeId { depth: d, x,     y + 1, z     },
            OctreeNodeId { depth: d, x + 1, y + 1, z     },
            OctreeNodeId { depth: d, x,     y,     z + 1 },
            OctreeNodeId { depth: d, x + 1, y,     z + 1 },
            OctreeNodeId { depth: d, x,     y + 1, z + 1 },
            OctreeNodeId { depth: d, x + 1, y + 1, z + 1 },
        ]
    }
}
```

### Neighbor Lookup

For seam stitching, we need to know adjacent nodes. Unlike the quadtree's cross-face complexity, the octree is simpler — just offset coordinates. At boundaries (x/y/z = 0 or max), the neighbor is outside the tree (empty space).

```rust
impl OctreeNodeId {
    /// Get the neighbor in a given direction (+/-X, +/-Y, +/-Z).
    pub fn neighbor(&self, dir: IVec3) -> Option<OctreeNodeId> {
        let max_coord = (1u32 << self.depth) - 1;
        let nx = self.x as i64 + dir.x as i64;
        let ny = self.y as i64 + dir.y as i64;
        let nz = self.z as i64 + dir.z as i64;

        if nx < 0 || ny < 0 || nz < 0
            || nx > max_coord as i64
            || ny > max_coord as i64
            || nz > max_coord as i64
        {
            return None; // Outside the planet's bounding cube
        }

        Some(OctreeNodeId {
            depth: self.depth,
            x: nx as u32,
            y: ny as u32,
            z: nz as u32,
        })
    }
}
```

---

## 3. Surface Extraction Pipeline

### Integration with `isosurface` Crate

The `isosurface` crate expects a `Source` trait implementation:

```rust
use isosurface::source::Source;

struct DensityFieldSource<'a> {
    field: &'a dyn DensityField,
    /// Transform from [0,1]^3 sample space to world space
    offset: Vec3,
    scale: f32,
}

impl<'a> Source for DensityFieldSource<'a> {
    fn sample(&self, x: f32, y: f32, z: f32) -> f32 {
        let world_pos = self.offset + Vec3::new(x, y, z) * self.scale;
        self.field.sample(world_pos)
    }
}
```

### Mesh Generation Per Chunk

```rust
pub fn extract_chunk_mesh(
    density: &dyn DensityField,
    node: OctreeNodeId,
    planet_radius: f32,
    resolution: usize, // e.g., 32
) -> Mesh {
    let (min, max) = node.bounds(planet_radius);
    let source = DensityFieldSource {
        field: density,
        offset: min,
        scale: max.x - min.x, // node is cubic
    };

    // Use isosurface crate's Marching Cubes or Dual Contouring
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    // MarchingCubes::new(resolution).extract(&source, &mut extractor);
    // ... convert to Bevy Mesh

    build_bevy_mesh(vertices, indices)
}
```

### Mesh Resolution

Each chunk gets a fixed grid resolution (e.g., 32^3 = 32,768 sample points). This is comparable to the current 33x33 = 1,089 vertices per chunk, but in 3D. The actual triangle count depends on how much surface passes through the chunk — chunks with little surface area produce few triangles.

### Normals

Two approaches:
1. **Density gradient**: compute `gradient(pos)` at each vertex — smooth, analytical
2. **Face normals**: compute from triangle geometry — faceted but cheap

Start with density gradient for quality; face normals can be a performance fallback.

### UVs and Texturing

Triplanar projection: for each vertex, compute UV from world position projected onto the dominant axis plane. This avoids the UV distortion problems of sphere projection and works naturally with arbitrary topology (caves, overhangs).

---

## 4. LOD Decision Logic

### Screen-Space Error (adapted from current system)

```rust
fn should_split(
    node: OctreeNodeId,
    planet_radius: f32,
    camera_pos: Vec3,
    perspective_scale: f32,
    resolution: usize,
) -> bool {
    let node_size = node.size(planet_radius);
    let geometric_error = node_size / resolution as f32;
    let distance = (node.center(planet_radius) - camera_pos).length();
    let pixel_error = geometric_error / distance * perspective_scale;

    pixel_error > SPLIT_THRESHOLD
}
```

### Constraints (same as current system)

- Max 1 level difference between adjacent leaves (forced splits propagate)
- Max N splits per frame (16 currently, may need tuning for 3D)
- Max depth limit (15, giving ~1m resolution at planet scale)
- Only split nodes that contain the surface

### Memory Considerations

An octree has 8 children per node vs 4 for a quadtree. At the same depth, the octree has 2x more nodes per axis (8^d vs 4^d). However, the sparsity optimization (skip nodes without surface) means the actual node count is proportional to the surface area, not the volume. A sphere's surface at depth d has ~6 * 4^d surface-crossing nodes — similar to the current quadtree.

---

## 5. Seam Stitching Approaches

### Option A: Transvoxel (Recommended)

Eric Lengyel's Transvoxel algorithm generates special transition cells at LOD boundaries:
- Cells touching a coarser neighbor use a modified lookup table
- Produces watertight meshes with no cracks
- Well-documented (paper + book available)
- Most complex to implement but best visual results

### Option B: Skirt Geometry (Simplest)

Extend each chunk's boundary vertices downward (toward planet center) by a small amount:
- Dead simple to implement
- Hides gaps by overlapping
- Can produce slight visual artifacts at extreme viewing angles
- Good for prototyping; can be replaced later

### Option C: Boundary Sample Agreement

Force adjacent chunks to use the same density samples at their shared boundary:
- The coarser chunk's boundary samples become the authoritative values
- The finer chunk interpolates its boundary to match
- Moderate complexity, clean results
- Similar to the current quadtree's edge-snapping approach

### Recommendation

Start with **Option B (skirts)** for the initial implementation to get things working visually. Then upgrade to **Option A (Transvoxel)** or **Option C** for production quality. The rendering pipeline doesn't need to change — only the mesh generation step differs.

---

## 6. `big_space` Integration Details

### Grid Cell Size

With `GridCell<i64>` and default cell size of 1 unit, each cell is 1 km (since our world units are km). For a planet at radius 6360, the planet spans ~12,720 cells across. This is well within i64 range.

For interplanetary distances (e.g., Earth-Moon ~384,400 km), we need ~384,400 cells. Still trivially within i64 range.

### Entity Hierarchy

```
Camera
  ├── GridCell<i64>
  ├── FloatingOrigin
  ├── Transform (local offset within cell)
  └── SpaceCamera

Planet (parent entity)
  ├── GridCell<i64>  (planet position in solar system)
  ├── Transform      (always IDENTITY, position is in GridCell)
  ├── Planet { radius, density_config, ... }
  │
  ├── Chunk (child entity)
  │   ├── GridCell<i64>  (inherited from parent? or own?)
  │   ├── Transform      (offset from planet center)
  │   ├── Mesh3d
  │   └── MeshMaterial3d
  │
  └── Atmosphere (child entity)
      ├── Transform
      ├── Mesh3d (icosphere)
      └── MeshMaterial3d<AtmosphereMaterial>
```

### Camera System Changes

The `SpaceCamera` movement system needs to update `GridCell` when the camera moves:
- Small movements: update `Transform` only
- Crossing cell boundary: update `GridCell`, reset `Transform` offset
- `big_space` may handle this automatically via its systems

---

## 7. Orbital Mechanics

### Orbit Component

```rust
#[derive(Component)]
pub struct Orbit {
    /// Distance from parent body at closest approach (km)
    pub semi_major_axis: f64,
    /// 0.0 = circular, 0.0-1.0 = elliptical
    pub eccentricity: f64,
    /// Tilt of orbital plane relative to reference plane (radians)
    pub inclination: f32,
    /// Rotation of the ascending node (radians)
    pub longitude_of_ascending_node: f32,
    /// Rotation of periapsis within the orbital plane (radians)
    pub argument_of_periapsis: f32,
    /// Orbital period (seconds of simulation time)
    pub period: f64,
    /// Starting angle at time=0 (radians, 0 = periapsis)
    pub initial_mean_anomaly: f64,
    /// Entity this body orbits (None = orbits the origin/star)
    pub parent: Option<Entity>,
}
```

### Keplerian Position Calculation

Each frame, compute the body's position from elapsed time:

```rust
fn orbital_position(orbit: &Orbit, time: f64) -> DVec3 {
    // 1. Mean anomaly: linear in time
    let mean_anomaly = orbit.initial_mean_anomaly
        + (2.0 * std::f64::consts::PI / orbit.period) * time;

    // 2. Eccentric anomaly: solve Kepler's equation M = E - e*sin(E)
    //    Newton's method, converges in 3-5 iterations for e < 0.9
    let eccentric_anomaly = solve_kepler(mean_anomaly, orbit.eccentricity);

    // 3. True anomaly: angle from periapsis
    let cos_e = eccentric_anomaly.cos();
    let true_anomaly = ((1.0 - orbit.eccentricity * cos_e).atan2(
        ((1.0 - orbit.eccentricity.powi(2)).sqrt() * eccentric_anomaly.sin()),
    ));

    // 4. Radius (distance from focus)
    let radius = orbit.semi_major_axis * (1.0 - orbit.eccentricity * cos_e);

    // 5. Position in orbital plane (2D)
    let x_orb = radius * true_anomaly.cos();
    let y_orb = radius * true_anomaly.sin();

    // 6. Rotate into 3D by inclination, ascending node, arg of periapsis
    rotate_to_3d(x_orb, y_orb, orbit)
}

fn solve_kepler(mean_anomaly: f64, eccentricity: f64) -> f64 {
    let mut e = mean_anomaly; // initial guess
    for _ in 0..10 {
        let de = (e - eccentricity * e.sin() - mean_anomaly)
            / (1.0 - eccentricity * e.cos());
        e -= de;
        if de.abs() < 1e-12 { break; }
    }
    e
}
```

### Hierarchical Orbits

Moons orbit planets, planets orbit the star. The `parent` field on `Orbit` enables this:

```rust
fn update_orbital_positions(
    time: Res<Time>,
    mut query: Query<(&Orbit, &mut GridCell<i64>, &mut Transform)>,
    parent_query: Query<&GridCell<i64>>,
) {
    let t = time.elapsed_secs_f64();

    for (orbit, mut cell, mut transform) in &mut query {
        let local_pos = orbital_position(orbit, t);

        // Convert f64 km position to GridCell + Transform offset
        let cell_x = local_pos.x.floor() as i64;
        let cell_y = local_pos.y.floor() as i64;
        let cell_z = local_pos.z.floor() as i64;
        let offset = Vec3::new(
            (local_pos.x - cell_x as f64) as f32,
            (local_pos.y - cell_y as f64) as f32,
            (local_pos.z - cell_z as f64) as f32,
        );

        // If orbiting a parent body, add parent's grid cell
        let (base_x, base_y, base_z) = if let Some(parent_entity) = orbit.parent {
            if let Ok(parent_cell) = parent_query.get(parent_entity) {
                (parent_cell.x, parent_cell.y, parent_cell.z)
            } else {
                (0, 0, 0)
            }
        } else {
            (0, 0, 0) // orbits origin (star)
        };

        *cell = GridCell::<i64>::new(
            base_x + cell_x,
            base_y + cell_y,
            base_z + cell_z,
        );
        transform.translation = offset;
    }
}
```

### Sun Direction Per Planet

The atmosphere shader and directional light need to know where the sun is relative to each planet. With `big_space`, this is computed from grid cells:

```rust
fn update_sun_direction(
    star_query: Query<&GridCell<i64>, With<Star>>,
    mut planet_query: Query<(&GridCell<i64>, &mut Planet)>,
) {
    let Ok(star_cell) = star_query.single() else { return };

    for (planet_cell, mut planet) in &mut planet_query {
        // Direction from planet to star in grid-cell space
        let dx = (star_cell.x - planet_cell.x) as f32;
        let dy = (star_cell.y - planet_cell.y) as f32;
        let dz = (star_cell.z - planet_cell.z) as f32;
        planet.sun_direction = Vec3::new(dx, dy, dz).normalize();
    }
}
```

This replaces the current hardcoded sun position in `main.rs` with a dynamic value derived from actual entity positions.

### Example Planetary System

```rust
// Star at origin
commands.spawn((Star, GridCell::<i64>::default(), Transform::default()));

// Earth-like planet
let earth = commands.spawn((
    Planet { radius: 6360.0, /* ... */ },
    Orbit {
        semi_major_axis: 149_600_000.0, // 1 AU in km
        eccentricity: 0.017,
        inclination: 0.0,
        period: 365.25 * 24.0 * 3600.0, // 1 year in seconds
        initial_mean_anomaly: 0.0,
        parent: None, // orbits star
        ..default()
    },
    GridCell::<i64>::default(),
    Transform::default(),
)).id();

// Moon
commands.spawn((
    Planet { radius: 1737.0, /* ... */ },
    Orbit {
        semi_major_axis: 384_400.0,
        eccentricity: 0.055,
        inclination: 5.14_f32.to_radians(),
        period: 27.3 * 24.0 * 3600.0, // ~27 days
        initial_mean_anomaly: 0.0,
        parent: Some(earth), // orbits earth
        ..default()
    },
    GridCell::<i64>::default(),
    Transform::default(),
));
```

### Time Control

For demonstration purposes, orbital time should be controllable:

```rust
#[derive(Resource)]
pub struct OrbitalTime {
    pub speed: f64,  // 1.0 = realtime, 86400.0 = 1 day/sec
    pub paused: bool,
}
```

At 1x speed, planetary orbits take a real year. At 86400x, one orbit takes ~6 minutes. A UI slider or keyboard shortcut (e.g., `[`/`]` to slow/speed) makes this interactive.

---

## 8. Per-Planet Configuration

```rust
#[derive(Component)]
pub struct Planet {
    pub name: String,
    pub radius: f32,
    pub density: Box<dyn DensityField>,
    pub has_atmosphere: bool,
    pub atmosphere_config: Option<AtmosphereConfig>,
}

pub struct AtmosphereConfig {
    pub atmo_radius: f32,
    pub rayleigh_beta: Vec3,
    pub rayleigh_scale_height: f32,
    pub mie_beta: f32,
    pub mie_scale_height: f32,
    pub mie_g: f32,
}
```

Each planet spawns its own octree, chunk set, and optional atmosphere entity. The LOD system iterates over all planets and manages each independently.

---

## 8. Performance Budget

### Current Performance Profile
- 33x33 vertices per quadtree chunk (~2K triangles)
- ~hundreds of visible chunks at any time
- Async mesh generation on compute thread pool

### Target Performance Profile
- 32^3 sample grid per octree chunk (surface mesh is much less than 32K triangles — typically 1-5K per chunk)
- ~hundreds of visible chunks (similar — octree sparsity compensates for 3D)
- Async mesh generation (same pattern)
- Surface extraction is heavier than heightmap displacement, so per-chunk generation will be slower
- Compensate with: coarser resolution at distance, skip empty chunks, cache density samples

### Key Metrics to Monitor
- Chunk generation time (ms per chunk)
- Frame time impact of LOD evaluation
- Total triangle count on screen
- Memory usage (octree nodes + mesh data)
