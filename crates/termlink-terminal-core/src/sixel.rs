//! Bounded DEC SIXEL raster decoding. Placement and protocol dispatch are separate.
use crate::{
    TerminalError,
    graphics_resources::{ImageData, ImageFormat, MAX_IMAGE_BYTES, MAX_IMAGE_DIMENSION},
};

pub const MAX_SIXEL_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_SIXEL_PIXEL_WORK: u64 = 16 * 1024 * 1024;
pub type SixelPalette = [[u8; 3]; 256];

/// VT340 power-up RGB percentages (chapter 2, table 2-3). Extended registers start black until
/// explicitly defined; they do not depend on a renderer's local theme or ANSI palette.
/// Reference: https://vt100.net/docs/vt3xx-gp/chapter2.html (protocol values, no implementation copied).
pub fn default_palette() -> SixelPalette {
    let mut palette = [[0; 3]; 256];
    let rgb = [
        [0, 0, 0],
        [20, 20, 80],
        [80, 13, 13],
        [20, 80, 20],
        [80, 20, 80],
        [20, 80, 80],
        [80, 80, 20],
        [53, 53, 53],
        [26, 26, 26],
        [33, 33, 60],
        [60, 26, 26],
        [33, 60, 33],
        [60, 33, 60],
        [33, 60, 60],
        [60, 60, 33],
        [80, 80, 80],
    ];
    for (target, source) in palette.iter_mut().zip(rgb) {
        *target = source.map(percent);
    }
    palette
}

#[derive(Debug)]
pub struct SixelImage {
    pub image: Option<ImageData>,
    pub palette: SixelPalette,
    pub aspect_numerator: u16,
    pub aspect_denominator: u16,
    pub cursor_x: u32,
    pub cursor_y: u32,
}

struct Raster<'a> {
    palette: SixelPalette,
    current: Option<usize>,
    foreground: Option<[u8; 3]>,
    opaque: bool,
    aspect: (u16, u16),
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    work: u64,
    drawn: bool,
    target: Option<&'a mut ImageData>,
}

fn invalid(bytes: &[u8], offset: usize, reason: &'static str) -> TerminalError {
    TerminalError::UnsupportedPayload {
        offset,
        reason,
        byte: bytes.get(offset).copied(),
    }
}

/// Parameters exclude the DCS introducer/final. No palette changes escape on failure.
pub fn decode(
    parameters: &[u16],
    payload: &[u8],
    palette: &SixelPalette,
    background: [u8; 3],
) -> Result<SixelImage, TerminalError> {
    decode_with_colors(
        parameters,
        payload,
        palette,
        Some(palette[0]),
        Some(background),
    )
}

/// Ordered terminal colors apply until a color register is selected. Missing colors are rejected
/// only when the raster actually needs them; palette-only and transparent downloads remain valid.
pub fn decode_with_colors(
    parameters: &[u16],
    payload: &[u8],
    palette: &SixelPalette,
    foreground: Option<[u8; 3]>,
    background: Option<[u8; 3]>,
) -> Result<SixelImage, TerminalError> {
    if payload.len() > MAX_SIXEL_PAYLOAD_BYTES {
        return Err(TerminalError::ProtocolResourceLimit(
            "SIXEL payload exceeds 4 MiB",
        ));
    }
    let param = |index: usize| {
        parameters
            .get(index)
            .copied()
            .filter(|&v| v != u16::MAX)
            .unwrap_or(0)
    };
    if parameters.len() > 3 || param(0) > 9 || param(1) > 2 {
        return Err(TerminalError::Unsupported("SIXEL header parameters"));
    }
    let aspect = (
        match param(0) {
            2 => 5,
            3 | 4 => 3,
            7..=9 => 1,
            _ => 2,
        },
        1,
    );
    let mut measured = Raster {
        palette: *palette,
        current: None,
        foreground,
        opaque: param(1) != 1,
        aspect,
        x: 0,
        y: 0,
        width: 0,
        height: 0,
        work: 0,
        drawn: false,
        target: None,
    };
    measured.walk(payload)?;
    let image = if measured.width == 0 || measured.height == 0 {
        None
    } else {
        let background = if param(1) != 1 {
            Some(background.ok_or(TerminalError::Unsupported(
                "opaque SIXEL requires an ordered terminal background color",
            ))?)
        } else {
            None
        };
        let mut image =
            ImageData::allocate(measured.width, measured.height, ImageFormat::Rgba8888)?;
        if let Some(background) = background {
            for pixel in image.pixels_mut().chunks_exact_mut(4) {
                pixel.copy_from_slice(&[background[0], background[1], background[2], 255]);
            }
        }
        let mut painted = Raster {
            palette: *palette,
            current: None,
            foreground,
            opaque: param(1) != 1,
            aspect,
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            work: 0,
            drawn: false,
            target: Some(&mut image),
        };
        painted.walk(payload)?;
        Some(image)
    };
    Ok(SixelImage {
        image,
        palette: measured.palette,
        aspect_numerator: measured.aspect.0,
        aspect_denominator: measured.aspect.1,
        cursor_x: measured.x,
        cursor_y: measured.y,
    })
}

impl Raster<'_> {
    fn walk(&mut self, bytes: &[u8]) -> Result<(), TerminalError> {
        let mut index = 0;
        while index < bytes.len() {
            let position = index;
            let byte = bytes[index];
            index += 1;
            match byte {
                b'?'..=b'~' => self.sixel(byte - b'?', 1)?,
                b'!' => {
                    let (parameters, _, count) = parameters(bytes, &mut index, 1)?;
                    if count == 0 || index >= bytes.len() || !(b'?'..=b'~').contains(&bytes[index])
                    {
                        return Err(invalid(
                            bytes,
                            index,
                            "SIXEL repeat requires a count and a sixel character",
                        ));
                    }
                    self.sixel(bytes[index] - b'?', parameters[0].max(1))?;
                    index += 1;
                }
                b'#' => {
                    let start = index;
                    let (values, positions, count) = parameters(bytes, &mut index, 5)?;
                    if count == 0 || values[0] > 255 {
                        return Err(invalid(bytes, start, "SIXEL color register must be 0..255"));
                    }
                    let register = values[0] as usize;
                    self.current = Some(register);
                    if count > 1 {
                        let invalid_component = match values[1] {
                            1 => (2..5).find(|&i| values[i] > if i == 2 { 360 } else { 100 }),
                            2 => (2..5).find(|&i| values[i] > 100),
                            _ => Some(1),
                        };
                        if let Some(component) = invalid_component {
                            return Err(invalid(
                                bytes,
                                positions[component],
                                "SIXEL color requires HLS(0..360,0..100,0..100) or RGB(0..100)",
                            ));
                        }
                        self.palette[register] = if values[1] == 1 {
                            hls(values[2], values[3], values[4])
                        } else {
                            [percent(values[2]), percent(values[3]), percent(values[4])]
                        };
                    }
                }
                b'"' => {
                    if self.drawn {
                        return Err(invalid(
                            bytes,
                            position,
                            "SIXEL raster attributes must precede pixel data",
                        ));
                    }
                    let (values, positions, count) = parameters(bytes, &mut index, 4)?;
                    let invalid_axis =
                        (0..2).find(|&axis| values[axis] == 0 || values[axis] > u16::MAX as u32);
                    let invalid_position = if count < 2 {
                        Some(index)
                    } else {
                        invalid_axis.map(|axis| positions[axis])
                    };
                    if let Some(position) = invalid_position {
                        return Err(invalid(
                            bytes,
                            position,
                            "SIXEL aspect ratio requires two nonzero 16-bit integers",
                        ));
                    }
                    let divisor = gcd(values[0], values[1]);
                    self.aspect = ((values[0] / divisor) as u16, (values[1] / divisor) as u16);
                    self.width = self.width.max(values[2]);
                    self.height = self.height.max(values[3]);
                    self.check_dimensions()?;
                }
                b'$' => self.x = 0,
                b'-' => {
                    self.x = 0;
                    self.y = self
                        .y
                        .checked_add(6)
                        .filter(|&y| y <= MAX_IMAGE_DIMENSION)
                        .ok_or(TerminalError::ProtocolResourceLimit(
                            "SIXEL vertical position exceeds 4096",
                        ))?;
                }
                b'\t' | b'\n' | b'\r' => {}
                _ => {
                    return Err(invalid(
                        bytes,
                        position,
                        "unsupported SIXEL payload character",
                    ));
                }
            }
        }
        Ok(())
    }
    fn check_dimensions(&self) -> Result<(), TerminalError> {
        if self.width > MAX_IMAGE_DIMENSION
            || self.height > MAX_IMAGE_DIMENSION
            || u64::from(self.width) * u64::from(self.height) * 4 > MAX_IMAGE_BYTES as u64
        {
            return Err(TerminalError::ProtocolResourceLimit(
                "SIXEL image exceeds 4096 dimensions or 16 MiB pixels",
            ));
        }
        Ok(())
    }
    fn sixel(&mut self, value: u8, repeat: u32) -> Result<(), TerminalError> {
        let rgb = if value != 0 {
            Some(self.current.map(|index| self.palette[index]).or(self.foreground).ok_or(
                TerminalError::Unsupported("SIXEL requires an ordered terminal foreground color before selecting a register"),
            )?)
        } else {
            None
        };
        self.work = self
            .work
            .checked_add(u64::from(repeat) * 6)
            .filter(|&work| work <= MAX_SIXEL_PIXEL_WORK)
            .ok_or(TerminalError::ProtocolResourceLimit(
                "SIXEL pixel work exceeds 16777216",
            ))?;
        let end = self
            .x
            .checked_add(repeat)
            .filter(|&end| end <= MAX_IMAGE_DIMENSION)
            .ok_or(TerminalError::ProtocolResourceLimit(
                "SIXEL horizontal position exceeds 4096",
            ))?;
        let rows = if self.opaque || value == 0 {
            6
        } else {
            8 - value.leading_zeros()
        };
        self.width = self.width.max(end);
        self.height = self.height.max(self.y + rows);
        self.check_dimensions()?;
        if let Some(image) = self.target.as_deref_mut() {
            let width = image.width() as usize;
            let rgb = rgb.unwrap_or([0; 3]);
            for x in self.x..end {
                for bit in 0..6 {
                    if value & (1 << bit) == 0 {
                        continue;
                    }
                    let offset = ((self.y + bit) as usize * width + x as usize) * 4;
                    image.pixels_mut()[offset..offset + 4]
                        .copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
                }
            }
        }
        self.x = end;
        self.drawn = true;
        Ok(())
    }
}

fn parameters(
    bytes: &[u8],
    index: &mut usize,
    maximum: usize,
) -> Result<([u32; 5], [usize; 5], usize), TerminalError> {
    let mut values = [0; 5];
    let mut positions = [*index; 5];
    let mut count = 0;
    if *index >= bytes.len() || !(bytes[*index].is_ascii_digit() || bytes[*index] == b';') {
        return Ok((values, positions, count));
    }
    loop {
        if count == maximum {
            return Err(invalid(bytes, *index, "SIXEL parameter count exceeded"));
        }
        positions[count] = *index;
        while *index < bytes.len() && bytes[*index].is_ascii_digit() {
            values[count] = values[count]
                .checked_mul(10u32)
                .and_then(|v| v.checked_add(u32::from(bytes[*index] - b'0')))
                .ok_or_else(|| invalid(bytes, *index, "SIXEL decimal parameter overflow"))?;
            *index += 1;
        }
        count += 1;
        if *index >= bytes.len() || bytes[*index] != b';' {
            break;
        }
        *index += 1;
    }
    Ok((values, positions, count))
}
fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}
fn percent(value: u32) -> u8 {
    ((value * 255 + 50) / 100) as u8
}

// DEC hue starts at blue; rotate to conventional red=0. Integer arithmetic is deterministic.
fn hls(hue: u32, lightness: u32, saturation: u32) -> [u8; 3] {
    let hue = (hue + 240) % 360;
    let chroma = (100 - (2 * lightness as i32 - 100).unsigned_abs()) * saturation;
    let intermediate = chroma * (60 - (hue % 120).abs_diff(60)) / 60;
    let base = lightness * 100 - chroma / 2;
    let rgb = match hue / 60 {
        0 => [chroma, intermediate, 0],
        1 => [intermediate, chroma, 0],
        2 => [0, chroma, intermediate],
        3 => [0, intermediate, chroma],
        4 => [intermediate, 0, chroma],
        _ => [chroma, 0, intermediate],
    };
    rgb.map(|value| (((value + base) * 255 + 5000) / 10000) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn image(bytes: &[u8], transparent: bool) -> SixelImage {
        decode(
            &[7, u16::from(transparent)],
            bytes,
            &[[0; 3]; 256],
            [10, 20, 30],
        )
        .unwrap()
    }
    #[test]
    fn golden_colors_overprint_repeat_and_transparency() {
        let output = image(b"\"1;1;3;6#1;2;100;0;0!2@$#2;2;0;100;0A", true);
        let image = output.image.unwrap();
        assert_eq!((image.width(), image.height()), (3, 6));
        let pixel = |x: usize, y: usize| &image.pixels()[(y * 3 + x) * 4..(y * 3 + x + 1) * 4];
        assert_eq!(pixel(0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel(1, 0), [255, 0, 0, 255]);
        assert_eq!(pixel(0, 1), [0, 255, 0, 255]);
        assert_eq!(pixel(2, 0), [0, 0, 0, 0]);
        assert_eq!(output.palette[1], [255, 0, 0]);
        assert_eq!(output.palette[2], [0, 255, 0]);
        assert_eq!((output.aspect_numerator, output.aspect_denominator), (1, 1));
        assert_eq!((output.cursor_x, output.cursor_y), (1, 0));
        let opaque = self::image(b"\"1;1;2;2#1;2;100;0;0@", false).image.unwrap();
        assert_eq!(&opaque.pixels()[4..8], [10, 20, 30, 255]);
    }
    #[test]
    fn hls_primaries_grays_and_missing_color_parameters() {
        assert_eq!(hls(0, 50, 100), [0, 0, 255]);
        assert_eq!(hls(120, 50, 100), [255, 0, 0]);
        assert_eq!(hls(240, 50, 100), [0, 255, 0]);
        assert_eq!(hls(360, 50, 100), [0, 0, 255]);
        assert_eq!(hls(0, 50, 0), [128; 3]);
        assert_eq!(hls(0, 0, 100), [0; 3]);
        assert_eq!(hls(0, 100, 100), [255; 3]);
        assert_eq!(image(b"#5;2;100", true).palette[5], [255, 0, 0]);
        assert_eq!(image(b"#5;2;;100;", true).palette[5], [0, 255, 0]);
    }
    #[test]
    fn ordered_colors_are_only_required_when_used_and_opaque_sixels_cover_zero_bits() {
        let palette = default_palette();
        let decoded =
            decode_with_colors(&[7], b"@", &palette, Some([1, 2, 3]), Some([4, 5, 6])).unwrap();
        let image = decoded.image.unwrap();
        assert_eq!((image.width(), image.height()), (1, 6));
        assert_eq!(&image.pixels()[..4], &[1, 2, 3, 255]);
        assert!(
            image.pixels()[4..]
                .chunks_exact(4)
                .all(|p| p == [4, 5, 6, 255])
        );
        assert_eq!(decoded.palette, palette);
        assert!(decode_with_colors(&[], b"#1;2;100;0;0", &palette, None, None).is_ok());
        assert!(decode_with_colors(&[7, 1], b"#1@", &palette, None, None).is_ok());
        assert!(decode_with_colors(&[7, 1], b"@", &palette, None, None).is_err());
        assert!(decode_with_colors(&[7], b"#1@", &palette, None, None).is_err());
        assert!(matches!(
            decode_with_colors(&[7, 1], b"#1;3;0;0;0@", &palette, None, None),
            Err(TerminalError::UnsupportedPayload {
                offset: 3,
                byte: Some(b'3'),
                ..
            })
        ));
        assert!(matches!(
            decode_with_colors(&[7, 1], b"#1;2;0;101;0@", &palette, None, None),
            Err(TerminalError::UnsupportedPayload {
                offset: 7,
                byte: Some(b'1'),
                ..
            })
        ));
        assert!(matches!(
            decode_with_colors(&[7, 1], b"\"1;0", &palette, None, None),
            Err(TerminalError::UnsupportedPayload {
                offset: 3,
                byte: Some(b'0'),
                ..
            })
        ));
    }
    #[test]
    fn raster_minimum_is_not_a_clip_and_nextline_preserves_canvas() {
        let output = image(b"\"4;2;1;1#1;2;100;100;100~-$@", true);
        let image = output.image.unwrap();
        assert_eq!((image.width(), image.height()), (1, 7));
        assert_eq!(image.pixels(), &[255; 28]);
        assert_eq!((output.aspect_numerator, output.aspect_denominator), (2, 1));
        assert_eq!((output.cursor_x, output.cursor_y), (1, 6));
        assert!(self::image(b"#1;2;0;0;0", false).image.is_none());
    }
    #[test]
    fn malformed_and_oversized_input_never_commits_palette_or_allocates_an_unbounded_canvas() {
        let palette = [[3; 3]; 256];
        for bytes in [
            b"!".as_slice(),
            b"!2",
            b"!2x;",
            b"#256@",
            b"#1;2;101;0;0@",
            b"#1;1;361;0;0@",
            b"@\"1;1",
            b"\"0;1",
            b"\"1;1;4097;1",
            b"!42949672960@",
            b"!4097@",
            b"\"1;1;4096;4096",
            b"#1;3;0;0;0",
            b"#1;2;1;2;3;4",
            b"#1;2;100;0;0\x80",
        ] {
            assert!(
                decode(&[], bytes, &palette, [0; 3]).is_err(),
                "accepted {bytes:?}"
            );
            assert_eq!(palette, [[3; 3]; 256]);
        }
        assert!(matches!(
            decode(&[], b"abc\x80", &palette, [0; 3]),
            Err(TerminalError::UnsupportedPayload {
                offset: 3,
                byte: Some(128),
                ..
            })
        ));
        assert!(decode(&[10], b"", &palette, [0; 3]).is_err());
        let work = b"!4096~$".repeat(683);
        assert!(matches!(
            decode(&[], &work, &palette, [0; 3]),
            Err(TerminalError::ProtocolResourceLimit(
                "SIXEL pixel work exceeds 16777216"
            ))
        ));
    }
}
