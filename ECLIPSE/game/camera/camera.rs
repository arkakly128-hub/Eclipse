use eclipse_engine::input::MouseDelta;
use glam::{Mat4, Vec3};

const MOUSE_SENSITIVITY: f32 = 0.0022;
const MAX_PITCH: f32 = 1.52;

pub struct Camera {
    yaw: f32,
    pitch: f32,
    vertical_fov: f32,
}

impl Camera {
    pub fn first_person() -> Self {
        Self {
            yaw: -std::f32::consts::FRAC_PI_2,
            pitch: -0.08,
            vertical_fov: 68.0_f32.to_radians(),
        }
    }

    pub fn update(&mut self, mouse_delta: MouseDelta) {
        self.yaw = (self.yaw + mouse_delta.x * MOUSE_SENSITIVITY).rem_euclid(std::f32::consts::TAU);
        self.pitch = (self.pitch - mouse_delta.y * MOUSE_SENSITIVITY).clamp(-MAX_PITCH, MAX_PITCH);
    }

    pub fn forward(&self) -> Vec3 {
        let cos_pitch = self.pitch.cos();
        Vec3::new(
            self.yaw.cos() * cos_pitch,
            self.pitch.sin(),
            self.yaw.sin() * cos_pitch,
        )
        .normalize()
    }

    pub fn view_projection(&self, eye_position: Vec3, aspect_ratio: f32) -> Mat4 {
        let view = Mat4::look_at_rh(eye_position, eye_position + self.forward(), Vec3::Y);
        let projection = Mat4::perspective_rh(self.vertical_fov, aspect_ratio, 0.05, 240.0);
        projection * view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_delta_changes_yaw_and_pitch() {
        let mut camera = Camera::first_person();
        let initial_forward = camera.forward();

        camera.update(MouseDelta { x: 100.0, y: -50.0 });

        let changed_forward = camera.forward();
        assert!(changed_forward.x > initial_forward.x);
        assert!(changed_forward.y > initial_forward.y);
    }
}
