//! The picture of the display check: a quick visual check of a screen in the
//! style of the SMPTE RP 133 test pattern — grey steps from 0 to 100 %, a 5 %
//! square inside black and a 95 % square inside white, a continuous ramp, fine
//! line pairs and a border. It is generated for the exact pixel size of the
//! area that shows it, so that it can be drawn 1:1, and holds no patient data.
//! It is not a calibration.

/// A rectangle of pixels: columns `x .. x + width`, rows `y .. y + height`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    pub fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x && y >= self.y && x - self.x < self.width && y - self.y < self.height
    }

    /// The pixel at the centre (rounded down).
    pub fn center(&self) -> (u32, u32) {
        (self.x + self.width / 2, self.y + self.height / 2)
    }
}

/// A uniform patch: its level in percent and where it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Patch {
    pub percent: u8,
    pub rect: PixelRect,
}

/// The 8-bit code value of a grey level in percent: round(p × 255).
pub fn code_value(percent: u8) -> u8 {
    (f64::from(percent.min(100)) / 100.0 * 255.0).round() as u8
}

/// The level of the background: 50 %.
const BACKGROUND: u8 = 50;

/// The picture and its layout.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayCheck {
    pub width: u32,
    pub height: u32,
    /// RGBA, row-major, opaque grey.
    pub rgba: Vec<u8>,
    /// The grey steps 0, 10 … 100 %, left to right.
    pub steps: Vec<Patch>,
    /// The 5 % square inside the 0 % step.
    pub near_black: Patch,
    /// The 95 % square inside the 100 % step.
    pub near_white: Patch,
    /// The continuous ramp, black at the left to white at the right.
    pub ramp: PixelRect,
    /// Line pairs: 1-pixel vertical lines, 1-pixel horizontal lines, 2-pixel
    /// vertical lines, 1-pixel checkerboard.
    pub line_pairs: [PixelRect; 4],
    /// The gap between the steps and the ramp, where labels can go.
    pub labels: PixelRect,
}

impl DisplayCheck {
    /// The picture for an area of `width` × `height` pixels.
    pub fn new(width: u32, height: u32) -> Self {
        let (w, h) = (width.max(1), height.max(1));
        // Margin: 4 % of the shorter side, at least 8 pixels (but never more than a quarter of it).
        let margin = ((f64::from(w.min(h)) * 0.04).round() as u32).max(8).min(w.min(h) / 4);
        let (x0, y0) = (margin, margin);
        let inner_w = w.saturating_sub(2 * margin);
        let inner_h = h.saturating_sub(2 * margin);

        let steps_h = inner_h * 34 / 100;
        let steps: Vec<Patch> = (0..11u32)
            .map(|i| {
                let left = x0 + i * inner_w / 11;
                let right = x0 + (i + 1) * inner_w / 11;
                Patch {
                    percent: (i * 10) as u8,
                    rect: PixelRect { x: left, y: y0, width: right - left, height: steps_h },
                }
            })
            .collect();
        let square_in = |step: &Patch, percent: u8| {
            let r = step.rect;
            let side = (r.width.min(r.height) * 45 / 100).max(1).min(r.width.min(r.height));
            let rect =
                PixelRect { x: r.x + (r.width - side) / 2, y: r.y + (r.height - side) / 2, width: side, height: side };
            Patch { percent, rect }
        };
        let near_black = square_in(&steps[0], 5);
        let near_white = square_in(&steps[10], 95);

        let labels = PixelRect { x: x0, y: y0 + steps_h, width: inner_w, height: inner_h * 8 / 100 };
        let ramp = PixelRect { x: x0, y: labels.y + labels.height, width: inner_w, height: inner_h * 20 / 100 };
        let pairs_y = ramp.y + ramp.height + inner_h * 6 / 100;
        let pairs_h = (y0 + inner_h).saturating_sub(pairs_y);
        let block_w = inner_w.saturating_sub(3 * margin) / 4;
        let line_pairs = [0u32, 1, 2, 3].map(|k| PixelRect {
            x: x0 + k * (block_w + margin),
            y: pairs_y,
            width: block_w,
            height: pairs_h,
        });

        let mut check =
            Self { width: w, height: h, rgba: Vec::new(), steps, near_black, near_white, ramp, line_pairs, labels };
        let rgba =
            (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).flat_map(|(x, y)| grey(check.code_at(x, y))).collect();
        check.rgba = rgba;
        check
    }

    /// The 8-bit grey value of pixel (`x`, `y`) by the layout.
    fn code_at(&self, x: u32, y: u32) -> u8 {
        let (w, h) = (self.width, self.height);
        // The border: white at the edge, black inside it.
        if x == 0 || y == 0 || x + 1 == w || y + 1 == h {
            return 255;
        }
        if x == 1 || y == 1 || x + 2 == w || y + 2 == h {
            return 0;
        }
        for square in [self.near_black, self.near_white] {
            if square.rect.contains(x, y) {
                return code_value(square.percent);
            }
        }
        if let Some(step) = self.steps.iter().find(|s| s.rect.contains(x, y)) {
            return code_value(step.percent);
        }
        // A black frame of two pixels around the steps, so that the 50 % step stands out
        // from the background of the same level.
        if let (Some(first), Some(last)) = (self.steps.first(), self.steps.last()) {
            let (left, top) = (first.rect.x.saturating_sub(2), first.rect.y.saturating_sub(2));
            let (right, bottom) = (last.rect.x + last.rect.width + 2, last.rect.y + last.rect.height + 2);
            if (left..right).contains(&x) && (top..bottom).contains(&y) {
                return 0;
            }
        }
        if self.ramp.contains(x, y) {
            let span = f64::from(self.ramp.width.saturating_sub(1).max(1));
            return (f64::from(x - self.ramp.x) / span * 255.0).round().min(255.0) as u8;
        }
        if let Some(k) = self.line_pairs.iter().position(|r| r.contains(x, y)) {
            let (dx, dy) = (x - self.line_pairs[k].x, y - self.line_pairs[k].y);
            let on = match k {
                0 => dx % 2 == 0,
                1 => dy % 2 == 0,
                2 => (dx / 2) % 2 == 0,
                _ => (dx + dy) % 2 == 0,
            };
            return if on { 255 } else { 0 };
        }
        code_value(BACKGROUND)
    }

    /// The grey value of pixel (`x`, `y`) of the picture.
    pub fn grey_at(&self, x: u32, y: u32) -> Option<u8> {
        (x < self.width && y < self.height).then(|| self.rgba[((y * self.width + x) * 4) as usize])
    }
}

fn grey(v: u8) -> [u8; 4] {
    [v, v, v, 255]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_patch_reads_its_level(check: &DisplayCheck) {
        for step in &check.steps {
            let (cx, cy) = step.rect.center();
            // Away from the small squares: the top left quarter of the step.
            let (qx, qy) = (step.rect.x + step.rect.width / 8, step.rect.y + step.rect.height / 8);
            for (x, y) in [(qx, qy), if step.percent == 0 || step.percent == 100 { (qx, qy) } else { (cx, cy) }] {
                assert_eq!(check.grey_at(x, y), Some(code_value(step.percent)), "{} % at ({x}, {y})", step.percent);
            }
        }
        for square in [check.near_black, check.near_white] {
            let r = square.rect;
            for (x, y) in [(r.x, r.y), r.center(), (r.x + r.width - 1, r.y + r.height - 1)] {
                assert_eq!(check.grey_at(x, y), Some(code_value(square.percent)), "{} % at ({x}, {y})", square.percent);
            }
            // Just outside the square: the step around it.
            let around = if square.percent == 5 { 0 } else { 255 };
            assert_eq!(check.grey_at(r.x - 1, r.y), Some(around));
            assert_eq!(check.grey_at(r.x + r.width, r.y + r.height - 1), Some(around));
        }
    }

    /// The layout of 800 × 600 pixels, worked out by hand: margin 24 (4 % of
    /// 600), steps 752 / 11 ≈ 68.4 pixels wide and 187 high (34 % of 552).
    #[test]
    fn patches_are_at_their_pixels_for_800_by_600() {
        let check = DisplayCheck::new(800, 600);
        assert_eq!(check.rgba.len(), 800 * 600 * 4);
        assert_eq!(check.steps.len(), 11);
        assert_eq!(check.steps[0].rect, PixelRect { x: 24, y: 24, width: 68, height: 187 });
        assert_eq!(check.steps[5].rect, PixelRect { x: 365, y: 24, width: 69, height: 187 });
        assert_eq!(check.steps[10].rect, PixelRect { x: 707, y: 24, width: 69, height: 187 });
        assert_eq!(check.near_black.rect, PixelRect { x: 43, y: 102, width: 30, height: 30 });
        // The values: 0 around the 5 % square, 13 inside it, 128 at the centre of 50 %, 242 for 95 %.
        assert_eq!(check.grey_at(30, 30), Some(0));
        assert_eq!((check.grey_at(43, 102), check.grey_at(72, 131)), (Some(13), Some(13)));
        assert_eq!((check.grey_at(42, 102), check.grey_at(73, 131)), (Some(0), Some(0)));
        assert_eq!(check.grey_at(399, 117), Some(128));
        let (x, y) = check.near_white.rect.center();
        assert_eq!(check.grey_at(x, y), Some(242));
        assert_eq!([0, 5, 10, 50, 95, 100].map(code_value), [0, 13, 26, 128, 242, 255]);
        every_patch_reads_its_level(&check);
    }

    #[test]
    fn patches_are_at_their_pixels_for_1280_by_720() {
        let check = DisplayCheck::new(1280, 720);
        // Margin 29 (4 % of 720); steps 1222 / 11 ≈ 111 pixels wide, 225 high (34 % of 662).
        assert_eq!(check.steps[0].rect, PixelRect { x: 29, y: 29, width: 111, height: 225 });
        assert_eq!(check.steps[10].rect.x + check.steps[10].rect.width, 1280 - 29);
        every_patch_reads_its_level(&check);
        // Border: the outermost rows and columns white, the next ones black.
        for (x, y) in [(0, 0), (640, 0), (1279, 360), (640, 719)] {
            assert_eq!(check.grey_at(x, y), Some(255), "({x}, {y})");
        }
        for (x, y) in [(1, 1), (640, 1), (1278, 360), (640, 718)] {
            assert_eq!(check.grey_at(x, y), Some(0), "({x}, {y})");
        }
        assert_eq!(check.grey_at(10, 360), Some(128), "background");
        // The steps are framed in black, so that the 50 % step stands out from the background.
        let fifty = check.steps[5].rect;
        assert_eq!(check.grey_at(fifty.x + 5, fifty.y - 1), Some(0));
        assert_eq!(check.grey_at(fifty.x + 5, fifty.y + fifty.height + 1), Some(0));
        assert_eq!(check.grey_at(fifty.x + 5, fifty.y - 3), Some(128));
    }

    #[test]
    fn ramp_runs_from_black_to_white_and_lines_alternate() {
        let check = DisplayCheck::new(1024, 768);
        let r = check.ramp;
        let row: Vec<u8> = (r.x..r.x + r.width).filter_map(|x| check.grey_at(x, r.y + r.height / 2)).collect();
        assert_eq!((row[0], row[row.len() - 1]), (0, 255));
        assert!(row.windows(2).all(|w| w[0] <= w[1]), "monotonic");
        assert!(row.windows(2).all(|w| w[1] - w[0] <= 1), "no jumps: one code value at a time");
        let [vertical, horizontal, wide, checker] = check.line_pairs;
        let at = |x, y| check.grey_at(x, y).unwrap_or(1);
        let (vx, vy) = vertical.center();
        assert_ne!(at(vx, vy), at(vx + 1, vy));
        assert_eq!(at(vx, vy), at(vx, vy + 1));
        let (hx, hy) = horizontal.center();
        assert_ne!(at(hx, hy), at(hx, hy + 1));
        assert_eq!(at(hx, hy), at(hx + 1, hy));
        let (wx, wy) = (wide.x, wide.center().1);
        assert_eq!([at(wx, wy), at(wx + 1, wy), at(wx + 2, wy), at(wx + 3, wy)], [255, 255, 0, 0]);
        let (cx, cy) = checker.center();
        assert_ne!(at(cx, cy), at(cx + 1, cy));
        assert_ne!(at(cx, cy), at(cx, cy + 1));
        assert!(vertical.width > 10 && vertical.height > 10 && checker.x + checker.width <= 1024 - 24);
    }

    #[test]
    fn tiny_sizes_do_not_panic() {
        for (w, h) in [(0, 0), (1, 1), (3, 2), (20, 11), (64, 4000)] {
            let check = DisplayCheck::new(w, h);
            assert_eq!(check.rgba.len(), (check.width * check.height * 4) as usize);
        }
    }
}
