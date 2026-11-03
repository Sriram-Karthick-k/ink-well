//! Measurements: turning a length in canvas units into one a person reads, in the unit, format and precision they
//! chose, at the scale they set.
//!
//! The units are pixels, points, millimeters, centimeters, meters, kilometers, inches, feet, yards and miles; the
//! format is abbreviated, full or symbol; the precision is rounded, tenths, hundredths, thousandths or a fraction.
//! There is a "1:n" drawing scale, a pixels-per-inch setting and an auto-scale that picks the unit.

use serde::{Deserialize, Serialize};

/// A unit of length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Unit {
    Px,
    Pt,
    Mm,
    Cm,
    M,
    Km,
    In,
    Ft,
    Yd,
    Mi,
}

impl Unit {
    pub const ALL: [Self; 10] = [
        Self::Px,
        Self::Pt,
        Self::Mm,
        Self::Cm,
        Self::M,
        Self::Km,
        Self::In,
        Self::Ft,
        Self::Yd,
        Self::Mi,
    ];

    /// How long one of this unit is, in inches. A pixel is a size on the screen, so it takes the pixels per inch.
    #[must_use]
    pub fn inches(self, pixels_per_inch: f32) -> f32 {
        match self {
            Self::Px => 1.0 / pixels_per_inch.max(1.0),
            Self::Pt => 1.0 / 72.0,
            Self::Mm => 1.0 / 25.4,
            Self::Cm => 1.0 / 2.54,
            Self::M => 100.0 / 2.54,
            Self::Km => 100_000.0 / 2.54,
            Self::In => 1.0,
            Self::Ft => 12.0,
            Self::Yd => 36.0,
            Self::Mi => 63_360.0,
        }
    }

    /// The short name: `cm`, `in`.
    #[must_use]
    pub fn abbreviation(self) -> &'static str {
        match self {
            Self::Px => "px",
            Self::Pt => "pt",
            Self::Mm => "mm",
            Self::Cm => "cm",
            Self::M => "m",
            Self::Km => "km",
            Self::In => "in",
            Self::Ft => "ft",
            Self::Yd => "yd",
            Self::Mi => "mi",
        }
    }

    /// The name in full, for one of it or for many.
    #[must_use]
    pub fn name(self, plural: bool) -> &'static str {
        match (self, plural) {
            (Self::Px, false) => "pixel",
            (Self::Px, true) => "pixels",
            (Self::Pt, false) => "point",
            (Self::Pt, true) => "points",
            (Self::Mm, false) => "millimeter",
            (Self::Mm, true) => "millimeters",
            (Self::Cm, false) => "centimeter",
            (Self::Cm, true) => "centimeters",
            (Self::M, false) => "meter",
            (Self::M, true) => "meters",
            (Self::Km, false) => "kilometer",
            (Self::Km, true) => "kilometers",
            (Self::In, false) => "inch",
            (Self::In, true) => "inches",
            (Self::Ft, false) => "foot",
            (Self::Ft, true) => "feet",
            (Self::Yd, false) => "yard",
            (Self::Yd, true) => "yards",
            (Self::Mi, false) => "mile",
            (Self::Mi, true) => "miles",
        }
    }

    /// The mark inches and feet have (`″`, `′`); the others have none and keep their abbreviation.
    #[must_use]
    fn symbol(self) -> Option<&'static str> {
        match self {
            Self::In => Some("\u{2033}"),
            Self::Ft => Some("\u{2032}"),
            _ => None,
        }
    }

    fn metric(self) -> bool {
        matches!(self, Self::Mm | Self::Cm | Self::M | Self::Km)
    }

    fn imperial(self) -> bool {
        matches!(self, Self::In | Self::Ft | Self::Yd | Self::Mi)
    }
}

/// How a unit is written after the number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    /// `5 cm`
    Abbreviated,
    /// `5 centimeters`
    Full,
    /// `5″` for inches and feet, otherwise as abbreviated.
    Symbol,
}

impl Format {
    pub const ALL: [Self; 3] = [Self::Abbreviated, Self::Full, Self::Symbol];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Abbreviated => "Abbreviated",
            Self::Full => "Full",
            Self::Symbol => "Symbol",
        }
    }
}

/// How many places a number is written to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Precision {
    /// `12`
    Rounded,
    /// `12.3`
    Tenths,
    /// `12.35`
    Hundredths,
    /// `12.346`
    Thousandths,
    /// `12 3/8`, to the nearest sixteenth.
    Fraction,
}

impl Precision {
    pub const ALL: [Self; 5] = [
        Self::Rounded,
        Self::Tenths,
        Self::Hundredths,
        Self::Thousandths,
        Self::Fraction,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Rounded => "Whole",
            Self::Tenths => "Tenths",
            Self::Hundredths => "Hundredths",
            Self::Thousandths => "Thousandths",
            Self::Fraction => "Fractions",
        }
    }
}

/// Everything that decides how a length is written.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Measurement {
    pub unit: Unit,
    pub format: Format,
    pub precision: Precision,
    /// Pixels (canvas units) to an inch, which is what turns the canvas into paper.
    pub pixels_per_inch: f32,
    /// The drawing is `n` times smaller than what it shows (`1:n`); `None` for life size.
    pub scale: Option<f32>,
    /// Pick the unit that reads best (millimeters for a small length, meters for a big one) instead of `unit`.
    pub auto: bool,
}

impl Default for Measurement {
    fn default() -> Self {
        Self {
            unit: Unit::Px,
            format: Format::Abbreviated,
            precision: Precision::Rounded,
            pixels_per_inch: 96.0,
            scale: None,
            auto: false,
        }
    }
}

impl Measurement {
    /// The largest scale, `1:n`.
    pub const MAX_SCALE: f32 = 100_000.0;

    /// Brings every value into its valid range.
    #[must_use]
    pub fn clamped(mut self) -> Self {
        self.pixels_per_inch = if self.pixels_per_inch.is_finite() {
            self.pixels_per_inch.clamp(1.0, 4_800.0)
        } else {
            96.0
        };
        self.scale = self
            .scale
            .filter(|n| n.is_finite() && *n > 0.0)
            .map(|n| n.clamp(0.001, Self::MAX_SCALE));
        self
    }

    /// A length in canvas units as inches in the world: the drawing's scale is applied.
    fn world_inches(&self, canvas: f32) -> f32 {
        canvas / self.pixels_per_inch.max(1.0) * self.scale.unwrap_or(1.0)
    }

    /// The unit lengths are written in: the chosen one, or, with auto-scale on, whichever of its kind reads best.
    #[must_use]
    pub fn unit_for(&self, canvas: f32) -> Unit {
        if !self.auto || !(self.unit.metric() || self.unit.imperial()) {
            return self.unit;
        }
        let inches = self.world_inches(canvas).abs();
        if self.unit.metric() {
            let meters = inches * 0.0254;
            match meters {
                m if m < 0.01 => Unit::Mm,
                m if m < 1.0 => Unit::Cm,
                m if m < 1000.0 => Unit::M,
                _ => Unit::Km,
            }
        } else {
            match inches {
                i if i < 36.0 => Unit::In,
                i if i < 36.0 * 3.0 * 10.0 => Unit::Ft,
                i if i < 63_360.0 => Unit::Yd,
                _ => Unit::Mi,
            }
        }
    }

    /// A length in canvas units in `unit`, as a number.
    #[must_use]
    pub fn value(&self, canvas: f32, unit: Unit) -> f32 {
        self.world_inches(canvas) / unit.inches(self.pixels_per_inch)
    }

    /// A length as it is written: `12.5 cm`.
    #[must_use]
    pub fn length(&self, canvas: f32) -> String {
        let unit = self.unit_for(canvas);
        self.write(self.value(canvas, unit), unit)
    }

    /// An area in canvas units squared, written in the square of the unit: `12.5 cm²`.
    #[must_use]
    pub fn area(&self, canvas: f32) -> String {
        let unit = self.unit_for(canvas.abs().sqrt());
        let side = self.value(1.0, unit);
        let number = self.number(canvas * side * side);
        match self.format {
            Format::Full => format!("{number} square {}", unit.name(true)),
            _ => format!("{number} {}\u{b2}", unit.abbreviation()),
        }
    }

    /// An angle in degrees: `45.0°`. It has no unit to choose, and is written to the same precision (a fraction
    /// is written in tenths).
    #[must_use]
    pub fn angle(&self, degrees: f32) -> String {
        let precision = match self.precision {
            Precision::Fraction => Precision::Tenths,
            p => p,
        };
        format!("{}\u{b0}", write_number(degrees, precision))
    }

    fn number(&self, value: f32) -> String {
        write_number(value, self.precision)
    }

    /// `value` with its unit after it, as the format says.
    fn write(&self, value: f32, unit: Unit) -> String {
        let number = self.number(value);
        match self.format {
            Format::Abbreviated => format!("{number} {}", unit.abbreviation()),
            Format::Full => {
                let one = number == "1";
                format!("{number} {}", unit.name(!one))
            }
            Format::Symbol => match unit.symbol() {
                Some(symbol) => format!("{number}{symbol}"),
                None => format!("{number} {}", unit.abbreviation()),
            },
        }
    }
}

/// `value` to the places the precision says. A fraction is the nearest sixteenth, with the whole part first.
#[must_use]
fn write_number(value: f32, precision: Precision) -> String {
    if !value.is_finite() {
        return "0".to_owned();
    }
    match precision {
        Precision::Rounded => format!("{:.0}", value + 0.0),
        Precision::Tenths => format!("{value:.1}"),
        Precision::Hundredths => format!("{value:.2}"),
        Precision::Thousandths => format!("{value:.3}"),
        Precision::Fraction => {
            let sixteenths = (value.abs() * 16.0).round() as i64;
            let (whole, part) = (sixteenths / 16, sixteenths % 16);
            let sign = if value < 0.0 && sixteenths > 0 {
                "-"
            } else {
                ""
            };
            if part == 0 {
                return format!("{sign}{whole}");
            }
            let divisor = gcd(part, 16);
            let fraction = format!("{}/{}", part / divisor, 16 / divisor);
            if whole == 0 {
                format!("{sign}{fraction}")
            } else {
                format!("{sign}{whole} {fraction}")
            }
        }
    }
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inches() -> Measurement {
        Measurement {
            unit: Unit::In,
            ..Measurement::default()
        }
    }

    #[test]
    fn pixels_per_inch_turn_canvas_units_into_inches_and_the_other_units() {
        let m = Measurement {
            unit: Unit::Cm,
            ..Measurement::default()
        };
        // 96 canvas units are an inch, which is 2.54 centimeters.
        assert!((m.value(96.0, Unit::In) - 1.0).abs() < 1e-5);
        assert!((m.value(96.0, Unit::Cm) - 2.54).abs() < 1e-4);
        assert!((m.value(96.0, Unit::Mm) - 25.4).abs() < 1e-3);
        assert!((m.value(96.0, Unit::Pt) - 72.0).abs() < 1e-3);
        assert!((m.value(96.0, Unit::Px) - 96.0).abs() < 1e-3);
        assert!((m.value(96.0 * 36.0, Unit::Yd) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn the_scale_makes_the_world_bigger_than_the_drawing() {
        let m = Measurement {
            unit: Unit::M,
            scale: Some(50.0),
            ..Measurement::default()
        };
        // An inch on the page at 1:50 is 50 inches, 1.27 meters.
        assert!((m.value(96.0, Unit::M) - 1.27).abs() < 1e-4);
        let life_size = Measurement { scale: None, ..m };
        assert!((life_size.value(96.0, Unit::M) - 0.0254).abs() < 1e-5);
    }

    #[test]
    fn the_formats_write_the_unit_three_ways() {
        let mut m = Measurement {
            unit: Unit::Cm,
            precision: Precision::Tenths,
            ..Measurement::default()
        };
        assert_eq!(m.length(96.0), "2.5 cm");
        m.format = Format::Full;
        assert_eq!(m.length(96.0), "2.5 centimeters");
        m.format = Format::Symbol;
        assert_eq!(m.length(96.0), "2.5 cm", "centimeters have no symbol");
        m.unit = Unit::In;
        m.precision = Precision::Rounded;
        assert_eq!(m.length(96.0), "1\u{2033}");
        m.unit = Unit::Ft;
        assert_eq!(m.length(96.0 * 24.0), "2\u{2032}");
    }

    #[test]
    fn one_of_a_unit_is_not_plural() {
        let mut m = inches();
        m.format = Format::Full;
        assert_eq!(m.length(96.0), "1 inch");
        assert_eq!(m.length(192.0), "2 inches");
        m.unit = Unit::Ft;
        assert_eq!(m.length(96.0 * 12.0), "1 foot");
    }

    #[test]
    fn precision_writes_the_places_asked_for() {
        let mut m = inches();
        for (precision, expected) in [
            (Precision::Rounded, "1 in"),
            (Precision::Tenths, "1.3 in"),
            (Precision::Hundredths, "1.26 in"),
            (Precision::Thousandths, "1.256 in"),
        ] {
            m.precision = precision;
            // 1.2561 inches: not on a rounding edge at any of these.
            assert_eq!(m.length(96.0 * 1.2561), expected, "{precision:?}");
        }
    }

    #[test]
    fn fractions_are_to_the_nearest_sixteenth_and_in_lowest_terms() {
        for (inches_wide, expected) in [
            (0.0, "0 in"),
            (1.0, "1 in"),
            (0.5, "1/2 in"),
            (2.25, "2 1/4 in"),
            (3.1875, "3 3/16 in"),
            (0.03, "0 in"),
            (0.97, "1 in"),
            (5.5 + 1.0 / 32.0 + 0.001, "5 9/16 in"),
        ] {
            let m = Measurement {
                precision: Precision::Fraction,
                ..inches()
            };
            assert_eq!(m.length(96.0 * inches_wide), expected, "{inches_wide}");
        }
    }

    #[test]
    fn auto_scale_picks_the_unit_that_reads_best_in_the_same_system() {
        let metric = Measurement {
            unit: Unit::Cm,
            auto: true,
            ..Measurement::default()
        };
        assert_eq!(metric.unit_for(96.0 * 0.1), Unit::Mm, "2.5 mm");
        assert_eq!(metric.unit_for(96.0 * 4.0), Unit::Cm, "10 cm");
        assert_eq!(metric.unit_for(96.0 * 100.0), Unit::M, "2.5 m");
        assert_eq!(metric.unit_for(96.0 * 100_000.0), Unit::Km);
        let imperial = Measurement {
            unit: Unit::In,
            auto: true,
            ..Measurement::default()
        };
        assert_eq!(imperial.unit_for(96.0 * 10.0), Unit::In);
        assert_eq!(imperial.unit_for(96.0 * 100.0), Unit::Ft);
        let pixels = Measurement {
            unit: Unit::Px,
            auto: true,
            ..Measurement::default()
        };
        assert_eq!(
            pixels.unit_for(1.0e9),
            Unit::Px,
            "pixels and points are not part of either system"
        );
    }

    #[test]
    fn areas_are_in_the_square_of_the_unit_and_angles_in_degrees() {
        let m = Measurement {
            unit: Unit::Cm,
            precision: Precision::Tenths,
            ..Measurement::default()
        };
        // 96 by 96 canvas units is 2.54 by 2.54 cm: 6.4516 square cm.
        assert_eq!(m.area(96.0 * 96.0), "6.5 cm\u{b2}");
        let full = Measurement {
            format: Format::Full,
            ..m
        };
        assert_eq!(full.area(96.0 * 96.0), "6.5 square centimeters");
        assert_eq!(m.angle(45.0), "45.0\u{b0}");
        let fraction = Measurement {
            precision: Precision::Fraction,
            ..m
        };
        assert_eq!(
            fraction.angle(45.04),
            "45.0\u{b0}",
            "an angle is never written as a fraction"
        );
    }

    #[test]
    fn nonsense_never_makes_nonsense_text() {
        let m = inches();
        assert_eq!(m.length(f32::NAN), "0 in");
        assert_eq!(m.length(f32::INFINITY), "0 in");
        assert_eq!(m.length(-96.0), "-1 in");
        let broken = Measurement {
            pixels_per_inch: f32::NAN,
            scale: Some(-3.0),
            ..Measurement::default()
        }
        .clamped();
        assert!((broken.pixels_per_inch - 96.0).abs() < 1e-6);
        assert_eq!(broken.scale, None);
        let huge = Measurement {
            scale: Some(1.0e12),
            ..Measurement::default()
        }
        .clamped();
        assert_eq!(huge.scale, Some(Measurement::MAX_SCALE));
    }

    #[test]
    fn a_setting_file_with_only_some_of_it_still_reads() {
        let m: Measurement =
            serde_json::from_str(r#"{"unit":"mm","precision":"hundredths"}"#).expect("valid");
        assert_eq!(m.unit, Unit::Mm);
        assert_eq!(m.precision, Precision::Hundredths);
        assert_eq!(m.format, Format::Abbreviated);
        assert!((m.pixels_per_inch - 96.0).abs() < 1e-6);
    }
}
