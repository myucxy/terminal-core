//! Bounded PNG (ISO/IEC 15948) decoding to straight RGBA8, including Adam7.
//! Metadata is never decompressed. No file access, embedded profiles or animation execution.
use crate::{
    TerminalError,
    graphics_resources::{GraphicsScratch, ImageData, ImageFormat, MAX_IMAGE_BYTES},
    graphics_zlib,
};

pub const MAX_PNG_BYTES: usize = 32 * 1024 * 1024;
const MAX_SCANLINE_BYTES: usize = 2 * MAX_IMAGE_BYTES + 16 * 1024;
const MAX_CHUNKS: usize = 65_536;
const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
// x origin, y origin, x stride, y stride; non-interlaced is a single pass.
const ADAM7: [(usize, usize, usize, usize); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];
const PLAIN: [(usize, usize, usize, usize); 1] = [(0, 0, 1, 1)];

const fn crc_table() -> [u32; 256] {
    let mut result = [0; 256];
    let mut index = 0;
    while index < 256 {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 8 {
            value = (value >> 1) ^ if value & 1 != 0 { 0xedb8_8320 } else { 0 };
            bit += 1;
        }
        result[index] = value;
        index += 1;
    }
    result
}
fn crc(bytes: &[u8]) -> u32 {
    const TABLE: [u32; 256] = crc_table();
    let mut value = !0u32;
    for &byte in bytes {
        value = TABLE[((value as u8) ^ byte) as usize] ^ (value >> 8);
    }
    !value
}
fn invalid(input: &[u8], offset: usize, reason: &'static str) -> TerminalError {
    TerminalError::UnsupportedPayload {
        offset,
        reason,
        byte: input.get(offset).copied(),
    }
}
fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().expect("checked PNG u32"))
}

#[derive(Clone, Copy)]
struct Header {
    width: usize,
    height: usize,
    depth: u8,
    color: u8,
    channels: usize,
    interlace: bool,
}
impl Header {
    fn parse(input: &[u8], offset: usize, bytes: &[u8]) -> Result<Self, TerminalError> {
        if bytes.len() != 13 {
            return Err(invalid(input, offset, "PNG IHDR length must be 13"));
        }
        let width = be32(&bytes[..4]);
        let height = be32(&bytes[4..8]);
        ImageFormat::Rgba8888.byte_count(width, height)?;
        let depth = bytes[8];
        let color = bytes[9];
        let channels = match (color, depth) {
            (0, 1 | 2 | 4 | 8 | 16) => 1,
            (2, 8 | 16) => 3,
            (3, 1 | 2 | 4 | 8) => 1,
            (4, 8 | 16) => 2,
            (6, 8 | 16) => 4,
            _ => {
                return Err(invalid(
                    input,
                    offset + 8,
                    "PNG color type and bit depth combination is unsupported",
                ));
            }
        };
        if bytes[10] != 0 {
            return Err(invalid(
                input,
                offset + 10,
                "PNG compression method must be deflate",
            ));
        }
        if bytes[11] != 0 {
            return Err(invalid(
                input,
                offset + 11,
                "PNG filter method must be zero",
            ));
        }
        if bytes[12] > 1 {
            return Err(invalid(
                input,
                offset + 12,
                "PNG interlace method must be zero or Adam7",
            ));
        }
        Ok(Self {
            width: width as usize,
            height: height as usize,
            depth,
            color,
            channels,
            interlace: bytes[12] == 1,
        })
    }
    fn passes(&self) -> &'static [(usize, usize, usize, usize)] {
        if self.interlace { &ADAM7 } else { &PLAIN }
    }
    fn row_bytes(&self, width: usize) -> usize {
        (width * self.channels * usize::from(self.depth)).div_ceil(8)
    }
    fn scanline_bytes(&self) -> usize {
        self.passes()
            .iter()
            .map(|&(x, y, dx, dy)| {
                let w = self.width.saturating_sub(x).div_ceil(dx);
                let h = self.height.saturating_sub(y).div_ceil(dy);
                if w == 0 {
                    0
                } else {
                    (self.row_bytes(w) + 1) * h
                }
            })
            .sum()
    }
}

struct Png<'a> {
    header: Header,
    palette: &'a [u8],
    transparency: &'a [u8],
    idat: Vec<(usize, usize)>,
    compressed_bytes: usize,
}
fn inspect(input: &[u8]) -> Result<Png<'_>, TerminalError> {
    if input.len() > MAX_PNG_BYTES {
        return Err(TerminalError::ProtocolResourceLimit(
            "PNG input exceeds 32 MiB",
        ));
    }
    if !input.starts_with(SIGNATURE) {
        return Err(invalid(input, 0, "invalid PNG signature"));
    }
    let mut header = None;
    let mut palette = None;
    let mut transparency = None;
    let mut idat = Vec::new();
    let mut compressed_bytes = 0;
    let mut offset = 8;
    let mut chunks = 0;
    let mut data_closed = false;
    let mut ended = false;
    while offset < input.len() {
        chunks += 1;
        if chunks > MAX_CHUNKS {
            return Err(TerminalError::ProtocolResourceLimit(
                "PNG chunk count exceeds 65536",
            ));
        }
        if ended || input.len() - offset < 12 {
            return Err(invalid(input, offset, "trailing or truncated PNG chunk"));
        }
        let length = be32(&input[offset..offset + 4]) as usize;
        if length > input.len() - offset - 12 {
            return Err(invalid(input, offset, "PNG chunk length exceeds input"));
        }
        let kind = &input[offset + 4..offset + 8];
        let start = offset + 8;
        let end = start + length;
        if !kind.iter().all(u8::is_ascii_alphabetic) || kind[2] & 32 != 0 {
            return Err(invalid(
                input,
                offset + 4,
                "PNG chunk type or reserved bit is invalid",
            ));
        }
        if crc(&input[offset + 4..end]) != be32(&input[end..end + 4]) {
            return Err(invalid(input, end, "PNG chunk CRC32 mismatch"));
        }
        if header.is_none() && kind != b"IHDR" {
            return Err(invalid(input, offset + 4, "PNG must begin with IHDR"));
        }
        if !idat.is_empty() && kind != b"IDAT" {
            data_closed = true;
        }
        let data = &input[start..end];
        match kind {
            b"IHDR" => {
                if header.is_some() {
                    return Err(invalid(input, offset + 4, "duplicate PNG IHDR"));
                }
                header = Some(Header::parse(input, start, data)?);
            }
            b"PLTE" => {
                let h = header.expect("IHDR already checked");
                if palette.is_some()
                    || transparency.is_some()
                    || !idat.is_empty()
                    || length == 0
                    || !length.is_multiple_of(3)
                    || length > 768
                    || matches!(h.color, 0 | 4)
                    || (h.color == 3 && length / 3 > 1usize << h.depth)
                {
                    return Err(invalid(
                        input,
                        start,
                        "PNG palette length, color type or order is invalid",
                    ));
                }
                palette = Some(data);
            }
            b"tRNS" => {
                let h = header.expect("IHDR already checked");
                let valid = match h.color {
                    0 | 2 => {
                        length == if h.color == 0 { 2 } else { 6 }
                            && data.chunks_exact(2).all(|v| {
                                u32::from(u16::from_be_bytes([v[0], v[1]])) < 1u32 << h.depth
                            })
                    }
                    3 => palette.is_some_and(|p| length > 0 && length <= p.len() / 3),
                    _ => false,
                };
                if transparency.is_some() || !idat.is_empty() || !valid {
                    return Err(invalid(
                        input,
                        start,
                        "PNG transparency length, samples or order is invalid",
                    ));
                }
                transparency = Some(data);
            }
            b"IDAT" => {
                if data_closed || (header.expect("IHDR").color == 3 && palette.is_none()) {
                    return Err(invalid(
                        input,
                        start,
                        "PNG IDAT order or required palette is invalid",
                    ));
                }
                idat.try_reserve(1)
                    .map_err(|_| TerminalError::LimitExceeded)?;
                idat.push((start, end));
                compressed_bytes += length;
            }
            b"IEND" => {
                if length != 0 || idat.is_empty() {
                    return Err(invalid(
                        input,
                        start,
                        "PNG IEND must be empty and follow IDAT",
                    ));
                }
                ended = true;
            }
            b"acTL" | b"fcTL" | b"fdAT" => {
                return Err(invalid(
                    input,
                    offset + 4,
                    "animated PNG is not supported by the static image profile",
                ));
            }
            _ if kind[0] & 32 == 0 => {
                return Err(invalid(input, offset + 4, "unknown required PNG chunk"));
            }
            _ => {} // Ancillary metadata is deliberately not expanded or executed.
        }
        offset = end + 4;
    }
    if !ended {
        return Err(invalid(input, offset, "PNG missing IEND"));
    }
    Ok(Png {
        header: header.expect("IEND follows IHDR"),
        palette: palette.unwrap_or(&[]),
        transparency: transparency.unwrap_or(&[]),
        idat,
        compressed_bytes,
    })
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i32::from(a) + i32::from(b) - i32::from(c);
    let da = (p - i32::from(a)).abs();
    let db = (p - i32::from(b)).abs();
    let dc = (p - i32::from(c)).abs();
    if da <= db && da <= dc {
        a
    } else if db <= dc {
        b
    } else {
        c
    }
}
fn sample(row: &[u8], index: usize, depth: u8) -> u16 {
    match depth {
        16 => u16::from_be_bytes([row[index * 2], row[index * 2 + 1]]),
        8 => u16::from(row[index]),
        _ => {
            let bit = index * usize::from(depth);
            u16::from((row[bit / 8] >> (8 - usize::from(depth) - bit % 8)) & ((1u8 << depth) - 1))
        }
    }
}
impl Png<'_> {
    fn compressed_error(&self, input: &[u8], error: TerminalError) -> TerminalError {
        match error {
            TerminalError::UnsupportedPayload {
                mut offset, reason, ..
            } => {
                for &(start, end) in &self.idat {
                    if offset < end - start {
                        return invalid(input, start + offset, reason);
                    }
                    offset -= end - start;
                }
                // End-of-stream is an exclusive source offset, never a fabricated payload byte.
                TerminalError::UnsupportedPayload {
                    offset: self.idat.last().expect("IDAT").1,
                    reason,
                    byte: None,
                }
            }
            other => other,
        }
    }
    fn pixel(&self, row: &[u8], x: usize, row_offset: usize) -> Result<[u8; 4], TerminalError> {
        let h = self.header;
        let mut channels = [0u16; 4];
        for (i, c) in channels[..h.channels].iter_mut().enumerate() {
            *c = sample(row, x * h.channels + i, h.depth);
        }
        let scale = |v: u16| {
            if h.depth == 16 {
                (v >> 8) as u8
            } else {
                (u32::from(v) * 255 / ((1u32 << h.depth) - 1)) as u8
            }
        };
        let transparent = !self.transparency.is_empty()
            && matches!(h.color, 0 | 2)
            && self
                .transparency
                .chunks_exact(2)
                .enumerate()
                .all(|(i, v)| channels[i] == u16::from_be_bytes([v[0], v[1]]));
        Ok(match h.color {
            0 => [
                scale(channels[0]),
                scale(channels[0]),
                scale(channels[0]),
                if transparent { 0 } else { 255 },
            ],
            2 => [
                scale(channels[0]),
                scale(channels[1]),
                scale(channels[2]),
                if transparent { 0 } else { 255 },
            ],
            3 => {
                let index = channels[0] as usize;
                if index >= self.palette.len() / 3 {
                    let byte_offset = x * usize::from(h.depth) / 8;
                    return Err(TerminalError::UnsupportedDecodedPayload {
                        domain: "PNG unfiltered scanlines",
                        reason: "PNG decoded palette index exceeds PLTE",
                        offset: row_offset + byte_offset,
                        byte: row[byte_offset],
                    });
                }
                [
                    self.palette[index * 3],
                    self.palette[index * 3 + 1],
                    self.palette[index * 3 + 2],
                    self.transparency.get(index).copied().unwrap_or(255),
                ]
            }
            4 => [
                scale(channels[0]),
                scale(channels[0]),
                scale(channels[0]),
                scale(channels[1]),
            ],
            6 => [
                scale(channels[0]),
                scale(channels[1]),
                scale(channels[2]),
                scale(channels[3]),
            ],
            _ => unreachable!("validated PNG color type"),
        })
    }
}

/// Validate the entire container before allocating pixel/deflate buffers. Returns uncommitted pixels.
pub fn decode(input: &[u8]) -> Result<ImageData, TerminalError> {
    let png = inspect(input)?;
    let h = png.header;
    let mut compressed = GraphicsScratch::allocate(png.compressed_bytes, MAX_PNG_BYTES)?;
    let mut offset = 0;
    for &(start, end) in &png.idat {
        compressed.bytes_mut()[offset..offset + end - start].copy_from_slice(&input[start..end]);
        offset += end - start;
    }
    let mut scanlines = GraphicsScratch::allocate(h.scanline_bytes(), MAX_SCANLINE_BYTES)?;
    graphics_zlib::inflate_exact(compressed.bytes(), scanlines.bytes_mut())
        .map_err(|e| png.compressed_error(input, e))?;
    drop(compressed);
    let mut image = ImageData::allocate(h.width as u32, h.height as u32, ImageFormat::Rgba8888)?;
    let bpp = (h.channels * usize::from(h.depth)).div_ceil(8);
    let bytes = scanlines.bytes_mut();
    let mut offset = 0;
    for &(x, y, dx, dy) in h.passes() {
        let width = h.width.saturating_sub(x).div_ceil(dx);
        let height = h.height.saturating_sub(y).div_ceil(dy);
        if width == 0 || height == 0 {
            continue;
        }
        let row_bytes = h.row_bytes(width);
        for row in 0..height {
            let filter = bytes[offset];
            let start = offset + 1;
            if filter > 4 {
                return Err(TerminalError::UnsupportedDecodedPayload {
                    domain: "PNG scanlines",
                    reason: "PNG decoded scanline has invalid filter (expected 0..4)",
                    offset,
                    byte: filter,
                });
            }
            for index in 0..row_bytes {
                let a = if index >= bpp {
                    bytes[start + index - bpp]
                } else {
                    0
                };
                let b = if row > 0 {
                    bytes[offset - row_bytes + index]
                } else {
                    0
                };
                let c = if row > 0 && index >= bpp {
                    bytes[offset - row_bytes + index - bpp]
                } else {
                    0
                };
                let predictor = match filter {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                    4 => paeth(a, b, c),
                    _ => unreachable!(),
                };
                bytes[start + index] = bytes[start + index].wrapping_add(predictor);
            }
            for col in 0..width {
                let pixel = png.pixel(&bytes[start..start + row_bytes], col, start)?;
                let dest = ((y + row * dy) * h.width + x + col * dx) * 4;
                image.pixels_mut()[dest..dest + 4].copy_from_slice(&pixel);
            }
            offset += row_bytes + 1;
        }
    }
    debug_assert_eq!(offset, bytes.len());
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hex(s: &str) -> Vec<u8> {
        assert_eq!(s.len() % 2, 0);
        s.as_bytes()
            .chunks_exact(2)
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect()
    }
    fn vectors() -> impl Iterator<Item = (&'static str, u32, u32, Vec<u8>, Vec<u8>)> {
        include_str!("../tests/fixtures/graphics-png.txt")
            .lines()
            .filter(|s| !s.starts_with('#'))
            .map(|line| {
                let fields: Vec<_> = line.split('|').collect();
                (
                    fields[0],
                    fields[1].parse().unwrap(),
                    fields[2].parse().unwrap(),
                    hex(fields[3]),
                    hex(fields[4]),
                )
            })
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&crc(&out[start..]).to_be_bytes());
    }
    fn stored_zlib(raw: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0x78, 0x01, 1];
        let length = raw.len() as u16;
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(&(!length).to_le_bytes());
        bytes.extend_from_slice(raw);
        let (mut a, mut b) = (1u32, 0u32);
        for &byte in raw {
            a = (a + u32::from(byte)) % 65521;
            b = (b + a) % 65521;
        }
        bytes.extend_from_slice(&((b << 16) | a).to_be_bytes());
        bytes
    }
    fn small_png(depth: u8, color: u8, extras: &[(&[u8; 4], &[u8])], raw: &[u8]) -> Vec<u8> {
        let mut bytes = SIGNATURE.to_vec();
        let mut ihdr = [0u8; 13];
        ihdr[3] = 1;
        ihdr[7] = 1;
        ihdr[8] = depth;
        ihdr[9] = color;
        chunk(&mut bytes, b"IHDR", &ihdr);
        for &(kind, data) in extras {
            chunk(&mut bytes, kind, data);
        }
        chunk(&mut bytes, b"IDAT", &stored_zlib(raw));
        chunk(&mut bytes, b"IEND", &[]);
        bytes
    }
    #[test]
    fn all_png_color_depths_filters_adam7_and_split_idat_match_source_pixels() {
        assert_eq!(crc(b"123456789"), 0xcbf4_3926);
        let mut count = 0;
        for (name, w, h, bytes, rgba) in vectors() {
            let image = decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            assert_eq!(
                (image.width(), image.height(), image.format()),
                (w, h, ImageFormat::Rgba8888),
                "{name}"
            );
            assert_eq!(image.pixels(), rgba, "{name}");
            count += 1;
        }
        assert_eq!(count, 35);
    }
    #[test]
    fn invalid_containers_fail_before_decode_and_never_accept_truncated_or_trailing_images() {
        let bytes = small_png(8, 6, &[], &[0, 10, 20, 30, 40]);
        assert!(decode(&bytes).is_ok());
        for end in 0..bytes.len() {
            assert!(decode(&bytes[..end]).is_err(), "end {end}");
        }
        for offset in 0..bytes.len() {
            let mut bad = bytes.clone();
            bad[offset] ^= 0x80;
            assert!(decode(&bad).is_err(), "offset {offset}");
        }
        let mut trailing = bytes;
        trailing.push(0);
        assert!(decode(&trailing).is_err());
        for kind in [b"acTL", b"ZZZZ"] {
            assert!(decode(&small_png(8, 6, &[(kind, &[])], &[0, 1, 2, 3, 4])).is_err());
        }
        let bad_filter = small_png(8, 6, &[], &[5, 1, 2, 3, 4]);
        let error = decode(&bad_filter).unwrap_err();
        assert!(matches!(
            error,
            TerminalError::UnsupportedDecodedPayload {
                domain: "PNG scanlines",
                offset: 0,
                byte: 5,
                ..
            }
        ));
        assert!(error.to_string().contains("not an original stream offset"));
        let overflow = small_png(8, 6, &[], &[0, 1, 2, 3, 4, 5]);
        assert!(decode(&overflow).is_err());
        assert!(decode(&small_png(8, 3, &[(b"PLTE", &[1, 2, 3])], &[0, 1])).is_err());
    }
    #[test]
    fn required_palette_transparency_order_and_exact_16bit_transparency_are_enforced() {
        assert!(decode(&small_png(8, 3, &[], &[0, 0])).is_err());
        assert!(
            decode(&small_png(
                8,
                3,
                &[(b"tRNS", &[0]), (b"PLTE", &[1, 2, 3])],
                &[0, 0]
            ))
            .is_err()
        );
        assert!(decode(&small_png(8, 6, &[(b"tRNS", &[0, 0])], &[0, 1, 2, 3, 4])).is_err());
        assert!(decode(&small_png(1, 0, &[(b"tRNS", &[0, 2])], &[0, 0])).is_err());
        for low in [0x34, 0x35] {
            let bytes = small_png(16, 0, &[(b"tRNS", &[0x12, 0x34])], &[0, 0x12, low]);
            assert_eq!(
                decode(&bytes).unwrap().pixels(),
                &[0x12, 0x12, 0x12, if low == 0x34 { 0 } else { 255 }]
            );
        }
        // Ancillary compressed text is ignored, never decompressed as image data.
        assert!(decode(&small_png(8, 6, &[(b"zTXt", &[1, 2, 3])], &[0, 1, 2, 3, 4])).is_ok());
    }
    #[test]
    fn idat_error_offsets_map_to_actual_container_bytes_and_large_dimensions_fail_early() {
        let mut bytes = small_png(8, 6, &[], &[0, 1, 2, 3, 4]);
        let png = inspect(&bytes).unwrap();
        let start = png.idat[0].0;
        let end = png.idat[0].1;
        bytes[start] = 0;
        let sum = crc(&bytes[start - 4..end]);
        bytes[end..end + 4].copy_from_slice(&sum.to_be_bytes());
        assert!(
            matches!(decode(&bytes),Err(TerminalError::UnsupportedPayload { offset, byte:Some(0),.. }) if offset==start)
        );
        let mut ihdr = [0u8; 13];
        ihdr[..4].copy_from_slice(&4096u32.to_be_bytes());
        ihdr[4..8].copy_from_slice(&4096u32.to_be_bytes());
        ihdr[8] = 8;
        ihdr[9] = 6;
        let mut large = SIGNATURE.to_vec();
        chunk(&mut large, b"IHDR", &ihdr);
        assert!(matches!(
            decode(&large),
            Err(TerminalError::ProtocolResourceLimit(_))
        ));
    }
}
