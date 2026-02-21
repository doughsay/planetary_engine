use bevy::prelude::*;

/// The six faces of the cube that gets spherified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CubeFace {
    Top,    // +Y
    Bottom, // -Y
    Left,   // -X
    Right,  // +X
    Front,  // +Z
    Back,   // -Z
}

impl CubeFace {
    pub const ALL: [CubeFace; 6] = [
        CubeFace::Top,
        CubeFace::Bottom,
        CubeFace::Left,
        CubeFace::Right,
        CubeFace::Front,
        CubeFace::Back,
    ];

    /// Returns (face_normal, axis_a, axis_b) — the same convention as the
    /// original planet.rs used.
    pub fn axes(self) -> (Vec3, Vec3, Vec3) {
        let normal = self.normal();
        let axis_a = Vec3::new(normal.y, normal.z, normal.x);
        let axis_b = normal.cross(axis_a);
        (normal, axis_a, axis_b)
    }

    pub fn normal(self) -> Vec3 {
        match self {
            CubeFace::Top => Vec3::Y,
            CubeFace::Bottom => Vec3::NEG_Y,
            CubeFace::Left => Vec3::NEG_X,
            CubeFace::Right => Vec3::X,
            CubeFace::Front => Vec3::Z,
            CubeFace::Back => Vec3::NEG_Z,
        }
    }
}

/// Identifies which edge of a face we're looking at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    /// u = 0 (left edge, axis_a minimum)
    Left,
    /// u = 1 (right edge, axis_a maximum)
    Right,
    /// v = 0 (bottom edge, axis_b minimum)
    Bottom,
    /// v = 1 (top edge, axis_b maximum)
    Top,
}

/// Describes how to map coordinates when crossing from one face to an adjacent face.
/// `neighbor` is the adjacent face.
/// `edge` is which edge of the *neighbor* face we arrive at.
/// `flip` indicates whether the parametric coordinate along the shared edge is reversed.
#[derive(Debug, Clone, Copy)]
pub struct Adjacency {
    pub neighbor: CubeFace,
    pub edge: Edge,
    pub flip: bool,
}

/// Hardcoded adjacency table.
/// For each face, gives [Left, Right, Bottom, Top] adjacencies.
///
/// This was derived by working through which cube faces share edges
/// and how the UV axes align when crossing.
pub fn face_adjacency(face: CubeFace, edge: Edge) -> Adjacency {
    use CubeFace as F;
    use Edge as E;
    match (face, edge) {
        // Top (+Y): axis_a=X, axis_b=-Z
        (F::Top, E::Left)   => Adjacency { neighbor: F::Left,  edge: E::Top,    flip: false },
        (F::Top, E::Right)  => Adjacency { neighbor: F::Right, edge: E::Top,    flip: false },
        (F::Top, E::Bottom) => Adjacency { neighbor: F::Front, edge: E::Top,    flip: false },
        (F::Top, E::Top)    => Adjacency { neighbor: F::Back,  edge: E::Top,    flip: true },

        // Bottom (-Y): axis_a=-X, axis_b=-Z
        (F::Bottom, E::Left)   => Adjacency { neighbor: F::Right, edge: E::Bottom, flip: false },
        (F::Bottom, E::Right)  => Adjacency { neighbor: F::Left,  edge: E::Bottom, flip: false },
        (F::Bottom, E::Bottom) => Adjacency { neighbor: F::Front, edge: E::Bottom, flip: false },
        (F::Bottom, E::Top)    => Adjacency { neighbor: F::Back,  edge: E::Bottom, flip: true },

        // Left (-X): axis_a=-Z, axis_b=-Y
        (F::Left, E::Left)   => Adjacency { neighbor: F::Front,  edge: E::Left,  flip: false },
        (F::Left, E::Right)  => Adjacency { neighbor: F::Back,   edge: E::Right, flip: false },
        (F::Left, E::Bottom) => Adjacency { neighbor: F::Top,    edge: E::Left,  flip: false },
        (F::Left, E::Top)    => Adjacency { neighbor: F::Bottom, edge: E::Right, flip: false },

        // Right (+X): axis_a=Z, axis_b=-Y
        (F::Right, E::Left)   => Adjacency { neighbor: F::Back,   edge: E::Left,  flip: false },
        (F::Right, E::Right)  => Adjacency { neighbor: F::Front,  edge: E::Right, flip: false },
        (F::Right, E::Bottom) => Adjacency { neighbor: F::Top,    edge: E::Right, flip: false },
        (F::Right, E::Top)    => Adjacency { neighbor: F::Bottom, edge: E::Left,  flip: false },

        // Front (+Z): axis_a=Y, axis_b=-X
        (F::Front, E::Left)   => Adjacency { neighbor: F::Bottom, edge: E::Bottom, flip: false },
        (F::Front, E::Right)  => Adjacency { neighbor: F::Top,    edge: E::Bottom, flip: false },
        (F::Front, E::Bottom) => Adjacency { neighbor: F::Right,  edge: E::Right,  flip: false },
        (F::Front, E::Top)    => Adjacency { neighbor: F::Left,   edge: E::Left,   flip: false },

        // Back (-Z): axis_a=-Y, axis_b=-X
        (F::Back, E::Left)   => Adjacency { neighbor: F::Top,    edge: E::Top,    flip: true },
        (F::Back, E::Right)  => Adjacency { neighbor: F::Bottom, edge: E::Top,    flip: true },
        (F::Back, E::Bottom) => Adjacency { neighbor: F::Right,  edge: E::Left,   flip: false },
        (F::Back, E::Top)    => Adjacency { neighbor: F::Left,   edge: E::Right,  flip: false },
    }
}

/// Uniquely identifies any node in the quadtree across all 6 cube faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId {
    pub face: CubeFace,
    pub depth: u8,
    pub x: u32,
    pub y: u32,
}

impl NodeId {
    /// Root node for a given face.
    pub fn root(face: CubeFace) -> Self {
        Self { face, depth: 0, x: 0, y: 0 }
    }

    /// The UV bounds of this node within its face, as (u_min, v_min, u_max, v_max).
    /// At depth 0, covers [0,1]×[0,1]. Each subdivision halves each dimension.
    pub fn uv_bounds(&self) -> (f32, f32, f32, f32) {
        let size = 1.0 / (1u32 << self.depth) as f32;
        let u_min = self.x as f32 * size;
        let v_min = self.y as f32 * size;
        (u_min, v_min, u_min + size, v_min + size)
    }

    /// Approximate center of this node projected onto the unit sphere.
    pub fn center_on_sphere(&self) -> Vec3 {
        let (u_min, v_min, u_max, v_max) = self.uv_bounds();
        let u = (u_min + u_max) * 0.5;
        let v = (v_min + v_max) * 0.5;
        let (normal, axis_a, axis_b) = self.face.axes();
        let point_on_cube = normal + (u - 0.5) * 2.0 * axis_a + (v - 0.5) * 2.0 * axis_b;
        point_on_cube.normalize()
    }

    /// Arc length approximation: the angular size of this node times radius=1.
    pub fn arc_length(&self) -> f32 {
        // Each face spans ~90° (π/2 radians). Subdivisions halve the angular size.
        let face_arc = std::f32::consts::FRAC_PI_2;
        face_arc / (1u32 << self.depth) as f32
    }

    /// The four children of this node (depth+1).
    pub fn children(&self) -> [NodeId; 4] {
        let cx = self.x * 2;
        let cy = self.y * 2;
        let d = self.depth + 1;
        [
            NodeId { face: self.face, depth: d, x: cx, y: cy },
            NodeId { face: self.face, depth: d, x: cx + 1, y: cy },
            NodeId { face: self.face, depth: d, x: cx, y: cy + 1 },
            NodeId { face: self.face, depth: d, x: cx + 1, y: cy + 1 },
        ]
    }

    /// Parent of this node. Returns None for root nodes (depth 0).
    pub fn parent(&self) -> Option<NodeId> {
        if self.depth == 0 {
            return None;
        }
        Some(NodeId {
            face: self.face,
            depth: self.depth - 1,
            x: self.x / 2,
            y: self.y / 2,
        })
    }

    /// Neighbors in all 4 directions: [Left(u-1), Right(u+1), Bottom(v-1), Top(v+1)].
    /// Handles cross-face lookups when the neighbor falls off this face's edge.
    pub fn neighbors(&self) -> [Option<NodeId>; 4] {
        let grid_size = 1u32 << self.depth;
        [
            self.neighbor_in_direction(Edge::Left, grid_size),
            self.neighbor_in_direction(Edge::Right, grid_size),
            self.neighbor_in_direction(Edge::Bottom, grid_size),
            self.neighbor_in_direction(Edge::Top, grid_size),
        ]
    }

    fn neighbor_in_direction(&self, dir: Edge, grid_size: u32) -> Option<NodeId> {
        match dir {
            Edge::Left => {
                if self.x > 0 {
                    Some(NodeId { face: self.face, depth: self.depth, x: self.x - 1, y: self.y })
                } else {
                    self.cross_face_neighbor(Edge::Left, grid_size)
                }
            }
            Edge::Right => {
                if self.x + 1 < grid_size {
                    Some(NodeId { face: self.face, depth: self.depth, x: self.x + 1, y: self.y })
                } else {
                    self.cross_face_neighbor(Edge::Right, grid_size)
                }
            }
            Edge::Bottom => {
                if self.y > 0 {
                    Some(NodeId { face: self.face, depth: self.depth, x: self.x, y: self.y - 1 })
                } else {
                    self.cross_face_neighbor(Edge::Bottom, grid_size)
                }
            }
            Edge::Top => {
                if self.y + 1 < grid_size {
                    Some(NodeId { face: self.face, depth: self.depth, x: self.x, y: self.y + 1 })
                } else {
                    self.cross_face_neighbor(Edge::Top, grid_size)
                }
            }
        }
    }

    fn cross_face_neighbor(&self, edge: Edge, grid_size: u32) -> Option<NodeId> {
        let adj = face_adjacency(self.face, edge);
        let max = grid_size - 1;

        // `t` is the parametric coordinate along the shared edge (0..grid_size-1)
        let t = match edge {
            Edge::Left | Edge::Right => self.y,
            Edge::Bottom | Edge::Top => self.x,
        };

        let t_mapped = if adj.flip { max - t } else { t };

        // Place the result on the neighbor face at the correct edge
        let (nx, ny) = match adj.edge {
            Edge::Left => (0, t_mapped),
            Edge::Right => (max, t_mapped),
            Edge::Bottom => (t_mapped, 0),
            Edge::Top => (t_mapped, max),
        };

        Some(NodeId {
            face: adj.neighbor,
            depth: self.depth,
            x: nx,
            y: ny,
        })
    }
}

/// Tracks the state of each node in a single face's quadtree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeState {
    Leaf,
    Split,
}

/// Quadtree for one cube face. Uses a flat HashMap for O(1) lookups.
pub struct FaceQuadtree {
    pub face: CubeFace,
    pub nodes: std::collections::HashMap<NodeId, NodeState>,
}

impl FaceQuadtree {
    pub fn new(face: CubeFace) -> Self {
        let mut nodes = std::collections::HashMap::new();
        nodes.insert(NodeId::root(face), NodeState::Leaf);
        Self { face, nodes }
    }

    /// All current leaf nodes.
    pub fn leaves(&self) -> Vec<NodeId> {
        self.nodes
            .iter()
            .filter(|(_, state)| **state == NodeState::Leaf)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Split a leaf node into 4 children. Returns the new children.
    pub fn split(&mut self, node: NodeId) -> Option<[NodeId; 4]> {
        if self.nodes.get(&node) != Some(&NodeState::Leaf) {
            return None;
        }
        self.nodes.insert(node, NodeState::Split);
        let children = node.children();
        for &child in &children {
            self.nodes.insert(child, NodeState::Leaf);
        }
        Some(children)
    }

    /// Merge 4 children back into their parent leaf. Returns the parent.
    pub fn merge(&mut self, parent: NodeId) -> bool {
        if self.nodes.get(&parent) != Some(&NodeState::Split) {
            return false;
        }
        let children = parent.children();
        // All children must be leaves
        if !children.iter().all(|c| self.nodes.get(c) == Some(&NodeState::Leaf)) {
            return false;
        }
        for &child in &children {
            self.nodes.remove(&child);
        }
        self.nodes.insert(parent, NodeState::Leaf);
        true
    }

    /// Get the depth of the leaf node containing a given (depth, x, y) position.
    /// Walks up the tree until it finds the leaf.
    pub fn leaf_depth_at(&self, face: CubeFace, depth: u8, x: u32, y: u32) -> u8 {
        let mut d = depth;
        let mut cx = x;
        let mut cy = y;
        loop {
            let id = NodeId { face, depth: d, x: cx, y: cy };
            if let Some(&NodeState::Leaf) = self.nodes.get(&id) {
                return d;
            }
            if d == 0 {
                return 0;
            }
            d -= 1;
            cx /= 2;
            cy /= 2;
        }
    }
}
