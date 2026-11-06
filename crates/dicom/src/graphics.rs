//! Scanner graphics: the overlay planes of an image (groups 6000–601E,
//! PS3.3 C.9.2), read as 1-bit planes for each frame
//! (`openspec/changes/display-fidelity/design.md`, D6).
//!
//! Planes with Overlay Data (60xx,3000) are decoded by `dicom-pixeldata`
//! (`decode_overlay`) and repacked to bits; planes of the retired form, kept in
//! unused bits of the pixel data, are taken from the decoded samples here, for
//! every transfer syntax. A malformed plane is left out; it never fails the
//! image.

use std::sync::Arc;

use dicom_core::Tag;
use dicom_object::{DefaultDicomObject, InMemDicomObject};
use dicom_pixeldata::PixelDecoder;
use log::debug;
use summrise_imaging::GraphicsPlane;

use crate::attrs;
use crate::pixels::Layout;

/// Which image frames (0-based) a plane applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frames {
    /// No Number of Frames in Overlay: every frame.
    All,
    /// Overlay frame `k` applies to image frame `first + k`, for `count` frames.
    From { first: u32, count: u32 },
}

impl Frames {
    /// The overlay frame for image frame `frame`, if the plane applies to it.
    fn overlay_frame(self, frame: u32) -> Option<u32> {
        match self {
            Self::All => Some(0),
            Self::From { first, count } => frame.checked_sub(first).filter(|&k| k < count),
        }
    }
}

#[derive(Debug, Clone)]
enum Source {
    /// From Overlay Data: one plane per overlay frame.
    Data(Vec<Arc<GraphicsPlane>>),
    /// The retired form: bit `bit` of every sample.
    Embedded { bit: u16, size: (u32, u32), origin: (i64, i64) },
}

#[derive(Debug, Clone)]
struct Overlay {
    source: Source,
    frames: Frames,
}

/// The overlay planes of one image file.
#[derive(Debug, Clone, Default)]
pub(crate) struct Overlays {
    planes: Vec<Overlay>,
}

impl Overlays {
    /// Reads every overlay plane of `obj`. `layout` tells which bits hold the
    /// stored values (the retired form must lie outside them);
    /// `samples_per_pixel` other than 1 rules the retired form out.
    pub(crate) fn read(obj: &DefaultDicomObject, layout: &Layout, samples_per_pixel: u16) -> Self {
        let base: &InMemDicomObject = obj;
        let mut planes = Vec::new();
        for index in 0..16u8 {
            let group = 0x6000 + 2 * u16::from(index);
            let has_data = base.get(Tag(group, 0x3000)).is_some();
            if !has_data && base.get(Tag(group, 0x0010)).is_none() {
                continue;
            }
            let frames = frames_of(base, group);
            let source = if has_data {
                match obj.decode_overlay(index) {
                    Ok(Some(plane)) => data_planes(&plane),
                    Ok(None) => None,
                    Err(e) => {
                        debug!("overlay plane {group:04X} left out: {e}");
                        None
                    }
                }
            } else {
                embedded(base, group, layout, samples_per_pixel)
            };
            planes.extend(source.map(|source| Overlay { source, frames }));
        }
        Self { planes }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.planes.is_empty()
    }

    /// The planes over image frame `frame` of `width` × `height` pixels;
    /// `raw` is that frame's samples as decoded (for the retired form).
    pub(crate) fn for_frame(&self, frame: u32, raw: &[u16], width: u32, height: u32) -> Vec<Arc<GraphicsPlane>> {
        self.planes
            .iter()
            .filter_map(|overlay| {
                let k = overlay.frames.overlay_frame(frame)?;
                match &overlay.source {
                    Source::Data(planes) => planes.get(k as usize).cloned(),
                    Source::Embedded { bit, size: (cols, rows), origin } => {
                        let (cols, rows) = ((*cols).min(width), (*rows).min(height));
                        (raw.len() >= (width * height) as usize).then(|| {
                            Arc::new(GraphicsPlane::from_fn(cols, rows, *origin, |x, y| {
                                (raw[(y * width + x) as usize] >> bit) & 1 == 1
                            }))
                        })
                    }
                }
            })
            .collect()
    }
}

/// Number of Frames in Overlay and Image Frame Origin of group `group`.
fn frames_of(obj: &InMemDicomObject, group: u16) -> Frames {
    match attrs::uint(obj, Tag(group, 0x0015)) {
        None => Frames::All,
        Some(count) => {
            let origin = attrs::uint(obj, Tag(group, 0x0051)).unwrap_or(1).max(1);
            Frames::From { first: origin - 1, count: count.max(1) }
        }
    }
}

/// The image column and row (0-based) under the first overlay pixel, from
/// Overlay Origin (row, column, 1-based).
fn origin_of(row: i64, column: i64) -> (i64, i64) {
    (column - 1, row - 1)
}

/// The frames of a plane decoded from Overlay Data, one bit per pixel.
fn data_planes(plane: &dicom_pixeldata::OverlayPlane) -> Option<Source> {
    let [row, column] = plane.origin();
    let origin = origin_of(i64::from(row), i64::from(column));
    let (cols, rows) = (u32::from(plane.columns()), u32::from(plane.rows()));
    let frames: Option<Vec<Arc<GraphicsPlane>>> = (0..plane.number_of_frames())
        .map(|k| {
            let bytes = plane.frame_data(k).ok()?;
            GraphicsPlane::from_bytes(cols, rows, origin, bytes).map(Arc::new)
        })
        .collect();
    frames.filter(|f| !f.is_empty()).map(Source::Data)
}

/// A plane of the retired form in group `group`, if it can be told apart
/// from the image: its bit lies outside the stored bits and inside the sample.
fn embedded(obj: &InMemDicomObject, group: u16, layout: &Layout, samples_per_pixel: u16) -> Option<Source> {
    let bit = attrs::uint(obj, Tag(group, 0x0102))? as u16;
    let rows = attrs::uint(obj, Tag(group, 0x0010))?;
    let cols = attrs::uint(obj, Tag(group, 0x0011))?;
    let stored = layout.shift..layout.shift + layout.bits_stored;
    if samples_per_pixel != 1 || stored.contains(&bit) || bit >= layout.bits_allocated.min(16) {
        debug!("overlay plane {group:04X} in bit {bit} cannot be told apart from the image; left out");
        return None;
    }
    let origin = obj
        .get(Tag(group, 0x0050))
        .and_then(|e| e.to_multi_int::<i64>().ok())
        .filter(|v| v.len() == 2)
        .map_or((0, 0), |v| origin_of(v[0], v[1]));
    Some(Source::Embedded { bit, size: (cols, rows), origin })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_follow_the_frame_origin() {
        let from_two = Frames::From { first: 1, count: 2 };
        assert_eq!([0, 1, 2, 3].map(|f| from_two.overlay_frame(f)), [None, Some(0), Some(1), None]);
        assert_eq!([0, 7].map(|f| Frames::All.overlay_frame(f)), [Some(0), Some(0)]);
        assert_eq!(origin_of(11, 21), (20, 10));
        assert_eq!(origin_of(1, 1), (0, 0));
        assert_eq!(origin_of(-3, 0), (-1, -4));
    }
}
