use crate::mesh::ChunkedMesh;
use std::error::Error;
use std::fmt;
use std::sync::Arc;

const LEAF_TRIANGLE_COUNT: usize = 8;
const BVH_STACK_CAPACITY: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CylinderCollider {
    pub base: [f32; 3],
    pub radius: f32,
    pub height: f32,
}

impl CylinderCollider {
    pub fn intersects(&self, other: &Self) -> bool {
        let dx = self.base[0] - other.base[0];
        let dz = self.base[2] - other.base[2];
        let radii = self.radius + other.radius;
        let vertical_overlap = self.base[1] < other.base[1] + other.height
            && other.base[1] < self.base[1] + self.height;
        dx * dx + dz * dz < radii * radii && vertical_overlap
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: [f32; 3],
    pub direction: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RaycastHit {
    pub distance: f32,
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub triangle_index: u32,
}

#[derive(Debug, PartialEq)]
pub enum CollisionError {
    IndexCountNotTriangular,
    IndexOutOfBounds {
        triangle_index: usize,
        vertex_index: u32,
    },
    NonFiniteVertex {
        vertex_index: u32,
    },
    TooManyTriangles,
}

impl fmt::Display for CollisionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IndexCountNotTriangular => {
                write!(formatter, "mesh index count is not divisible by three")
            }
            Self::IndexOutOfBounds {
                triangle_index,
                vertex_index,
            } => write!(
                formatter,
                "triangle {triangle_index} references missing vertex {vertex_index}"
            ),
            Self::NonFiniteVertex { vertex_index } => {
                write!(
                    formatter,
                    "mesh vertex {vertex_index} has a non-finite position"
                )
            }
            Self::TooManyTriangles => write!(formatter, "mesh has too many triangles for the BVH"),
        }
    }
}

impl Error for CollisionError {}

#[derive(Debug)]
pub struct TriangleMeshCollider {
    mesh: Arc<ChunkedMesh>,
    triangle_order: Vec<u32>,
    nodes: Vec<BvhNode>,
}

impl TriangleMeshCollider {
    pub fn new(mesh: Arc<ChunkedMesh>) -> Result<Self, CollisionError> {
        if mesh.indices.len() % 3 != 0 {
            return Err(CollisionError::IndexCountNotTriangular);
        }

        let triangle_count = mesh.indices.len() / 3;
        if triangle_count > u32::MAX as usize / 2 {
            return Err(CollisionError::TooManyTriangles);
        }

        for (triangle_index, indices) in mesh.indices.chunks_exact(3).enumerate() {
            for &vertex_index in indices {
                let Some(vertex) = mesh.vertices.get(vertex_index as usize) else {
                    return Err(CollisionError::IndexOutOfBounds {
                        triangle_index,
                        vertex_index,
                    });
                };
                if !vertex
                    .position
                    .iter()
                    .all(|coordinate| coordinate.is_finite())
                {
                    return Err(CollisionError::NonFiniteVertex { vertex_index });
                }
            }
        }

        let mut collider = Self {
            mesh,
            triangle_order: (0..triangle_count as u32).collect(),
            nodes: Vec::with_capacity(triangle_count.div_ceil(LEAF_TRIANGLE_COUNT) * 2),
        };
        if triangle_count > 0 {
            let triangle_count = collider.triangle_order.len();
            collider.build_node(0, triangle_count);
        }
        Ok(collider)
    }

    pub fn raycast(&self, ray: Ray, max_distance: f32) -> Option<RaycastHit> {
        if self.nodes.is_empty()
            || !max_distance.is_finite()
            || max_distance <= 0.0
            || !ray.origin.iter().all(|coordinate| coordinate.is_finite())
            || !ray
                .direction
                .iter()
                .all(|coordinate| coordinate.is_finite())
        {
            return None;
        }

        let direction_length = dot(ray.direction, ray.direction).sqrt();
        if !direction_length.is_finite() || direction_length <= f32::EPSILON {
            return None;
        }
        let direction = scale(ray.direction, direction_length.recip());
        let ray = Ray {
            origin: ray.origin,
            direction,
        };

        let mut closest_distance = max_distance;
        let mut closest_hit = None;
        let mut stack = [0_u32; BVH_STACK_CAPACITY];
        let mut stack_len = 1;
        stack[0] = 0;

        while stack_len > 0 {
            stack_len -= 1;
            let node_index = stack[stack_len] as usize;
            let node = self.nodes[node_index];
            if node.bounds.ray_interval(ray, closest_distance).is_none() {
                continue;
            }

            if node.count > 0 {
                for order_index in node.first..node.first + node.count {
                    let triangle_index = self.triangle_order[order_index as usize];
                    let Some((distance, normal)) =
                        self.intersect_triangle(triangle_index, ray, closest_distance)
                    else {
                        continue;
                    };
                    closest_distance = distance;
                    closest_hit = Some(RaycastHit {
                        distance,
                        position: add(ray.origin, scale(ray.direction, distance)),
                        normal,
                        triangle_index,
                    });
                }
            } else {
                stack[stack_len] = node.left;
                stack[stack_len + 1] = node.right;
                stack_len += 2;
            }
        }

        closest_hit
    }

    fn build_node(&mut self, first: usize, end: usize) -> u32 {
        let bounds = self.range_bounds(first, end);
        let node_index = self.nodes.len() as u32;
        self.nodes.push(BvhNode {
            bounds,
            ..BvhNode::default()
        });

        let triangle_count = end - first;
        if triangle_count <= LEAF_TRIANGLE_COUNT {
            self.nodes[node_index as usize].first = first as u32;
            self.nodes[node_index as usize].count = triangle_count as u32;
            return node_index;
        }

        let axis = bounds.longest_axis();
        let middle = first + triangle_count / 2;
        let mesh = &self.mesh;
        let triangle_order = &mut self.triangle_order;
        triangle_order[first..end].select_nth_unstable_by(middle - first, |left, right| {
            triangle_centroid(mesh, *left)[axis].total_cmp(&triangle_centroid(mesh, *right)[axis])
        });

        let left = self.build_node(first, middle);
        let right = self.build_node(middle, end);
        self.nodes[node_index as usize].left = left;
        self.nodes[node_index as usize].right = right;
        node_index
    }

    fn range_bounds(&self, first: usize, end: usize) -> Bounds {
        let mut bounds = Bounds::empty();
        for &triangle_index in &self.triangle_order[first..end] {
            for vertex in self.triangle_vertices(triangle_index) {
                bounds.include(vertex);
            }
        }
        bounds
    }

    fn triangle_vertices(&self, triangle_index: u32) -> [[f32; 3]; 3] {
        let first_index = triangle_index as usize * 3;
        let indices = &self.mesh.indices[first_index..first_index + 3];
        [
            self.mesh.vertices[indices[0] as usize].position,
            self.mesh.vertices[indices[1] as usize].position,
            self.mesh.vertices[indices[2] as usize].position,
        ]
    }

    fn intersect_triangle(
        &self,
        triangle_index: u32,
        ray: Ray,
        max_distance: f32,
    ) -> Option<(f32, [f32; 3])> {
        let [a, b, c] = self.triangle_vertices(triangle_index);
        let edge_ab = subtract(b, a);
        let edge_ac = subtract(c, a);
        let perpendicular = cross(ray.direction, edge_ac);
        let determinant = dot(edge_ab, perpendicular);
        if determinant.abs() <= 1.0e-7 {
            return None;
        }

        let inverse_determinant = determinant.recip();
        let from_a = subtract(ray.origin, a);
        let u = dot(from_a, perpendicular) * inverse_determinant;
        if !(0.0..=1.0).contains(&u) {
            return None;
        }

        let perpendicular = cross(from_a, edge_ab);
        let v = dot(ray.direction, perpendicular) * inverse_determinant;
        if v < 0.0 || u + v > 1.0 {
            return None;
        }

        let distance = dot(edge_ac, perpendicular) * inverse_determinant;
        if distance < 0.0 || distance > max_distance {
            return None;
        }

        let normal = normalize(cross(edge_ab, edge_ac))?;
        Some((distance, normal))
    }
}

fn triangle_centroid(mesh: &ChunkedMesh, triangle_index: u32) -> [f32; 3] {
    let first_index = triangle_index as usize * 3;
    let indices = &mesh.indices[first_index..first_index + 3];
    let a = mesh.vertices[indices[0] as usize].position;
    let b = mesh.vertices[indices[1] as usize].position;
    let c = mesh.vertices[indices[2] as usize].position;
    scale(add(add(a, b), c), 1.0 / 3.0)
}

#[derive(Clone, Copy, Debug, Default)]
struct BvhNode {
    bounds: Bounds,
    left: u32,
    right: u32,
    first: u32,
    count: u32,
}

#[derive(Clone, Copy, Debug)]
struct Bounds {
    min: [f32; 3],
    max: [f32; 3],
}

impl Default for Bounds {
    fn default() -> Self {
        Self::empty()
    }
}

impl Bounds {
    fn empty() -> Self {
        Self {
            min: [f32::INFINITY; 3],
            max: [f32::NEG_INFINITY; 3],
        }
    }

    fn include(&mut self, point: [f32; 3]) {
        for (axis, coordinate) in point.iter().enumerate() {
            self.min[axis] = self.min[axis].min(*coordinate);
            self.max[axis] = self.max[axis].max(*coordinate);
        }
    }

    fn longest_axis(self) -> usize {
        let extent = subtract(self.max, self.min);
        if extent[0] >= extent[1] && extent[0] >= extent[2] {
            0
        } else if extent[1] >= extent[2] {
            1
        } else {
            2
        }
    }

    fn ray_interval(self, ray: Ray, max_distance: f32) -> Option<(f32, f32)> {
        let mut near = 0.0_f32;
        let mut far = max_distance;
        for axis in 0..3 {
            let origin = ray.origin[axis];
            let direction = ray.direction[axis];
            if direction.abs() <= f32::EPSILON {
                if origin < self.min[axis] || origin > self.max[axis] {
                    return None;
                }
                continue;
            }

            let inverse_direction = direction.recip();
            let mut first = (self.min[axis] - origin) * inverse_direction;
            let mut second = (self.max[axis] - origin) * inverse_direction;
            if first > second {
                std::mem::swap(&mut first, &mut second);
            }
            near = near.max(first);
            far = far.min(second);
            if near > far {
                return None;
            }
        }
        Some((near, far))
    }
}

fn add(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn subtract(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn scale(vector: [f32; 3], factor: f32) -> [f32; 3] {
    [vector[0] * factor, vector[1] * factor, vector[2] * factor]
}

fn dot(left: [f32; 3], right: [f32; 3]) -> f32 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn cross(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn normalize(vector: [f32; 3]) -> Option<[f32; 3]> {
    let length = dot(vector, vector).sqrt();
    if !length.is_finite() || length <= f32::EPSILON {
        return None;
    }
    Some(scale(vector, length.recip()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::MeshVertex;

    #[test]
    fn cylinder_colliders_require_horizontal_and_vertical_overlap() {
        let player = CylinderCollider {
            base: [0.0, 0.0, 0.0],
            radius: 0.35,
            height: 1.8,
        };
        let trunk = CylinderCollider {
            base: [0.5, 0.0, 0.0],
            radius: 0.2,
            height: 1.5,
        };
        let foliage_only = CylinderCollider {
            base: [0.0, 2.0, 0.0],
            radius: 0.35,
            height: 1.0,
        };

        assert!(player.intersects(&trunk));
        assert!(!foliage_only.intersects(&trunk));
    }

    fn triangle_mesh() -> Arc<ChunkedMesh> {
        Arc::new(ChunkedMesh {
            vertices: vec![
                MeshVertex {
                    position: [0.0, 0.0, 0.0],
                    normal: [0.0, 1.0, 0.0],
                    color: [1.0; 3],
                    texture_data: [0.0; 3],
                },
                MeshVertex {
                    position: [0.0, 0.0, 1.0],
                    normal: [0.0, 1.0, 0.0],
                    color: [1.0; 3],
                    texture_data: [0.0; 3],
                },
                MeshVertex {
                    position: [1.0, 0.0, 0.0],
                    normal: [0.0, 1.0, 0.0],
                    color: [1.0; 3],
                    texture_data: [0.0; 3],
                },
            ],
            indices: vec![0, 1, 2],
            chunks: Vec::new(),
        })
    }

    #[test]
    fn raycast_reports_closest_triangle_hit() {
        let collider = TriangleMeshCollider::new(triangle_mesh()).unwrap();
        let hit = collider
            .raycast(
                Ray {
                    origin: [0.2, 1.0, 0.2],
                    direction: [0.0, -3.0, 0.0],
                },
                2.0,
            )
            .unwrap();

        assert!((hit.distance - 1.0).abs() < 1.0e-5);
        assert!((hit.position[1]).abs() < 1.0e-5);
        assert!(hit.normal[1] > 0.99);
        assert_eq!(hit.triangle_index, 0);
    }

    #[test]
    fn raycast_rejects_misses_and_invalid_mesh_indices() {
        let collider = TriangleMeshCollider::new(triangle_mesh()).unwrap();
        assert!(collider
            .raycast(
                Ray {
                    origin: [2.0, 1.0, 2.0],
                    direction: [0.0, -1.0, 0.0],
                },
                2.0,
            )
            .is_none());

        let valid_mesh = triangle_mesh();
        let invalid_mesh = ChunkedMesh {
            vertices: valid_mesh.vertices.clone(),
            indices: vec![99, 1, 2],
            chunks: Vec::new(),
        };
        assert!(matches!(
            TriangleMeshCollider::new(Arc::new(invalid_mesh)),
            Err(CollisionError::IndexOutOfBounds { .. })
        ));
    }
}
