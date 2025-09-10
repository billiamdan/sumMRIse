use nalgebra::{Matrix4, Vector3};
use serde::{Deserialize, Serialize};
use summrise_core::models::ViewportSettings;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Viewport {
    pub window_center: f64,
    pub window_width: f64,
    pub zoom: f64,
    pub pan: (f64, f64),
    pub rotation: (f64, f64, f64),
    pub projection_matrix: Matrix4<f32>,
    pub view_matrix: Matrix4<f32>,
}

impl Viewport {
    pub fn new() -> Self {
        Self {
            window_center: 0.0,
            window_width: 1000.0,
            zoom: 1.0,
            pan: (0.0, 0.0),
            rotation: (0.0, 0.0, 0.0),
            projection_matrix: Matrix4::identity(),
            view_matrix: Matrix4::identity(),
        }
    }

    pub fn update(&mut self, settings: &ViewportSettings) {
        self.window_center = settings.window_center;
        self.window_width = settings.window_width;
        self.zoom = settings.zoom;
        self.pan = settings.pan;
        self.rotation = settings.rotation;
        
        self.update_matrices();
    }

    fn update_matrices(&mut self) {
        // Update projection matrix (orthographic for medical imaging)
        self.projection_matrix = Matrix4::new_orthographic(
            -1.0, 1.0, -1.0, 1.0, -10.0, 10.0
        );

        // Update view matrix with rotation and pan
        let rotation_matrix = Matrix4::from_euler_angles(
            self.rotation.0 as f32,
            self.rotation.1 as f32,
            self.rotation.2 as f32,
        );
        
        let translation = Vector3::new(
            self.pan.0 as f32,
            self.pan.1 as f32,
            0.0
        );
        
        self.view_matrix = Matrix4::new_translation(&translation) * rotation_matrix;
    }

    pub fn get_window_level(&self) -> (f32, f32) {
        (self.window_center as f32, self.window_width as f32)
    }

    pub fn get_transform_matrix(&self) -> Matrix4<f32> {
        self.projection_matrix * self.view_matrix
    }
}
