use glam::{Mat4, Vec3, Vec4};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn empty() -> Self {
        Self {
            min: Vec3::splat(f32::INFINITY),
            max: Vec3::splat(f32::NEG_INFINITY),
        }
    }

    pub fn include(&mut self, point: Vec3) {
        self.min = self.min.min(point);
        self.max = self.max.max(point);
    }

    pub fn union(&mut self, other: Self) {
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);
    }

    pub fn center(self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn expanded(self, amount: f32) -> Self {
        let expansion = Vec3::splat(amount);
        Self {
            min: self.min - expansion,
            max: self.max + expansion,
        }
    }

    pub fn distance_squared(self, point: Vec3) -> f32 {
        let closest = point.clamp(self.min, self.max);
        closest.distance_squared(point)
    }

    pub fn intersects(self, frustum: &Frustum) -> bool {
        frustum.planes.iter().all(|plane| {
            let positive = Vec3::new(
                if plane.x >= 0.0 {
                    self.max.x
                } else {
                    self.min.x
                },
                if plane.y >= 0.0 {
                    self.max.y
                } else {
                    self.min.y
                },
                if plane.z >= 0.0 {
                    self.max.z
                } else {
                    self.min.z
                },
            );
            plane.truncate().dot(positive) + plane.w >= 0.0
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frustum {
    planes: [Vec4; 6],
}

impl Frustum {
    pub fn from_view_projection(view_projection: Mat4) -> Self {
        let columns = view_projection.to_cols_array_2d();
        let row = |index: usize| {
            Vec4::new(
                columns[0][index],
                columns[1][index],
                columns[2][index],
                columns[3][index],
            )
        };
        let row0 = row(0);
        let row1 = row(1);
        let row2 = row(2);
        let row3 = row(3);
        Self {
            planes: [
                row3 + row0,
                row3 - row0,
                row3 + row1,
                row3 - row1,
                row2,
                row3 - row2,
            ]
            .map(|plane| plane / plane.truncate().length().max(f32::EPSILON)),
        }
    }

    pub fn contains(&self, bounds: Aabb) -> bool {
        bounds.intersects(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frustum_keeps_ahead_bounds_and_rejects_bounds_behind_camera() {
        let camera_position = Vec3::new(100.0, 1.65, 100.0);
        let view = Mat4::look_at_rh(
            camera_position,
            camera_position + Vec3::new(0.0, -0.08, -1.0).normalize(),
            Vec3::Y,
        );
        let projection = Mat4::perspective_rh(68.0_f32.to_radians(), 16.0 / 9.0, 0.05, 240.0);
        let frustum = Frustum::from_view_projection(projection * view);
        let ahead = Aabb {
            min: Vec3::new(75.0, 0.0, 50.0),
            max: Vec3::new(100.0, 1.26, 75.0),
        };
        let behind = Aabb {
            min: Vec3::new(75.0, 0.0, 125.0),
            max: Vec3::new(100.0, 1.26, 150.0),
        };

        assert!(frustum.contains(ahead));
        assert!(!frustum.contains(behind));
    }
}
