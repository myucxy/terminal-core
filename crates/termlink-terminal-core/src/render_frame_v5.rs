//! Renderer-only V5 image and explicit DRCS layers over canonical V4 text.
use crate::{
    Terminal, TerminalError, crc32c,
    graphics_layout::{ImageLayout, MAX_PLACEMENT_CELLS, Ratio, SourceRectangle},
    graphics_resources::{GraphicsResources, ImageFormat},
    graphics_state::{GraphicClip, GraphicProtocol, MAX_PLACEMENTS, Placement},
    history_frame, render_frame,
};

pub const MAX_RENDER_FRAME_V5_BYTES: usize = 64 * 1024 * 1024;
const HEADER: usize = 64;
const DRAW: usize = 144;
const GLYPH_DRAW: usize = 32;
const REQUIRED_FEATURES: u32 = 7;
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageDraw {
    pub placement: u64,
    pub resource: u64,
    pub column: i32,
    pub row: i32,
    pub z: i32,
    pub image_id: u32,
    pub layout: ImageLayout,
    pub protocol: GraphicProtocol,
    pub clip: GraphicClip,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GlyphDraw {
    pub column: u32,
    pub row: u32,
    pub glyph: crate::drcs_bank::GlyphReference,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    pub revision: u64,
    pub columns: u32,
    pub rows: u32,
    pub text_v4: Vec<u8>,
    pub resources: GraphicsResources,
    pub images: Vec<ImageDraw>,
    pub glyphs: Vec<GlyphDraw>,
}
struct Inspected<'a> {
    text: &'a [u8],
    directory: crate::graphics_resources::ResourceFrame<'a>,
    images: Vec<ImageDraw>,
    glyphs: Vec<GlyphDraw>,
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("checked V5 field"),
    )
}
fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("checked V5 field"),
    )
}
fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn put64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

impl Terminal {
    pub fn export_render_frame_v5(&self, offset_rows: u32) -> Result<Vec<u8>, TerminalError> {
        self.ensure_presentation_eligible()?;
        let source = self.visible_screen();
        let offset = offset_rows.min(source.viewport_top());
        let top = source.viewport_top() - offset;
        let window;
        let screen = if offset == 0 {
            source
        } else {
            window = source.render_window(offset);
            &window
        };
        let text = render_frame::export_v4_text_layer(
            self,
            screen,
            offset == 0 && self.cursor_visible && self.active_page() == self.visible_page(),
        )?;
        let page = if self.active_alternate {
            7
        } else {
            self.visible_page()
        };
        export_projection(
            source,
            screen,
            top,
            page,
            self.graphics.as_ref().map(|g| g.resources()),
            self.graphics.as_ref().map_or(&[], |g| g.placements()),
            text,
        )
    }
}

pub(crate) fn export_projection(
    source: &crate::screen::Screen,
    screen: &crate::screen::Screen,
    top: u32,
    page: u8,
    resources: Option<&GraphicsResources>,
    placements: &[Placement],
    text_v4: Vec<u8>,
) -> Result<Vec<u8>, TerminalError> {
    let mut images = Vec::new();
    let mut resources = resources.cloned().unwrap_or_default();
    if placements.iter().any(|p| p.anchor.page == page) {
        let index = crate::terminal_graphics::GraphicsAnchorIndex::new(source)?;
        for placement in placements.iter().filter(|p| p.anchor.page == page) {
            let a = placement.anchor;
            let Some((stored_row, column)) = index.locate(a.logical_line, a.cell_offset) else {
                return Err(TerminalError::MalformedSnapshot);
            };
            let column = column as i32 + placement.origin_x;
            let row = stored_row as i32 - top as i32 + placement.origin_y;
            let clip = placement.clip;
            if !intersects(column, row, clip, screen.columns, screen.rows) {
                continue;
            }
            images
                .try_reserve(1)
                .map_err(|_| TerminalError::LimitExceeded)?;
            images.push(ImageDraw {
                placement: placement.id,
                resource: placement.resource,
                column,
                row,
                z: placement.z,
                image_id: placement.image_id,
                layout: placement.layout,
                protocol: placement.protocol,
                clip,
            });
        }
    }
    images.sort_by_key(|image| (image.z, image.image_id, image.placement));
    let mut glyphs = Vec::new();
    for (index, cell) in screen.visible_cells().iter().enumerate() {
        if let Some(glyph) = cell.glyph {
            glyphs
                .try_reserve(1)
                .map_err(|_| TerminalError::LimitExceeded)?;
            glyphs.push(GlyphDraw {
                column: index as u32 % screen.columns,
                row: index as u32 / screen.columns,
                glyph,
            });
        }
    }
    resources.retain_referenced(
        images
            .iter()
            .map(|i| i.resource)
            .chain(glyphs.iter().map(|g| g.glyph.resource)),
    )?;
    Frame {
        revision: u64_at(&text_v4, 24),
        columns: screen.columns,
        rows: screen.rows,
        text_v4,
        resources,
        images,
        glyphs,
    }
    .encode()
}

fn intersects(column: i32, row: i32, clip: GraphicClip, columns: u32, rows: u32) -> bool {
    i64::from(column) + i64::from(clip.right) > 0
        && i64::from(row) + i64::from(clip.bottom) > 0
        && i64::from(column) + i64::from(clip.left) < i64::from(columns)
        && i64::from(row) + i64::from(clip.top) < i64::from(rows)
}

impl Frame {
    fn validate(&self) -> Result<(), TerminalError> {
        let bad = TerminalError::MalformedSnapshot;
        crate::validate_dimensions(self.columns, self.rows)?;
        history_frame::validate_full_render_v4(&self.text_v4)?;
        if u64_at(&self.text_v4, 24) != self.revision
            || u64_at(&self.text_v4, 32) != self.revision
            || u32_at(&self.text_v4, 40) != self.columns
            || u32_at(&self.text_v4, 44) != self.rows
            || self.images.len() > MAX_PLACEMENTS
            || self.glyphs.len() > self.columns as usize * self.rows as usize
        {
            return Err(bad);
        }
        Self::validate_draws(
            &self.images,
            &self.glyphs,
            &self.text_v4,
            self.columns,
            self.rows,
            |id| {
                self.resources
                    .get(id)
                    .map(|r| (r.image().width(), r.image().height(), r.image().format()))
            },
            self.resources.entries().iter().map(|r| r.id()),
        )
    }

    fn validate_draws(
        images: &[ImageDraw],
        glyphs: &[GlyphDraw],
        text: &[u8],
        columns: u32,
        rows: u32,
        metadata: impl Fn(u64) -> Option<(u32, u32, ImageFormat)>,
        resource_ids: impl Iterator<Item = u64>,
    ) -> Result<(), TerminalError> {
        let bad = TerminalError::MalformedSnapshot;
        let mut previous = None;
        let mut ids = Vec::new();
        ids.try_reserve_exact(images.len())
            .map_err(|_| TerminalError::LimitExceeded)?;
        for image in images {
            let order = (image.z, image.image_id, image.placement);
            if image.placement == 0
                || previous.is_some_and(|p| p >= order)
                || image.column.unsigned_abs() > MAX_PLACEMENT_CELLS
                || image.row.unsigned_abs() > MAX_PLACEMENT_CELLS
                || image.clip.left >= image.clip.right
                || image.clip.top >= image.clip.bottom
                || image.clip.right > image.layout.columns
                || image.clip.bottom > image.layout.rows
                || !intersects(image.column, image.row, image.clip, columns, rows)
                || !(1..=MAX_PLACEMENT_CELLS).contains(&image.layout.columns)
                || !(1..=MAX_PLACEMENT_CELLS).contains(&image.layout.rows)
                || (image.protocol == GraphicProtocol::Sixel && image.image_id != 0)
            {
                return Err(bad);
            }
            ids.push(image.placement);
            let (width, height, format) = metadata(image.resource).ok_or(bad.clone())?;
            let s = image.layout.source;
            if format != ImageFormat::Rgba8888
                || s.width == 0
                || s.height == 0
                || s.x >= width
                || s.y >= height
                || s.width > width - s.x
                || s.height > height - s.y
            {
                return Err(bad);
            }
            for (i, r) in [
                image.layout.x,
                image.layout.y,
                image.layout.width,
                image.layout.height,
            ]
            .into_iter()
            .enumerate()
            {
                if r.numerator > 1 << 48
                    || r.denominator == 0
                    || r.denominator > 1 << 32
                    || (i >= 2 && r.numerator == 0)
                    || Ratio::new(r.numerator, r.denominator) != r
                {
                    return Err(bad);
                }
            }
            for (start, length, limit) in [
                (image.layout.x, image.layout.width, image.layout.columns),
                (image.layout.y, image.layout.height, image.layout.rows),
            ] {
                let extent = u128::from(start.numerator) * u128::from(length.denominator)
                    + u128::from(length.numerator) * u128::from(start.denominator);
                if extent
                    > u128::from(limit)
                        * u128::from(start.denominator)
                        * u128::from(length.denominator)
                {
                    return Err(bad);
                }
            }
            previous = Some(order);
        }
        let mut previous_glyph = None;
        for draw in glyphs {
            let order = (draw.row, draw.column);
            if draw.column >= columns
                || draw.row >= rows
                || previous_glyph.is_some_and(|prior| prior >= order)
                || !history_frame::is_single_cell_glyph(text, draw.column, draw.row)
            {
                return Err(bad);
            }
            let g = draw.glyph;
            let (width, height, format) = metadata(g.resource).ok_or(bad.clone())?;
            if format != ImageFormat::Alpha8
                || g.width == 0
                || g.height == 0
                || g.x >= width
                || g.width > width - g.x
                || g.height > height
            {
                return Err(bad);
            }
            previous_glyph = Some(order);
        }
        ids.sort_unstable();
        if ids.windows(2).any(|pair| pair[0] == pair[1])
            || resource_ids.into_iter().any(|id| {
                !images.iter().any(|i| i.resource == id)
                    && !glyphs.iter().any(|g| g.glyph.resource == id)
            })
        {
            return Err(bad);
        }
        Ok(())
    }
    pub fn encode(&self) -> Result<Vec<u8>, TerminalError> {
        self.validate()?;
        let resources = self.resources.encode()?;
        let size = HEADER
            + self.text_v4.len()
            + resources.len()
            + self.images.len() * DRAW
            + self.glyphs.len() * GLYPH_DRAW;
        if size > MAX_RENDER_FRAME_V5_BYTES {
            return Err(TerminalError::ProtocolResourceLimit(
                "Render V5 exceeds 64 MiB",
            ));
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|_| TerminalError::LimitExceeded)?;
        bytes.resize(size, 0);
        bytes[..12].copy_from_slice(b"TLRFRM5\0\x05\0\x40\0");
        put32(&mut bytes, 12, size as u32);
        put64(&mut bytes, 16, self.revision);
        for (at, value) in [
            (24, self.columns),
            (28, self.rows),
            (32, self.text_v4.len() as u32),
            (36, resources.len() as u32),
            (40, self.images.len() as u32),
            (44, self.glyphs.len() as u32),
            (48, REQUIRED_FEATURES),
        ] {
            put32(&mut bytes, at, value);
        }
        let mut offset = HEADER;
        bytes[offset..offset + self.text_v4.len()].copy_from_slice(&self.text_v4);
        offset += self.text_v4.len();
        bytes[offset..offset + resources.len()].copy_from_slice(&resources);
        offset += resources.len();
        for image in &self.images {
            put64(&mut bytes, offset, image.placement);
            put64(&mut bytes, offset + 8, image.resource);
            for (i, v) in [
                image.column as u32,
                image.row as u32,
                image.z as u32,
                image.image_id,
                image.layout.source.x,
                image.layout.source.y,
                image.layout.source.width,
                image.layout.source.height,
            ]
            .into_iter()
            .enumerate()
            {
                put32(&mut bytes, offset + 16 + i * 4, v);
            }
            for (i, r) in [
                image.layout.x,
                image.layout.y,
                image.layout.width,
                image.layout.height,
            ]
            .into_iter()
            .enumerate()
            {
                put64(&mut bytes, offset + 48 + i * 16, r.numerator);
                put64(&mut bytes, offset + 56 + i * 16, r.denominator);
            }
            put32(&mut bytes, offset + 112, image.layout.columns);
            put32(&mut bytes, offset + 116, image.layout.rows);
            put32(
                &mut bytes,
                offset + 120,
                match image.protocol {
                    GraphicProtocol::Sixel => 1,
                    GraphicProtocol::Kitty => 2,
                },
            );
            for (i, value) in [
                image.clip.left,
                image.clip.top,
                image.clip.right,
                image.clip.bottom,
            ]
            .into_iter()
            .enumerate()
            {
                put32(&mut bytes, offset + 128 + i * 4, value);
            }
            offset += DRAW;
        }
        for draw in &self.glyphs {
            put32(&mut bytes, offset, draw.column);
            put32(&mut bytes, offset + 4, draw.row);
            put64(&mut bytes, offset + 8, draw.glyph.resource);
            put32(&mut bytes, offset + 16, draw.glyph.x);
            put32(&mut bytes, offset + 20, draw.glyph.width);
            put32(&mut bytes, offset + 24, draw.glyph.height);
            offset += GLYPH_DRAW;
        }
        let sum = crc32c::checksum_parts(&[&bytes[..52], &bytes[56..]]);
        put32(&mut bytes, 52, sum);
        Ok(bytes)
    }
    fn inspect(bytes: &[u8]) -> Result<Inspected<'_>, TerminalError> {
        let bad = TerminalError::MalformedSnapshot;
        if bytes.len() < HEADER
            || bytes.len() > MAX_RENDER_FRAME_V5_BYTES
            || &bytes[..12] != b"TLRFRM5\0\x05\0\x40\0"
        {
            return Err(bad);
        }
        let text_len = u32_at(bytes, 32) as usize;
        let resource_len = u32_at(bytes, 36) as usize;
        let count = u32_at(bytes, 40) as usize;
        let glyph_count = u32_at(bytes, 44) as usize;
        if !(240..=crate::MAX_RENDER_FRAME_V4_BYTES).contains(&text_len)
            || !(48..=crate::graphics_resources::MAX_GRAPHICS_RESOURCE_FRAME_BYTES)
                .contains(&resource_len)
            || count > MAX_PLACEMENTS
            || glyph_count > crate::MAX_CELLS
            || bytes.len()
                != HEADER + text_len + resource_len + count * DRAW + glyph_count * GLYPH_DRAW
            || u32_at(bytes, 12) as usize != bytes.len()
            || u32_at(bytes, 48) != REQUIRED_FEATURES
            || u64_at(bytes, 56) != 0
            || u32_at(bytes, 52) != crc32c::checksum_parts(&[&bytes[..52], &bytes[56..]])
        {
            return Err(bad);
        }
        let text = &bytes[HEADER..HEADER + text_len];
        history_frame::validate_full_render_v4(text)?;
        if u64_at(text, 24) != u64_at(bytes, 16)
            || u64_at(text, 32) != u64_at(bytes, 16)
            || u32_at(text, 40) != u32_at(bytes, 24)
            || u32_at(text, 44) != u32_at(bytes, 28)
        {
            return Err(bad);
        }
        let mut images = Vec::new();
        images
            .try_reserve_exact(count)
            .map_err(|_| TerminalError::LimitExceeded)?;
        let mut offset = HEADER + text_len + resource_len;
        for _ in 0..count {
            if u32_at(bytes, offset + 124) != 0 {
                return Err(bad);
            }
            let protocol = match u32_at(bytes, offset + 120) {
                1 => GraphicProtocol::Sixel,
                2 => GraphicProtocol::Kitty,
                _ => return Err(bad),
            };
            let r = |i: usize| Ratio {
                numerator: u64_at(bytes, offset + 48 + i * 16),
                denominator: u64_at(bytes, offset + 56 + i * 16),
            };
            images.push(ImageDraw {
                placement: u64_at(bytes, offset),
                resource: u64_at(bytes, offset + 8),
                column: u32_at(bytes, offset + 16) as i32,
                row: u32_at(bytes, offset + 20) as i32,
                z: u32_at(bytes, offset + 24) as i32,
                image_id: u32_at(bytes, offset + 28),
                protocol,
                clip: GraphicClip {
                    left: u32_at(bytes, offset + 128),
                    top: u32_at(bytes, offset + 132),
                    right: u32_at(bytes, offset + 136),
                    bottom: u32_at(bytes, offset + 140),
                },
                layout: ImageLayout {
                    source: SourceRectangle {
                        x: u32_at(bytes, offset + 32),
                        y: u32_at(bytes, offset + 36),
                        width: u32_at(bytes, offset + 40),
                        height: u32_at(bytes, offset + 44),
                    },
                    x: r(0),
                    y: r(1),
                    width: r(2),
                    height: r(3),
                    columns: u32_at(bytes, offset + 112),
                    rows: u32_at(bytes, offset + 116),
                },
            });
            offset += DRAW;
        }
        let mut glyphs = Vec::new();
        glyphs
            .try_reserve_exact(glyph_count)
            .map_err(|_| TerminalError::LimitExceeded)?;
        for _ in 0..glyph_count {
            if u32_at(bytes, offset + 28) != 0 {
                return Err(bad);
            }
            glyphs.push(GlyphDraw {
                column: u32_at(bytes, offset),
                row: u32_at(bytes, offset + 4),
                glyph: crate::drcs_bank::GlyphReference {
                    resource: u64_at(bytes, offset + 8),
                    x: u32_at(bytes, offset + 16),
                    width: u32_at(bytes, offset + 20),
                    height: u32_at(bytes, offset + 24),
                },
            });
            offset += GLYPH_DRAW;
        }
        let directory = GraphicsResources::inspect(
            &bytes[HEADER + text_len..HEADER + text_len + resource_len],
        )?;
        Self::validate_draws(
            &images,
            &glyphs,
            text,
            u32_at(bytes, 24),
            u32_at(bytes, 28),
            |id| directory.metadata(id),
            directory.ids(),
        )?;
        Ok(Inspected {
            text,
            directory,
            images,
            glyphs,
        })
    }
    /// Validate every draw and resource without allocating any pixel payload.
    pub(crate) fn inspect_text(bytes: &[u8]) -> Result<&[u8], TerminalError> {
        Ok(Self::inspect(bytes)?.text)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, TerminalError> {
        let Inspected {
            text,
            directory,
            images,
            glyphs,
        } = Self::inspect(bytes)?;
        let mut text_v4 = Vec::new();
        text_v4
            .try_reserve_exact(text.len())
            .map_err(|_| TerminalError::LimitExceeded)?;
        text_v4.extend_from_slice(text);
        let resources = directory.materialize()?;
        Ok(Self {
            revision: u64_at(bytes, 16),
            columns: u32_at(bytes, 24),
            rows: u32_at(bytes, 28),
            text_v4,
            resources,
            images,
            glyphs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Event, HostGeometryV1, OutputProfile, TerminalOptions, WidthProfile,
        graphics_state::Anchor, kitty_graphics::TransferDecoder,
    };
    fn terminal() -> Terminal {
        Terminal::new_with_graphics_profile(
            10,
            4,
            0,
            TerminalOptions {
                history_capacity_rows: 20,
            },
            OutputProfile::Utf8,
            WidthProfile::Unicode16,
        )
        .unwrap()
    }
    fn add(t: &mut Terminal, line: u64, z: i32, id: u32) {
        let request = format!("Ga=T,s=1,v=1,i={id},c=2,r=2,z={z};AQIDBA==");
        let command = TransferDecoder::default()
            .push(request.as_bytes())
            .unwrap()
            .unwrap();
        t.graphics
            .as_mut()
            .unwrap()
            .apply_kitty(
                command,
                Anchor {
                    page: 1,
                    logical_line: line,
                    cell_offset: 0,
                },
                HostGeometryV1 {
                    cell_width: 10,
                    cell_height: 20,
                    ..Default::default()
                },
                &[],
            )
            .unwrap();
    }
    #[test]
    fn frames_have_strict_text_revision_only_referenced_pixels_and_deterministic_z_order() {
        let mut t = terminal();
        add(&mut t, 0, 2, 4);
        add(&mut t, 1, -1, 8);
        let bytes = t.export_render_frame_v5(0).unwrap();
        let frame = Frame::decode(&bytes).unwrap();
        assert_eq!(
            frame.images.iter().map(|i| i.image_id).collect::<Vec<_>>(),
            vec![8, 4]
        );
        assert_eq!(frame.resources.entries().len(), 2);
        assert_eq!(frame.encode().unwrap(), bytes);
        let snapshot = t.export_snapshot_v2().unwrap();
        let restored = Terminal::import_snapshot_v2(&snapshot).unwrap();
        assert_eq!(restored.export_render_frame_v5(0).unwrap(), bytes);
        assert_eq!(
            Frame::decode(
                &Terminal::new(10, 4)
                    .unwrap()
                    .export_render_frame_v5(0)
                    .unwrap()
            )
            .unwrap()
            .images
            .len(),
            0
        );
    }
    #[test]
    fn text_only_frames_project_without_image_anchors_across_large_history() {
        let mut t = Terminal::new_with_graphics_profile(
            10,
            4,
            0,
            TerminalOptions {
                history_capacity_rows: 5_200,
            },
            OutputProfile::Utf8,
            WidthProfile::Unicode16,
        )
        .unwrap();
        for (new_lines, expected_history) in [(0, 0), (2_003, 2_000), (3_200, 5_200)] {
            if new_lines > 0 {
                let output = b"x\r\n".repeat(new_lines);
                t.apply_event(Event::Output {
                    source_sequence: t.next_source_sequence(),
                    bytes: &output,
                })
                .unwrap();
            }
            assert_eq!(t.scrollback_info().0, expected_history);
            for offset in [0, 17.min(expected_history)] {
                let frame = Frame::decode(&t.export_render_frame_v5(offset).unwrap()).unwrap();
                assert!(frame.images.is_empty());
                assert!(frame.glyphs.is_empty());
                assert!(frame.resources.entries().is_empty());
                assert_eq!(frame.rows, 4);
            }
        }
    }
    #[test]
    fn placements_on_another_page_do_not_require_image_anchor_projection() {
        let mut t = terminal();
        add(&mut t, 0, 0, 1);
        t.apply_event(Event::Output {
            source_sequence: t.next_source_sequence(),
            bytes: b"\x1b[?1049h",
        })
        .unwrap();
        let frame = Frame::decode(&t.export_render_frame_v5(0).unwrap()).unwrap();
        assert!(frame.images.is_empty());
        assert!(frame.resources.entries().is_empty());
        t.apply_event(Event::Output {
            source_sequence: t.next_source_sequence(),
            bytes: b"\x1b[?1049l",
        })
        .unwrap();
        assert_eq!(
            Frame::decode(&t.export_render_frame_v5(0).unwrap())
                .unwrap()
                .images
                .len(),
            1
        );
    }
    #[test]
    fn live_and_frozen_history_project_anchors_without_changing_the_authority() {
        let mut t = terminal();
        add(&mut t, 0, 0, 1);
        let original = t.clone();
        t.apply_event(Event::Output {
            source_sequence: 0,
            bytes: b"a\r\nb\r\nc\r\nd\r\ne",
        })
        .unwrap();
        let live = Frame::decode(&t.export_render_frame_v5(0).unwrap()).unwrap();
        assert_eq!(live.images[0].row, -1);
        let history = Frame::decode(&t.export_render_frame_v5(1).unwrap()).unwrap();
        assert_eq!(history.images[0].row, 0);
        assert_eq!(
            Frame::decode(&original.export_render_frame_v5(0).unwrap())
                .unwrap()
                .images[0]
                .row,
            0
        );
        let checkpoint = t.export_snapshot_v2().unwrap();
        t.export_render_frame_v5(1).unwrap();
        assert_eq!(t.export_snapshot_v2().unwrap(), checkpoint);
    }
    #[test]
    fn corrupted_headers_positions_resources_and_ratios_do_not_decode() {
        let mut t = terminal();
        add(&mut t, 0, 0, 1);
        let bytes = t.export_render_frame_v5(0).unwrap();
        let draw = HEADER + u32_at(&bytes, 32) as usize + u32_at(&bytes, 36) as usize;
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
            48,
            52,
            56,
            draw + 8,
            draw + 16,
            draw + 20,
            draw + 40,
            draw + 120,
            draw + 124,
        ] {
            let mut bad = bytes.clone();
            bad[at] ^= 0x80;
            if at >= 56 {
                let sum = crc32c::checksum_parts(&[&bad[..52], &bad[56..]]);
                put32(&mut bad, 52, sum);
            }
            assert!(Frame::decode(&bad).is_err(), "at {at}");
        }
        let mut bad = bytes.clone();
        put64(&mut bad, draw + 56, 0);
        let sum = crc32c::checksum_parts(&[&bad[..52], &bad[56..]]);
        put32(&mut bad, 52, sum);
        assert!(Frame::decode(&bad).is_err());
        for end in [0, 63, 64, bytes.len() - 1] {
            assert!(Frame::decode(&bytes[..end]).is_err());
        }
    }

    #[test]
    fn invalid_draws_are_rejected_before_allocating_any_pixels() {
        let mut t = terminal();
        add(&mut t, 0, 0, 1);
        let bytes = t.export_render_frame_v5(0).unwrap();
        let draw = HEADER + u32_at(&bytes, 32) as usize + u32_at(&bytes, 36) as usize;
        let before = crate::graphics_resources::image_allocation_attempts();
        for (offset, value) in [
            (draw + 8, 99),
            (draw + 16, 99),
            (draw + 20, 99),
            (draw + 40, 0),
            (draw + 56, 0),
            (draw + 112, 0),
            (draw + 128, 2),
            (draw + 132, 2),
            (draw + 136, 0),
            (draw + 140, 3),
        ] {
            let mut bad = bytes.clone();
            put32(&mut bad, offset, value);
            let sum = crc32c::checksum_parts(&[&bad[..52], &bad[56..]]);
            put32(&mut bad, 52, sum);
            assert!(Frame::decode(&bad).is_err(), "invalid field {offset}");
            assert_eq!(
                crate::graphics_resources::image_allocation_attempts(),
                before
            );
        }
        assert!(Frame::decode(&bytes).is_ok());
        assert_eq!(
            crate::graphics_resources::image_allocation_attempts(),
            before + 1
        );
    }

    #[test]
    fn explicit_glyphs_roundtrip_and_invalid_references_never_allocate_pixels() {
        let mut frame = Frame::decode(
            &Terminal::new(10, 4)
                .unwrap()
                .export_render_frame_v5(0)
                .unwrap(),
        )
        .unwrap();
        let resource = frame
            .resources
            .insert(
                crate::graphics_resources::ImageData::allocate(4, 6, ImageFormat::Alpha8).unwrap(),
            )
            .unwrap();
        frame.glyphs.push(GlyphDraw {
            column: 1,
            row: 0,
            glyph: crate::drcs_bank::GlyphReference {
                resource,
                x: 1,
                width: 3,
                height: 6,
            },
        });
        let bytes = frame.encode().unwrap();
        assert_eq!(u32_at(&bytes, 44), 1);
        assert_eq!(u32_at(&bytes, 48), REQUIRED_FEATURES);
        assert_eq!(Frame::decode(&bytes).unwrap(), frame);
        let draw = bytes.len() - GLYPH_DRAW;
        let before = crate::graphics_resources::image_allocation_attempts();
        for (offset, value) in [
            (draw, 10),
            (draw + 4, 4),
            (draw + 8, 99),
            (draw + 16, 4),
            (draw + 20, 0),
            (draw + 24, 7),
            (draw + 28, 1),
            (48, 1),
        ] {
            let mut bad = bytes.clone();
            put32(&mut bad, offset, value);
            let sum = crc32c::checksum_parts(&[&bad[..52], &bad[56..]]);
            put32(&mut bad, 52, sum);
            assert!(Frame::decode(&bad).is_err(), "invalid glyph field {offset}");
            assert_eq!(
                crate::graphics_resources::image_allocation_attempts(),
                before
            );
        }
        frame.glyphs.push(frame.glyphs[0].clone());
        assert!(frame.encode().is_err());
        frame.glyphs.pop();
        frame
            .resources
            .insert(
                crate::graphics_resources::ImageData::allocate(1, 1, ImageFormat::Alpha8).unwrap(),
            )
            .unwrap();
        assert!(frame.encode().is_err());
    }

    #[test]
    fn clip_must_intersect_viewport_and_glyph_must_target_single_narrow_cell() {
        let mut t = terminal();
        add(&mut t, 0, 0, 1);
        let mut frame = Frame::decode(&t.export_render_frame_v5(0).unwrap()).unwrap();
        frame.images[0].column = -1;
        frame.images[0].row = -1;
        assert!(Frame::decode(&frame.encode().unwrap()).is_ok());
        frame.images[0].clip.right = 1;
        assert!(frame.encode().is_err());
        frame.images[0].clip.right = 2;
        frame.images[0].clip.bottom = 1;
        assert!(frame.encode().is_err());
        let mut text = Terminal::new(10, 4).unwrap();
        text.apply_event(Event::Output {
            source_sequence: 0,
            bytes: "界e\u{301}".as_bytes(),
        })
        .unwrap();
        let mut frame = Frame::decode(&text.export_render_frame_v5(0).unwrap()).unwrap();
        let resource = frame
            .resources
            .insert(
                crate::graphics_resources::ImageData::allocate(1, 1, ImageFormat::Alpha8).unwrap(),
            )
            .unwrap();
        frame.glyphs.push(GlyphDraw {
            column: 0,
            row: 0,
            glyph: crate::drcs_bank::GlyphReference {
                resource,
                x: 0,
                width: 1,
                height: 1,
            },
        });
        for column in [0, 1, 2] {
            frame.glyphs[0].column = column;
            assert!(frame.encode().is_err(), "invalid glyph cell {column}");
        }
        frame.glyphs[0].column = 3;
        assert!(frame.encode().is_ok());
    }
}
