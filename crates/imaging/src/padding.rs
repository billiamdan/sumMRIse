//! Pixel padding: pixels around the scanned area that hold no measurement,
//! such as the corners outside the round field of a CT slice (PS3.3
//! C.7.5.1.1.2).
//!
//! An image keeps its values as decoded; its padding is recorded as the range
//! of values that the padding pixels have and no other pixel of the image has
//! ([`Padding`]). Statistics, rendering, read-out and resampling treat a value
//! in that range, or a value that is not a finite number, as "outside the
//! scanned area". No second copy of the image is needed.

use crate::volume::VoxelBuffer;

/// The modality values that mark padding in one image: a closed interval
/// that holds the padding pixels and no other pixel of the image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Padding {
    pub low: f32,
    pub high: f32,
}

impl Padding {
    /// The interval between `a` and `b`, in either order.
    pub fn new(a: f32, b: f32) -> Self {
        Self { low: a.min(b), high: a.max(b) }
    }

    #[inline]
    pub fn contains(&self, value: f32) -> bool {
        value >= self.low && value <= self.high
    }
}

/// Whether `value` stands for no measurement: padding of its image, or not
/// a finite number (outside a volume, or padding that is stored as `NaN`).
#[inline]
pub fn is_outside(value: f32, padding: Option<Padding>) -> bool {
    !value.is_finite() || padding.is_some_and(|p| p.contains(value))
}

/// `value`, or `NaN` where it is padding: for statistics and rendering, which
/// skip values that are not finite.
#[inline]
pub fn visible(value: f32, padding: Option<Padding>) -> f32 {
    if padding.is_some_and(|p| p.contains(value)) { f32::NAN } else { value }
}

/// Smallest and largest value of `buffer` that is a measurement.
pub fn min_max(buffer: &VoxelBuffer, padding: Option<Padding>) -> Option<(f32, f32)> {
    if padding.is_none() {
        return buffer.min_max();
    }
    (0..buffer.len()).map(|i| buffer.get(i)).filter(|&v| !is_outside(v, padding)).fold(None, |range, v| match range {
        None => Some((v, v)),
        Some((lo, hi)) => Some((v.min(lo), v.max(hi))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_values_are_outside_and_left_out_of_the_range() {
        let padding = Some(Padding::new(-3024.0, -3024.0));
        assert!(is_outside(-3024.0, padding) && is_outside(f32::NAN, None) && is_outside(f32::INFINITY, None));
        assert!(!is_outside(-1000.0, padding) && !is_outside(-3024.0, None));
        assert!(visible(-3024.0, padding).is_nan());
        assert_eq!(visible(40.0, padding), 40.0);
        let ct = VoxelBuffer::compact(vec![-3024.0, -1000.0, 40.0, 700.0, -3024.0]);
        assert_eq!(min_max(&ct, padding), Some((-1000.0, 700.0)));
        assert_eq!(min_max(&ct, None), Some((-3024.0, 700.0)));
        // A range in either order; nothing but padding: no range at all.
        assert_eq!(Padding::new(10.0, 0.0), Padding { low: 0.0, high: 10.0 });
        let all = VoxelBuffer::compact(vec![0.0, 5.0, 10.0]);
        assert_eq!(min_max(&all, Some(Padding::new(0.0, 10.0))), None);
    }
}
