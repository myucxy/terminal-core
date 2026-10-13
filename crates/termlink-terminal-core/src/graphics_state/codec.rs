//! TLGS1: resource store + soft-font bank + SIXEL palette + Kitty directory + anchored placements.
use super::*;
use crate::{
    crc32c,
    drcs_bank::DRCS_BANK_BYTES,
    graphics_layout::{Ratio, SourceRectangle},
    graphics_resources::{ImageFormat, MAX_GRAPHICS_RESOURCE_FRAME_BYTES, ResourceFrame},
};

const HEADER: usize = 64;
const IMAGE: usize = 24;
const PLACEMENT: usize = 168;
pub const MAX_GRAPHICS_STATE_BYTES: usize = HEADER
    + MAX_GRAPHICS_RESOURCE_FRAME_BYTES
    + DRCS_BANK_BYTES
    + 768
    + MAX_IMAGE_RESOURCES * IMAGE
    + MAX_PLACEMENTS * PLACEMENT;
fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("bounded graphics record"),
    )
}
fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("bounded graphics record"),
    )
}
fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn put64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

struct StateFrame<'a> {
    resources: ResourceFrame<'a>,
    state: GraphicsState,
}

impl GraphicsState {
    pub fn encode(&self) -> Result<Vec<u8>, TerminalError> {
        let resources = self.resources.encode()?;
        let size = HEADER
            + resources.len()
            + DRCS_BANK_BYTES
            + 768
            + self.images.len() * IMAGE
            + self.placements.len() * PLACEMENT;
        if size > MAX_GRAPHICS_STATE_BYTES {
            return Err(TerminalError::ProtocolResourceLimit(
                "graphics state object exceeds bounded envelope",
            ));
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|_| TerminalError::LimitExceeded)?;
        bytes.resize(size, 0);
        bytes[..12].copy_from_slice(b"TLGS1\0\0\0\x01\0\x40\0");
        put32(&mut bytes, 12, size as u32);
        put64(&mut bytes, 16, self.next_placement);
        put32(&mut bytes, 24, self.next_kitty_id);
        put32(&mut bytes, 28, self.images.len() as u32);
        put32(&mut bytes, 32, self.placements.len() as u32);
        put32(&mut bytes, 36, resources.len() as u32);
        put32(&mut bytes, 44, u32::from(self.sixel_scrolling));
        let mut offset = HEADER;
        bytes[offset..offset + resources.len()].copy_from_slice(&resources);
        offset += resources.len();
        bytes[offset..offset + DRCS_BANK_BYTES].copy_from_slice(&self.fonts.encode());
        offset += DRCS_BANK_BYTES;
        for rgb in &self.palette {
            bytes[offset..offset + 3].copy_from_slice(rgb);
            offset += 3;
        }
        for image in &self.images {
            put32(&mut bytes, offset, image.id);
            put32(&mut bytes, offset + 4, image.number);
            put64(&mut bytes, offset + 8, image.resource);
            offset += IMAGE;
        }
        for p in &self.placements {
            put64(&mut bytes, offset, p.id);
            put64(&mut bytes, offset + 8, p.resource);
            put64(&mut bytes, offset + 16, p.anchor.logical_line);
            put32(&mut bytes, offset + 24, p.anchor.cell_offset);
            put32(&mut bytes, offset + 28, p.image_id);
            put32(&mut bytes, offset + 32, p.placement_id);
            put32(&mut bytes, offset + 36, p.z as u32);
            bytes[offset + 40] = p.anchor.page;
            bytes[offset + 41] = match p.protocol {
                GraphicProtocol::Sixel => 1,
                GraphicProtocol::Kitty => 2,
            };
            for (i, value) in [
                p.layout.source.x,
                p.layout.source.y,
                p.layout.source.width,
                p.layout.source.height,
            ]
            .into_iter()
            .enumerate()
            {
                put32(&mut bytes, offset + 48 + i * 4, value);
            }
            for (i, value) in [p.layout.x, p.layout.y, p.layout.width, p.layout.height]
                .into_iter()
                .enumerate()
            {
                put64(&mut bytes, offset + 64 + i * 16, value.numerator);
                put64(&mut bytes, offset + 72 + i * 16, value.denominator);
            }
            put32(&mut bytes, offset + 128, p.layout.columns);
            put32(&mut bytes, offset + 132, p.layout.rows);
            put32(&mut bytes, offset + 144, p.origin_x as u32);
            put32(&mut bytes, offset + 148, p.origin_y as u32);
            put32(&mut bytes, offset + 152, p.clip.left);
            put32(&mut bytes, offset + 156, p.clip.top);
            put32(&mut bytes, offset + 160, p.clip.right);
            put32(&mut bytes, offset + 164, p.clip.bottom);
            offset += PLACEMENT;
        }
        debug_assert_eq!(offset, size);
        let crc = crc32c::checksum_parts(&[&bytes[..40], &bytes[44..]]);
        put32(&mut bytes, 40, crc);
        Ok(bytes)
    }
    /// Decodes into detached state; no existing terminal resource or ACK is changed on failure.
    pub fn decode(bytes: &[u8]) -> Result<Self, TerminalError> {
        Self::decode_with_admission(bytes, |_| Ok(()))
    }
    /// The owning terminal validates logical anchors after metadata validation, before pixels.
    pub(crate) fn decode_with_admission(
        bytes: &[u8],
        check: impl FnOnce(&[Placement]) -> Result<(), TerminalError>,
    ) -> Result<Self, TerminalError> {
        let StateFrame {
            resources,
            mut state,
        } = Self::inspect_state(bytes)?;
        check(&state.placements)?;
        state.resources = resources.materialize()?;
        Ok(state)
    }
    /// Checks the complete state and all references without allocating image pixels.
    pub(crate) fn inspect_resources(bytes: &[u8]) -> Result<ResourceFrame<'_>, TerminalError> {
        Ok(Self::inspect_state(bytes)?.resources)
    }
    fn inspect_state(bytes: &[u8]) -> Result<StateFrame<'_>, TerminalError> {
        let malformed = TerminalError::MalformedSnapshot;
        if bytes.len() < HEADER
            || bytes.len() > MAX_GRAPHICS_STATE_BYTES
            || &bytes[..12] != b"TLGS1\0\0\0\x01\0\x40\0"
        {
            return Err(malformed);
        }
        let images = u32_at(bytes, 28) as usize;
        let placements = u32_at(bytes, 32) as usize;
        let resource_len = u32_at(bytes, 36) as usize;
        if images > MAX_IMAGE_RESOURCES
            || placements > MAX_PLACEMENTS
            || !(48..=MAX_GRAPHICS_RESOURCE_FRAME_BYTES).contains(&resource_len)
            || bytes.len()
                != HEADER
                    + resource_len
                    + DRCS_BANK_BYTES
                    + 768
                    + images * IMAGE
                    + placements * PLACEMENT
            || u32_at(bytes, 12) as usize != bytes.len()
            || u64_at(bytes, 16) == 0
            || u32_at(bytes, 24) == 0
            || u32_at(bytes, 44) & !1 != 0
            || bytes[48..64].iter().any(|&b| b != 0)
            || u32_at(bytes, 40) != crc32c::checksum_parts(&[&bytes[..40], &bytes[44..]])
        {
            return Err(malformed);
        }
        let mut offset = HEADER + resource_len + DRCS_BANK_BYTES;
        let mut palette = [[0u8; 3]; 256];
        for rgb in &mut palette {
            rgb.copy_from_slice(&bytes[offset..offset + 3]);
            offset += 3;
        }
        let mut directory = Vec::new();
        directory
            .try_reserve_exact(images)
            .map_err(|_| TerminalError::LimitExceeded)?;
        let mut previous_resource = 0;
        for _ in 0..images {
            let entry = KittyImage {
                id: u32_at(bytes, offset),
                number: u32_at(bytes, offset + 4),
                resource: u64_at(bytes, offset + 8),
            };
            if entry.resource <= previous_resource
                || u64_at(bytes, offset + 16) != 0
                || (entry.id == 0 && entry.number != 0)
                || (entry.id != 0 && directory.iter().any(|i: &KittyImage| i.id == entry.id))
            {
                return Err(malformed);
            }
            previous_resource = entry.resource;
            directory.push(entry);
            offset += IMAGE;
        }
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(placements)
            .map_err(|_| TerminalError::LimitExceeded)?;
        let mut previous_id = 0;
        for _ in 0..placements {
            let protocol = match bytes[offset + 41] {
                1 => GraphicProtocol::Sixel,
                2 => GraphicProtocol::Kitty,
                _ => return Err(malformed),
            };
            let mut ratios = [Ratio::new(0, 1); 4];
            for (i, ratio) in ratios.iter_mut().enumerate() {
                *ratio = Ratio {
                    numerator: u64_at(bytes, offset + 64 + i * 16),
                    denominator: u64_at(bytes, offset + 72 + i * 16),
                };
            }
            let entry = Placement {
                id: u64_at(bytes, offset),
                resource: u64_at(bytes, offset + 8),
                anchor: Anchor {
                    logical_line: u64_at(bytes, offset + 16),
                    cell_offset: u32_at(bytes, offset + 24),
                    page: bytes[offset + 40],
                },
                image_id: u32_at(bytes, offset + 28),
                placement_id: u32_at(bytes, offset + 32),
                z: u32_at(bytes, offset + 36) as i32,
                origin_x: u32_at(bytes, offset + 144) as i32,
                origin_y: u32_at(bytes, offset + 148) as i32,
                clip: GraphicClip {
                    left: u32_at(bytes, offset + 152),
                    top: u32_at(bytes, offset + 156),
                    right: u32_at(bytes, offset + 160),
                    bottom: u32_at(bytes, offset + 164),
                },
                protocol,
                layout: ImageLayout {
                    source: SourceRectangle {
                        x: u32_at(bytes, offset + 48),
                        y: u32_at(bytes, offset + 52),
                        width: u32_at(bytes, offset + 56),
                        height: u32_at(bytes, offset + 60),
                    },
                    x: ratios[0],
                    y: ratios[1],
                    width: ratios[2],
                    height: ratios[3],
                    columns: u32_at(bytes, offset + 128),
                    rows: u32_at(bytes, offset + 132),
                },
            };
            if entry.id <= previous_id
                || entry.id >= u64_at(bytes, 16)
                || entry.resource == 0
                || entry.validate().is_err()
                || bytes[offset + 42..offset + 48].iter().any(|&b| b != 0)
                || u64_at(bytes, offset + 136) != 0
                || (entry.protocol == GraphicProtocol::Sixel
                    && (entry.image_id != 0 || entry.placement_id != 0))
                || (entry.image_id == 0 && entry.placement_id != 0)
            {
                return Err(malformed);
            }
            if entry.image_id != 0
                && entry.placement_id != 0
                && entries.iter().any(|p: &Placement| {
                    p.protocol == entry.protocol
                        && p.image_id == entry.image_id
                        && p.placement_id == entry.placement_id
                        && (p.resource != entry.resource
                            || p.layout != entry.layout
                            || p.z != entry.z)
                })
            {
                return Err(malformed);
            }
            previous_id = entry.id;
            entries.push(entry);
            offset += PLACEMENT;
        }
        // All structure, font and cross-resource references precede large pixel allocation.
        let resource_frame = GraphicsResources::inspect(&bytes[HEADER..HEADER + resource_len])?;
        let fonts = DrcsBank::decode_with_metadata(
            &bytes[HEADER + resource_len..HEADER + resource_len + DRCS_BANK_BYTES],
            |id| resource_frame.metadata(id),
        )?;
        for image in &directory {
            if resource_frame
                .metadata(image.resource)
                .is_none_or(|(_, _, format)| format != ImageFormat::Rgba8888)
            {
                return Err(malformed);
            }
        }
        for entry in &entries {
            let (width, height, format) = resource_frame
                .metadata(entry.resource)
                .ok_or(malformed.clone())?;
            let source = entry.layout.source;
            if format != ImageFormat::Rgba8888
                || source.width == 0
                || source.height == 0
                || source.x >= width
                || source.y >= height
                || source.width > width - source.x
                || source.height > height - source.y
            {
                return Err(malformed);
            }
            if entry.protocol == GraphicProtocol::Kitty
                && !directory
                    .iter()
                    .any(|i| i.id == entry.image_id && i.resource == entry.resource)
            {
                return Err(malformed);
            }
        }
        Ok(StateFrame {
            resources: resource_frame,
            state: Self {
                resources: GraphicsResources::default(),
                fonts,
                palette,
                sixel_scrolling: u32_at(bytes, 44) != 0,
                images: directory,
                placements: entries,
                next_placement: u64_at(bytes, 16),
                next_kitty_id: u32_at(bytes, 24),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kitty_graphics::TransferDecoder;
    fn state() -> GraphicsState {
        let mut state = GraphicsState::default();
        let command = TransferDecoder::default()
            .push(b"Ga=T,s=2,v=1,i=7,p=4,c=4,r=3,z=-1;AQIDBAUGBwg=")
            .unwrap()
            .unwrap();
        state
            .apply_kitty(
                command,
                Anchor {
                    page: 3,
                    logical_line: 77,
                    cell_offset: 19,
                },
                HostGeometryV1 {
                    cell_width: 10,
                    cell_height: 20,
                    ..Default::default()
                },
                &[],
            )
            .unwrap();
        state
            .install_font(
                crate::drcs::decode(&[1, 1, 0, 5, 1, 2, 8, 0], b" @@").unwrap(),
                &[],
            )
            .unwrap();
        state
    }
    fn seal(bytes: &mut [u8]) {
        let crc = crc32c::checksum_parts(&[&bytes[..40], &bytes[44..]]);
        put32(bytes, 40, crc);
    }
    #[test]
    fn canonical_state_roundtrip_retains_pixels_fonts_positions_palette_and_next_ids() {
        let state = state();
        let bytes = state.encode().unwrap();
        let restored = GraphicsState::decode(&bytes).unwrap();
        assert_eq!(restored, state);
        assert_eq!(restored.encode().unwrap(), bytes);
        assert_eq!(
            GraphicsState::decode(&GraphicsState::default().encode().unwrap()).unwrap(),
            GraphicsState::default()
        );
    }
    #[test]
    fn extended_placement_bytes_preserve_signed_origins_clips_mode_and_fragments() {
        let mut state = state();
        state.set_sixel_scrolling(false);
        state
            .remap_placements(
                3,
                |p| {
                    let mut first = p.clone();
                    first.origin_x = -2;
                    first.origin_y = -1;
                    first.clip = GraphicClip {
                        left: 1,
                        top: 1,
                        right: 2,
                        bottom: 3,
                    };
                    let mut second = first.clone();
                    second.clip.left = 2;
                    second.clip.right = 4;
                    second.anchor.logical_line += 1;
                    Ok(vec![first, second])
                },
                &[],
            )
            .unwrap();
        let bytes = state.encode().unwrap();
        let offset = HEADER + u32_at(&bytes, 36) as usize + DRCS_BANK_BYTES + 768 + IMAGE;
        assert_eq!(u32_at(&bytes, 44), 0);
        assert_eq!(bytes.len(), offset + 2 * 168);
        assert_eq!(&bytes[offset + 136..offset + 144], &[0; 8]);
        for (relative, expected) in [
            (144, -2i32 as u32),
            (148, -1i32 as u32),
            (152, 1),
            (156, 1),
            (160, 2),
            (164, 3),
        ] {
            assert_eq!(u32_at(&bytes, offset + relative), expected);
        }
        let before = crate::graphics_resources::image_allocation_attempts();
        let resources = GraphicsState::inspect_resources(&bytes).unwrap();
        assert!(resources.metadata(state.placements()[0].resource).is_some());
        assert_eq!(
            crate::graphics_resources::image_allocation_attempts(),
            before
        );
        assert_eq!(GraphicsState::decode(&bytes).unwrap(), state);
        for (relative, value) in [
            (144, 4097),
            (148, -4097i32 as u32),
            (152, 2),
            (156, 3),
            (160, 5),
            (164, 4),
        ] {
            let mut bad = bytes.clone();
            put32(&mut bad, offset + relative, value);
            seal(&mut bad);
            assert!(
                GraphicsState::inspect_resources(&bad).is_err(),
                "offset {relative}"
            );
        }
        let mut bad = bytes;
        put32(&mut bad, offset + PLACEMENT + 36, 0);
        seal(&mut bad);
        assert!(
            GraphicsState::inspect_resources(&bad).is_err(),
            "fragment z mismatch"
        );
    }
    #[test]
    fn structure_corruption_resealed_frames_and_truncation_are_rejected() {
        let bytes = state().encode().unwrap();
        let placement = HEADER + u32_at(&bytes, 36) as usize + DRCS_BANK_BYTES + 768 + IMAGE;
        for at in [
            0,
            8,
            10,
            12,
            16,
            24,
            28,
            32,
            36,
            40,
            44,
            63,
            placement,
            placement + 8,
            placement + 40,
            placement + 41,
            placement + 42,
            placement + 56,
            placement + 72,
            placement + 131,
            placement + 136,
        ] {
            let mut bad = bytes.clone();
            bad[at] ^= 0x80;
            if at >= 44 {
                seal(&mut bad);
            }
            assert!(GraphicsState::decode(&bad).is_err(), "offset {at}");
        }
        for end in [0, 63, 64, bytes.len() - 1] {
            assert!(GraphicsState::decode(&bytes[..end]).is_err());
        }
        let mut bad = bytes.clone();
        put64(&mut bad, placement + 72, 0);
        seal(&mut bad);
        assert!(GraphicsState::decode(&bad).is_err());
        let mut bad = bytes;
        put64(&mut bad, placement + 72, 2);
        seal(&mut bad);
        assert!(GraphicsState::decode(&bad).is_err());
    }
    #[test]
    fn invalid_cross_references_are_rejected_before_pixel_allocation() {
        let bytes = state().encode().unwrap();
        let font = HEADER + u32_at(&bytes, 36) as usize;
        let image = font + DRCS_BANK_BYTES + 768;
        let placement = image + IMAGE;
        let before = crate::graphics_resources::image_allocation_attempts();
        for offset in [image + 8, placement + 8, placement + 56] {
            let mut bad = bytes.clone();
            put32(&mut bad, offset, u32::MAX);
            seal(&mut bad);
            assert!(GraphicsState::decode(&bad).is_err());
            assert!(GraphicsState::inspect_resources(&bad).is_err());
            assert_eq!(
                crate::graphics_resources::image_allocation_attempts(),
                before
            );
        }
        let mut bad = bytes.clone();
        put64(&mut bad, font + 32 + 16 + 8, u64::MAX);
        let crc = crc32c::checksum_parts(&[
            &bad[font..font + 24],
            &bad[font + 28..font + DRCS_BANK_BYTES],
        ]);
        put32(&mut bad, font + 24, crc);
        seal(&mut bad);
        assert!(GraphicsState::decode(&bad).is_err());
        assert!(GraphicsState::inspect_resources(&bad).is_err());
        assert_eq!(
            crate::graphics_resources::image_allocation_attempts(),
            before
        );
        assert!(GraphicsState::decode(&bytes).is_ok());
        assert_eq!(
            crate::graphics_resources::image_allocation_attempts(),
            before + 2
        );
    }
    #[test]
    fn placement_admission_rejects_missing_screen_anchors_before_pixel_allocation() {
        let original = state();
        let bytes = original.encode().unwrap();
        let placement = HEADER + u32_at(&bytes, 36) as usize + DRCS_BANK_BYTES + 768 + IMAGE;
        let before = crate::graphics_resources::image_allocation_attempts();
        let mut bad = bytes.clone();
        put64(&mut bad, placement + 16, 999);
        seal(&mut bad);
        let mut checked = false;
        assert_eq!(
            GraphicsState::decode_with_admission(&bad, |placements| {
                checked = true;
                if placements.iter().any(|p| p.anchor.logical_line != 77) {
                    Err(TerminalError::MalformedSnapshot)
                } else {
                    Ok(())
                }
            }),
            Err(TerminalError::MalformedSnapshot)
        );
        assert!(checked);
        assert_eq!(
            crate::graphics_resources::image_allocation_attempts(),
            before
        );
        bad[44] |= 2;
        seal(&mut bad);
        assert!(
            GraphicsState::decode_with_admission(&bad, |_| panic!(
                "unvalidated metadata must not reach admission"
            ))
            .is_err()
        );
        assert_eq!(
            crate::graphics_resources::image_allocation_attempts(),
            before
        );
        let restored = GraphicsState::decode_with_admission(&bytes, |placements| {
            assert_eq!(placements, original.placements());
            assert_eq!(
                crate::graphics_resources::image_allocation_attempts(),
                before
            );
            Ok(())
        })
        .unwrap();
        assert_eq!(restored, original);
        assert_eq!(
            crate::graphics_resources::image_allocation_attempts(),
            before + 2
        );
    }
}
