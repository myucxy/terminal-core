//! Strict, allocation-bounded validation for renderer-only history fragments.
use crate::{TerminalError, render_frame};

pub(crate) struct Metrics {
    pub rows: u32,
    pub utf8_bytes: u32,
    pub soft_wrapped: Vec<bool>,
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated field"),
    )
}

pub(crate) fn validate(bytes: &[u8]) -> Result<Metrics, TerminalError> {
    validate_bounded(
        bytes,
        crate::MAX_HISTORY_PAGE_BYTES,
        crate::MAX_HISTORY_PAGE_CELLS as usize,
    )
}

pub(crate) fn validate_full_render_v4(bytes: &[u8]) -> Result<Metrics, TerminalError> {
    validate_bounded(bytes, crate::MAX_RENDER_FRAME_V4_BYTES, crate::MAX_CELLS)
}

/// Only call after full V4 validation, before materializing V5 pixel resources.
pub(crate) fn is_single_cell_glyph(bytes: &[u8], column: u32, row: u32) -> bool {
    let columns = u32_at(bytes, 40);
    let rows = u32_at(bytes, 44);
    if column >= columns || row >= rows {
        return false;
    }
    let data = u32_at(bytes, 96 + 4 * 24 + 8) as usize;
    let row_flags = u32_at(bytes, data + 12 + row as usize * 8);
    let usable = if row_flags >> 1 == 0 {
        columns
    } else {
        (columns / 2).max(1)
    };
    let cell =
        data + 8 + rows as usize * 8 + (row as usize * columns as usize + column as usize) * 12;
    if column >= usable || u32_at(bytes, cell + 8) & 1 != 0 {
        return false;
    }
    let glyphs = u32_at(bytes, 96 + 8) as usize;
    let record = glyphs + (u32_at(bytes, cell) as usize - 1) * 8;
    let start = glyphs + u32_at(bytes, record) as usize;
    let len = u16::from_le_bytes([bytes[record + 4], bytes[record + 5]]) as usize;
    bytes[record + 6] == 1
        && std::str::from_utf8(&bytes[start..start + len]).is_ok_and(|s| s.chars().count() == 1)
}

fn validate_bounded(
    bytes: &[u8],
    max_bytes: usize,
    max_cells: usize,
) -> Result<Metrics, TerminalError> {
    let bad = TerminalError::MalformedSnapshot;
    if bytes.len() > max_bytes {
        return Err(bad);
    }
    render_frame::validate_v4_header(bytes)?;
    let columns = u32_at(bytes, 40) as usize;
    let rows = u32_at(bytes, 44) as usize;
    let cells = columns * rows;
    if cells > max_cells {
        return Err(bad);
    }
    let mut sections = [&[][..]; 6];
    let mut counts = [0usize; 6];
    let mut end = 240;
    for index in 0..6 {
        let record = 96 + index * 24;
        let offset = u32_at(bytes, record + 8) as usize;
        let length = u32_at(bytes, record + 12) as usize;
        let count = u32_at(bytes, record + 16) as usize;
        if offset != end
            || length > bytes.len() - end
            || u32_at(bytes, record + 20) != [8, 16, 4, 16, 0, 0][index]
        {
            return Err(bad);
        }
        let next = (offset + length + 7) & !7;
        if next > bytes.len() || bytes[offset + length..next].iter().any(|&v| v != 0) {
            return Err(bad);
        }
        sections[index] = &bytes[offset..offset + length];
        counts[index] = count;
        end = next;
    }
    if end != bytes.len()
        || !(1..=cells).contains(&counts[0])
        || !(1..=cells).contains(&counts[1])
        || counts[2] != 264
        || counts[3] > crate::osc::MAX_HYPERLINK_ENTRIES
        || counts[4] != cells
        || counts[5] != sections[5].len()
        || counts[5] > crate::osc::MAX_TITLE_BYTES
    {
        return Err(bad);
    }
    std::str::from_utf8(sections[5]).map_err(|_| bad.clone())?;
    let glyphs = sections[0];
    let max_glyph = if u32_at(bytes, 60) == 2 {
        48
    } else {
        crate::MAX_GRAPHEME_BYTES
    };
    let mut text_offset = counts[0] * 8;
    if text_offset > glyphs.len() {
        return Err(bad);
    }
    let mut previous = &[][..];
    for index in 0..counts[0] {
        let record = &glyphs[index * 8..index * 8 + 8];
        let len = u16::from_le_bytes([record[4], record[5]]) as usize;
        if u32_at(record, 0) as usize != text_offset
            || !(1..=max_glyph).contains(&len)
            || !matches!(record[6], 1 | 2)
            || record[7] != 0
            || len > glyphs.len() - text_offset
        {
            return Err(bad);
        }
        let text = &glyphs[text_offset..text_offset + len];
        std::str::from_utf8(text).map_err(|_| bad.clone())?;
        if text <= previous {
            return Err(bad);
        }
        previous = text;
        text_offset += len;
    }
    if text_offset != glyphs.len() {
        return Err(bad);
    }
    let renditions = sections[1];
    if renditions.len() != counts[1] * 16
        || renditions
            .chunks_exact(16)
            .zip(renditions[16..].chunks_exact(16))
            .any(|(a, b)| a >= b)
    {
        return Err(bad);
    }
    let palette = sections[2];
    if palette.len() != 264 * 4 {
        return Err(bad);
    }
    for index in 0..264 {
        let value = u32_at(palette, index * 4);
        if value != u32::MAX
            && (value > if index < 260 { 0xff_ffff } else { 257 }
                || (index >= 260 && value == (256 + index % 2) as u32))
        {
            return Err(bad);
        }
    }
    let links = sections[3];
    let mut known_links = [false; crate::osc::MAX_HYPERLINK_ENTRIES + 1];
    let mut used_links = known_links;
    text_offset = counts[3] * 16;
    if text_offset > links.len()
        || links.len() - text_offset > crate::osc::MAX_HYPERLINK_TOTAL_BYTES
    {
        return Err(bad);
    }
    let mut previous_id = 0;
    for index in 0..counts[3] {
        let record = &links[index * 16..index * 16 + 16];
        let id = u32_at(record, 0) as usize;
        let len = u32_at(record, 8) as usize;
        if id <= previous_id
            || id >= known_links.len()
            || u32_at(record, 4) as usize != text_offset
            || !(1..=crate::osc::MAX_HYPERLINK_URI_BYTES).contains(&len)
            || len > links.len() - text_offset
            || u32_at(record, 12) != 0
        {
            return Err(bad);
        }
        std::str::from_utf8(&links[text_offset..text_offset + len]).map_err(|_| bad.clone())?;
        known_links[id] = true;
        previous_id = id;
        text_offset += len;
    }
    if text_offset != links.len() {
        return Err(bad);
    }
    let data = sections[4];
    if data.len() != 8 + rows * 8 + cells * 12 || u32_at(data, 0) != 8 || u32_at(data, 4) != 12 {
        return Err(bad);
    }
    let cell_start = 8 + rows * 8;
    let mut used_glyphs = vec![false; counts[0]];
    let mut used_renditions = vec![false; counts[1]];
    let mut metrics = Metrics {
        rows: rows as u32,
        utf8_bytes: 0,
        soft_wrapped: Vec::with_capacity(rows),
    };
    for row in 0..rows {
        let meaningful = u32_at(data, 8 + row * 8) as usize;
        let flags = u32_at(data, 12 + row * 8);
        let usable = if flags >> 1 == 0 {
            columns
        } else {
            (columns / 2).max(1)
        };
        if flags > 7
            || meaningful > usable
            || (flags & 1 != 0 && (row + 1 == rows || flags >> 1 != 0))
            || (flags >> 1 != 0 && metrics.soft_wrapped.last() == Some(&true))
        {
            return Err(bad);
        }
        metrics.soft_wrapped.push(flags & 1 != 0);
        let mut wide = None;
        for column in 0..columns {
            let offset = cell_start + (row * columns + column) * 12;
            let glyph = u32_at(data, offset) as usize;
            let rendition = u32_at(data, offset + 4) as usize;
            let meta = u32_at(data, offset + 8);
            let link = (meta >> 2) as usize;
            if rendition >= counts[1]
                || meta & !0x1fff != 0
                || (link != 0 && (link >= known_links.len() || !known_links[link]))
            {
                return Err(bad);
            }
            used_renditions[rendition] = true;
            if link != 0 {
                used_links[link] = true;
            }
            if meta & 1 != 0 {
                if glyph != 0 || wide != Some((rendition, meta & !1)) || column == meaningful {
                    return Err(bad);
                }
                wide = None;
            } else {
                if wide.is_some() || !(1..=counts[0]).contains(&glyph) {
                    return Err(bad);
                }
                used_glyphs[glyph - 1] = true;
                let glyph_offset = (glyph - 1) * 8;
                if column >= usable {
                    let text = u32_at(glyphs, glyph_offset) as usize;
                    let len =
                        u16::from_le_bytes([glyphs[glyph_offset + 4], glyphs[glyph_offset + 5]])
                            as usize;
                    if meta != 0 || &glyphs[text..text + len] != b" " {
                        return Err(bad);
                    }
                }
                let width = glyphs[glyph_offset + 6] as usize;
                if width == 2 {
                    if column + 1 >= usable {
                        return Err(bad);
                    }
                    wide = Some((rendition, meta & !1));
                }
                if column < meaningful {
                    metrics.utf8_bytes +=
                        u16::from_le_bytes([glyphs[glyph_offset + 4], glyphs[glyph_offset + 5]])
                            as u32;
                }
            }
        }
    }
    if used_glyphs.contains(&false) || used_renditions.contains(&false) || used_links != known_links
    {
        return Err(bad);
    }
    Ok(metrics)
}
