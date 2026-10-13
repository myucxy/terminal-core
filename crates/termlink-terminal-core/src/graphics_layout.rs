//! Deterministic image layout in cell units. Rational coordinates survive DPI/font changes and
//! snapshots without accumulating floating-point rounding or using Android-local geometry.
use crate::{
    HostGeometryV1, TerminalError, graphics_resources::ImageFormat, kitty_graphics::Control,
};

pub const MAX_PLACEMENT_CELLS: u32 = 4096;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ratio {
    pub numerator: u64,
    pub denominator: u64,
}
impl Ratio {
    pub(crate) fn new(numerator: u64, denominator: u64) -> Self {
        assert_ne!(denominator, 0);
        let (mut a, mut b) = (numerator, denominator);
        while b != 0 {
            let r = a % b;
            a = b;
            b = r;
        }
        Self {
            numerator: numerator / a,
            denominator: denominator / a,
        }
    }
    fn integer(value: u64) -> Self {
        Self::new(value, 1)
    }
    fn add(self, other: Self) -> Self {
        Self::new(
            self.numerator * other.denominator + other.numerator * self.denominator,
            self.denominator * other.denominator,
        )
    }
    fn subtract(self, other: Self) -> Self {
        Self::new(
            self.numerator * other.denominator - other.numerator * self.denominator,
            self.denominator * other.denominator,
        )
    }
    fn divide(self, value: u64) -> Self {
        Self::new(self.numerator, self.denominator * value)
    }
    fn multiply(self, value: u64) -> Self {
        Self::new(self.numerator * value, self.denominator)
    }
    fn at_most(self, other: Self) -> bool {
        u128::from(self.numerator) * u128::from(other.denominator)
            <= u128::from(other.numerator) * u128::from(self.denominator)
    }
    fn ceil(self) -> u64 {
        self.numerator.div_ceil(self.denominator)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceRectangle {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageLayout {
    pub source: SourceRectangle,
    /// Image rectangle within its allocation, including offsets and letterboxing.
    pub x: Ratio,
    pub y: Ratio,
    pub width: Ratio,
    pub height: Ratio,
    /// Allocation rectangle; also the default Kitty cursor advance.
    pub columns: u32,
    pub rows: u32,
}
impl ImageLayout {
    pub(crate) fn validate(self) -> Result<(), TerminalError> {
        if !(1..=MAX_PLACEMENT_CELLS).contains(&self.columns)
            || !(1..=MAX_PLACEMENT_CELLS).contains(&self.rows)
            || self.source.width == 0
            || self.source.height == 0
        {
            return Err(TerminalError::MalformedSnapshot);
        }
        for (i, ratio) in [self.x, self.y, self.width, self.height]
            .into_iter()
            .enumerate()
        {
            if ratio.numerator > 1 << 48
                || ratio.denominator == 0
                || ratio.denominator > 1 << 32
                || (i >= 2 && ratio.numerator == 0)
                || Ratio::new(ratio.numerator, ratio.denominator) != ratio
                || u128::from(ratio.numerator)
                    > u128::from(ratio.denominator) * u128::from(MAX_PLACEMENT_CELLS)
            {
                return Err(TerminalError::MalformedSnapshot);
            }
        }
        for (origin, extent, maximum) in [
            (self.x, self.width, self.columns),
            (self.y, self.height, self.rows),
        ] {
            let total = u128::from(origin.numerator) * u128::from(extent.denominator)
                + u128::from(extent.numerator) * u128::from(origin.denominator);
            if total
                > u128::from(maximum)
                    * u128::from(origin.denominator)
                    * u128::from(extent.denominator)
            {
                return Err(TerminalError::MalformedSnapshot);
            }
        }
        Ok(())
    }
}

fn geometry(value: HostGeometryV1) -> Result<(u64, u64), TerminalError> {
    value.validate()?;
    if value.cell_width == 0 {
        return Err(TerminalError::Unsupported(
            "image placement requires ordered cell pixel geometry from the selected display host",
        ));
    }
    Ok((u64::from(value.cell_width), u64::from(value.cell_height)))
}

pub fn kitty(
    control: &Control,
    width: u32,
    height: u32,
    host: HostGeometryV1,
) -> Result<Option<ImageLayout>, TerminalError> {
    ImageFormat::Rgba8888.byte_count(width, height)?;
    let x = control.number(b'x');
    let y = control.number(b'y');
    if x >= width || y >= height {
        return Ok(None);
    }
    let source = SourceRectangle {
        x,
        y,
        width: (if control.number(b'w') == 0 {
            width
        } else {
            control.number(b'w')
        })
        .min(width - x),
        height: (if control.number(b'h') == 0 {
            height
        } else {
            control.number(b'h')
        })
        .min(height - y),
    };
    let (cell_width, cell_height) = geometry(host)?;
    let ox = u64::from(control.number(b'X'));
    let oy = u64::from(control.number(b'Y'));
    if ox >= cell_width || oy >= cell_height {
        return Err(TerminalError::Unsupported(
            "Kitty X/Y offset must be smaller than the ordered cell pixel dimensions",
        ));
    }
    let columns = control.number(b'c');
    let rows = control.number(b'r');
    if columns > MAX_PLACEMENT_CELLS || rows > MAX_PLACEMENT_CELLS {
        return Err(TerminalError::ProtocolResourceLimit(
            "Kitty placement exceeds 4096 cells per axis",
        ));
    }
    let sw = u64::from(source.width);
    let sh = u64::from(source.height);
    let available_width = u64::from(columns) * cell_width;
    let available_height = u64::from(rows) * cell_height;
    let (image_width, image_height, draw_x, draw_y) = match (columns, rows) {
        (0, 0) => (
            Ratio::integer(sw),
            Ratio::integer(sh),
            Ratio::integer(ox),
            Ratio::integer(oy),
        ),
        (_, 0) => {
            let w = available_width - ox;
            (
                Ratio::integer(w),
                Ratio::new(w * sh, sw),
                Ratio::integer(ox),
                Ratio::integer(oy),
            )
        }
        (0, _) => {
            let h = available_height - oy;
            (
                Ratio::new(h * sw, sh),
                Ratio::integer(h),
                Ratio::integer(ox),
                Ratio::integer(oy),
            )
        }
        _ => {
            let aw = available_width - ox;
            let ah = available_height - oy;
            let (w, h) = if Ratio::new(aw, sw).at_most(Ratio::new(ah, sh)) {
                (Ratio::integer(aw), Ratio::new(aw * sh, sw))
            } else {
                (Ratio::new(ah * sw, sh), Ratio::integer(ah))
            };
            (
                w,
                h,
                Ratio::integer(ox).add(Ratio::integer(aw).subtract(w).divide(2)),
                Ratio::integer(oy).add(Ratio::integer(ah).subtract(h).divide(2)),
            )
        }
    };
    let columns = if columns == 0 {
        Ratio::integer(ox)
            .add(image_width)
            .divide(cell_width)
            .ceil()
    } else {
        u64::from(columns)
    };
    let rows = if rows == 0 {
        Ratio::integer(oy)
            .add(image_height)
            .divide(cell_height)
            .ceil()
    } else {
        u64::from(rows)
    };
    if columns > u64::from(MAX_PLACEMENT_CELLS) || rows > u64::from(MAX_PLACEMENT_CELLS) {
        return Err(TerminalError::ProtocolResourceLimit(
            "Kitty aspect-preserving placement exceeds 4096 cells",
        ));
    }
    Ok(Some(ImageLayout {
        source,
        x: draw_x.divide(cell_width),
        y: draw_y.divide(cell_height),
        width: image_width.divide(cell_width),
        height: image_height.divide(cell_height),
        columns: columns as u32,
        rows: rows as u32,
    }))
}

pub fn sixel(
    width: u32,
    height: u32,
    aspect_numerator: u16,
    aspect_denominator: u16,
    host: HostGeometryV1,
) -> Result<ImageLayout, TerminalError> {
    ImageFormat::Rgba8888.byte_count(width, height)?;
    if aspect_numerator == 0 || aspect_denominator == 0 {
        return Err(TerminalError::Unsupported(
            "SIXEL pixel aspect ratio must be positive",
        ));
    }
    let (cell_width, cell_height) = geometry(host)?;
    let width_cells = Ratio::new(u64::from(width), cell_width);
    let height_cells = Ratio::new(u64::from(height), cell_height)
        .multiply(u64::from(aspect_numerator))
        .divide(u64::from(aspect_denominator));
    if width_cells.ceil() > u64::from(MAX_PLACEMENT_CELLS)
        || height_cells.ceil() > u64::from(MAX_PLACEMENT_CELLS)
    {
        return Err(TerminalError::ProtocolResourceLimit(
            "SIXEL aspect ratio exceeds 4096 placement cells",
        ));
    }
    Ok(ImageLayout {
        source: SourceRectangle {
            x: 0,
            y: 0,
            width,
            height,
        },
        x: Ratio::integer(0),
        y: Ratio::integer(0),
        width: width_cells,
        height: height_cells,
        columns: width_cells.ceil() as u32,
        rows: height_cells.ceil() as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn host() -> HostGeometryV1 {
        HostGeometryV1 {
            cell_width: 10,
            cell_height: 20,
            ..Default::default()
        }
    }
    fn layout(control: &[u8], width: u32, height: u32) -> ImageLayout {
        kitty(&Control::parse(control).unwrap().0, width, height, host())
            .unwrap()
            .unwrap()
    }
    #[test]
    fn requested_box_preserves_aspect_and_offsets_do_not_grow_the_allocation() {
        let value = layout(b"Ga=p,c=4,r=2", 200, 100);
        assert_eq!(
            (value.x, value.y, value.width, value.height),
            (
                Ratio::integer(0),
                Ratio::new(1, 2),
                Ratio::integer(4),
                Ratio::integer(1)
            )
        );
        assert_eq!((value.columns, value.rows), (4, 2));
        let offset = layout(b"Ga=p,c=1,r=1,X=2,Y=4", 20, 20);
        assert_eq!(
            (offset.x, offset.y, offset.width, offset.height),
            (
                Ratio::new(1, 5),
                Ratio::new(2, 5),
                Ratio::new(4, 5),
                Ratio::new(2, 5)
            )
        );
        assert_eq!((offset.columns, offset.rows), (1, 1));
    }
    #[test]
    fn crop_intersection_single_axis_scaling_and_natural_dimensions_are_exact() {
        let value = layout(b"Ga=p,x=9,y=3,w=4294967295,h=10", 20, 8);
        assert_eq!(
            value.source,
            SourceRectangle {
                x: 9,
                y: 3,
                width: 11,
                height: 5
            }
        );
        assert_eq!(
            (value.width, value.height, value.columns, value.rows),
            (Ratio::new(11, 10), Ratio::new(1, 4), 2, 1)
        );
        let value = layout(b"Ga=p,c=3,X=1", 7, 3);
        assert_eq!(
            (value.width, value.height),
            (Ratio::new(29, 10), Ratio::new(87, 140))
        );
        let value = layout(b"Ga=p,r=3,Y=1", 7, 3);
        assert_eq!(
            (value.width, value.height),
            (Ratio::new(413, 30), Ratio::new(59, 20))
        );
        assert!(
            kitty(&Control::parse(b"Ga=p,x=20").unwrap().0, 20, 8, host())
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn no_invented_geometry_and_no_overflow_or_unbounded_aspect_expansion() {
        let c = Control::parse(b"Ga=p").unwrap().0;
        assert!(kitty(&c, 1, 1, HostGeometryV1::default()).is_err());
        for control in [
            b"Ga=p,c=4294967295".as_slice(),
            b"Ga=p,X=10",
            b"Ga=p,Y=20",
            b"Ga=p,c=4096",
        ] {
            assert!(kitty(&Control::parse(control).unwrap().0, 1, 4096, host()).is_err());
        }
        let value = sixel(5, 3, 2, 3, host()).unwrap();
        assert_eq!(
            (value.width, value.height),
            (Ratio::new(1, 2), Ratio::new(1, 10))
        );
        assert!(sixel(1, 4096, u16::MAX, 1, host()).is_err());
        assert!(sixel(1, 1, 0, 1, host()).is_err());
    }
}
