//! Pure image maths for SumMRIse.
//!
//! This crate has no I/O and no UI dependencies so that the clinically
//! relevant computations — patient geometry, orientation labelling,
//! multiplanar reformatting and VOI windowing — are deterministic and unit
//! tested in isolation, and identical on every platform.

pub mod geometry;
pub mod mpr;
pub mod volume;
pub mod window;

pub use geometry::{AnatomicalPlane, EdgeLabels, ImagePlane, orientation_label};
pub use glam::DVec3;
pub use mpr::{MprAxis, MprExtent, MprImage, Slab, SlabMode, reslice};
pub use volume::{Frame, FramePixels, Volume, VolumeError, VolumeGeometry, VoxelBuffer, sort_by_position};
pub use window::{CT_PRESETS, PresetSource, VoiFunction, Window, WindowPreset, auto_window};
