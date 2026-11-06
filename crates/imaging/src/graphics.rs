//! Scanner graphics: 1-bit planes that a scanner stores with an image (DICOM
//! overlay planes, PS3.3 C.9.2) — marks, outlines and text — and their
//! compositing over the rendered picture. The planes are kept next to the
//! image and drawn at render time; they never change a pixel value.

/// Colour of scanner graphics (RGBA): cyan, which stands out on grey and
/// differs from the yellow orientation letters and the view colours.
pub const GRAPHICS_COLOR: [u8; 4] = [0x3D, 0xE0, 0xFF, 0xFF];

/// One plane of scanner graphics over an image, one bit per pixel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphicsPlane {
    width: u32,
    height: u32,
    /// Image column and row (0-based) under the first pixel of the plane;
    /// may be negative or beyond the image.
    origin: (i64, i64),
    /// Row-major bits, 64 per word.
    bits: Vec<u64>,
}

impl GraphicsPlane {
    /// A plane of `width` × `height` pixels at `origin` whose pixel (x, y)
    /// is set where `set(x, y)` is true.
    pub fn from_fn(width: u32, height: u32, origin: (i64, i64), set: impl Fn(u32, u32) -> bool) -> Self {
        let n = width as usize * height as usize;
        let mut bits = vec![0u64; n.div_ceil(64)];
        for y in 0..height {
            for x in 0..width {
                if set(x, y) {
                    let i = y as usize * width as usize + x as usize;
                    bits[i / 64] |= 1 << (i % 64);
                }
            }
        }
        Self { width, height, origin, bits }
    }

    /// A plane from one byte per pixel, row-major, non-zero meaning set (the
    /// form `dicom-pixeldata` unpacks overlay data to). `None` when `bytes`
    /// is shorter than the plane.
    pub fn from_bytes(width: u32, height: u32, origin: (i64, i64), bytes: &[u8]) -> Option<Self> {
        let n = width as usize * height as usize;
        let bytes = bytes.get(..n)?;
        let mut bits = vec![0u64; n.div_ceil(64)];
        for (i, _) in bytes.iter().enumerate().filter(|(_, b)| **b != 0) {
            bits[i / 64] |= 1 << (i % 64);
        }
        Some(Self { width, height, origin, bits })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Image column and row (0-based) under the first pixel of the plane.
    pub fn origin(&self) -> (i64, i64) {
        self.origin
    }

    /// Whether pixel (`x`, `y`) of the plane is set.
    #[inline]
    pub fn is_set(&self, x: u32, y: u32) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let i = y as usize * self.width as usize + x as usize;
        self.bits[i / 64] >> (i % 64) & 1 == 1
    }

    /// Whether the plane draws over image pixel (`x`, `y`).
    pub fn covers(&self, x: i64, y: i64) -> bool {
        let (px, py) = (x - self.origin.0, y - self.origin.1);
        match (u32::try_from(px), u32::try_from(py)) {
            (Ok(px), Ok(py)) => self.is_set(px, py),
            _ => false,
        }
    }

    /// Number of set pixels.
    pub fn count(&self) -> usize {
        self.bits.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// Memory held by the bits.
    pub fn byte_size(&self) -> usize {
        self.bits.len() * 8
    }
}

/// Draws `planes` in `color` over the RGBA picture of `width` × `height`
/// pixels: every pixel under a set bit takes the colour.
pub fn composite<'a>(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    planes: impl IntoIterator<Item = &'a GraphicsPlane>,
    color: [u8; 4],
) {
    let (w, h) = (i64::from(width), i64::from(height));
    if rgba.len() < (w * h * 4) as usize {
        return;
    }
    for plane in planes {
        let (ox, oy) = plane.origin;
        // Only the rows and columns of the plane that lie on the picture.
        let rows = (0i64.max(-oy))..(i64::from(plane.height).min(h - oy));
        let cols = (0i64.max(-ox))..(i64::from(plane.width).min(w - ox));
        for py in rows {
            for px in cols.clone() {
                if plane.is_set(px as u32, py as u32) {
                    let i = (((oy + py) * w + ox + px) * 4) as usize;
                    rgba[i..i + 4].copy_from_slice(&color);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The outline of a `w` × `h` rectangle.
    fn outline(w: u32, h: u32, origin: (i64, i64)) -> GraphicsPlane {
        GraphicsPlane::from_fn(w, h, origin, |x, y| x == 0 || y == 0 || x == w - 1 || y == h - 1)
    }

    #[test]
    fn planes_hold_one_bit_per_pixel() {
        let plane = outline(16, 8, (20, 10));
        assert_eq!((plane.width(), plane.height(), plane.origin()), (16, 8, (20, 10)));
        assert_eq!(plane.count(), 2 * 16 + 2 * 6);
        assert_eq!(plane.byte_size(), 16, "128 pixels in two words");
        assert!(plane.is_set(0, 0) && plane.is_set(15, 7) && !plane.is_set(5, 3) && !plane.is_set(16, 0));
        assert!(plane.covers(20, 10) && plane.covers(35, 17) && !plane.covers(19, 10) && !plane.covers(25, 13));
        let bytes: Vec<u8> = (0..128).map(|i| u8::from(plane.is_set(i % 16, i / 16))).collect();
        assert_eq!(GraphicsPlane::from_bytes(16, 8, (20, 10), &bytes), Some(plane));
        assert_eq!(GraphicsPlane::from_bytes(16, 8, (0, 0), &bytes[..100]), None);
    }

    #[test]
    fn compositing_colours_set_pixels_only_and_clips_to_the_picture() {
        let (w, h) = (24u32, 12u32);
        let grey = [90u8, 90, 90, 255];
        let mut rgba: Vec<u8> = grey.repeat((w * h) as usize);
        // One plane half outside the picture (origin at column 20, row −2).
        let plane = outline(8, 6, (20, -2));
        composite(&mut rgba, w, h, [&plane], GRAPHICS_COLOR);
        let at = |x: u32, y: u32| &rgba[((y * w + x) * 4) as usize..((y * w + x) * 4 + 4) as usize];
        assert_eq!(at(20, 0), GRAPHICS_COLOR, "left edge of the outline");
        assert_eq!(at(23, 3), GRAPHICS_COLOR, "bottom edge, clipped at the right of the picture");
        assert_eq!(at(21, 1), grey, "inside the outline");
        assert_eq!(at(19, 3), grey, "left of the plane");
        let coloured = rgba.chunks(4).filter(|p| *p == GRAPHICS_COLOR).count();
        // Visible: rows 0–3 of the picture, columns 20–23: the left edge (4) and the bottom row (3 more).
        assert_eq!(coloured, 7);
        // A plane entirely outside, or a buffer of the wrong size, changes nothing.
        let before = rgba.clone();
        composite(&mut rgba, w, h, [&outline(4, 4, (-10, -10)), &outline(4, 4, (100, 0))], GRAPHICS_COLOR);
        composite(&mut rgba, w + 1, h, [&plane], GRAPHICS_COLOR);
        assert_eq!(rgba, before);
    }
}
