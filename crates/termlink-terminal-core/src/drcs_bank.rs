//! Two DECDLD soft sets, each with 80/132-column renditions. Glyphs use explicit atlas references.
use crate::{
    TerminalError, crc32c,
    drcs::DrcsDownload,
    graphics_resources::{GraphicsResources, ImageData, ImageFormat},
};

pub const DRCS_BANK_BYTES: usize = 192;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct GlyphReference {
    pub resource: u64,
    pub x: u32,
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GlyphLookup {
    HardCharset,
    Undefined,
    Glyph(GlyphReference),
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct Face {
    rows: u8,
    cell_width: u8,
    cell_height: u8,
    resource: u64,
    defined: [u64; 2],
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct Font {
    designator: Vec<u8>,
    characters96: bool,
    matrix_height: u8,
    generation: u64,
    faces: [Option<Face>; 2],
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DrcsBank {
    next_generation: u64,
    fonts: [Option<Font>; 2],
}
impl Default for DrcsBank {
    fn default() -> Self {
        Self {
            next_generation: 1,
            fonts: [None, None],
        }
    }
}
fn valid_designator(bytes: &[u8]) -> bool {
    (1..=3).contains(&bytes.len())
        && bytes[..bytes.len() - 1]
            .iter()
            .all(|b| (0x20..=0x2f).contains(b))
        && (0x30..=0x7e).contains(bytes.last().unwrap())
}
impl DrcsBank {
    pub fn referenced_resources(&self) -> impl Iterator<Item = u64> + '_ {
        self.fonts
            .iter()
            .flatten()
            .flat_map(|f| f.faces.iter().flatten())
            .map(|f| f.resource)
    }
    /// Clears names and references without reusing load generations or mutating frozen resources.
    pub fn clear(&mut self) {
        self.fonts = [None, None];
    }
    pub fn lookup(
        &self,
        designator: &[u8],
        characters96: bool,
        columns: u32,
        rows: u32,
        position: u8,
    ) -> GlyphLookup {
        let Some(font) = self
            .fonts
            .iter()
            .flatten()
            .filter(|f| f.designator == designator && f.characters96 == characters96)
            .max_by_key(|f| f.generation)
        else {
            return GlyphLookup::HardCharset;
        };
        if position >= 96 || (!characters96 && !matches!(position, 1..=94)) {
            return GlyphLookup::HardCharset;
        }
        let face = if matches!(columns, 80 | 132) && matches!(rows, 24 | 36 | 48) {
            font.faces[if columns == 80 { 0 } else { 1 }]
                .as_ref()
                .filter(|face| u32::from(face.rows) == rows)
        } else {
            font.faces
                .iter()
                .enumerate()
                .filter_map(|(variant, face)| {
                    face.as_ref().map(|face| {
                        let design_columns = if variant == 0 { 80_u32 } else { 132_u32 };
                        (
                            design_columns.abs_diff(columns),
                            u32::from(face.rows).abs_diff(rows),
                            variant,
                            face,
                        )
                    })
                })
                .min_by_key(|(column_distance, row_distance, variant, _)| {
                    (*column_distance, *row_distance, *variant)
                })
                .map(|(_, _, _, face)| face)
        };
        let Some(face) = face else {
            return GlyphLookup::Undefined;
        };
        if face.defined[position as usize / 64] & (1 << (position % 64)) == 0 {
            return GlyphLookup::Undefined;
        }
        GlyphLookup::Glyph(GlyphReference {
            resource: face.resource,
            x: u32::from(position) * u32::from(face.cell_width),
            width: u32::from(face.cell_width),
            height: u32::from(face.cell_height),
        })
    }
    /// Bank and resource insertion commit together. Superseded resources remain until the terminal GC
    /// has checked cells/history too; this component must never invalidate another owner's reference.
    pub fn install(
        &mut self,
        download: DrcsDownload,
        resources: &mut GraphicsResources,
    ) -> Result<u8, TerminalError> {
        let variant = match download.columns {
            80 => 0,
            132 => 1,
            _ => {
                return Err(TerminalError::Unsupported(
                    "DRCS font width must be 80 or 132 columns",
                ));
            }
        };
        if download.font > 2
            || download.erase > 2
            || !matches!(download.rows, 24 | 36 | 48)
            || !valid_designator(&download.designator)
            || !(1..=16).contains(&download.matrix_height)
            || !valid_cell(
                variant,
                download.rows,
                download.cell_width,
                download.cell_height,
            )
            || download.matrix_height > download.cell_height
            || download.glyphs.len() > 96
        {
            return Err(TerminalError::Unsupported(
                "invalid decoded DRCS font metadata",
            ));
        }
        let next =
            self.next_generation
                .checked_add(1)
                .ok_or(TerminalError::ProtocolResourceLimit(
                    "DRCS load generations exhausted",
                ))?;
        let index = if download.font == 0 {
            self.fonts.iter().position(Option::is_none).unwrap_or(0)
        } else {
            usize::from(download.font - 1)
        };
        let mut candidate = self.clone();
        let mut store = resources.clone();
        let compatible = candidate.fonts[index].as_ref().is_some_and(|f| {
            f.designator == download.designator
                && f.characters96 == download.characters96
                && f.matrix_height == download.matrix_height
        });
        if !compatible || download.erase == 2 {
            candidate.fonts[index] = Some(Font {
                designator: download.designator.clone(),
                characters96: download.characters96,
                matrix_height: download.matrix_height,
                generation: self.next_generation,
                faces: [None, None],
            });
        }
        let font = candidate.fonts[index].as_mut().expect("font initialized");
        let preserve = download.erase == 1
            && font.faces[variant].as_ref().is_some_and(|f| {
                f.rows == download.rows
                    && f.cell_width == download.cell_width
                    && f.cell_height == download.cell_height
            });
        let mut defined = if preserve {
            font.faces[variant].as_ref().unwrap().defined
        } else {
            [0; 2]
        };
        let mut mask = ImageData::allocate(
            u32::from(download.cell_width) * 96,
            u32::from(download.cell_height),
            ImageFormat::Alpha8,
        )?;
        if preserve {
            let source = store
                .get(font.faces[variant].as_ref().unwrap().resource)
                .ok_or(TerminalError::MalformedSnapshot)?
                .image();
            if source.format() != ImageFormat::Alpha8
                || source.width() != mask.width()
                || source.height() != mask.height()
            {
                return Err(TerminalError::MalformedSnapshot);
            }
            mask.pixels_mut().copy_from_slice(source.pixels());
        }
        let mut loaded = [0u64; 2];
        let stride = mask.width() as usize;
        for glyph in download.glyphs {
            let position = usize::from(glyph.position);
            if position >= 96
                || (!download.characters96 && !matches!(position, 1..=94))
                || loaded[position / 64] & (1 << (position % 64)) != 0
                || glyph.mask.format() != ImageFormat::Alpha8
                || glyph.mask.width() != u32::from(download.cell_width)
                || glyph.mask.height() != u32::from(download.cell_height)
            {
                return Err(TerminalError::Unsupported(
                    "invalid decoded DRCS glyph position or dimensions",
                ));
            }
            loaded[position / 64] |= 1 << (position % 64);
            defined[position / 64] |= 1 << (position % 64);
            let width = usize::from(download.cell_width);
            for (row, pixels) in glyph.mask.pixels().chunks_exact(width).enumerate() {
                let start = row * stride + position * width;
                mask.pixels_mut()[start..start + width].copy_from_slice(pixels);
            }
        }
        font.faces[variant] = if defined == [0; 2] {
            None
        } else {
            Some(Face {
                rows: download.rows,
                cell_width: download.cell_width,
                cell_height: download.cell_height,
                resource: store.insert(mask)?,
                defined,
            })
        };
        font.generation = self.next_generation;
        candidate.next_generation = next;
        candidate.validate(&store)?;
        *self = candidate;
        *resources = store;
        Ok(index as u8 + 1)
    }
    fn validate(&self, resources: &GraphicsResources) -> Result<(), TerminalError> {
        self.validate_with_metadata(|id| {
            resources
                .get(id)
                .map(|r| (r.image().width(), r.image().height(), r.image().format()))
        })
    }
    fn validate_with_metadata(
        &self,
        metadata: impl Fn(u64) -> Option<(u32, u32, ImageFormat)>,
    ) -> Result<(), TerminalError> {
        let malformed = TerminalError::MalformedSnapshot;
        if self.next_generation == 0 {
            return Err(malformed);
        }
        let mut generations = [0; 2];
        for (i, font) in self.fonts.iter().enumerate() {
            let Some(font) = font else { continue };
            if !valid_designator(&font.designator)
                || !(1..=16).contains(&font.matrix_height)
                || font.generation == 0
                || font.generation >= self.next_generation
                || generations.contains(&font.generation)
            {
                return Err(malformed);
            }
            generations[i] = font.generation;
            for (variant, face) in font.faces.iter().enumerate() {
                let Some(face) = face else { continue };
                if !valid_cell(variant, face.rows, face.cell_width, face.cell_height)
                    || font.matrix_height > face.cell_height
                    || face.defined == [0; 2]
                    || face.defined[1] >> 32 != 0
                    || (!font.characters96
                        && (face.defined[0] & 1 != 0 || face.defined[1] & (1 << 31) != 0))
                {
                    return Err(malformed);
                }
                let (width, height, format) = metadata(face.resource).ok_or(malformed.clone())?;
                if format != ImageFormat::Alpha8
                    || width != u32::from(face.cell_width) * 96
                    || height != u32::from(face.cell_height)
                {
                    return Err(malformed);
                }
            }
        }
        Ok(())
    }
    pub fn encode(&self) -> [u8; DRCS_BANK_BYTES] {
        let mut bytes = [0u8; DRCS_BANK_BYTES];
        bytes[..12].copy_from_slice(b"TLDRCS1\0\x01\0\x20\0");
        bytes[12..16].copy_from_slice(&(DRCS_BANK_BYTES as u32).to_le_bytes());
        bytes[16..24].copy_from_slice(&self.next_generation.to_le_bytes());
        for (index, font) in self.fonts.iter().enumerate() {
            let Some(font) = font else { continue };
            let base = 32 + index * 80;
            bytes[base] = 1;
            bytes[base + 1] = font.designator.len() as u8;
            bytes[base + 2] = u8::from(font.characters96);
            bytes[base + 3] = font.matrix_height;
            bytes[base + 4..base + 4 + font.designator.len()].copy_from_slice(&font.designator);
            bytes[base + 8..base + 16].copy_from_slice(&font.generation.to_le_bytes());
            for (variant, face) in font.faces.iter().enumerate() {
                let Some(face) = face else { continue };
                let start = base + 16 + variant * 32;
                bytes[start..start + 4].copy_from_slice(&[
                    1,
                    face.rows,
                    face.cell_width,
                    face.cell_height,
                ]);
                for (i, value) in [face.resource, face.defined[0], face.defined[1]]
                    .iter()
                    .enumerate()
                {
                    bytes[start + 8 + i * 8..start + 16 + i * 8]
                        .copy_from_slice(&value.to_le_bytes());
                }
            }
        }
        let crc = crc32c::checksum_parts(&[&bytes[..24], &bytes[28..]]);
        bytes[24..28].copy_from_slice(&crc.to_le_bytes());
        bytes
    }
    pub fn decode(bytes: &[u8], resources: &GraphicsResources) -> Result<Self, TerminalError> {
        Self::decode_with_metadata(bytes, |id| {
            resources
                .get(id)
                .map(|r| (r.image().width(), r.image().height(), r.image().format()))
        })
    }
    pub(crate) fn decode_with_metadata(
        bytes: &[u8],
        metadata: impl Fn(u64) -> Option<(u32, u32, ImageFormat)>,
    ) -> Result<Self, TerminalError> {
        let malformed = TerminalError::MalformedSnapshot;
        if bytes.len() != DRCS_BANK_BYTES || &bytes[..12] != b"TLDRCS1\0\x01\0\x20\0" {
            return Err(malformed);
        }
        let u32_at = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let u64_at = |at| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
        if u32_at(12) as usize != DRCS_BANK_BYTES
            || u32_at(24) != crc32c::checksum_parts(&[&bytes[..24], &bytes[28..]])
            || u32_at(28) != 0
        {
            return Err(malformed);
        }
        let mut bank = Self {
            next_generation: u64_at(16),
            fonts: [None, None],
        };
        for index in 0..2 {
            let base = 32 + index * 80;
            if bytes[base] == 0 {
                if bytes[base..base + 80].iter().any(|&b| b != 0) {
                    return Err(malformed);
                }
                continue;
            }
            let length = bytes[base + 1] as usize;
            if bytes[base] != 1
                || !(1..=3).contains(&length)
                || bytes[base + 2] > 1
                || bytes[base + 4 + length..base + 8].iter().any(|&b| b != 0)
            {
                return Err(malformed);
            }
            let mut font = Font {
                designator: bytes[base + 4..base + 4 + length].to_vec(),
                characters96: bytes[base + 2] != 0,
                matrix_height: bytes[base + 3],
                generation: u64_at(base + 8),
                faces: [None, None],
            };
            for variant in 0..2 {
                let start = base + 16 + variant * 32;
                if bytes[start] == 0 {
                    if bytes[start..start + 32].iter().any(|&b| b != 0) {
                        return Err(malformed);
                    }
                    continue;
                }
                if bytes[start] != 1 || u32_at(start + 4) != 0 {
                    return Err(malformed);
                }
                font.faces[variant] = Some(Face {
                    rows: bytes[start + 1],
                    cell_width: bytes[start + 2],
                    cell_height: bytes[start + 3],
                    resource: u64_at(start + 8),
                    defined: [u64_at(start + 16), u64_at(start + 24)],
                });
            }
            bank.fonts[index] = Some(font);
        }
        bank.validate_with_metadata(metadata)?;
        Ok(bank)
    }
}

fn valid_cell(variant: usize, rows: u8, width: u8, height: u8) -> bool {
    let width_valid = if variant == 0 {
        width == 10
    } else {
        width == 6 || (width == 7 && height == 20)
    };
    width_valid
        && (matches!((rows, height), (24, 16) | (36, 10) | (48, 8))
            || (matches!(rows, 24 | 36 | 48) && height == 20))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drcs;
    fn download(font: u16, erase: u16, pos: u16, set: u16, body: &[u8]) -> DrcsDownload {
        drcs::decode(&[font, pos, erase, 5, set, 2, 8, 0], body).unwrap()
    }
    fn lookup(bank: &DrcsBank, pos: u8) -> GlyphReference {
        match bank.lookup(b" @", false, 80, 24, pos) {
            GlyphLookup::Glyph(g) => g,
            other => panic!("{other:?}"),
        }
    }
    fn pixels(resources: &GraphicsResources, g: GlyphReference) -> Vec<u8> {
        let mask = resources.get(g.resource).unwrap().image();
        let stride = mask.width() as usize;
        (0..g.height as usize)
            .flat_map(|y| {
                mask.pixels()
                    [y * stride + g.x as usize..y * stride + g.x as usize + g.width as usize]
                    .iter()
                    .copied()
            })
            .collect()
    }
    #[test]
    fn partial_replacement_is_immutable_and_erase_means_undefined_not_blank() {
        let mut bank = DrcsBank::default();
        let mut resources = GraphicsResources::default();
        assert_eq!(
            bank.install(download(0, 0, 1, 1, b" @~;?"), &mut resources)
                .unwrap(),
            1
        );
        let old = lookup(&bank, 1);
        let frozen = resources.clone();
        let frozen_bank = bank.clone();
        let old_pixels = pixels(&frozen, old);
        assert!(pixels(&resources, lookup(&bank, 2)).iter().all(|&v| v == 0));
        bank.install(download(1, 1, 1, 1, b" @@"), &mut resources)
            .unwrap();
        assert_ne!(old.resource, lookup(&bank, 1).resource);
        assert_ne!(old_pixels, pixels(&resources, lookup(&bank, 1)));
        assert_eq!(pixels(&frozen, lookup(&frozen_bank, 1)), old_pixels);
        assert_eq!(lookup(&bank, 2).resource, lookup(&bank, 1).resource);
        bank.install(download(1, 0, 1, 1, b" @@"), &mut resources)
            .unwrap();
        assert_eq!(bank.lookup(b" @", false, 80, 24, 2), GlyphLookup::Undefined);
        assert_eq!(
            bank.lookup(b" @", false, 80, 24, 0),
            GlyphLookup::HardCharset
        );
        assert_eq!(bank.lookup(b" @", false, 80, 36, 1), GlyphLookup::Undefined);
    }
    #[test]
    fn automatic_slots_latest_designation_width_renditions_and_erase_all() {
        let mut bank = DrcsBank::default();
        let mut resources = GraphicsResources::default();
        assert_eq!(
            bank.install(download(0, 0, 1, 1, b" @@"), &mut resources)
                .unwrap(),
            1
        );
        let first = lookup(&bank, 1);
        assert_eq!(
            bank.install(download(0, 0, 1, 1, b" @B"), &mut resources)
                .unwrap(),
            2
        );
        assert_ne!(first.resource, lookup(&bank, 1).resource);
        bank.install(download(2, 1, 1, 2, b" @~"), &mut resources)
            .unwrap();
        assert!(matches!(
            bank.lookup(b" @", false, 132, 24, 1),
            GlyphLookup::Glyph(_)
        ));
        assert!(matches!(
            bank.lookup(b" @", false, 80, 24, 1),
            GlyphLookup::Glyph(_)
        ));
        bank.install(download(2, 2, 1, 2, b" @@"), &mut resources)
            .unwrap();
        assert_eq!(bank.lookup(b" @", false, 80, 24, 1), GlyphLookup::Undefined);
        assert_eq!(
            bank.install(download(0, 0, 1, 1, b"&%@~"), &mut resources)
                .unwrap(),
            1
        );
        assert!(matches!(
            bank.lookup(b"&%@", false, 80, 24, 1),
            GlyphLookup::Glyph(_)
        ));
    }
    #[test]
    fn arbitrary_viewports_choose_nearest_loaded_face_without_glyph_fallback() {
        let mut bank = DrcsBank::default();
        let mut resources = GraphicsResources::default();
        bank.install(download(1, 0, 1, 1, b" @~"), &mut resources)
            .unwrap();
        bank.install(download(1, 1, 2, 12, b" @~"), &mut resources)
            .unwrap();
        let GlyphLookup::Glyph(narrow) = bank.lookup(b" @", false, 80, 24, 1) else {
            panic!("80x24 face missing");
        };
        let GlyphLookup::Glyph(wide) = bank.lookup(b" @", false, 132, 36, 2) else {
            panic!("132x36 face missing");
        };
        assert_ne!(narrow.resource, wide.resource);
        for geometry in [(40, 24), (80, 32), (106, 30)] {
            assert_eq!(
                bank.lookup(b" @", false, geometry.0, geometry.1, 1),
                GlyphLookup::Glyph(narrow)
            );
            assert_eq!(
                bank.lookup(b" @", false, geometry.0, geometry.1, 2),
                GlyphLookup::Undefined
            );
        }
        for geometry in [(106, 32), (132, 32), (u32::MAX, u32::MAX)] {
            assert_eq!(
                bank.lookup(b" @", false, geometry.0, geometry.1, 2),
                GlyphLookup::Glyph(wide)
            );
            assert_eq!(
                bank.lookup(b" @", false, geometry.0, geometry.1, 1),
                GlyphLookup::Undefined
            );
        }
        assert_eq!(bank.lookup(b" @", false, 80, 36, 1), GlyphLookup::Undefined);
        assert_eq!(
            bank.lookup(b" @", false, 132, 24, 2),
            GlyphLookup::Undefined
        );
    }
    #[test]
    fn bank_codec_validates_references_padding_and_generations_before_installation() {
        let mut bank = DrcsBank::default();
        let mut resources = GraphicsResources::default();
        bank.install(download(1, 0, 1, 1, b" @@"), &mut resources)
            .unwrap();
        let bytes = bank.encode();
        let restored_resources = GraphicsResources::decode(&resources.encode().unwrap()).unwrap();
        let restored = DrcsBank::decode(&bytes, &restored_resources).unwrap();
        assert_eq!(restored, bank);
        assert_eq!(restored.encode(), bytes);
        for offset in [
            0, 8, 10, 12, 16, 24, 28, 32, 33, 34, 35, 36, 38, 39, 40, 48, 49, 50, 51, 52, 56, 79,
            80, 111, 112,
        ] {
            let mut bad = bytes;
            bad[offset] ^= 0x80;
            if offset >= 28 {
                let sum = crc32c::checksum_parts(&[&bad[..24], &bad[28..]]);
                bad[24..28].copy_from_slice(&sum.to_le_bytes());
            }
            assert!(
                DrcsBank::decode(&bad, &resources).is_err(),
                "offset {offset}"
            );
        }
        for end in 0..bytes.len() {
            assert!(DrcsBank::decode(&bytes[..end], &resources).is_err());
        }
        assert!(DrcsBank::decode(&bytes, &GraphicsResources::default()).is_err());
        bank.clear();
        assert!(bank.referenced_resources().next().is_none());
        assert_ne!(bank.encode(), DrcsBank::default().encode());
    }
    #[test]
    fn capacity_failure_keeps_both_bank_and_resources_unchanged() {
        let mut bank = DrcsBank::default();
        let mut resources = GraphicsResources::default();
        for _ in 0..crate::graphics_resources::MAX_IMAGE_RESOURCES {
            resources
                .insert(ImageData::allocate(1, 1, ImageFormat::Alpha8).unwrap())
                .unwrap();
        }
        let before = resources.encode().unwrap();
        let bank_before = bank.encode();
        assert!(
            bank.install(download(1, 0, 1, 1, b" @@"), &mut resources)
                .is_err()
        );
        assert_eq!(resources.encode().unwrap(), before);
        assert_eq!(bank.encode(), bank_before);
    }
}
