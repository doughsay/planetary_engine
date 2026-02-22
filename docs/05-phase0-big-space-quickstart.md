# Phase 0: `big_space` Integration — Quick Start

This is the recommended starting point. It's low-risk, educational, and lays the foundation for everything else.

## Step 1: Add Dependency

```toml
# Cargo.toml
[dependencies]
bevy = "0.18.0"
big_space = "0.12.0"
noise = "0.9.0"
```

## Step 2: Add the Plugin

In `main.rs`, add `BigSpacePlugin` to the app:

```rust
use big_space::BigSpacePlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(BigSpacePlugin::<i64>::default())
        // ... existing plugins ...
        .run();
}
```

## Step 3: Tag the Camera

Add `GridCell` and `FloatingOrigin` to the camera entity:

```rust
use big_space::{GridCell, FloatingOrigin};

// In the camera spawn system:
commands.spawn((
    // existing camera components...
    Camera3d::default(),
    SpaceCamera::default(),
    SpaceCameraState::default(),
    // NEW: big_space components
    GridCell::<i64>::default(),
    FloatingOrigin,
));
```

## Step 4: Tag Planet Entities

Add `GridCell` to the planet and its children:

```rust
// Planet chunks, atmosphere, etc.
commands.spawn((
    Mesh3d(mesh_handle),
    MeshMaterial3d(material_handle),
    transform,
    GridCell::<i64>::default(), // planet is at grid origin
));
```

## Step 5: Verify

1. `cargo run` — everything should look the same
2. Fly to 100,000+ km from the planet — check for precision artifacts
3. Objects should remain stable (no jittering) at any distance

## Step 6: Explore `big_space` API

Once basic integration works, experiment with:
- Spawning an entity at a distant `GridCell` (e.g., `GridCell::<i64>::new(384400, 0, 0)` for Moon distance)
- The `big_space` debug/diagnostic features if any
- Understanding how `GlobalTransform` is computed from `GridCell + Transform`

## What to Watch For

- **System ordering**: `big_space` systems need to run before rendering. Check if its plugin handles this automatically.
- **Atmosphere shader**: The atmosphere uses `NoFrustumCulling` and positions vertices in the shader. This should work fine with `big_space` since the shader reads from uniforms, not from the transform. But verify.
- **Starfield/Galaxy**: These are positioned at far distance. They may need special handling with `big_space` (or they may just work since they use camera-relative positioning in their shaders).

## Expected Issues

- The camera movement system (`camera.rs`) currently modifies `Transform` directly. With `big_space`, large movements should update the `GridCell` instead. `big_space` may handle this recenter automatically — check its documentation.
- If chunks are spawned as children of the planet entity, they may inherit the parent's `GridCell`. Verify this behavior.

## Success Criteria

- [ ] App compiles and runs with `big_space`
- [ ] Planet renders identically to before
- [ ] Camera can fly 100,000+ km without jitter
- [ ] Atmosphere, starfield, galaxy still render correctly
- [ ] No regression in frame rate
