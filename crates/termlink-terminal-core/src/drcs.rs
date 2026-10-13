//! DECDLD payload decoding, using VT510 parameter semantics and explicit bounded cells.
use crate::{
    TerminalError,
    graphics_resources::{ImageData, ImageFormat},
};

pub const MAX_DRCS_PAYLOAD_BYTES: usize = 16 * 1024;

#[derive(Debug)]
pub struct DrcsDownload {
    pub font: u8,
    pub erase: u8,
    pub columns: u16,
    pub rows: u8,
    pub full_cell: bool,
    pub characters96: bool,
    pub matrix_height: u8,
    pub cell_width: u8,
    pub cell_height: u8,
    pub designator: Vec<u8>,
    pub glyphs: Vec<DrcsGlyph>,
}
#[derive(Debug)]
pub struct DrcsGlyph {
    pub position: u8,
    pub mask: ImageData,
}

fn invalid(bytes: &[u8], offset: usize, reason: &'static str) -> TerminalError {
    TerminalError::UnsupportedPayload {
        offset,
        reason,
        byte: bytes.get(offset).copied(),
    }
}

/// Returned masks are not installed until the enclosing terminal transaction commits.
pub fn decode(parameters: &[u16], payload: &[u8]) -> Result<DrcsDownload, TerminalError> {
    if payload.len() > MAX_DRCS_PAYLOAD_BYTES {
        return Err(TerminalError::ProtocolResourceLimit(
            "DECDLD payload exceeds 16384 bytes",
        ));
    }
    let param = |index: usize| {
        parameters
            .get(index)
            .copied()
            .filter(|&v| v != u16::MAX)
            .unwrap_or(0)
    };
    if parameters.len() > 8
        || param(0) > 2
        || param(1) > 95
        || param(2) > 2
        || param(3) == 1
        || param(3) > 10
        || param(5) > 2
        || param(7) > 1
    {
        return Err(TerminalError::Unsupported(
            "DECDLD font, position, erase, width, usage or repertoire parameter",
        ));
    }
    let (columns, rows, cell_height) = match param(4) {
        0 | 1 => (80, 24, 16),
        2 => (132, 24, 16),
        11 => (80, 36, 10),
        12 => (132, 36, 10),
        21 => (80, 48, 8),
        22 => (132, 48, 8),
        _ => return Err(TerminalError::Unsupported("DECDLD font set size")),
    };
    let full_cell = param(5) == 2;
    let cell_width = if columns == 80 { 10 } else { 6 };
    let maximum_width = if full_cell {
        cell_width
    } else if columns == 80 {
        8
    } else {
        5
    };
    let legacy = (2..=4).contains(&param(3));
    let width = if legacy {
        u32::from(param(3) + 3)
    } else if param(3) == 0 {
        maximum_width
    } else {
        u32::from(param(3))
    };
    let height = if legacy {
        10
    } else if param(6) == 0 {
        cell_height
    } else {
        u32::from(param(6))
    };
    if !legacy && (width > maximum_width || height > cell_height) {
        return Err(TerminalError::Unsupported(
            "DECDLD width/height combination for selected font set",
        ));
    }
    let output_height = if legacy { 20 } else { cell_height };
    let output_width = cell_width.max(width);
    let characters96 = param(7) == 1;
    let mut designator_end = 0;
    while designator_end < payload.len() && (0x20..=0x2f).contains(&payload[designator_end]) {
        designator_end += 1;
    }
    if designator_end > 2
        || designator_end >= payload.len()
        || !(0x30..=0x7e).contains(&payload[designator_end])
    {
        return Err(invalid(
            payload,
            designator_end,
            "DECDLD designator requires up to two intermediates and a final byte",
        ));
    }
    designator_end += 1;
    let mut glyphs = Vec::new();
    glyphs
        .try_reserve_exact(96)
        .map_err(|_| TerminalError::LimitExceeded)?;
    let mut position = usize::from(param(1));
    let body = &payload[designator_end..];
    let mut source_offset = designator_end;
    // An empty body is an erase/empty definition request. Empty patterns between separators are blank glyphs.
    if !body.is_empty() {
        for encoded in body.split(|&byte| byte == b';') {
            if position > 95 {
                return Err(invalid(
                    payload,
                    source_offset,
                    "DECDLD glyph position exceeds 95",
                ));
            }
            let mut mask = ImageData::allocate(output_width, output_height, ImageFormat::Alpha8)?;
            let mut x = 0;
            let mut y = 0;
            for (index, &byte) in encoded.iter().enumerate() {
                match byte {
                    b'?'..=b'~' => {
                        if x >= width {
                            return Err(invalid(
                                payload,
                                source_offset + index,
                                "DECDLD glyph row exceeds matrix width",
                            ));
                        }
                        let sixel = byte - b'?';
                        for bit in 0..6 {
                            if sixel & (1 << bit) == 0 {
                                continue;
                            }
                            if y + bit >= height {
                                return Err(invalid(
                                    payload,
                                    source_offset + index,
                                    "DECDLD set pixel exceeds matrix height",
                                ));
                            }
                            let px = x + if full_cell {
                                0
                            } else {
                                (output_width - width) / 2
                            };
                            let py = (y + bit) * if legacy { 2 } else { 1 };
                            mask.pixels_mut()[(py * output_width + px) as usize] = 255;
                            if legacy {
                                mask.pixels_mut()[((py + 1) * output_width + px) as usize] = 255;
                            }
                        }
                        x += 1;
                    }
                    b'/' => {
                        y += 6;
                        x = 0;
                        if y >= height {
                            return Err(invalid(
                                payload,
                                source_offset + index,
                                "DECDLD sixel band exceeds matrix height",
                            ));
                        }
                    }
                    b'\r' | b'\n' | b'\t' => {}
                    _ => {
                        return Err(invalid(
                            payload,
                            source_offset + index,
                            "unsupported DECDLD glyph character",
                        ));
                    }
                }
            }
            if characters96 || (1..=94).contains(&position) {
                glyphs.push(DrcsGlyph {
                    position: position as u8,
                    mask,
                });
            }
            source_offset += encoded.len() + 1;
            position += 1;
        }
    }
    Ok(DrcsDownload {
        font: param(0) as u8,
        erase: param(2) as u8,
        columns,
        rows,
        full_cell,
        characters96,
        matrix_height: height as u8,
        cell_width: output_width as u8,
        cell_height: output_height as u8,
        designator: payload[..designator_end].to_vec(),
        glyphs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_cell_sixel_bands_and_blank_glyph_have_exact_alpha_pixels() {
        let value = decode(&[1, 1, 0, 5, 1, 2, 8, 0], b" @~?@/B;").unwrap();
        assert_eq!(value.designator, b" @");
        assert_eq!(
            (value.font, value.columns, value.rows, value.erase),
            (1, 80, 24, 0)
        );
        assert_eq!(value.glyphs.len(), 2);
        assert_eq!(value.glyphs[0].position, 1);
        let mask = &value.glyphs[0].mask;
        assert_eq!((mask.width(), mask.height()), (10, 16));
        for y in 0..16 {
            for x in 0..10 {
                let expected = x == 0 && y < 8 || x == 2 && y == 0;
                assert_eq!(
                    mask.pixels()[y * 10 + x],
                    if expected { 255 } else { 0 },
                    "{x},{y}"
                );
            }
        }
        assert!(value.glyphs[1].mask.pixels().iter().all(|&p| p == 0));
    }
    #[test]
    fn repertoire_endpoints_default_font_and_legacy_doubled_height() {
        let ignored = decode(&[0, 0, 2, 5, 1, 2, 8, 0], b" @~").unwrap();
        assert!(ignored.glyphs.is_empty());
        let complete = decode(&[0, 0, 2, 5, 1, 2, 8, 1], b" @~").unwrap();
        assert_eq!(complete.glyphs[0].position, 0);
        let legacy = decode(&[1, 1, 0, 2, 1, 1, 99, 0], b"&%C@/G").unwrap();
        let mask = &legacy.glyphs[0].mask;
        assert_eq!((mask.width(), mask.height()), (10, 20));
        assert_eq!(mask.pixels()[2], 255);
        assert_eq!(mask.pixels()[12], 255);
        assert_eq!(mask.pixels()[182], 255);
        assert_eq!(mask.pixels()[192], 255);
        assert!(decode(&[2, 95, 1, 5, 2, 2, 8, 1], b" @@").is_ok());
    }
    #[test]
    fn all_standard_screen_sets_and_errors_are_explicit_and_bounded() {
        for (screen, columns, rows, height) in [
            (0, 80, 24, 16),
            (1, 80, 24, 16),
            (2, 132, 24, 16),
            (11, 80, 36, 10),
            (12, 132, 36, 10),
            (21, 80, 48, 8),
            (22, 132, 48, 8),
        ] {
            let font = decode(&[1, 1, 0, 0, screen, 2, 0, 0], b" @@").unwrap();
            assert_eq!(
                (font.columns, font.rows, font.glyphs[0].mask.height()),
                (columns, rows, height)
            );
        }
        for (parameters, bytes) in [
            (vec![3], b" @".as_slice()),
            (vec![0, 96], b" @"),
            (vec![0, 1, 3], b" @"),
            (vec![0, 1, 0, 1], b" @"),
            (vec![0, 1, 0, 11], b" @"),
            (vec![0, 1, 0, 5, 3], b" @"),
            (vec![0, 1, 0, 5, 1, 3], b" @"),
            (vec![0, 1, 0, 5, 1, 2, 17], b" @"),
            (vec![0, 1, 0, 5, 1, 2, 8, 2], b" @"),
            (vec![0, 95, 0, 5, 1, 2, 8, 1], b" @@@;@"),
            (vec![0, 1, 0, 5, 1, 2, 8, 0], b" @@@@@@@"),
            (vec![0, 1, 0, 5, 1, 2, 8, 0], b" @~/~"),
            (vec![], b"   @"),
            (vec![], b" @!"),
            (vec![], b""),
            (vec![], b"\x80"),
        ] {
            assert!(
                decode(&parameters, bytes).is_err(),
                "accepted {parameters:?} {bytes:?}"
            );
        }
        assert!(matches!(
            decode(&[], b" @\x80"),
            Err(TerminalError::UnsupportedPayload {
                offset: 2,
                byte: Some(128),
                ..
            })
        ));
    }
}
