//! Placement geometry follows physical screen mutations without resampling immutable pixels.
use crate::{
    Terminal, TerminalError,
    graphics_layout::MAX_PLACEMENT_CELLS,
    graphics_state::{Anchor, GraphicClip, GraphicProtocol, MAX_PLACEMENTS, Placement},
    screen::{Screen, ScrollCause},
    terminal_graphics::GraphicsAnchorIndex,
};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct GraphicAnchorCache {
    generation: u64,
    columns: u32,
    rows: u32,
    stored_rows: u32,
    index: Arc<GraphicsAnchorIndex>,
}
impl std::fmt::Debug for GraphicAnchorCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphicAnchorCache")
            .field("generation", &self.generation)
            .field("stored_rows", &self.stored_rows)
            .finish_non_exhaustive()
    }
}
impl GraphicAnchorCache {
    fn matches(&self, screen: &Screen) -> bool {
        self.generation == screen.graphic_anchor_generation
            && self.columns == screen.columns
            && self.rows == screen.rows
            && self.stored_rows == screen.stored_rows()
    }
    fn load(screen: &mut Screen) -> Result<Arc<GraphicsAnchorIndex>, TerminalError> {
        if let Some(cache) = &screen.graphic_anchor_cache
            && cache.matches(screen)
        {
            return Ok(Arc::clone(&cache.index));
        }
        let index = motion_index(screen)?;
        screen.graphic_anchor_cache = Some(Self {
            generation: screen.graphic_anchor_generation,
            columns: screen.columns,
            rows: screen.rows,
            stored_rows: screen.stored_rows(),
            index: Arc::clone(&index),
        });
        Ok(index)
    }
}

#[cfg(test)]
std::thread_local! {
    static MOTION_INDEX_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static FORCE_SLOW_MOTION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn motion_index(screen: &Screen) -> Result<Arc<GraphicsAnchorIndex>, TerminalError> {
    #[cfg(test)]
    MOTION_INDEX_BUILDS.with(|count| count.set(count.get() + 1));
    Ok(Arc::new(GraphicsAnchorIndex::new(screen)?))
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum GraphicMotion {
    Preserve,
    Observed,
    Scroll {
        top: u32,
        bottom: u32,
        left: u32,
        right: u32,
        delta: i32,
    },
    EraseSixel {
        top: u32,
        bottom: u32,
        left: u32,
        right: u32,
    },
    ClearHistory,
    Reflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Rectangle {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
}
impl Rectangle {
    fn intersection(self, other: Self) -> Option<Self> {
        let result = Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        };
        (result.left < result.right && result.top < result.bottom).then_some(result)
    }
}

#[derive(Clone, Copy, Debug)]
struct Positioned {
    x: i64,
    y: i64,
    clip: GraphicClip,
}
impl Positioned {
    fn bounds(self) -> Rectangle {
        Rectangle {
            left: self.x + i64::from(self.clip.left),
            top: self.y + i64::from(self.clip.top),
            right: self.x + i64::from(self.clip.right),
            bottom: self.y + i64::from(self.clip.bottom),
        }
    }
    fn crop(self, bounds: Rectangle) -> Option<Self> {
        let intersection = self.bounds().intersection(bounds)?;
        Some(Self {
            clip: GraphicClip {
                left: (intersection.left - self.x) as u32,
                top: (intersection.top - self.y) as u32,
                right: (intersection.right - self.x) as u32,
                bottom: (intersection.bottom - self.y) as u32,
            },
            ..self
        })
    }
    fn outside(self, area: Rectangle) -> Vec<Self> {
        let bounds = self.bounds();
        let Some(inside) = bounds.intersection(area) else {
            return vec![self];
        };
        // Four disjoint strips retain the content outside an erased or scrolling rectangle.
        [
            Rectangle {
                bottom: inside.top,
                ..bounds
            },
            Rectangle {
                top: inside.bottom,
                ..bounds
            },
            Rectangle {
                top: inside.top,
                bottom: inside.bottom,
                right: inside.left,
                ..bounds
            },
            Rectangle {
                top: inside.top,
                bottom: inside.bottom,
                left: inside.right,
                ..bounds
            },
        ]
        .into_iter()
        .filter_map(|rect| self.crop(rect))
        .collect()
    }
    fn scroll(self, area: Rectangle, delta: i32) -> Vec<Self> {
        let mut pieces = self.outside(area);
        let Some(mut moved) = self.crop(area) else {
            return pieces;
        };
        moved.y += i64::from(delta);
        if let Some(moved) = moved.crop(area) {
            pieces.push(moved);
        }
        pieces
    }
    fn at_anchor(
        self,
        original: &Placement,
        anchor: Anchor,
        index: &GraphicsAnchorIndex,
    ) -> Option<Placement> {
        let (row, column) = index.locate(anchor.logical_line, anchor.cell_offset)?;
        let origin_x = self.x - i64::from(column);
        let origin_y = self.y - i64::from(row);
        if origin_x.unsigned_abs() > u64::from(MAX_PLACEMENT_CELLS)
            || origin_y.unsigned_abs() > u64::from(MAX_PLACEMENT_CELLS)
        {
            return None;
        }
        let mut placement = original.clone();
        placement.anchor = anchor;
        placement.origin_x = origin_x as i32;
        placement.origin_y = origin_y as i32;
        placement.clip = self.clip;
        Some(placement)
    }
    fn anchor(
        self,
        original: &Placement,
        screen: &Screen,
        index: &GraphicsAnchorIndex,
        preserve_anchor: bool,
    ) -> Result<Placement, TerminalError> {
        if preserve_anchor && let Some(placement) = self.at_anchor(original, original.anchor, index)
        {
            return Ok(placement);
        }
        let bounds = self
            .bounds()
            .intersection(Rectangle {
                left: 0,
                top: 0,
                right: i64::from(screen.columns),
                bottom: i64::from(screen.stored_rows()),
            })
            .ok_or(TerminalError::MalformedSnapshot)?;
        for row in bounds.top as usize..bounds.bottom as usize {
            let metadata = screen.row_metadata[row];
            let rendition = screen.line_renditions[row];
            let scale = if rendition == crate::LineRendition::SingleWidth {
                1
            } else {
                2
            };
            let column = (bounds.left as u32 / scale).min(rendition.columns(screen.columns) - 1);
            let anchor = Anchor {
                page: original.anchor.page,
                logical_line: metadata.logical_line_id,
                cell_offset: metadata.logical_cell_offset + column,
            };
            if let Some(placement) = self.at_anchor(original, anchor, index) {
                return Ok(placement);
            }
        }
        Err(TerminalError::MalformedSnapshot)
    }
}

impl Terminal {
    fn graphic_page_screen(&self, page: u8) -> Result<&Screen, TerminalError> {
        match page {
            1 => Some(&self.main),
            7 => Some(&self.alternate),
            2..=6 => self.pages.extra[usize::from(page - 2)].as_ref(),
            _ => None,
        }
        .ok_or(TerminalError::MalformedSnapshot)
    }
    fn graphic_page_screen_mut(&mut self, page: u8) -> Result<&mut Screen, TerminalError> {
        match page {
            1 => Some(&mut self.main),
            7 => Some(&mut self.alternate),
            2..=6 => self.pages.extra[usize::from(page - 2)].as_mut(),
            _ => None,
        }
        .ok_or(TerminalError::MalformedSnapshot)
    }
    /// Runs inside the terminal event candidate; any error rejects that whole transaction.
    pub(crate) fn with_graphics_motion(
        &mut self,
        motion: GraphicMotion,
        operation: impl FnOnce(&mut Screen) -> Result<(), TerminalError>,
    ) -> Result<(), TerminalError> {
        let page = if self.active_alternate {
            7
        } else {
            self.active_page()
        };
        self.with_page_graphics_motion(page, motion, operation)
    }

    pub(crate) fn with_page_graphics_motion(
        &mut self,
        page: u8,
        motion: GraphicMotion,
        operation: impl FnOnce(&mut Screen) -> Result<(), TerminalError>,
    ) -> Result<(), TerminalError> {
        let Some(graphics) = &self.graphics else {
            return operation(self.graphic_page_screen_mut(page)?);
        };
        let count = graphics
            .placements()
            .iter()
            .filter(|p| p.anchor.page == page)
            .count();
        if count == 0 {
            return operation(self.graphic_page_screen_mut(page)?);
        }
        let use_cache = matches!(motion, GraphicMotion::Observed);
        #[cfg(test)]
        let use_cache = use_cache && !FORCE_SLOW_MOTION.with(|flag| flag.get());
        let screen = self.graphic_page_screen_mut(page)?;
        let old_index = if use_cache {
            GraphicAnchorCache::load(screen)?
        } else {
            screen.graphic_anchor_cache = None;
            motion_index(screen)?
        };
        let screen = self.graphic_page_screen(page)?;
        let old_top = screen.viewport_top();
        let old_stored = screen.stored_rows();
        let old_generation = screen.scrollback_generation_rows();
        let old_storage = Rectangle {
            left: 0,
            top: 0,
            right: i64::from(screen.columns),
            bottom: i64::from(old_stored),
        };
        let graphics = self.graphics.as_ref().expect("graphics profile");
        let mut positions = Vec::new();
        positions
            .try_reserve_exact(count)
            .map_err(|_| TerminalError::LimitExceeded)?;
        let mut reflow_anchors = Vec::new();
        let mut reflow_rows = Vec::new();
        if matches!(motion, GraphicMotion::Reflow) {
            reflow_anchors
                .try_reserve_exact(count)
                .map_err(|_| TerminalError::LimitExceeded)?;
            reflow_rows
                .try_reserve_exact(screen.row_metadata.len())
                .map_err(|_| TerminalError::LimitExceeded)?;
            for (metadata, rendition) in screen.row_metadata.iter().zip(&screen.line_renditions) {
                let scale = if *rendition == crate::LineRendition::SingleWidth {
                    1
                } else {
                    2
                };
                reflow_rows.push((
                    Anchor {
                        page,
                        logical_line: metadata.logical_line_id,
                        cell_offset: metadata.logical_cell_offset,
                    },
                    scale,
                    rendition.columns(screen.columns),
                ));
            }
        }
        for p in graphics
            .placements()
            .iter()
            .filter(|p| p.anchor.page == page)
        {
            let (row, column) = old_index
                .locate(p.anchor.logical_line, p.anchor.cell_offset)
                .ok_or(TerminalError::MalformedSnapshot)?;
            positions.push(Positioned {
                x: i64::from(column) + i64::from(p.origin_x),
                y: i64::from(row) + i64::from(p.origin_y),
                clip: p.clip,
            });
            if matches!(motion, GraphicMotion::Reflow) {
                reflow_anchors.push((p.anchor.logical_line, p.anchor.cell_offset));
            }
        }
        let cached_observation = use_cache.then(|| Arc::clone(&old_index));
        let old_index =
            matches!(motion, GraphicMotion::Reflow | GraphicMotion::Preserve).then_some(old_index);
        if matches!(motion, GraphicMotion::Reflow) {
            self.graphic_page_screen_mut(page)?.graphic_reflow_anchors = reflow_anchors;
        }
        if matches!(motion, GraphicMotion::Observed) {
            self.graphic_page_screen_mut(page)?.graphic_scrolls = Some(Vec::new());
        }
        let operation_result = operation(self.graphic_page_screen_mut(page)?);
        if matches!(motion, GraphicMotion::Reflow) {
            self.graphic_page_screen_mut(page)?
                .graphic_reflow_anchors
                .clear();
        }
        let scrolls = if matches!(motion, GraphicMotion::Observed) {
            self.graphic_page_screen_mut(page)?
                .graphic_scrolls
                .take()
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        operation_result?;
        let screen = self.graphic_page_screen(page)?;
        let unchanged_geometry = scrolls.is_empty()
            && cached_observation.as_ref().is_some_and(|before| {
                screen
                    .graphic_anchor_cache
                    .as_ref()
                    .is_some_and(|cache| cache.matches(screen) && Arc::ptr_eq(before, &cache.index))
            });
        if unchanged_geometry
            && positions.iter().all(|position| {
                let bounds = position.bounds();
                bounds.intersection(old_storage) == Some(bounds)
            })
        {
            // Event finalization performs resource GC; unchanged geometry needs no placement work.
            return Ok(());
        }
        let index = if use_cache {
            let screen = self.graphic_page_screen_mut(page)?;
            if !scrolls.is_empty() {
                screen.graphic_anchor_cache = None;
            }
            GraphicAnchorCache::load(screen)?
        } else {
            motion_index(self.graphic_page_screen(page)?)?
        };
        let screen = self.graphic_page_screen(page)?;
        let storage = Rectangle {
            left: 0,
            top: 0,
            right: i64::from(screen.columns),
            bottom: i64::from(screen.stored_rows()),
        };
        let generated = screen
            .scrollback_generation_rows()
            .checked_sub(old_generation)
            .ok_or(TerminalError::MalformedSnapshot)?;
        let trim = u64::from(old_stored)
            .checked_add(generated)
            .and_then(|n| n.checked_sub(u64::from(screen.stored_rows())))
            .and_then(|n| i64::try_from(n).ok());
        let graphics = self
            .graphics
            .as_ref()
            .expect("graphics profile survives screen operation");
        let mut mapped = Vec::new();
        mapped
            .try_reserve_exact(count)
            .map_err(|_| TerminalError::LimitExceeded)?;
        let mut resulting_count = graphics.placements().len() - count;
        for (p, mut position) in graphics
            .placements()
            .iter()
            .filter(|p| p.anchor.page == page)
            .zip(positions)
        {
            let mut preferred_anchor = p.anchor;
            let pieces = match motion {
                GraphicMotion::Observed => {
                    let mut pieces = vec![position];
                    for scroll in &scrolls {
                        if scroll.trimmed != 0 {
                            for piece in &mut pieces {
                                piece.y -= i64::from(scroll.trimmed);
                            }
                        } else if scroll.top != scroll.bottom {
                            let area = Rectangle {
                                left: i64::from(scroll.left),
                                right: i64::from(scroll.right),
                                top: i64::from(scroll.top),
                                bottom: i64::from(scroll.bottom),
                            };
                            let mut next = Vec::new();
                            for piece in pieces {
                                let fragments = piece.scroll(area, scroll.delta);
                                if next.len() + fragments.len() > MAX_PLACEMENTS {
                                    return Err(TerminalError::ProtocolResourceLimit(
                                        "terminal graphic placements exceed 4096",
                                    ));
                                }
                                next.try_reserve(fragments.len())
                                    .map_err(|_| TerminalError::LimitExceeded)?;
                                next.extend(fragments);
                            }
                            pieces = next;
                        }
                    }
                    pieces
                }
                GraphicMotion::Reflow => {
                    if let Some((row, column)) =
                        index.locate(p.anchor.logical_line, p.anchor.cell_offset)
                    {
                        position.x = i64::from(column) + i64::from(p.origin_x);
                        position.y = i64::from(row) + i64::from(p.origin_y);
                    } else {
                        // A retained lower source row can survive eviction of the original anchor.
                        let bounds = position.bounds();
                        let mut replacement = None;
                        for (row, &(mut anchor, scale, columns)) in reflow_rows
                            .iter()
                            .enumerate()
                            .take(bounds.bottom.min(i64::from(old_stored)).max(0) as usize)
                            .skip(bounds.top.max(0) as usize)
                        {
                            anchor.cell_offset +=
                                (bounds.left.max(0) as u32 / scale).min(columns - 1);
                            let Some((old_row, old_column)) = old_index
                                .as_ref()
                                .expect("reflow index")
                                .locate(anchor.logical_line, anchor.cell_offset)
                            else {
                                continue;
                            };
                            if old_row as usize != row {
                                continue;
                            }
                            if let Some((new_row, new_column)) =
                                index.locate(anchor.logical_line, anchor.cell_offset)
                            {
                                replacement = Some((
                                    anchor,
                                    i64::from(new_column) - i64::from(old_column),
                                    i64::from(new_row) - i64::from(old_row),
                                ));
                                break;
                            }
                        }
                        let Some((anchor, dx, dy)) = replacement else {
                            mapped.push(Vec::new());
                            continue;
                        };
                        preferred_anchor = anchor;
                        position.x += dx;
                        position.y += dy;
                    }
                    vec![position]
                }
                GraphicMotion::EraseSixel {
                    top,
                    bottom,
                    left,
                    right,
                } => {
                    if p.protocol == GraphicProtocol::Sixel {
                        position.outside(Rectangle {
                            left: i64::from(left),
                            right: i64::from(right),
                            top: i64::from(old_top) + i64::from(top),
                            bottom: i64::from(old_top) + i64::from(bottom),
                        })
                    } else {
                        vec![position]
                    }
                }
                _ if generated != 0 => {
                    position.y -= trim.ok_or(TerminalError::MalformedSnapshot)?;
                    vec![position]
                }
                GraphicMotion::ClearHistory => {
                    position.y -= i64::from(old_top);
                    vec![position]
                }
                GraphicMotion::Scroll {
                    top,
                    bottom,
                    left,
                    right,
                    delta,
                } => {
                    let area = Rectangle {
                        left: i64::from(left),
                        right: i64::from(right),
                        top: i64::from(old_top) + i64::from(top),
                        bottom: i64::from(old_top) + i64::from(bottom),
                    };
                    position.scroll(area, delta)
                }
                GraphicMotion::Preserve => vec![position],
            };
            let mut fragments = Vec::new();
            fragments
                .try_reserve_exact(pieces.len())
                .map_err(|_| TerminalError::LimitExceeded)?;
            let preserve_kitty = matches!(motion, GraphicMotion::EraseSixel { .. })
                && p.protocol == GraphicProtocol::Kitty;
            // A fixed-position edit can extend a preceding wrapped row across this anchor.
            // Reanchor in that case; keeping an anchor on another row distorts later reflow.
            let preserve_anchor = !matches!(motion, GraphicMotion::Preserve)
                || old_index
                    .as_ref()
                    .expect("preserve index")
                    .locate(p.anchor.logical_line, p.anchor.cell_offset)
                    .map(|(row, _)| row)
                    == index
                        .locate(p.anchor.logical_line, p.anchor.cell_offset)
                        .map(|(row, _)| row);
            for piece in pieces.into_iter().filter_map(|p| {
                if preserve_kitty {
                    Some(p)
                } else {
                    p.crop(storage)
                }
            }) {
                if resulting_count == MAX_PLACEMENTS {
                    return Err(TerminalError::ProtocolResourceLimit(
                        "terminal graphic placements exceed 4096",
                    ));
                }
                let mut original = p.clone();
                original.anchor = preferred_anchor;
                let fragment = piece.anchor(&original, screen, &index, preserve_anchor)?;
                fragments.push(fragment);
                resulting_count += 1;
            }
            mapped.push(fragments);
        }
        let cell_resources = self.cell_graphic_resources();
        let mut mapped = mapped.into_iter();
        self.graphics
            .as_mut()
            .expect("graphics profile")
            .remap_placements(
                page,
                |_| {
                    Ok(mapped
                        .next()
                        .expect("one mapping per original page placement"))
                },
                &cell_resources,
            )
    }

    pub(crate) fn graphics_scroll_up(
        &mut self,
        count: u32,
        top: u32,
        bottom: u32,
        left: u32,
        right: u32,
    ) -> Result<(), TerminalError> {
        if count == 0 {
            return Ok(());
        }
        if top >= bottom
            || bottom > self.active_screen().rows
            || left >= right
            || right > self.active_screen().columns
            || count > bottom - top
            || count > u32::from(u16::MAX)
        {
            return Err(TerminalError::MalformedSnapshot);
        }
        let revision = self
            .next_source_sequence
            .checked_add(1)
            .ok_or(TerminalError::LimitExceeded)?;
        self.with_graphics_motion(
            GraphicMotion::Scroll {
                top,
                bottom,
                left,
                right,
                delta: -(count as i32),
            },
            |screen| {
                screen.scroll_up_rectangle_with_cause(
                    count as u16,
                    top,
                    bottom,
                    left,
                    right,
                    ScrollCause::Output(revision),
                );
                Ok(())
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        HostGeometryV1, OutputProfile, TerminalOptions, WidthProfile,
        kitty_graphics::TransferDecoder,
    };

    fn force_slow<R>(operation: impl FnOnce() -> R) -> R {
        struct Restore(bool);
        impl Drop for Restore {
            fn drop(&mut self) {
                FORCE_SLOW_MOTION.with(|flag| flag.set(self.0));
            }
        }
        let _restore = Restore(FORCE_SLOW_MOTION.with(|flag| flag.replace(true)));
        operation()
    }

    fn output(terminal: &mut Terminal, bytes: &[u8]) {
        terminal
            .apply_event(crate::Event::Output {
                source_sequence: terminal.next_source_sequence(),
                bytes,
            })
            .unwrap();
    }

    fn terminal(history: u32) -> Terminal {
        let mut terminal = Terminal::new_with_graphics_profile(
            6,
            4,
            0,
            TerminalOptions {
                history_capacity_rows: history,
            },
            OutputProfile::Utf8,
            WidthProfile::Unicode16,
        )
        .unwrap();
        terminal.host_geometry = HostGeometryV1 {
            cell_width: 10,
            cell_height: 20,
            ..Default::default()
        };
        terminal
    }
    fn place(terminal: &mut Terminal, row: usize, column: u32, columns: u32, rows: u32) {
        place_on(terminal, 1, row, column, columns, rows);
    }
    fn place_on(
        terminal: &mut Terminal,
        page: u8,
        row: usize,
        column: u32,
        columns: u32,
        rows: u32,
    ) {
        let metadata = terminal.graphic_page_screen(page).unwrap().row_metadata[row];
        let request = format!("Ga=T,s=1,v=1,i={page},p=1,c={columns},r={rows},C=1;AQIDBA==");
        let command = TransferDecoder::default()
            .push(request.as_bytes())
            .unwrap()
            .unwrap();
        terminal
            .graphics
            .as_mut()
            .unwrap()
            .apply_kitty(
                command,
                Anchor {
                    page,
                    logical_line: metadata.logical_line_id,
                    cell_offset: metadata.logical_cell_offset + column,
                },
                terminal.host_geometry,
                &[],
            )
            .unwrap();
    }
    fn pixels_in_cells(terminal: &Terminal) -> Vec<(i64, i64, u32, u32)> {
        let index = GraphicsAnchorIndex::new(terminal.active_screen()).unwrap();
        let mut cells = Vec::new();
        for p in terminal.graphics.as_ref().unwrap().placements() {
            let (row, column) = index
                .locate(p.anchor.logical_line, p.anchor.cell_offset)
                .unwrap();
            for y in p.clip.top..p.clip.bottom {
                for x in p.clip.left..p.clip.right {
                    cells.push((
                        i64::from(column) + i64::from(p.origin_x) + i64::from(x),
                        i64::from(row) + i64::from(p.origin_y) + i64::from(y),
                        x,
                        y,
                    ));
                }
            }
        }
        cells.sort_unstable();
        cells
    }
    #[test]
    fn signed_origin_crops_to_storage_and_survives_row_metadata_replacement() {
        let mut terminal = terminal(0);
        place(&mut terminal, 1, 0, 4, 4);
        terminal
            .graphics
            .as_mut()
            .unwrap()
            .remap_placements(
                1,
                |p| {
                    let mut p = p.clone();
                    p.origin_x = -2;
                    p.origin_y = -2;
                    Ok(vec![p])
                },
                &[],
            )
            .unwrap();
        let old_anchor = terminal.graphics.as_ref().unwrap().placements()[0].anchor;
        terminal
            .with_graphics_motion(GraphicMotion::Preserve, |screen| {
                screen.row_metadata[1].logical_line_id = screen.next_logical_line_id;
                screen.next_logical_line_id += 1;
                screen.set_line_rendition(crate::LineRendition::DoubleWidth)
            })
            .unwrap();
        let placement = &terminal.graphics.as_ref().unwrap().placements()[0];
        assert_ne!(placement.anchor, old_anchor);
        assert_eq!((placement.origin_x, placement.origin_y), (-2, -1));
        assert_eq!(
            placement.clip,
            GraphicClip {
                left: 2,
                top: 1,
                right: 4,
                bottom: 4
            }
        );
        assert_eq!(pixels_in_cells(&terminal).len(), 6);
        terminal.validate_graphics_anchors().unwrap();
        assert_eq!(
            crate::render_frame_v5::Frame::decode(&terminal.export_render_frame_v5(0).unwrap())
                .unwrap()
                .images
                .len(),
            1
        );
    }
    #[test]
    fn head_eviction_keeps_lower_pixels_and_ed3_does_not_change_frozen_history() {
        let mut terminal = terminal(1);
        place(&mut terminal, 0, 0, 3, 4);
        let original = terminal.graphics.as_ref().unwrap().placements()[0].clone();
        terminal.graphics_scroll_up(1, 0, 4, 0, 6).unwrap();
        assert_eq!(terminal.main.viewport_top(), 1);
        let capture = crate::HistoryCapture::capture(&terminal).unwrap();
        let frozen = capture.viewport_v5(1).unwrap().0;
        terminal.graphics_scroll_up(2, 0, 4, 0, 6).unwrap();
        let placement = &terminal.graphics.as_ref().unwrap().placements()[0];
        assert_eq!(
            (
                placement.origin_y,
                placement.clip.top,
                placement.clip.bottom
            ),
            (-2, 2, 4)
        );
        assert_eq!(placement.resource, original.resource);
        assert_eq!(placement.layout, original.layout);
        assert_eq!(pixels_in_cells(&terminal).len(), 6);
        terminal
            .with_graphics_motion(GraphicMotion::ClearHistory, |screen| {
                screen.erase_display(3)
            })
            .unwrap();
        assert_eq!(terminal.main.viewport_top(), 0);
        let placement = &terminal.graphics.as_ref().unwrap().placements()[0];
        assert_eq!(
            (
                placement.origin_y,
                placement.clip.top,
                placement.clip.bottom
            ),
            (-3, 3, 4)
        );
        assert_eq!(pixels_in_cells(&terminal).len(), 3);
        terminal.validate_graphics_anchors().unwrap();
        assert_eq!(capture.viewport_v5(1).unwrap().0, frozen);
        assert_eq!(
            Terminal::import_snapshot_v2(&terminal.export_snapshot_v2().unwrap()).unwrap(),
            terminal
        );
    }
    #[test]
    fn partial_scroll_preserves_outside_pixels_and_discards_pixels_crossing_margins() {
        let mut terminal = terminal(0);
        place(&mut terminal, 0, 0, 6, 4);
        let original = pixels_in_cells(&terminal);
        terminal.graphics_scroll_up(1, 1, 3, 1, 5).unwrap();
        let mut expected: Vec<_> = original
            .iter()
            .filter_map(|&(x, y, sx, sy)| {
                if (1..5).contains(&x) && (1..3).contains(&y) {
                    (y == 2).then_some((x, y - 1, sx, sy))
                } else {
                    Some((x, y, sx, sy))
                }
            })
            .collect();
        expected.sort_unstable();
        assert_eq!(pixels_in_cells(&terminal), expected);
        assert_eq!(terminal.graphics.as_ref().unwrap().placements().len(), 5);
        terminal
            .with_graphics_motion(
                GraphicMotion::Scroll {
                    top: 1,
                    bottom: 3,
                    left: 1,
                    right: 5,
                    delta: 1,
                },
                |screen| {
                    screen.scroll_down_rectangle(1, 1, 3, 1, 5);
                    Ok(())
                },
            )
            .unwrap();
        for cell in &mut expected {
            if (1..5).contains(&cell.0) && cell.1 == 1 {
                cell.1 += 1;
            }
        }
        expected.sort_unstable();
        assert_eq!(pixels_in_cells(&terminal), expected);
        terminal.validate_graphics_anchors().unwrap();
        assert_eq!(
            Terminal::import_snapshot_v2(&terminal.export_snapshot_v2().unwrap()).unwrap(),
            terminal
        );
    }
    #[test]
    fn observed_motion_consumes_actual_scrolls_in_order_and_clears_its_log() {
        let mut terminal = terminal(0);
        place(&mut terminal, 0, 0, 6, 4);
        let mut explicit = terminal.clone();
        explicit.graphics_scroll_up(1, 1, 3, 1, 5).unwrap();
        explicit
            .with_graphics_motion(
                GraphicMotion::Scroll {
                    top: 1,
                    bottom: 3,
                    left: 1,
                    right: 5,
                    delta: 1,
                },
                |screen| {
                    screen.scroll_down_rectangle(1, 1, 3, 1, 5);
                    Ok(())
                },
            )
            .unwrap();
        terminal
            .with_graphics_motion(GraphicMotion::Observed, |screen| {
                screen.scroll_up_rectangle_with_cause(1, 1, 3, 1, 5, ScrollCause::Output(1));
                screen.scroll_down_rectangle(1, 1, 3, 1, 5);
                Ok(())
            })
            .unwrap();
        assert_eq!(pixels_in_cells(&terminal), pixels_in_cells(&explicit));
        assert!(terminal.active_screen().graphic_scrolls.is_none());
        let before = pixels_in_cells(&terminal);
        terminal
            .with_graphics_motion(GraphicMotion::Observed, |_| Ok(()))
            .unwrap();
        assert_eq!(pixels_in_cells(&terminal), before);
        assert!(
            terminal
                .with_graphics_motion(GraphicMotion::Observed, |_| Err(
                    TerminalError::LimitExceeded
                ))
                .is_err()
        );
        assert!(terminal.active_screen().graphic_scrolls.is_none());
    }
    #[test]
    fn observed_history_append_and_trim_keep_the_same_surviving_source_rows() {
        let mut terminal = terminal(1);
        place(&mut terminal, 0, 0, 3, 4);
        terminal
            .with_graphics_motion(GraphicMotion::Observed, |screen| {
                screen.scroll_up_rectangle_with_cause(1, 0, 4, 0, 6, ScrollCause::Output(1));
                screen.scroll_up_rectangle_with_cause(2, 0, 4, 0, 6, ScrollCause::Output(1));
                Ok(())
            })
            .unwrap();
        assert_eq!(
            pixels_in_cells(&terminal),
            vec![
                (0, 0, 0, 2),
                (0, 1, 0, 3),
                (1, 0, 1, 2),
                (1, 1, 1, 3),
                (2, 0, 2, 2),
                (2, 1, 2, 3),
            ]
        );
        terminal.validate_graphics_anchors().unwrap();
    }
    #[test]
    fn reflow_tracks_the_logical_anchor_without_changing_the_allocation_or_origin() {
        let mut terminal = terminal(10);
        for character in "abcdefghij".chars() {
            terminal.main.print(character).unwrap();
        }
        place(&mut terminal, 1, 0, 3, 2);
        let original = terminal.graphics.as_ref().unwrap().placements()[0].clone();
        terminal
            .with_graphics_motion(GraphicMotion::Reflow, |screen| screen.resize_reflow(3, 4))
            .unwrap();
        let placement = &terminal.graphics.as_ref().unwrap().placements()[0];
        assert_eq!(placement.anchor, original.anchor);
        assert_eq!(placement.layout, original.layout);
        assert_eq!(
            (placement.origin_x, placement.origin_y),
            (original.origin_x, original.origin_y)
        );
        assert_eq!(
            GraphicsAnchorIndex::new(&terminal.main)
                .unwrap()
                .locate(placement.anchor.logical_line, placement.anchor.cell_offset),
            Some((2, 0))
        );
        terminal.validate_graphics_anchors().unwrap();
    }
    #[test]
    fn pending_wrap_scrolls_images_only_when_a_print_actually_wraps() {
        let mut terminal = terminal(0);
        place(&mut terminal, 0, 0, 6, 4);
        terminal.main.position_cursor(3, 5);
        terminal.main.print('x').unwrap();
        assert!(terminal.main.wrap_pending);
        let before = pixels_in_cells(&terminal);
        terminal
            .with_graphics_motion(GraphicMotion::Observed, |screen| screen.print('\u{301}'))
            .unwrap();
        assert_eq!(pixels_in_cells(&terminal), before);
        assert!(terminal.main.wrap_pending);
        terminal
            .with_graphics_motion(GraphicMotion::Observed, |screen| screen.print('y'))
            .unwrap();
        let mut expected: Vec<_> = before
            .into_iter()
            .filter_map(|(x, y, sx, sy)| (y > 0).then_some((x, y - 1, sx, sy)))
            .collect();
        expected.sort_unstable();
        assert_eq!(pixels_in_cells(&terminal), expected);
        terminal.validate_graphics_anchors().unwrap();
    }
    #[test]
    fn hidden_page_motion_leaves_active_page_and_main_placements_unchanged() {
        let mut terminal = terminal(0);
        terminal.pages.extra[0] = Some(Screen::new(6, 4));
        place(&mut terminal, 0, 0, 2, 2);
        place_on(&mut terminal, 2, 0, 0, 2, 2);
        let main = terminal.main.clone();
        let main_placement = terminal.graphics.as_ref().unwrap().placements()[0].clone();
        terminal
            .with_page_graphics_motion(2, GraphicMotion::Observed, |screen| {
                screen.scroll_down_rectangle(1, 0, 4, 0, 6);
                Ok(())
            })
            .unwrap();
        assert_eq!(terminal.active_page(), 1);
        assert!(!terminal.active_alternate);
        assert_eq!(terminal.main, main);
        assert_eq!(
            terminal.graphics.as_ref().unwrap().placements()[0],
            main_placement
        );
        let placement = &terminal.graphics.as_ref().unwrap().placements()[1];
        let index = GraphicsAnchorIndex::new(terminal.pages.extra[0].as_ref().unwrap()).unwrap();
        assert_eq!(
            index.locate(placement.anchor.logical_line, placement.anchor.cell_offset),
            Some((1, 0))
        );
        assert_eq!((placement.origin_x, placement.origin_y), (0, 0));
        terminal.validate_graphics_anchors().unwrap();
        for page in [0, 3, 8] {
            assert!(
                terminal
                    .with_page_graphics_motion(page, GraphicMotion::Preserve, |_| panic!(
                        "missing page operation"
                    ))
                    .is_err()
            );
        }
    }
    #[test]
    fn reflow_pins_blank_image_anchor_and_clears_runtime_pins_on_success_and_error() {
        let mut terminal = terminal(10);
        place(&mut terminal, 0, 5, 1, 2);
        let original = terminal.graphics.as_ref().unwrap().placements()[0].anchor;
        terminal
            .with_page_graphics_motion(1, GraphicMotion::Reflow, |screen| {
                screen.resize_reflow(3, 4)
            })
            .unwrap();
        assert_eq!(terminal.graphics.as_ref().unwrap().placements().len(), 1);
        assert_eq!(
            terminal.graphics.as_ref().unwrap().placements()[0].anchor,
            original
        );
        assert!(terminal.main.graphic_reflow_anchors.is_empty());
        assert!(
            GraphicsAnchorIndex::new(&terminal.main)
                .unwrap()
                .locate(original.logical_line, original.cell_offset)
                .is_some()
        );
        assert!(
            terminal
                .with_page_graphics_motion(1, GraphicMotion::Reflow, |screen| {
                    assert_eq!(
                        screen.graphic_reflow_anchors,
                        vec![(original.logical_line, original.cell_offset)]
                    );
                    Err(TerminalError::LimitExceeded)
                })
                .is_err()
        );
        assert!(terminal.main.graphic_reflow_anchors.is_empty());
    }
    #[test]
    fn output_keeps_kitty_column_anchor_for_later_narrower_reflow() {
        let mut terminal = Terminal::new_with_graphics_profile(
            80,
            4,
            0,
            TerminalOptions {
                history_capacity_rows: 10,
            },
            OutputProfile::Utf8,
            WidthProfile::Unicode16,
        )
        .unwrap();
        terminal
            .apply_event(crate::Event::HostGeometry {
                source_sequence: 0,
                geometry: HostGeometryV1 {
                    cell_width: 10,
                    cell_height: 20,
                    ..Default::default()
                },
            })
            .unwrap();
        terminal
            .apply_event(crate::Event::Output {
                source_sequence: terminal.next_source_sequence(),
                bytes: b"\x1b[1;80H\x1b_Ga=T,s=1,v=1,i=1,p=1,c=1,r=1,C=1,q=2;AQIDBA==\x1b\\",
            })
            .unwrap();
        let original = terminal.graphics.as_ref().unwrap().placements()[0].clone();
        assert_eq!(original.anchor.cell_offset, 79);
        terminal
            .apply_event(crate::Event::Output {
                source_sequence: terminal.next_source_sequence(),
                bytes: b"X\r\nY",
            })
            .unwrap();
        assert_eq!(
            terminal.graphics.as_ref().unwrap().placements()[0].anchor,
            original.anchor
        );
        terminal
            .apply_event(crate::Event::Resize {
                source_sequence: terminal.next_source_sequence(),
                columns: 40,
                rows: 4,
            })
            .unwrap();
        let placement = &terminal.graphics.as_ref().unwrap().placements()[0];
        assert_eq!(placement.anchor, original.anchor);
        assert_eq!((placement.origin_x, placement.origin_y), (0, 0));
        assert_eq!(
            GraphicsAnchorIndex::new(&terminal.main)
                .unwrap()
                .locate(placement.anchor.logical_line, placement.anchor.cell_offset),
            Some((1, 39))
        );
        let frame = crate::render_frame_v5::Frame::decode(
            &terminal
                .export_render_frame_v5(terminal.main.viewport_top())
                .unwrap(),
        )
        .unwrap();
        assert_eq!((frame.images[0].column, frame.images[0].row), (39, 1));
    }
    #[test]
    fn sixel_erase_removes_only_inside_pixels_and_preserves_kitty_identity_and_clip() {
        let mut terminal = terminal(0);
        place(&mut terminal, 0, 0, 6, 4);
        let kitty = terminal.graphics.as_ref().unwrap().placements()[0].clone();
        let decoded = crate::sixel::decode(
            &[0, 1],
            b"~",
            terminal.graphics.as_ref().unwrap().palette(),
            [0; 3],
        )
        .unwrap();
        let layout = crate::graphics_layout::kitty(
            &crate::kitty_graphics::Control::parse(b"Ga=p,c=6,r=4")
                .unwrap()
                .0,
            1,
            6,
            terminal.host_geometry,
        )
        .unwrap()
        .unwrap();
        terminal
            .graphics
            .as_mut()
            .unwrap()
            .apply_sixel_layout(decoded, kitty.anchor, Some(layout), (0, 0), None, &[])
            .unwrap();
        let frozen = terminal.graphics.as_ref().unwrap().clone();
        terminal
            .with_graphics_motion(
                GraphicMotion::EraseSixel {
                    top: 1,
                    bottom: 3,
                    left: 1,
                    right: 5,
                },
                |screen| {
                    screen.erase_rectangle(1, 1, 3, 5);
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(terminal.graphics.as_ref().unwrap().placements()[0], kitty);
        assert_eq!(terminal.graphics.as_ref().unwrap().placements().len(), 5);
        let mut only_sixel = terminal.clone();
        only_sixel
            .graphics
            .as_mut()
            .unwrap()
            .retain_placements(|p| p.protocol == GraphicProtocol::Sixel, &[])
            .unwrap();
        let mut expected = Vec::new();
        for x in 0..6 {
            for y in 0..4 {
                if !(1..5).contains(&x) || !(1..3).contains(&y) {
                    expected.push((i64::from(x), i64::from(y), x, y));
                }
            }
        }
        assert_eq!(pixels_in_cells(&only_sixel), expected);
        terminal
            .with_graphics_motion(
                GraphicMotion::EraseSixel {
                    top: 0,
                    bottom: 4,
                    left: 0,
                    right: 6,
                },
                |_| Ok(()),
            )
            .unwrap();
        assert_eq!(terminal.graphics.as_ref().unwrap().placements(), &[kitty]);
        assert_eq!(
            terminal
                .graphics
                .as_ref()
                .unwrap()
                .resources()
                .entries()
                .len(),
            1
        );
        assert_eq!(frozen.resources().entries().len(), 2);
        terminal.validate_graphics_anchors().unwrap();
    }
    #[test]
    fn reflow_head_eviction_reanchors_the_retained_lower_half_at_its_real_column() {
        let mut terminal = terminal(0);
        place(&mut terminal, 0, 2, 2, 4);
        terminal.main.position_cursor(3, 0);
        let original = terminal.graphics.as_ref().unwrap().placements()[0].clone();
        terminal
            .with_graphics_motion(GraphicMotion::Reflow, |screen| screen.resize_reflow(6, 2))
            .unwrap();
        let placement = &terminal.graphics.as_ref().unwrap().placements()[0];
        assert_ne!(placement.anchor, original.anchor);
        assert_eq!(placement.anchor.cell_offset, 2);
        assert_eq!((placement.origin_x, placement.origin_y), (0, -2));
        assert_eq!(
            placement.clip,
            GraphicClip {
                left: 0,
                top: 2,
                right: 2,
                bottom: 4
            }
        );
        assert_eq!(
            pixels_in_cells(&terminal),
            vec![(2, 0, 0, 2), (2, 1, 0, 3), (3, 0, 1, 2), (3, 1, 1, 3)]
        );
        assert_eq!(placement.resource, original.resource);
        assert_eq!(placement.layout, original.layout);
        terminal.validate_graphics_anchors().unwrap();
    }
    #[test]
    fn cached_motion_matches_full_remapping_for_mixed_text_and_geometry_changes() {
        let mut fast = Terminal::new_with_graphics_profile(
            24,
            6,
            0,
            TerminalOptions {
                history_capacity_rows: 64,
            },
            OutputProfile::Utf8,
            WidthProfile::Unicode16,
        )
        .unwrap();
        fast.host_geometry = HostGeometryV1 {
            cell_width: 10,
            cell_height: 20,
            ..Default::default()
        };
        let mut slow = fast.clone();
        let samples = [
            "\x1b[2;3H\x1b_Ga=T,s=1,v=1,i=1,p=1,c=2,r=2,C=1,q=2;AQIDBA==\x1b\\",
            "ascii text a\u{301} b\u{301}\r\nline two\r\nline three",
            "\x1b[2;24Hx\u{301}Y\u{2764}\u{fe0f}",
            "\x1b[2;5r\x1b[?69h\x1b[3;20s\x1b[4;19Habcd\r\nEFGH",
            "\x1b[2S\x1b[1T\x1b[3;3H\x1b[1L\x1b[1M",
            "\x1b[2;3H\x1bM\x1b[?69l\x1b[r\r\nplain again",
            "\x1b[2;1H\x1b#6wide\x1b#5\x1b[0K",
            "\x1b[6;1Ha\r\nb\r\nc\r\nd\r\ne\r\nf\r\ng\r\nh",
            "\x1b[3J\x1b[1;1Hdone",
        ];
        for iteration in 0..16 {
            for sample in samples {
                output(&mut fast, sample.as_bytes());
                force_slow(|| output(&mut slow, sample.as_bytes()));
                assert_eq!(fast, slow, "iteration {iteration}, sample {sample:?}");
                assert_eq!(
                    fast.export_snapshot_v2().unwrap(),
                    slow.export_snapshot_v2().unwrap()
                );
                assert_eq!(
                    fast.export_render_frame_v5(0).unwrap(),
                    slow.export_render_frame_v5(0).unwrap()
                );
            }
            let columns = if iteration % 2 == 0 { 12 } else { 24 };
            fast.apply_event(crate::Event::Resize {
                source_sequence: fast.next_source_sequence(),
                columns,
                rows: 6,
            })
            .unwrap();
            force_slow(|| {
                slow.apply_event(crate::Event::Resize {
                    source_sequence: slow.next_source_sequence(),
                    columns,
                    rows: 6,
                })
                .unwrap()
            });
            assert_eq!(
                fast.export_snapshot_v2().unwrap(),
                slow.export_snapshot_v2().unwrap()
            );
        }
    }
    #[test]
    fn ordinary_text_builds_one_index_instead_of_two_per_character_with_long_history() {
        let mut fast = Terminal::new_with_graphics_profile(
            80,
            4,
            0,
            TerminalOptions {
                history_capacity_rows: 2048,
            },
            OutputProfile::Utf8,
            WidthProfile::Unicode16,
        )
        .unwrap();
        fast.host_geometry = HostGeometryV1 {
            cell_width: 10,
            cell_height: 20,
            ..Default::default()
        };
        for _ in 0..512 {
            fast.main
                .scroll_up_rectangle_with_cause(4, 0, 4, 0, 80, ScrollCause::Output(1));
        }
        assert_eq!(fast.main.viewport_top(), 2048);
        let top = fast.main.viewport_top() as usize;
        place(&mut fast, top, 2, 2, 2);
        let mut slow = fast.clone();
        let before = MOTION_INDEX_BUILDS.with(|count| count.get());
        let started = std::time::Instant::now();
        for _ in 0..128 {
            fast.with_graphics_motion(GraphicMotion::Observed, |screen| {
                screen.position_cursor(0, 0);
                screen.print('x')
            })
            .unwrap();
        }
        let fast_time = started.elapsed();
        let fast_builds = MOTION_INDEX_BUILDS.with(|count| count.get()) - before;
        let before = MOTION_INDEX_BUILDS.with(|count| count.get());
        let started = std::time::Instant::now();
        force_slow(|| {
            for _ in 0..128 {
                slow.with_graphics_motion(GraphicMotion::Observed, |screen| {
                    screen.position_cursor(0, 0);
                    screen.print('x')
                })
                .unwrap();
            }
        });
        let slow_time = started.elapsed();
        let slow_builds = MOTION_INDEX_BUILDS.with(|count| count.get()) - before;
        assert_eq!(fast, slow);
        assert_eq!(fast.main.row_metadata, slow.main.row_metadata);
        assert_eq!((fast_builds, slow_builds), (1, 256));
        eprintln!(
            "graphics motion 2048 history rows / 128 ordinary chars: fast {fast_time:?}, full {slow_time:?}; index builds {fast_builds} vs {slow_builds}"
        );
    }
    #[test]
    fn cached_indices_are_isolated_across_screen_clones_and_generation_wraparound() {
        let mut terminal = terminal(0);
        place(&mut terminal, 1, 2, 2, 2);
        terminal
            .with_graphics_motion(GraphicMotion::Observed, |_| Ok(()))
            .unwrap();
        let mut cloned = terminal.clone();
        let original_index =
            Arc::clone(&terminal.main.graphic_anchor_cache.as_ref().unwrap().index);
        assert!(Arc::ptr_eq(
            &original_index,
            &cloned.main.graphic_anchor_cache.as_ref().unwrap().index
        ));
        cloned.main.graphic_anchor_generation = u64::MAX;
        cloned
            .with_graphics_motion(GraphicMotion::Observed, |_| Ok(()))
            .unwrap();
        cloned
            .with_graphics_motion(GraphicMotion::Observed, |screen| {
                screen.position_cursor(1, 0);
                screen.set_line_rendition(crate::LineRendition::DoubleWidth)
            })
            .unwrap();
        assert_eq!(cloned.main.graphic_anchor_generation, 0);
        assert!(!Arc::ptr_eq(
            &original_index,
            &cloned.main.graphic_anchor_cache.as_ref().unwrap().index
        ));
        assert!(Arc::ptr_eq(
            &original_index,
            &terminal.main.graphic_anchor_cache.as_ref().unwrap().index
        ));
        let before = MOTION_INDEX_BUILDS.with(|count| count.get());
        terminal
            .with_graphics_motion(GraphicMotion::Observed, |screen| {
                screen.row_metadata[0].sealed_revision = 99;
                Ok(())
            })
            .unwrap();
        assert_eq!(MOTION_INDEX_BUILDS.with(|count| count.get()), before);
        terminal.validate_graphics_anchors().unwrap();
        cloned.validate_graphics_anchors().unwrap();
    }
}
