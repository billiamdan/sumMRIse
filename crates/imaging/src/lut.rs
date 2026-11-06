//! Lookup tables (PS3.3 C.11 and C.7.6.3.1.5): the tables of the Modality LUT
//! Sequence, of the VOI LUT Sequence ("contrast tables") and of palette colour
//! images all map an input value to a table entry the same way.

/// Why a table cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LutError {
    #[error("the lookup table has no entries")]
    Empty,
    #[error("the lookup table has {0} bits per entry; 1 to 16 are possible")]
    Bits(u16),
}

/// A DICOM lookup table: entry `i` is the output for the input value
/// `first + i`. Inputs below `first` take the first entry, inputs beyond the
/// last one the last entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupTable {
    first: i64,
    bits: u8,
    data: Vec<u16>,
    /// The largest output possible: 2^bits − 1, or the largest entry when a
    /// file understates its bits.
    max: u16,
}

impl LookupTable {
    /// A table of `data` whose first entry is the output for `first`, with
    /// `bits` bits per entry (the third value of the LUT Descriptor).
    pub fn new(first: i64, bits: u16, data: Vec<u16>) -> Result<Self, LutError> {
        if data.is_empty() {
            return Err(LutError::Empty);
        }
        if !(1..=16).contains(&bits) {
            return Err(LutError::Bits(bits));
        }
        let nominal = u16::try_from((1u32 << bits) - 1).unwrap_or(u16::MAX);
        let max = data.iter().copied().max().unwrap_or(0).max(nominal);
        Ok(Self { first, bits: bits as u8, data, max })
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// The input value mapped to the first entry.
    pub fn first(&self) -> i64 {
        self.first
    }

    /// Bits per entry, as recorded.
    pub fn bits(&self) -> u8 {
        self.bits
    }

    pub fn entries(&self) -> &[u16] {
        &self.data
    }

    /// The largest output possible (2^bits − 1, or the largest entry if that is larger).
    pub fn output_max(&self) -> u16 {
        self.max
    }

    /// The entry for the input `input`, clamped to the table.
    #[inline]
    pub fn entry(&self, input: i64) -> u16 {
        let last = self.data.len() as i64 - 1;
        self.data[input.saturating_sub(self.first).clamp(0, last) as usize]
    }

    /// The entry for a value that need not be a whole number: rounded to the
    /// nearest input value first. `NaN` takes the first entry.
    #[inline]
    pub fn entry_for(&self, value: f64) -> u16 {
        if value.is_nan() {
            return self.data[0];
        }
        // Saturating float-to-int conversion: far-away values land on the first or last entry.
        self.entry(value.round() as i64)
    }

    /// The output for `value` as a fraction of the largest output: 0 … 1.
    #[inline]
    pub fn fraction(&self, value: f64) -> f64 {
        f64::from(self.entry_for(value)) / f64::from(self.max.max(1))
    }

    /// The smallest and the largest input value the table holds an entry for.
    pub fn input_range(&self) -> (i64, i64) {
        (self.first, self.first + self.data.len() as i64 - 1)
    }

    /// The smallest and the largest entry.
    pub fn output_range(&self) -> (u16, u16) {
        let lo = self.data.iter().copied().min().unwrap_or(0);
        let hi = self.data.iter().copied().max().unwrap_or(0);
        (lo, hi)
    }

    /// The smallest and the largest entry for the inputs `low ..= high`.
    pub fn output_range_of(&self, low: i64, high: i64) -> (u16, u16) {
        let (low, high) = (low.min(high), low.max(high));
        let (first, last) = self.input_range();
        // Inputs outside the table share the first or last entry.
        let from = low.clamp(first, last);
        let to = high.clamp(first, last);
        (from..=to).map(|input| self.entry(input)).fold((u16::MAX, 0), |(lo, hi), v| (lo.min(v), hi.max(v)))
    }
}

/// A contrast table stored with an image (one item of the VOI LUT Sequence):
/// maps modality values to grey levels along the stored curve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiLut {
    /// LUT Explanation, title-cased, or a numbered name.
    pub name: String,
    pub table: LookupTable,
}

/// The red, green and blue tables of a palette colour image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub red: LookupTable,
    pub green: LookupTable,
    pub blue: LookupTable,
}

impl Palette {
    /// The colour of index `index`, 8 bits per channel.
    #[inline]
    pub fn rgb(&self, index: i64) -> [u8; 3] {
        [to_8bit(&self.red, index), to_8bit(&self.green, index), to_8bit(&self.blue, index)]
    }

    /// Interleaved 8-bit RGB of `indices`.
    pub fn to_rgb(&self, indices: impl IntoIterator<Item = i64>) -> Vec<u8> {
        indices.into_iter().flat_map(|i| self.rgb(i)).collect()
    }
}

/// The entry of `table` for `index`, scaled to 0 … 255 by rounding.
#[inline]
fn to_8bit(table: &LookupTable, index: i64) -> u8 {
    let max = u32::from(table.output_max().max(1));
    let entry = u32::from(table.entry(index));
    ((entry * 255 * 2 + max) / (2 * max)).min(255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_outside_the_table_take_its_first_or_last_entry() {
        let t = LookupTable::new(-2, 8, vec![10, 20, 30, 40]).expect("table");
        assert_eq!((t.len(), t.first(), t.bits()), (4, -2, 8));
        assert_eq!(t.input_range(), (-2, 1));
        assert_eq!([t.entry(-100), t.entry(-2), t.entry(-1), t.entry(1), t.entry(500)], [10, 10, 20, 40, 40]);
        // Values in between are rounded to the nearest input.
        assert_eq!([t.entry_for(-1.4), t.entry_for(-0.4), t.entry_for(f64::NAN), t.entry_for(1e30)], [20, 30, 10, 40]);
        assert_eq!(t.output_range(), (10, 40));
        assert_eq!(t.output_range_of(0, -5), (10, 30));
        assert_eq!(t.output_range_of(7, 9), (40, 40));
    }

    #[test]
    fn fractions_are_relative_to_the_bits_of_the_descriptor() {
        // 12 bits: 4095 is white, whatever the largest entry.
        let t = LookupTable::new(0, 12, vec![0, 640, 4095]).expect("table");
        assert_eq!(t.output_max(), 4095);
        assert!((t.fraction(1.0) - 640.0 / 4095.0).abs() < 1e-12);
        // A file that understates its bits: the largest entry is white.
        let t = LookupTable::new(0, 8, vec![0, 300, 600]).expect("table");
        assert_eq!(t.output_max(), 600);
        assert_eq!(t.fraction(2.0), 1.0);
        assert_eq!(LookupTable::new(0, 0, vec![1]), Err(LutError::Bits(0)));
        assert_eq!(LookupTable::new(0, 17, vec![1]), Err(LutError::Bits(17)));
        assert_eq!(LookupTable::new(0, 8, Vec::new()), Err(LutError::Empty));
    }

    #[test]
    fn palette_scales_16_bit_entries_to_8_bits() {
        let table = |entries: [u16; 4], bits| LookupTable::new(0, bits, entries.to_vec()).expect("table");
        let eight =
            Palette { red: table([0, 1, 2, 200], 8), green: table([0, 1, 2, 30], 8), blue: table([0, 1, 2, 60], 8) };
        assert_eq!(eight.rgb(3), [200, 30, 60]);
        let sixteen = Palette {
            red: table([0, 0, 0, 200 * 257], 16),
            green: table([0, 0, 0, 30 * 257], 16),
            blue: table([0, 0, 0, 60 * 257], 16),
        };
        assert_eq!(sixteen.rgb(3), [200, 30, 60]);
        assert_eq!(sixteen.to_rgb([3, 0, 9]), [200, 30, 60, 0, 0, 0, 200, 30, 60]);
        // Rounded, not truncated: 32896 of 65535 is 128.0004 of 255.
        let mid = Palette { red: table([32896; 4], 16), green: table([128; 4], 16), blue: table([127; 4], 16) };
        assert_eq!(mid.rgb(0), [128, 0, 0]);
    }
}
