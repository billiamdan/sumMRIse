//! Pure image maths for SumMRIse.
//!
//! This crate has no I/O and no UI dependencies so that the clinically
//! relevant computations — patient geometry, orientation labelling,
//! multiplanar reformatting and VOI windowing — are deterministic and unit
//! tested in isolation, and identical on every platform.

pub mod colormap;
pub mod crossref;
pub mod fusion;
pub mod geometry;
pub mod graphics;
pub mod lut;
pub mod measure;
pub mod mpr;
pub mod padding;
pub mod plane;
pub mod render3d;
pub mod test_pattern;
pub mod volume;
pub mod window;

pub use colormap::Colormap;
pub use fusion::{OverlayLook, PixelGrid};
pub use geometry::{AnatomicalPlane, EdgeLabels, ImagePlane, orientation_label};
pub use glam::{DQuat, DVec3};
pub use graphics::GraphicsPlane;
pub use lut::{LookupTable, LutError, Palette, VoiLut};
pub use measure::Calibration;
pub use mpr::{MprAxis, MprExtent, MprImage, Slab, SlabMode, reslice, reslice_plane};
pub use padding::Padding;
pub use plane::PlaneBasis;
pub use render3d::{Camera, Look, Quality, RenderVolume, Rendering, StandardView, Style, render};
pub use volume::{Frame, FramePixels, Volume, VolumeError, VolumeGeometry, VoxelBuffer, sort_by_position};
pub use window::{CT_PRESETS, PresetSource, VoiFunction, Window, WindowPreset, auto_window};
