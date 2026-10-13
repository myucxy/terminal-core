//! Committed graphic resources, protocol identities and anchored placements. The terminal caller
//! supplies row anchors and live cell references; this component never mutates terminal text.
use crate::{
    HostGeometryV1, TerminalError,
    drcs::DrcsDownload,
    drcs_bank::DrcsBank,
    graphics_layout::{self, ImageLayout, MAX_PLACEMENT_CELLS},
    graphics_resources::{GraphicsResources, MAX_IMAGE_RESOURCES},
    kitty_graphics::{Content, Control, DecodedCommand},
    sixel::{SixelImage, SixelPalette},
};
mod codec;
pub use codec::MAX_GRAPHICS_STATE_BYTES;

pub const MAX_PLACEMENTS: usize = 4096;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Anchor {
    pub page: u8,
    pub logical_line: u64,
    pub cell_offset: u32,
}
impl Anchor {
    fn validate(self) -> Result<(), TerminalError> {
        if !(1..=7).contains(&self.page) || self.logical_line == u64::MAX {
            return Err(TerminalError::Unsupported(
                "graphic anchor requires page 1..7 and a valid logical line (zero is the initial line)",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphicProtocol {
    Sixel,
    Kitty,
}
/// Half-open cell rectangle relative to the full, unchanged allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphicClip {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}
impl GraphicClip {
    pub fn full(layout: ImageLayout) -> Self {
        Self {
            left: 0,
            top: 0,
            right: layout.columns,
            bottom: layout.rows,
        }
    }
    fn validate(self, layout: ImageLayout) -> Result<(), TerminalError> {
        if self.left >= self.right
            || self.top >= self.bottom
            || self.right > layout.columns
            || self.bottom > layout.rows
        {
            return Err(TerminalError::MalformedSnapshot);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Placement {
    pub id: u64,
    pub resource: u64,
    pub image_id: u32,
    pub placement_id: u32,
    pub protocol: GraphicProtocol,
    pub anchor: Anchor,
    pub layout: ImageLayout,
    /// Full allocation origin relative to the current anchor; clipping never rescales the image.
    pub origin_x: i32,
    pub origin_y: i32,
    pub clip: GraphicClip,
    pub z: i32,
}
impl Placement {
    fn validate(&self) -> Result<(), TerminalError> {
        self.anchor.validate()?;
        self.layout.validate()?;
        self.clip.validate(self.layout)?;
        if self.origin_x.unsigned_abs() > MAX_PLACEMENT_CELLS
            || self.origin_y.unsigned_abs() > MAX_PLACEMENT_CELLS
        {
            return Err(TerminalError::MalformedSnapshot);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct KittyImage {
    id: u32,
    number: u32,
    resource: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphicsState {
    resources: GraphicsResources,
    fonts: DrcsBank,
    palette: SixelPalette,
    sixel_scrolling: bool,
    images: Vec<KittyImage>,
    placements: Vec<Placement>,
    next_placement: u64,
    next_kitty_id: u32,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlacementResult {
    pub control: Control,
    pub cursor_columns: u32,
    pub cursor_rows: u32,
    pub placement: Option<u64>,
}

#[derive(Clone, Copy, Debug)]
pub struct GraphicsViewport {
    pub page: u8,
    pub columns: u32,
    pub rows: u32,
    pub cursor_column: u32,
    pub cursor_row: u32,
}
impl Default for GraphicsState {
    fn default() -> Self {
        Self {
            resources: GraphicsResources::default(),
            fonts: DrcsBank::default(),
            palette: crate::sixel::default_palette(),
            sixel_scrolling: true,
            images: Vec::new(),
            placements: Vec::new(),
            next_placement: 1,
            next_kitty_id: 1,
        }
    }
}
impl GraphicsState {
    pub fn resources(&self) -> &GraphicsResources {
        &self.resources
    }
    pub fn fonts(&self) -> &DrcsBank {
        &self.fonts
    }
    pub fn palette(&self) -> &SixelPalette {
        &self.palette
    }
    pub fn sixel_scrolling(&self) -> bool {
        self.sixel_scrolling
    }
    pub fn set_sixel_scrolling(&mut self, value: bool) {
        self.sixel_scrolling = value;
    }
    pub fn placements(&self) -> &[Placement] {
        &self.placements
    }
    fn collect(&mut self, cell_resources: &[u64]) -> Result<(), TerminalError> {
        self.images.retain(|image| {
            image.id != 0 || self.placements.iter().any(|p| p.resource == image.resource)
        });
        self.resources.retain_referenced(
            self.images
                .iter()
                .map(|i| i.resource)
                .chain(self.placements.iter().map(|p| p.resource))
                .chain(self.fonts.referenced_resources())
                .chain(cell_resources.iter().copied()),
        )
    }
    pub fn collect_unused(&mut self, cell_resources: &[u64]) -> Result<(), TerminalError> {
        let mut candidate = self.clone();
        candidate.collect(cell_resources)?;
        *self = candidate;
        Ok(())
    }
    /// Screen owns visibility/reflow decisions; removal is atomic with resource collection.
    pub fn retain_placements(
        &mut self,
        mut retain: impl FnMut(&Placement) -> bool,
        cell_resources: &[u64],
    ) -> Result<(), TerminalError> {
        let mut candidate = self.clone();
        candidate.placements.retain(|p| retain(p));
        candidate.collect(cell_resources)?;
        *self = candidate;
        Ok(())
    }
    /// Reanchors or splits one page atomically while preserving pixels and logical image identity.
    pub fn remap_placements(
        &mut self,
        page: u8,
        mut mapper: impl FnMut(&Placement) -> Result<Vec<Placement>, TerminalError>,
        cell_resources: &[u64],
    ) -> Result<(), TerminalError> {
        if !(1..=7).contains(&page) {
            return Err(TerminalError::MalformedSnapshot);
        }
        let mut candidate = self.clone();
        let mut entries = Vec::new();
        for original in &self.placements {
            let fragments = if original.anchor.page == page {
                mapper(original)?
            } else {
                vec![original.clone()]
            };
            if entries
                .len()
                .checked_add(fragments.len())
                .is_none_or(|n| n > MAX_PLACEMENTS)
            {
                return Err(TerminalError::ProtocolResourceLimit(
                    "terminal graphic placements exceed 4096",
                ));
            }
            entries
                .try_reserve(fragments.len())
                .map_err(|_| TerminalError::LimitExceeded)?;
            for (index, mut entry) in fragments.into_iter().enumerate() {
                if entry.resource != original.resource
                    || entry.protocol != original.protocol
                    || entry.image_id != original.image_id
                    || entry.placement_id != original.placement_id
                    || entry.layout != original.layout
                    || entry.z != original.z
                    || entry.anchor.page != original.anchor.page
                {
                    return Err(TerminalError::MalformedSnapshot);
                }
                entry.validate()?;
                entry.id = if index == 0 {
                    original.id
                } else {
                    let id = candidate.next_placement;
                    candidate.next_placement =
                        id.checked_add(1)
                            .ok_or(TerminalError::ProtocolResourceLimit(
                                "graphic placement IDs exhausted",
                            ))?;
                    id
                };
                entries.push(entry);
            }
        }
        entries.sort_unstable_by_key(|p| p.id);
        candidate.placements = entries;
        candidate.collect(cell_resources)?;
        *self = candidate;
        Ok(())
    }
    /// Full terminal reset preserves monotonic internal identities; frozen history remains immutable.
    pub fn reset(&mut self, retained_cell_resources: &[u64]) -> Result<(), TerminalError> {
        let mut candidate = self.clone();
        candidate.images.clear();
        candidate.placements.clear();
        candidate.fonts.clear();
        candidate.palette = Self::default().palette;
        candidate.sixel_scrolling = true;
        candidate.collect(retained_cell_resources)?;
        *self = candidate;
        Ok(())
    }
    pub fn install_font(
        &mut self,
        download: DrcsDownload,
        cell_resources: &[u64],
    ) -> Result<u8, TerminalError> {
        let mut candidate = self.clone();
        candidate.collect(cell_resources)?;
        let id = candidate
            .fonts
            .install(download, &mut candidate.resources)?;
        candidate.collect(cell_resources)?;
        *self = candidate;
        Ok(id)
    }
    fn add_placement(&mut self, mut entry: Placement) -> Result<u64, TerminalError> {
        entry.validate()?;
        if entry.image_id != 0 && entry.placement_id != 0 {
            self.placements.retain(|p| {
                p.protocol != entry.protocol
                    || p.image_id != entry.image_id
                    || p.placement_id != entry.placement_id
            });
        }
        if self.placements.len() >= MAX_PLACEMENTS {
            return Err(TerminalError::ProtocolResourceLimit(
                "terminal graphic placements exceed 4096",
            ));
        }
        let next =
            self.next_placement
                .checked_add(1)
                .ok_or(TerminalError::ProtocolResourceLimit(
                    "graphic placement IDs exhausted",
                ))?;
        self.placements
            .try_reserve(1)
            .map_err(|_| TerminalError::LimitExceeded)?;
        let id = self.next_placement;
        self.next_placement = next;
        entry.id = id;
        self.placements.push(entry);
        Ok(id)
    }
    fn find_image(&self, control: &Control) -> Option<&KittyImage> {
        if control.number(b'i') != 0 {
            self.images.iter().find(|i| i.id == control.number(b'i'))
        } else if control.number(b'I') != 0 {
            self.images
                .iter()
                .rev()
                .find(|i| i.number == control.number(b'I'))
        } else {
            None
        }
    }
    fn allocate_kitty_id(&mut self) -> Result<u32, TerminalError> {
        // At most 256 occupied IDs; no scan across a caller-controlled 32-bit range.
        for _ in 0..=MAX_IMAGE_RESOURCES {
            let id = self.next_kitty_id;
            self.next_kitty_id = self.next_kitty_id.checked_add(1).unwrap_or(1);
            if !self.images.iter().any(|i| i.id == id) {
                return Ok(id);
            }
        }
        Err(TerminalError::ProtocolResourceLimit(
            "Kitty image IDs unavailable",
        ))
    }
    fn admit_kitty_image(
        &mut self,
        bytes: usize,
        cell_resources: &[u64],
    ) -> Result<(), TerminalError> {
        while self.images.len() >= MAX_IMAGE_RESOURCES
            || self.resources.entries().len() >= MAX_IMAGE_RESOURCES
            || self.resources.pixel_bytes() + bytes
                > crate::graphics_resources::MAX_SESSION_IMAGE_BYTES
        {
            // Evict oldest unplaced uploads first, never a visible/history/font/cell resource.
            let Some(index) = self.images.iter().position(|image| {
                !self.placements.iter().any(|p| p.resource == image.resource)
                    && !cell_resources.contains(&image.resource)
                    && !self
                        .fonts
                        .referenced_resources()
                        .any(|id| id == image.resource)
            }) else {
                return Err(TerminalError::ProtocolResourceLimit(
                    "Kitty image budget exhausted: all remaining resources are referenced",
                ));
            };
            self.images.remove(index);
            self.collect(cell_resources)?;
        }
        Ok(())
    }
    /// Query decodes and validates outside this state and commits no resource. Broker/delete commands
    /// require their explicit host/viewport handler; they cannot accidentally fall through as success.
    pub fn apply_kitty(
        &mut self,
        command: DecodedCommand,
        anchor: Anchor,
        host: HostGeometryV1,
        cell_resources: &[u64],
    ) -> Result<PlacementResult, TerminalError> {
        let mut control = command.control;
        let action = control.action();
        if action == b'q' {
            if !matches!(command.content, Content::Image(_)) {
                return Err(TerminalError::Unsupported(
                    "Kitty query broker data is unresolved",
                ));
            }
            return Ok(PlacementResult {
                control,
                cursor_columns: 0,
                cursor_rows: 0,
                placement: None,
            });
        }
        let mut candidate = self.clone();
        candidate.collect(cell_resources)?;
        let resource = match command.content {
            Content::Image(image) if matches!(action, b't' | b'T') => {
                if control.number(b'i') == 0 && control.number(b'I') != 0 {
                    let id = candidate.allocate_kitty_id()?;
                    control.set(b'i', i64::from(id));
                }
                let id = control.number(b'i');
                if id != 0
                    && let Some(old) = candidate.images.iter().find(|i| i.id == id).cloned()
                {
                    candidate.images.retain(|i| i.id != id);
                    candidate.placements.retain(|p| p.resource != old.resource);
                    candidate.collect(cell_resources)?;
                }
                candidate.admit_kitty_image(image.pixels().len(), cell_resources)?;
                let resource = candidate.resources.insert(image)?;
                candidate
                    .images
                    .try_reserve(1)
                    .map_err(|_| TerminalError::LimitExceeded)?;
                candidate.images.push(KittyImage {
                    id,
                    number: control.number(b'I'),
                    resource,
                });
                resource
            }
            Content::Display if action == b'p' => {
                let image = candidate
                    .find_image(&control)
                    .ok_or(TerminalError::Unsupported(
                        "Kitty ENOENT: image ID or number not found",
                    ))?;
                control.set(b'i', i64::from(image.id));
                image.resource
            }
            _ => {
                return Err(TerminalError::Unsupported(
                    "Kitty command requires the broker or deletion/viewport handler",
                ));
            }
        };
        let mut result = PlacementResult {
            control,
            cursor_columns: 0,
            cursor_rows: 0,
            placement: None,
        };
        if matches!(action, b'T' | b'p') {
            let image = candidate
                .resources
                .get(resource)
                .expect("inserted or resolved image")
                .image();
            if let Some(layout) =
                graphics_layout::kitty(&result.control, image.width(), image.height(), host)?
            {
                if result.control.number(b'C') == 0 {
                    result.cursor_columns = layout.columns;
                    result.cursor_rows = layout.rows;
                }
                result.placement = Some(candidate.add_placement(Placement {
                    id: 0,
                    resource,
                    image_id: result.control.number(b'i'),
                    placement_id: if result.control.number(b'i') == 0 {
                        0
                    } else {
                        result.control.number(b'p')
                    },
                    protocol: GraphicProtocol::Kitty,
                    anchor,
                    layout,
                    origin_x: 0,
                    origin_y: 0,
                    clip: GraphicClip::full(layout),
                    z: result.control.get(b'z').unwrap_or(0) as i32,
                })?);
            }
        }
        candidate.collect(cell_resources)?;
        *self = candidate;
        Ok(result)
    }
    pub fn apply_sixel(
        &mut self,
        decoded: SixelImage,
        anchor: Anchor,
        host: HostGeometryV1,
        cell_resources: &[u64],
    ) -> Result<Option<u64>, TerminalError> {
        let layout = decoded
            .image
            .as_ref()
            .map(|image| {
                graphics_layout::sixel(
                    image.width(),
                    image.height(),
                    decoded.aspect_numerator,
                    decoded.aspect_denominator,
                    host,
                )
            })
            .transpose()?;
        self.apply_sixel_layout(decoded, anchor, layout, (0, 0), None, cell_resources)
    }
    pub fn apply_sixel_layout(
        &mut self,
        decoded: SixelImage,
        anchor: Anchor,
        layout: Option<ImageLayout>,
        origin: (i32, i32),
        clip: Option<GraphicClip>,
        cell_resources: &[u64],
    ) -> Result<Option<u64>, TerminalError> {
        let mut candidate = self.clone();
        candidate.collect(cell_resources)?;
        let placement = if let (Some(image), Some(layout)) = (decoded.image, layout) {
            let source = layout.source;
            if source.x >= image.width()
                || source.y >= image.height()
                || source.width > image.width() - source.x
                || source.height > image.height() - source.y
            {
                return Err(TerminalError::MalformedSnapshot);
            }
            let resource = candidate.resources.insert(image)?;
            Some(candidate.add_placement(Placement {
                id: 0,
                resource,
                image_id: 0,
                placement_id: 0,
                protocol: GraphicProtocol::Sixel,
                anchor,
                layout,
                origin_x: origin.0,
                origin_y: origin.1,
                clip: clip.unwrap_or_else(|| GraphicClip::full(layout)),
                z: 0,
            })?)
        } else {
            None
        };
        candidate.palette = decoded.palette;
        *self = candidate;
        Ok(placement)
    }
    /// Exact-ID deletion works independently of the visible viewport. Coordinate/all-visible
    /// selectors are handled after resolving anchors in the owning Screen.
    pub fn delete_kitty_identity(
        &mut self,
        control: &Control,
        cell_resources: &[u64],
    ) -> Result<(), TerminalError> {
        if control.action() != b'd' {
            return Err(TerminalError::Unsupported(
                "Kitty deletion requires action d",
            ));
        }
        let selector = control.get(b'd').unwrap_or(i64::from(b'a')) as u8;
        if !matches!(selector, b'i' | b'I' | b'n' | b'N' | b'r' | b'R') {
            return Err(TerminalError::Unsupported(
                "Kitty deletion selector requires viewport coordinates",
            ));
        }
        let mut candidate = self.clone();
        let placement = control.number(b'p');
        let target = candidate.find_image(control).map(|i| i.id);
        let matches = |id: u32| match selector {
            b'i' | b'I' => id != 0 && id == control.number(b'i'),
            b'n' | b'N' => Some(id) == target,
            b'r' | b'R' => id != 0 && id >= control.number(b'x') && id <= control.number(b'y'),
            _ => false,
        };
        candidate.placements.retain(|p| {
            p.protocol != GraphicProtocol::Kitty
                || !matches(p.image_id)
                || (placement != 0 && p.placement_id != placement)
        });
        if selector.is_ascii_uppercase() {
            candidate.images.retain(|i| {
                !matches(i.id)
                    || candidate
                        .placements
                        .iter()
                        .any(|p| p.resource == i.resource)
            });
        }
        candidate.collect(cell_resources)?;
        *self = candidate;
        Ok(())
    }

    /// `locate` resolves a page's durable text anchor to viewport-relative cell coordinates.
    /// Negative rows are scrollback. Hidden pages are never affected by coordinate selectors.
    pub fn delete_kitty_in_viewport(
        &mut self,
        control: &Control,
        viewport: GraphicsViewport,
        locate: impl Fn(Anchor) -> Option<(i32, i32)>,
        cell_resources: &[u64],
    ) -> Result<(), TerminalError> {
        if control.action() != b'd' {
            return Err(TerminalError::Unsupported(
                "Kitty deletion requires action d",
            ));
        }
        let selector = control.get(b'd').unwrap_or(i64::from(b'a')) as u8;
        if matches!(selector, b'i' | b'I' | b'n' | b'N' | b'r' | b'R') {
            return self.delete_kitty_identity(control, cell_resources);
        }
        if !matches!(
            selector,
            b'a' | b'A'
                | b'c'
                | b'C'
                | b'p'
                | b'P'
                | b'q'
                | b'Q'
                | b'x'
                | b'X'
                | b'y'
                | b'Y'
                | b'z'
                | b'Z'
        ) {
            return Err(TerminalError::Unsupported(
                "Kitty viewport deletion selector is unsupported",
            ));
        }
        crate::validate_dimensions(viewport.columns, viewport.rows)?;
        if !(1..=7).contains(&viewport.page)
            || viewport.cursor_column >= viewport.columns
            || viewport.cursor_row >= viewport.rows
        {
            return Err(TerminalError::Unsupported(
                "Kitty deletion requires a valid page and cursor",
            ));
        }
        let mut removed = Vec::new();
        removed
            .try_reserve_exact(self.placements.len())
            .map_err(|_| TerminalError::LimitExceeded)?;
        let mut candidate = self.clone();
        for placement in &self.placements {
            if placement.protocol != GraphicProtocol::Kitty
                || placement.anchor.page != viewport.page
            {
                continue;
            }
            let Some((column, row)) = locate(placement.anchor) else {
                return Err(TerminalError::MalformedSnapshot);
            };
            let x_origin = i64::from(column) + i64::from(placement.origin_x);
            let y_origin = i64::from(row) + i64::from(placement.origin_y);
            let left = x_origin + i64::from(placement.clip.left);
            let top = y_origin + i64::from(placement.clip.top);
            let right = x_origin + i64::from(placement.clip.right);
            let bottom = y_origin + i64::from(placement.clip.bottom);
            let visible = left < i64::from(viewport.columns)
                && right > 0
                && top < i64::from(viewport.rows)
                && bottom > 0;
            let intersects_x =
                |x: i64| x >= 0 && x < i64::from(viewport.columns) && left <= x && x < right;
            let intersects_y =
                |y: i64| y >= 0 && y < i64::from(viewport.rows) && top <= y && y < bottom;
            let x = i64::from(control.number(b'x')) - 1;
            let y = i64::from(control.number(b'y')) - 1;
            let z = control.get(b'z').unwrap_or(0) as i32;
            let selected = match selector.to_ascii_lowercase() {
                b'a' => visible,
                b'c' => {
                    visible
                        && intersects_x(i64::from(viewport.cursor_column))
                        && intersects_y(i64::from(viewport.cursor_row))
                }
                b'p' => visible && intersects_x(x) && intersects_y(y),
                b'q' => visible && intersects_x(x) && intersects_y(y) && placement.z == z,
                b'x' => visible && intersects_x(x),
                b'y' => visible && intersects_y(y),
                b'z' => placement.z == z,
                _ => unreachable!(),
            };
            if selected {
                removed.push((placement.id, placement.resource));
            }
        }
        // Placements and selected IDs are in monotonic internal-ID order.
        candidate
            .placements
            .retain(|p| removed.binary_search_by_key(&p.id, |v| v.0).is_err());
        if selector.is_ascii_uppercase() {
            candidate.images.retain(|image| {
                !removed.iter().any(|p| p.1 == image.resource)
                    || candidate
                        .placements
                        .iter()
                        .any(|p| p.resource == image.resource)
            });
        }
        candidate.collect(cell_resources)?;
        *self = candidate;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kitty_graphics::{TransferDecoder, response};
    fn cmd(input: &[u8]) -> DecodedCommand {
        TransferDecoder::default().push(input).unwrap().unwrap()
    }
    fn anchor() -> Anchor {
        Anchor {
            page: 1,
            logical_line: 1,
            cell_offset: 0,
        }
    }
    fn host() -> HostGeometryV1 {
        HostGeometryV1 {
            cell_width: 10,
            cell_height: 20,
            ..Default::default()
        }
    }
    fn viewport() -> GraphicsViewport {
        GraphicsViewport {
            page: 1,
            columns: 10,
            rows: 4,
            cursor_column: 0,
            cursor_row: 0,
        }
    }
    fn locate(anchor: Anchor) -> Option<(i32, i32)> {
        Some((anchor.cell_offset as i32, anchor.logical_line as i32 - 5))
    }
    fn viewport_state() -> GraphicsState {
        let mut state = GraphicsState::default();
        for (id, page, line, column, z) in [
            (1, 1, 5, 0, 0),
            (2, 1, 6, 3, 7),
            (3, 1, 0, 0, 7),
            (4, 7, 5, 0, 0),
        ] {
            let request = format!("Ga=T,s=1,v=1,i={id},c=2,r=2,z={z},C=1;AQIDBA==");
            state
                .apply_kitty(
                    cmd(request.as_bytes()),
                    Anchor {
                        page,
                        logical_line: line,
                        cell_offset: column,
                    },
                    host(),
                    &[],
                )
                .unwrap();
        }
        state
    }
    #[test]
    fn remapping_splits_preserve_pixels_identity_and_canonical_snapshot_order() {
        let mut state = viewport_state();
        let original = state.placements()[0].clone();
        state
            .remap_placements(
                1,
                |p| {
                    if p.id != original.id {
                        return Ok(vec![p.clone()]);
                    }
                    let mut left = p.clone();
                    left.clip.right = 1;
                    left.origin_y = -1;
                    let mut right = p.clone();
                    right.clip.left = 1;
                    right.anchor.logical_line += 1;
                    Ok(vec![left, right])
                },
                &[],
            )
            .unwrap();
        assert_eq!(state.placements().len(), 5);
        assert_eq!(state.placements()[0].id, original.id);
        assert_eq!(state.placements()[4].id, 5);
        assert_eq!(state.placements()[4].resource, original.resource);
        assert_eq!(state.placements()[4].layout, original.layout);
        assert_eq!(state.resources().entries().len(), 4);
        assert_eq!(
            GraphicsState::decode(&state.encode().unwrap()).unwrap(),
            state
        );

        let before = state.clone();
        for mutate in [0, 1, 2, 3] {
            assert!(
                state
                    .remap_placements(
                        1,
                        |p| {
                            let mut changed = p.clone();
                            match mutate {
                                0 => changed.clip.left = changed.clip.right,
                                1 => changed.origin_x = i32::MIN,
                                2 => changed.resource += 1,
                                _ => changed.anchor.page = 7,
                            }
                            Ok(vec![changed])
                        },
                        &[]
                    )
                    .is_err()
            );
            assert_eq!(state, before);
        }
        assert!(
            state
                .remap_placements(0, |p| Ok(vec![p.clone()]), &[])
                .is_err()
        );
        assert!(
            state
                .remap_placements(1, |p| Ok(vec![p.clone(); MAX_PLACEMENTS + 1]), &[])
                .is_err()
        );
        assert!(
            state
                .remap_placements(1, |p| Ok(vec![p.clone()]), &[u64::MAX])
                .is_err()
        );
        assert_eq!(state, before);
    }
    #[test]
    fn viewport_deletion_tests_only_the_surviving_clip_at_its_signed_origin() {
        let mut state = viewport_state();
        state
            .remap_placements(
                1,
                |p| {
                    let mut p = p.clone();
                    if p.image_id == 1 {
                        p.origin_x = -1;
                        p.clip.left = 1;
                    }
                    Ok(vec![p])
                },
                &[],
            )
            .unwrap();
        state
            .delete_kitty_in_viewport(&cmd(b"Ga=d,d=p,x=2,y=1").control, viewport(), locate, &[])
            .unwrap();
        assert!(state.placements().iter().any(|p| p.image_id == 1));
        state
            .delete_kitty_in_viewport(&cmd(b"Ga=d,d=p,x=1,y=1").control, viewport(), locate, &[])
            .unwrap();
        assert!(!state.placements().iter().any(|p| p.image_id == 1));
    }
    #[test]
    fn sixel_layout_clipping_discards_hidden_pixels_and_rejects_invalid_geometry_atomically() {
        let mut state = GraphicsState::default();
        let decoded =
            crate::sixel::decode(&[0, 1], b"#1;2;100;0;0~", state.palette(), [0; 3]).unwrap();
        assert!(decoded.image.is_some());
        assert_eq!(
            state
                .apply_sixel_layout(decoded, anchor(), None, (0, 0), None, &[])
                .unwrap(),
            None
        );
        assert!(state.resources().entries().is_empty());
        assert_eq!(state.palette()[1], [255, 0, 0]);
        let before = state.clone();
        for invalid_origin in [false, true] {
            let decoded = crate::sixel::decode(&[0, 1], b"#1~", state.palette(), [0; 3]).unwrap();
            let layout = graphics_layout::sixel(1, 6, 1, 1, host()).unwrap();
            let clip = if invalid_origin {
                None
            } else {
                Some(GraphicClip {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 1,
                })
            };
            let origin = if invalid_origin { (0, -4097) } else { (0, 0) };
            assert!(
                state
                    .apply_sixel_layout(decoded, anchor(), Some(layout), origin, clip, &[])
                    .is_err()
            );
            assert_eq!(state, before);
        }
    }
    #[test]
    fn viewport_deletion_selectors_respect_cells_z_history_and_hidden_pages() {
        for (request, remaining) in [
            ("Ga=d", vec![3, 4]),
            ("Ga=d,d=c", vec![2, 3, 4]),
            ("Ga=d,d=p,x=4,y=2", vec![1, 3, 4]),
            ("Ga=d,d=q,x=4,y=2,z=7", vec![1, 3, 4]),
            ("Ga=d,d=q,x=4,y=2,z=8", vec![1, 2, 3, 4]),
            ("Ga=d,d=x,x=2", vec![2, 3, 4]),
            ("Ga=d,d=y,y=3", vec![1, 3, 4]),
            ("Ga=d,d=z,z=7", vec![1, 4]),
            ("Ga=d,d=p,x=0,y=1", vec![1, 2, 3, 4]),
            ("Ga=d,d=p,x=4294967295,y=2", vec![1, 2, 3, 4]),
        ] {
            let mut state = viewport_state();
            state
                .delete_kitty_in_viewport(&cmd(request.as_bytes()).control, viewport(), locate, &[])
                .unwrap();
            assert_eq!(
                state
                    .placements()
                    .iter()
                    .map(|p| p.image_id)
                    .collect::<Vec<_>>(),
                remaining,
                "{request}"
            );
            assert_eq!(
                state.resources().entries().len(),
                4,
                "lowercase preserves IDs for redisplay"
            );
            assert_eq!(
                GraphicsState::decode(&state.encode().unwrap()).unwrap(),
                state
            );
        }
    }
    #[test]
    fn uppercase_viewport_delete_keeps_other_placements_and_frozen_resources() {
        let mut state = viewport_state();
        state
            .apply_kitty(
                cmd(b"Ga=p,i=1,C=1"),
                Anchor {
                    page: 7,
                    logical_line: 0,
                    cell_offset: 0,
                },
                host(),
                &[],
            )
            .unwrap();
        let frozen = state.clone();
        state
            .delete_kitty_in_viewport(&cmd(b"Ga=d,d=A").control, viewport(), locate, &[])
            .unwrap();
        assert_eq!(
            state
                .placements()
                .iter()
                .map(|p| p.image_id)
                .collect::<Vec<_>>(),
            vec![3, 4, 1]
        );
        assert_eq!(state.resources().entries().len(), 3);
        assert_eq!(frozen.resources().entries().len(), 4);
        state
            .apply_kitty(cmd(b"Ga=p,i=1,C=1"), anchor(), host(), &[])
            .unwrap();
        assert!(
            state
                .apply_kitty(cmd(b"Ga=p,i=2,C=1"), anchor(), host(), &[])
                .is_err()
        );
    }
    #[test]
    fn unresolved_viewport_anchor_or_gc_error_leaves_deletion_atomic() {
        let mut state = viewport_state();
        let before = state.clone();
        let control = cmd(b"Ga=d,d=A").control;
        assert!(
            state
                .delete_kitty_in_viewport(
                    &control,
                    viewport(),
                    |a| if a.logical_line == 6 { None } else { locate(a) },
                    &[]
                )
                .is_err()
        );
        assert_eq!(state, before);
        assert!(
            state
                .delete_kitty_in_viewport(&control, viewport(), locate, &[u64::MAX])
                .is_err()
        );
        assert_eq!(state, before);
    }
    #[test]
    fn quota_evicts_oldest_unplaced_upload_without_invalidating_frozen_pixels() {
        let mut state = GraphicsState::default();
        for id in 1..=MAX_IMAGE_RESOURCES {
            let request = format!("Gs=1,v=1,i={id};AQIDBA==");
            state
                .apply_kitty(cmd(request.as_bytes()), anchor(), host(), &[])
                .unwrap();
        }
        state
            .apply_kitty(cmd(b"Ga=p,i=1,C=1"), anchor(), host(), &[])
            .unwrap();
        let frozen = state.clone();
        state
            .apply_kitty(cmd(b"Gs=1,v=1,i=257;BQYHCA=="), anchor(), host(), &[])
            .unwrap();
        assert_eq!(state.resources().entries().len(), MAX_IMAGE_RESOURCES);
        assert!(state.find_image(&cmd(b"Ga=p,i=1").control).is_some());
        assert!(state.find_image(&cmd(b"Ga=p,i=2").control).is_none());
        assert_eq!(
            frozen.resources().get(2).unwrap().image().pixels(),
            &[1, 2, 3, 4]
        );
        assert_eq!(
            GraphicsState::decode(&state.encode().unwrap()).unwrap(),
            state
        );
        for id in 3..=257 {
            let request = format!("Ga=p,i={id},C=1");
            state
                .apply_kitty(cmd(request.as_bytes()), anchor(), host(), &[])
                .unwrap();
        }
        let before = state.clone();
        assert!(
            state
                .apply_kitty(cmd(b"Gs=1,v=1,i=258;AQIDBA=="), anchor(), host(), &[])
                .is_err()
        );
        assert_eq!(state, before);
    }
    #[test]
    fn sixel_initial_palette_is_vt340_and_reset_restores_it_without_mutating_capture() {
        let mut state = GraphicsState::default();
        assert!(state.sixel_scrolling());
        state.set_sixel_scrolling(false);
        assert!(
            !GraphicsState::decode(&state.encode().unwrap())
                .unwrap()
                .sixel_scrolling()
        );
        assert_eq!(state.palette()[1], [51, 51, 204]);
        assert_eq!(state.palette()[2], [204, 33, 33]);
        assert_eq!(state.palette()[15], [204, 204, 204]);
        assert!(state.palette()[16..].iter().all(|rgb| *rgb == [0; 3]));
        let decoded = crate::sixel::decode(&[0, 1], b"#1~", state.palette(), [0; 3]).unwrap();
        assert_eq!(
            &decoded.image.as_ref().unwrap().pixels()[..4],
            &[51, 51, 204, 255]
        );
        state.apply_sixel(decoded, anchor(), host(), &[]).unwrap();
        let decoded = crate::sixel::decode(&[], b"#1;2;100;0;0", state.palette(), [0; 3]).unwrap();
        state.apply_sixel(decoded, anchor(), host(), &[]).unwrap();
        assert_eq!(state.palette()[1], [255, 0, 0]);
        let frozen = state.clone();
        state.reset(&[]).unwrap();
        assert!(state.sixel_scrolling());
        assert!(!frozen.sixel_scrolling());
        assert_eq!(state.palette()[1], [51, 51, 204]);
        assert_eq!(frozen.palette()[1], [255, 0, 0]);
    }
    #[test]
    fn transmit_place_replace_and_frozen_state_have_distinct_lifecycles() {
        let mut state = GraphicsState::default();
        state
            .apply_kitty(cmd(b"Gs=1,v=1,i=9;AQIDBA=="), anchor(), host(), &[])
            .unwrap();
        assert!(state.placements().is_empty());
        let result = state
            .apply_kitty(cmd(b"Ga=p,i=9,p=3,C=1"), anchor(), host(), &[])
            .unwrap();
        assert_eq!((result.cursor_columns, result.cursor_rows), (0, 0));
        let frozen = state.clone();
        let old = state.placements()[0].resource;
        state
            .apply_kitty(
                cmd(b"Ga=p,i=9,p=3"),
                Anchor {
                    cell_offset: 5,
                    ..anchor()
                },
                host(),
                &[],
            )
            .unwrap();
        assert_eq!(state.placements().len(), 1);
        assert_ne!(frozen.placements()[0].id, state.placements()[0].id);
        state
            .apply_kitty(cmd(b"Gs=1,v=1,i=9;BQYHCA=="), anchor(), host(), &[])
            .unwrap();
        assert!(state.placements().is_empty());
        assert!(state.resources().get(old).is_none());
        assert_eq!(
            frozen.resources().get(old).unwrap().image().pixels(),
            &[1, 2, 3, 4]
        );
        assert_eq!(state.resources().entries().len(), 1);
    }
    #[test]
    fn image_numbers_allocate_ids_and_query_never_changes_state() {
        let mut state = GraphicsState::default();
        let baseline = state.clone();
        state
            .apply_kitty(cmd(b"Ga=q,s=1,v=1,i=77;AQIDBA=="), anchor(), host(), &[])
            .unwrap();
        assert_eq!(state, baseline);
        let first = state
            .apply_kitty(cmd(b"Ga=T,s=1,v=1,I=8;AQIDBA=="), anchor(), host(), &[])
            .unwrap();
        assert_eq!(
            response(&first.control, true, "OK").unwrap(),
            b"\x1b_Gi=1,I=8;OK\x1b\\"
        );
        let second = state
            .apply_kitty(cmd(b"Ga=T,s=1,v=1,I=8;BQYHCA=="), anchor(), host(), &[])
            .unwrap();
        assert_ne!(first.control.number(b'i'), second.control.number(b'i'));
        let newest = state
            .apply_kitty(cmd(b"Ga=p,I=8"), anchor(), host(), &[])
            .unwrap();
        assert_eq!(newest.control.number(b'i'), second.control.number(b'i'));
    }
    #[test]
    fn late_layout_failure_rolls_back_replacement_and_resource_ids() {
        let mut state = GraphicsState::default();
        state
            .apply_kitty(cmd(b"Ga=T,s=1,v=1,i=9;AQIDBA=="), anchor(), host(), &[])
            .unwrap();
        let before = state.clone();
        assert!(
            state
                .apply_kitty(
                    cmd(b"Ga=T,s=1,v=1,i=9,X=999;BQYHCA=="),
                    anchor(),
                    host(),
                    &[]
                )
                .is_err()
        );
        assert_eq!(state, before);
        assert!(
            state
                .apply_kitty(
                    cmd(b"Ga=p,i=9"),
                    Anchor {
                        page: 0,
                        ..anchor()
                    },
                    host(),
                    &[]
                )
                .is_err()
        );
        assert_eq!(state, before);
    }
    #[test]
    fn uppercase_delete_frees_only_unreferenced_data_and_lowercase_allows_redisplay() {
        let mut state = GraphicsState::default();
        state
            .apply_kitty(cmd(b"Ga=T,s=1,v=1,i=9;AQIDBA=="), anchor(), host(), &[])
            .unwrap();
        let resource = state.placements()[0].resource;
        state
            .delete_kitty_identity(&Control::parse(b"Ga=d,d=i,i=9").unwrap().0, &[])
            .unwrap();
        assert!(state.placements().is_empty());
        assert!(state.resources().get(resource).is_some());
        state
            .apply_kitty(cmd(b"Ga=p,i=9"), anchor(), host(), &[])
            .unwrap();
        state
            .delete_kitty_identity(&Control::parse(b"Ga=d,d=I,i=9").unwrap().0, &[resource])
            .unwrap();
        assert!(state.resources().get(resource).is_some());
        state.collect_unused(&[]).unwrap();
        assert!(state.resources().get(resource).is_none());
    }
    #[test]
    fn drcs_replacements_collect_retired_atlases_but_retain_live_cells() {
        let mut state = GraphicsState::default();
        for _ in 0..300 {
            state
                .install_font(
                    crate::drcs::decode(&[1, 1, 0, 5, 1, 2, 8, 0], b" @@").unwrap(),
                    &[],
                )
                .unwrap();
        }
        assert_eq!(state.resources().entries().len(), 1);
        let old = state.fonts().referenced_resources().next().unwrap();
        state
            .install_font(
                crate::drcs::decode(&[1, 1, 0, 5, 1, 2, 8, 0], b" @B").unwrap(),
                &[old],
            )
            .unwrap();
        assert_eq!(state.resources().entries().len(), 2);
        state.collect_unused(&[]).unwrap();
        assert_eq!(state.resources().entries().len(), 1);
    }
    #[test]
    fn removing_anonymous_placements_and_reset_collect_resources_without_reusing_ids() {
        let mut state = GraphicsState::default();
        state
            .apply_kitty(cmd(b"Ga=T,s=1,v=1;AQIDBA=="), anchor(), host(), &[])
            .unwrap();
        let old_resource = state.placements()[0].resource;
        let old_id = state.placements()[0].id;
        let frozen = state.clone();
        assert!(state.retain_placements(|_| false, &[999]).is_err());
        assert_eq!(state, frozen);
        state.retain_placements(|_| false, &[]).unwrap();
        assert!(state.resources().entries().is_empty());
        state
            .apply_kitty(cmd(b"Ga=T,s=1,v=1;AQIDBA=="), anchor(), host(), &[])
            .unwrap();
        assert!(state.placements()[0].resource > old_resource);
        assert!(state.placements()[0].id > old_id);
        state.reset(&[]).unwrap();
        assert!(state.placements().is_empty());
        assert!(state.resources().entries().is_empty());
        assert_eq!(
            frozen
                .resources()
                .get(old_resource)
                .unwrap()
                .image()
                .pixels(),
            &[1, 2, 3, 4]
        );
        assert_eq!(
            GraphicsState::decode(&state.encode().unwrap()).unwrap(),
            state
        );
    }
}
