//! Colour maps for PET and nuclear-medicine images: lookup tables of 256 RGB
//! entries, indexed by the display level a window produces (entry 0 for the
//! low end of the window, entry 255 for the high end).
//!
//! | Map | Entry 0 | Entry 255 | Between |
//! |---|---|---|---|
//! | Grey | black | white | a grey ramp |
//! | Inverted grey | white | black | the grey ramp reversed: high uptake dark, as customary for PET |
//! | Hot iron | black | (255, 254, 252) | red rises by 2 per entry over entries 0–127 (then stays 255), green by 2 over 128–255, blue by 4 over 192–255: black, red, orange, yellow, nearly white |
//! | Rainbow | black | red (255, 0, 0) | linear between blue (entry 51), cyan (102), green (153) and yellow (204) |
//!
//! A map re-colours the grey picture of a window ([`recolour`]), so it works
//! with every grey pipeline; pixels outside a volume stay black. Rainbow is
//! offered because it is customary; the default for PET stays the inverted
//! grey scale.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use summrise_i18n::tr;

/// One of the colour maps offered for PET and nuclear-medicine series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Colormap {
    /// Black to white: the display of every other modality.
    #[default]
    Grey,
    /// White to black: high values dark.
    InvertedGrey,
    /// Black through red, orange and yellow to nearly white.
    HotIron,
    /// Black through blue, cyan, green and yellow to red.
    Rainbow,
}

/// The entries of one map, entry `i` for display level `i`.
pub type Table = [[u8; 3]; 256];

static GREY: Table = table(Colormap::Grey);
static INVERTED_GREY: Table = table(Colormap::InvertedGrey);
static HOT_IRON: Table = table(Colormap::HotIron);
static RAINBOW: Table = table(Colormap::Rainbow);

/// Control points of the rainbow, every 51 entries.
const RAINBOW_POINTS: [[u8; 3]; 6] = [[0, 0, 0], [0, 0, 255], [0, 255, 255], [0, 255, 0], [255, 255, 0], [255, 0, 0]];

/// Entry `i` of `map`.
const fn entry(map: Colormap, i: usize) -> [u8; 3] {
    // `i` is below 256 here, so every value below fits into a byte.
    let level = i as u8;
    match map {
        Colormap::Grey => [level, level, level],
        Colormap::InvertedGrey => [255 - level, 255 - level, 255 - level],
        Colormap::HotIron => {
            let red = if i < 128 { (2 * i) as u8 } else { 255 };
            let green = if i < 128 { 0 } else { (2 * (i - 128)) as u8 };
            let blue = if i < 192 { 0 } else { (4 * (i - 192)) as u8 };
            [red, green, blue]
        }
        Colormap::Rainbow => {
            let segment = if i >= 255 { 4 } else { i / 51 };
            let k = i - 51 * segment;
            let (from, to) = (RAINBOW_POINTS[segment], RAINBOW_POINTS[segment + 1]);
            let mut out = [0u8; 3];
            let mut c = 0;
            while c < 3 {
                // Rounded linear interpolation over the 51 entries of the segment.
                out[c] = ((from[c] as usize * (51 - k) + to[c] as usize * k + 25) / 51) as u8;
                c += 1;
            }
            out
        }
    }
}

const fn table(map: Colormap) -> Table {
    let mut out = [[0u8; 3]; 256];
    let mut i = 0;
    while i < 256 {
        out[i] = entry(map, i);
        i += 1;
    }
    out
}

impl Colormap {
    /// Every map, in the order they are offered.
    pub const ALL: [Self; 4] = [Self::Grey, Self::InvertedGrey, Self::HotIron, Self::Rainbow];

    /// The name shown to the user.
    pub fn label(self) -> &'static str {
        match self {
            // Translators: the names of the colour maps of PET and nuclear-medicine images.
            Self::Grey => tr!("Grey"),
            Self::InvertedGrey => tr!("Inverted grey"),
            Self::HotIron => tr!("Hot iron"),
            Self::Rainbow => tr!("Rainbow"),
        }
    }

    /// The map after this one in [`Self::ALL`]; after the last, the first.
    pub fn next(self) -> Self {
        let at = Self::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Self::ALL[(at + 1) % Self::ALL.len()]
    }

    /// The 256 entries of the map.
    pub fn table(self) -> &'static Table {
        match self {
            Self::Grey => &GREY,
            Self::InvertedGrey => &INVERTED_GREY,
            Self::HotIron => &HOT_IRON,
            Self::Rainbow => &RAINBOW,
        }
    }

    /// The colour of display level `level`.
    pub fn rgb(self, level: u8) -> [u8; 3] {
        self.table()[usize::from(level)]
    }

    /// The colour of a normalised display intensity `t` (0 to 1, as
    /// [`crate::Window::apply`] gives it).
    pub fn at(self, t: f64) -> [u8; 3] {
        self.rgb(level(t))
    }
}

/// The display level (0 to 255) of a normalised intensity, as the grey
/// picture rounds it.
pub fn level(t: f64) -> u8 {
    if t.is_finite() { (t.clamp(0.0, 1.0) * 255.0).round() as u8 } else { 0 }
}

/// Shows the grey picture `rgba` (RGBA8 as [`crate::window::gray_to_rgba`]
/// makes it, one pixel per value) through `map`: the grey level of every
/// pixel picks the entry. Pixels whose value (`value(i)`) is not finite —
/// outside a reformatted volume — stay as they are, black. The grey map
/// leaves the picture unchanged.
pub fn recolour(rgba: &mut [u8], value: impl Fn(usize) -> f32 + Sync, map: Colormap) {
    if map == Colormap::Grey {
        return;
    }
    let table = map.table();
    rgba.par_chunks_mut(4 * 4096).enumerate().for_each(|(chunk, pixels)| {
        let base = chunk * 4096;
        for (n, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            if value(base + n).is_finite() {
                let [r, g, b] = table[usize::from(pixel[0])];
                pixel[..3].copy_from_slice(&[r, g, b]);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::{Window, gray_to_rgba};

    #[test]
    fn end_points_of_every_map() {
        assert_eq!((Colormap::Grey.rgb(0), Colormap::Grey.rgb(255)), ([0, 0, 0], [255, 255, 255]));
        assert_eq!((Colormap::InvertedGrey.rgb(0), Colormap::InvertedGrey.rgb(255)), ([255, 255, 255], [0, 0, 0]));
        assert_eq!((Colormap::HotIron.rgb(0), Colormap::HotIron.rgb(255)), ([0, 0, 0], [255, 254, 252]));
        assert_eq!((Colormap::Rainbow.rgb(0), Colormap::Rainbow.rgb(255)), ([0, 0, 0], [255, 0, 0]));
        assert_eq!(Colormap::default(), Colormap::Grey);
    }

    #[test]
    fn hot_iron_runs_through_red_orange_and_yellow() {
        let hot = Colormap::HotIron;
        assert_eq!(hot.rgb(64), [128, 0, 0]);
        assert_eq!(hot.rgb(127), [254, 0, 0]);
        assert_eq!(hot.rgb(128), [255, 0, 0], "red at the middle of the window");
        assert_eq!(hot.rgb(160), [255, 64, 0], "orange");
        assert_eq!(hot.rgb(192), [255, 128, 0]);
        assert_eq!(hot.rgb(224), [255, 192, 128], "towards yellow and white");
        // Every channel rises monotonically: brighter means more uptake.
        let table = hot.table();
        assert!(table.windows(2).all(|w| (0..3).all(|c| w[1][c] >= w[0][c])));
    }

    #[test]
    fn rainbow_passes_through_its_control_points() {
        let rainbow = Colormap::Rainbow;
        let points = [(0, [0, 0, 0]), (51, [0, 0, 255]), (102, [0, 255, 255]), (153, [0, 255, 0])];
        for (level, colour) in points.into_iter().chain([(204, [255, 255, 0]), (255, [255, 0, 0])]) {
            assert_eq!(rainbow.rgb(level), colour, "entry {level}");
        }
        // Half-way between blue and cyan.
        assert_eq!(rainbow.rgb(76), [0, 125, 255]);
    }

    #[test]
    fn grey_maps_are_ramps() {
        for level in [0u8, 1, 77, 128, 254, 255] {
            assert_eq!(Colormap::Grey.rgb(level), [level; 3]);
            assert_eq!(Colormap::InvertedGrey.rgb(level), [255 - level; 3]);
        }
    }

    #[test]
    fn maps_cycle_and_have_names() {
        let mut map = Colormap::Grey;
        let mut seen = Vec::new();
        for _ in 0..4 {
            seen.push(map.label());
            map = map.next();
        }
        assert_eq!(map, Colormap::Grey, "after the last comes the first");
        assert_eq!(seen, ["Grey", "Inverted grey", "Hot iron", "Rainbow"]);
        assert_eq!(Colormap::HotIron.at(0.5), [255, 0, 0]);
        assert_eq!(Colormap::InvertedGrey.at(f64::NAN), [255, 255, 255], "no level: entry 0");
        assert_eq!((level(-1.0), level(2.0), level(0.4)), (0, 255, 102));
    }

    #[test]
    fn recolouring_follows_the_window_and_keeps_the_outside_black() {
        // SUV 0, 2.5, 5 and 9 in the window SUV 0-5, and a pixel outside the volume.
        let values = [0.0f32, 2.5, 5.0, 9.0, f32::NAN];
        let window = Window::from_range(0.0, 5.0);
        let grey = gray_to_rgba(values.len(), |i| values[i], &window, false);
        let pixel = |rgba: &[u8], i: usize| [rgba[4 * i], rgba[4 * i + 1], rgba[4 * i + 2], rgba[4 * i + 3]];

        let mut inverted = grey.clone();
        recolour(&mut inverted, |i| values[i], Colormap::InvertedGrey);
        assert_eq!(pixel(&inverted, 0), [255, 255, 255, 255], "no uptake: white");
        assert_eq!(pixel(&inverted, 2), [0, 0, 0, 255], "the top of the window: black");
        assert_eq!(pixel(&inverted, 3), [0, 0, 0, 255]);
        assert_eq!(pixel(&inverted, 4), [0, 0, 0, 255], "outside the volume stays black");

        let mut hot = grey.clone();
        recolour(&mut hot, |i| values[i], Colormap::HotIron);
        assert_eq!(pixel(&hot, 1), [255, 0, 0, 255], "the middle of the window is red");
        assert_eq!(pixel(&hot, 2), [255, 254, 252, 255]);
        assert_eq!(pixel(&hot, 4), [0, 0, 0, 255]);

        let mut unchanged = grey.clone();
        recolour(&mut unchanged, |i| values[i], Colormap::Grey);
        assert_eq!(unchanged, grey);

        // Rainbow over SUV 0–5: SUV 2, two fifths of the window, is cyan; SUV 5 and above red.
        let suv = [0.0f32, 2.0, 5.0, 9.0];
        let mut rainbow = gray_to_rgba(suv.len(), |i| suv[i], &window, false);
        recolour(&mut rainbow, |i| suv[i], Colormap::Rainbow);
        assert_eq!(pixel(&rainbow, 0), [0, 0, 0, 255]);
        assert_eq!(pixel(&rainbow, 1), [0, 255, 255, 255]);
        assert_eq!((pixel(&rainbow, 2), pixel(&rainbow, 3)), ([255, 0, 0, 255], [255, 0, 0, 255]));

        // Inversion (I) applies before the map.
        let mut inverted_hot = gray_to_rgba(values.len(), |i| values[i], &window, true);
        recolour(&mut inverted_hot, |i| values[i], Colormap::HotIron);
        assert_eq!(pixel(&inverted_hot, 0), [255, 254, 252, 255]);
        assert_eq!(pixel(&inverted_hot, 4), [0, 0, 0, 255]);
    }
}
