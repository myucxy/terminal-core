//! Immutable, sealed primary-history pagination. IDs are anchors, not sort keys.
use crate::{Terminal, TerminalError};

pub const HISTORY_LATEST_ANCHOR: u64 = u64::MAX;
pub const MAX_HISTORY_PAGE_LINES: u32 = 128;
pub const MAX_HISTORY_PAGE_CELLS: u32 = 65_536;
pub const MAX_HISTORY_PAGE_UTF8: u32 = 4 * 1024 * 1024;
pub const MAX_HISTORY_PAGE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_HISTORY_CAPTURE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_HISTORY_PAGE_V2_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_HISTORY_GRAPHICS_CAPTURE_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HistoryPageRequest {
    pub revision: u64,
    pub before_line_id: u64,
    pub max_lines: u32,
    pub max_cells: u32,
    pub max_utf8_bytes: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HistoryPageError {
    Terminal(TerminalError),
    InvalidRequest,
    StaleRevision,
    StaleAnchor,
    BudgetExceeded {
        required_cells: u32,
        required_utf8_bytes: u32,
    },
    WireBudgetExceeded,
    MalformedPage,
}
impl From<TerminalError> for HistoryPageError {
    fn from(value: TerminalError) -> Self {
        Self::Terminal(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryLine {
    pub id: u64,
    pub sealed_revision: u64,
    pub first_row: u32,
    pub row_count: u32,
    pub first_cell_offset: u32,
    pub head_truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryPage {
    pub revision: u64,
    pub requested_anchor: u64,
    pub next_before_line_id: u64,
    pub has_more_before: bool,
    pub head_truncated: bool,
    pub columns: u32,
    pub cells: u32,
    pub utf8_bytes: u32,
    pub lines: Vec<HistoryLine>,
    /// Ordered V4 fragments for TLHPV1, or V5 fragments for TLHPV2, at the frozen revision.
    pub frames: Vec<Vec<u8>>,
}

impl HistoryPage {
    /// TLHPV1 header (96 bytes), 40-byte logical-line directory, then
    /// length-prefixed V4 frames. CRC covers the entire envelope except 92..96.
    pub fn encode(&self, capture_id: u64) -> Result<Vec<u8>, HistoryPageError> {
        self.encode_version(capture_id, false)
    }

    /// TLHPV2 uses V5 fragments, required feature 1 at offset 72, and a 64 MiB limit.
    pub fn encode_v2(&self, capture_id: u64) -> Result<Vec<u8>, HistoryPageError> {
        self.encode_version(capture_id, true)
    }

    fn encode_version(&self, capture_id: u64, graphics: bool) -> Result<Vec<u8>, HistoryPageError> {
        if capture_id == 0 {
            return Err(HistoryPageError::InvalidRequest);
        }
        let size = self.validate(graphics)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|_| TerminalError::LimitExceeded)?;
        bytes.resize(96, 0);
        bytes[..8].copy_from_slice(if graphics {
            b"TLHPV2\0\0"
        } else {
            b"TLHPV1\0\0"
        });
        bytes[8..10].copy_from_slice(&(if graphics { 2u16 } else { 1u16 }).to_le_bytes());
        bytes[72..76].copy_from_slice(&u32::from(graphics).to_le_bytes());
        bytes[10..12].copy_from_slice(&96u16.to_le_bytes());
        bytes[12..16].copy_from_slice(&(size as u32).to_le_bytes());
        for (offset, value) in [
            (16, capture_id),
            (24, self.revision),
            (32, self.requested_anchor),
            (40, self.next_before_line_id),
        ] {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        for (offset, value) in [
            (48, self.lines.len() as u32),
            (52, self.frames.len() as u32),
            (56, self.columns),
            (60, self.cells),
            (64, self.utf8_bytes),
            (
                68,
                u32::from(self.has_more_before) | (u32::from(self.head_truncated) << 1),
            ),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        for line in &self.lines {
            bytes.extend_from_slice(&line.id.to_le_bytes());
            bytes.extend_from_slice(&line.sealed_revision.to_le_bytes());
            for value in [
                line.first_row,
                line.row_count,
                line.first_cell_offset,
                u32::from(line.head_truncated),
                0,
                0,
            ] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        for frame in &self.frames {
            bytes.extend_from_slice(&(frame.len() as u32).to_le_bytes());
            bytes.extend_from_slice(frame);
        }
        let crc = crate::crc32c::checksum_parts(&[&bytes[..92], &bytes[96..]]);
        bytes[92..96].copy_from_slice(&crc.to_le_bytes());
        Ok(bytes)
    }

    /// Bounded envelope decoder. V4 payloads are fully checked before publication.
    pub fn decode(bytes: &[u8], capture_id: u64) -> Result<Self, HistoryPageError> {
        Self::decode_version(bytes, capture_id, false)
    }

    pub fn decode_v2(bytes: &[u8], capture_id: u64) -> Result<Self, HistoryPageError> {
        Self::decode_version(bytes, capture_id, true)
    }

    fn decode_version(
        bytes: &[u8],
        capture_id: u64,
        graphics: bool,
    ) -> Result<Self, HistoryPageError> {
        let bad = HistoryPageError::MalformedPage;
        if capture_id == 0
            || !(96..=page_limit(graphics)).contains(&bytes.len())
            || &bytes[..8]
                != if graphics {
                    b"TLHPV2\0\0"
                } else {
                    b"TLHPV1\0\0"
                }
            || bytes[8..12] != [if graphics { 2 } else { 1 }, 0, 96, 0]
            || u32_at(bytes, 12) as usize != bytes.len()
            || u64_at(bytes, 16) != capture_id
            || u32_at(bytes, 72) != u32::from(graphics)
            || bytes[76..92].iter().any(|&v| v != 0)
            || u32_at(bytes, 68) & !3 != 0
            || crate::crc32c::checksum_parts(&[&bytes[..92], &bytes[96..]]) != u32_at(bytes, 92)
        {
            return Err(bad);
        }
        let line_count = u32_at(bytes, 48) as usize;
        let frame_count = u32_at(bytes, 52) as usize;
        let mut offset = 96 + line_count.min(MAX_HISTORY_PAGE_LINES as usize + 1) * 40;
        if line_count > MAX_HISTORY_PAGE_LINES as usize
            || frame_count > MAX_HISTORY_PAGE_CELLS as usize
            || offset > bytes.len()
            || frame_count > (bytes.len() - offset) / 100
        {
            return Err(bad);
        }
        let mut page = Self {
            revision: u64_at(bytes, 24),
            requested_anchor: u64_at(bytes, 32),
            next_before_line_id: u64_at(bytes, 40),
            has_more_before: u32_at(bytes, 68) & 1 != 0,
            head_truncated: u32_at(bytes, 68) & 2 != 0,
            columns: u32_at(bytes, 56),
            cells: u32_at(bytes, 60),
            utf8_bytes: u32_at(bytes, 64),
            lines: Vec::with_capacity(line_count),
            frames: Vec::with_capacity(frame_count),
        };
        for record in bytes[96..offset].chunks_exact(40) {
            if u32_at(record, 28) > 1 || record[32..].iter().any(|&v| v != 0) {
                return Err(bad);
            }
            page.lines.push(HistoryLine {
                id: u64_at(record, 0),
                sealed_revision: u64_at(record, 8),
                first_row: u32_at(record, 16),
                row_count: u32_at(record, 20),
                first_cell_offset: u32_at(record, 24),
                head_truncated: u32_at(record, 28) != 0,
            });
        }
        for _ in 0..frame_count {
            if bytes.len() - offset < 4 {
                return Err(bad);
            }
            let len = u32_at(bytes, offset) as usize;
            offset += 4;
            if len < 96 || len > bytes.len() - offset {
                return Err(bad);
            }
            // Check all indices and referenced text before allocating the owned copy.
            history_text(&bytes[offset..offset + len], graphics)?;
            page.frames.push(bytes[offset..offset + len].to_vec());
            offset += len;
        }
        if offset != bytes.len() {
            return Err(bad);
        }
        page.validate(graphics)?;
        Ok(page)
    }

    fn validate(&self, graphics: bool) -> Result<usize, HistoryPageError> {
        let bad = HistoryPageError::MalformedPage;
        if !(1..=crate::MAX_COLUMNS).contains(&self.columns)
            || self.lines.len() > MAX_HISTORY_PAGE_LINES as usize
            || self.cells > MAX_HISTORY_PAGE_CELLS
            || self.utf8_bytes > MAX_HISTORY_PAGE_UTF8
            || self.frames.len() > self.cells as usize
            || self.head_truncated != self.lines.first().is_some_and(|l| l.head_truncated)
            || self.next_before_line_id
                != self.lines.first().map_or(self.requested_anchor, |l| l.id)
            || (self.lines.is_empty()
                && (self.has_more_before
                    || !self.frames.is_empty()
                    || self.cells != 0
                    || self.utf8_bytes != 0))
        {
            return Err(bad);
        }
        let mut rows = 0u32;
        for (index, line) in self.lines.iter().enumerate() {
            if line.id == HISTORY_LATEST_ANCHOR
                || line.id == self.requested_anchor
                || line.sealed_revision == 0
                || line.sealed_revision > self.revision
                || line.first_row != rows
                || line.row_count == 0
                || (!line.head_truncated && line.first_cell_offset != 0)
                || self.lines[..index].iter().any(|prior| prior.id == line.id)
            {
                return Err(bad);
            }
            rows = rows.checked_add(line.row_count).ok_or(bad.clone())?;
            line.first_cell_offset
                .checked_add(
                    line.row_count
                        .checked_mul(self.columns)
                        .ok_or(bad.clone())?,
                )
                .ok_or(bad.clone())?;
        }
        if rows.checked_mul(self.columns) != Some(self.cells) {
            return Err(bad);
        }
        let mut frame_rows = 0u32;
        let mut text_bytes = 0u32;
        let mut width_profile = None;
        let mut size = 96 + self.lines.len() * 40;
        for frame in &self.frames {
            size = size
                .checked_add(4)
                .and_then(|v| v.checked_add(frame.len()))
                .ok_or(HistoryPageError::WireBudgetExceeded)?;
            if size > page_limit(graphics) {
                return Err(HistoryPageError::WireBudgetExceeded);
            }
            let frame = history_text(frame, graphics)?;
            let metrics = crate::render_frame::validate_history_frame(frame)?;
            if u64_at(frame, 24) != self.revision
                || u64_at(frame, 32) != self.revision
                || u32_at(frame, 40) != self.columns
                || u32_at(frame, 56) != 0
                || (width_profile.is_some() && width_profile != Some(u32_at(frame, 60)))
            {
                return Err(bad);
            }
            width_profile = Some(u32_at(frame, 60));
            for (row_index, &soft_wrap) in metrics.soft_wrapped.iter().enumerate() {
                let absolute = frame_rows + row_index as u32;
                let line = self
                    .lines
                    .iter()
                    .find(|l| absolute >= l.first_row && absolute < l.first_row + l.row_count)
                    .ok_or(bad.clone())?;
                // V4 ends each projection with a hard boundary; the directory
                // preserves logical continuation across fragment boundaries.
                if soft_wrap
                    != (absolute + 1 < line.first_row + line.row_count
                        && row_index + 1 < metrics.rows as usize)
                {
                    return Err(bad);
                }
            }
            frame_rows = frame_rows.checked_add(metrics.rows).ok_or(bad.clone())?;
            text_bytes = text_bytes
                .checked_add(metrics.utf8_bytes)
                .ok_or(bad.clone())?;
        }
        if frame_rows != rows || text_bytes != self.utf8_bytes {
            return Err(bad);
        }
        Ok(size)
    }
}

fn page_limit(graphics: bool) -> usize {
    if graphics {
        MAX_HISTORY_PAGE_V2_BYTES
    } else {
        MAX_HISTORY_PAGE_BYTES
    }
}

fn history_text(frame: &[u8], graphics: bool) -> Result<&[u8], TerminalError> {
    let text = if graphics {
        crate::render_frame_v5::Frame::inspect_text(frame)?
    } else {
        frame
    };
    crate::render_frame::validate_history_frame(text)?;
    Ok(text)
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated field"),
    )
}
fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("validated field"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Event, TerminalOptions};

    fn terminal(columns: u32, history: u32) -> Terminal {
        Terminal::new_with_options(
            columns,
            3,
            1,
            TerminalOptions {
                history_capacity_rows: history,
            },
        )
        .unwrap()
    }
    fn feed(t: &mut Terminal, text: &str) {
        t.apply_event(Event::Output {
            source_sequence: t.next_source_sequence(),
            bytes: text.as_bytes(),
        })
        .unwrap();
    }
    fn request(capture: &HistoryCapture) -> HistoryPageRequest {
        HistoryPageRequest {
            revision: capture.revision(),
            before_line_id: HISTORY_LATEST_ANCHOR,
            max_lines: 128,
            max_cells: MAX_HISTORY_PAGE_CELLS,
            max_utf8_bytes: MAX_HISTORY_PAGE_UTF8,
        }
    }
    #[test]
    fn sealed_pages_retry_exactly_and_stay_frozen_after_output_resize_ed3_and_alt() {
        let mut t = terminal(12, 30);
        feed(&mut t, "one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix\r\n");
        let capture = HistoryCapture::capture(&t).unwrap();
        let mut query = request(&capture);
        query.max_lines = 2;
        let latest = capture.page(query).unwrap();
        assert_eq!(latest.lines.len(), 2);
        assert!(latest.has_more_before);
        let encoded = latest.encode(17).unwrap();
        for frame in &latest.frames {
            assert_eq!(
                u64::from_le_bytes(frame[24..32].try_into().unwrap()),
                capture.revision()
            );
        }
        let earlier = capture
            .page(HistoryPageRequest {
                before_line_id: latest.next_before_line_id,
                ..query
            })
            .unwrap();
        assert!(!earlier.has_more_before);
        assert!(
            earlier
                .lines
                .iter()
                .all(|line| latest.lines.iter().all(|other| other.id != line.id))
        );
        feed(&mut t, "seven\r\neight\r\n\x1b[3J\x1b[?1049h");
        t.apply_event(Event::Resize {
            source_sequence: t.next_source_sequence(),
            columns: 6,
            rows: 4,
        })
        .unwrap();
        assert_eq!(capture.page(query).unwrap().encode(17).unwrap(), encoded);
        assert!(
            HistoryCapture::capture(&t)
                .unwrap()
                .page(request(&HistoryCapture::capture(&t).unwrap()))
                .unwrap()
                .lines
                .is_empty()
        );
        assert_eq!(
            capture.page(HistoryPageRequest {
                revision: 0,
                ..query
            }),
            Err(HistoryPageError::StaleRevision)
        );
        assert_eq!(
            capture.page(HistoryPageRequest {
                before_line_id: 9999,
                ..query
            }),
            Err(HistoryPageError::StaleAnchor)
        );
    }

    #[test]
    fn long_logical_line_spans_frames_but_never_pages_or_graphemes() {
        let mut t = terminal(2, 1000);
        feed(&mut t, &"界".repeat(600));
        feed(&mut t, "\r\nA\r\nB\r\nC\r\n");
        let capture = HistoryCapture::capture(&t).unwrap();
        let page = capture.page(request(&capture)).unwrap();
        let long = page
            .lines
            .iter()
            .find(|line| line.row_count == 600)
            .unwrap();
        assert!(!long.head_truncated);
        assert!(page.frames.len() >= 2);
        assert_eq!(
            HistoryPage::decode(&page.encode(19).unwrap(), 19).unwrap(),
            page
        );
        assert!(
            page.frames
                .iter()
                .all(|frame| u32::from_le_bytes(frame[44..48].try_into().unwrap()) <= 512)
        );
        let exclusive = page
            .lines
            .iter()
            .find(|line| line.first_row == 600)
            .unwrap()
            .id;
        assert_eq!(
            capture.page(HistoryPageRequest {
                before_line_id: exclusive,
                max_cells: 100,
                ..request(&capture)
            }),
            Err(HistoryPageError::BudgetExceeded {
                required_cells: 1200,
                required_utf8_bytes: 1800
            })
        );
    }

    #[test]
    fn truncated_head_and_unsealed_tail_are_explicit_and_snapshot_stable() {
        let mut t = terminal(4, 4);
        feed(&mut t, &"x".repeat(40));
        let incomplete = HistoryCapture::capture(&t).unwrap();
        assert!(
            incomplete
                .page(request(&incomplete))
                .unwrap()
                .lines
                .is_empty()
        );
        feed(&mut t, "\r\na\r\nb\r\nc\r\n");
        let capture = HistoryCapture::capture(&t).unwrap();
        let page = capture.page(request(&capture)).unwrap();
        assert!(page.head_truncated && page.lines[0].head_truncated);
        assert!(page.lines[0].first_cell_offset > 0);
        let restored = Terminal::import_snapshot_v2(&t.export_snapshot_v2().unwrap()).unwrap();
        let restored_capture = HistoryCapture::capture(&restored).unwrap();
        assert_eq!(
            page.encode(7).unwrap(),
            restored_capture
                .page(request(&restored_capture))
                .unwrap()
                .encode(7)
                .unwrap()
        );
    }

    #[test]
    fn alternate_screen_never_replaces_primary_capture_and_empty_anchor_is_not_forged() {
        let mut t = terminal(8, 10);
        feed(
            &mut t,
            "primary\r\nrow2\r\nrow3\r\nrow4\r\n\x1b[?1049hOTHER\r\n",
        );
        let capture = HistoryCapture::capture(&t).unwrap();
        assert!(!capture.page(request(&capture)).unwrap().lines.is_empty());
        let empty = HistoryCapture::capture(&terminal(8, 10)).unwrap();
        let page = empty.page(request(&empty)).unwrap();
        assert!(page.lines.is_empty() && page.frames.is_empty() && !page.has_more_before);
        assert_eq!(page.next_before_line_id, HISTORY_LATEST_ANCHOR);
        assert_eq!(
            empty.page(HistoryPageRequest {
                before_line_id: 0,
                ..request(&empty)
            }),
            Err(HistoryPageError::StaleAnchor)
        );
    }

    #[test]
    fn decoder_rejects_corruption_and_forged_metadata_without_panicking() {
        let mut t = terminal(12, 20);
        feed(
            &mut t,
            "\x1b]8;;https://example.test/\x1b\\\x1b[31;4m界e\u{301}\x1b]8;;\x1b\\\r\nB\r\nC\r\nD\r\nE\r\n",
        );
        let capture = HistoryCapture::capture(&t).unwrap();
        let page = capture.page(request(&capture)).unwrap();
        let bytes = page.encode(23).unwrap();
        assert_eq!(HistoryPage::decode(&bytes, 23).unwrap(), page);
        assert!(HistoryPage::decode(&bytes, 24).is_err());
        for end in 0..bytes.len() {
            assert!(HistoryPage::decode(&bytes[..end], 23).is_err());
        }
        for offset in [
            0, 8, 12, 16, 24, 32, 40, 48, 52, 56, 60, 64, 68, 72, 92, 96, 104, 112, 116, 120, 124,
            128,
        ] {
            let mut corrupt = bytes.clone();
            corrupt[offset] ^= 0x80;
            assert!(
                HistoryPage::decode(&corrupt, 23).is_err(),
                "offset {offset}"
            );
        }
        for offset in [48, 52, 56, 60, 64, 68, 72, 112, 116, 128] {
            let mut corrupt = bytes.clone();
            corrupt[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            let crc = crate::crc32c::checksum_parts(&[&corrupt[..92], &corrupt[96..]]);
            corrupt[92..96].copy_from_slice(&crc.to_le_bytes());
            assert!(HistoryPage::decode(&corrupt, 23).is_err());
        }
        // A forged inner frame can have valid checksums; references still need validation.
        let mut invalid = page.clone();
        let frame = &mut invalid.frames[0];
        let data = u32_at(frame, 96 + 4 * 24 + 8) as usize;
        let first_cell = data + 8 + u32_at(frame, 44) as usize * 8;
        frame[first_cell..first_cell + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        let crc = crate::crc32c::checksum(&frame[96..]);
        frame[88..92].copy_from_slice(&crc.to_le_bytes());
        let crc = crate::crc32c::checksum(&frame[..92]);
        frame[92..96].copy_from_slice(&crc.to_le_bytes());
        assert!(invalid.encode(23).is_err());
        assert!(page.encode(0).is_err());
    }

    #[test]
    fn unicode16_scaled_rows_and_nonmonotonic_anchors_roundtrip() {
        let mut t = Terminal::new_with_profiles(
            12,
            3,
            1,
            TerminalOptions {
                history_capacity_rows: 20,
            },
            crate::OutputProfile::Utf8,
            crate::WidthProfile::Unicode16,
        )
        .unwrap();
        feed(
            &mut t,
            "\x1b#6界\r\n\x1b#3top\r\n\x1b#4bot\r\n👨‍👩‍👦\r\nZ\r\nZ\r\n",
        );
        let end = t.main.viewport_top() as usize;
        for row in 0..end {
            t.main.row_metadata[row].logical_line_id = (end - row + 100) as u64;
        }
        t.main.next_logical_line_id = 200;
        let capture = HistoryCapture::capture(&t).unwrap();
        let page = capture.page(request(&capture)).unwrap();
        assert!(page.lines.windows(2).all(|lines| lines[0].id > lines[1].id));
        assert_eq!(
            HistoryPage::decode(&page.encode(5).unwrap(), 5).unwrap(),
            page
        );
        assert!(page.frames.iter().all(|frame| u32_at(frame, 60) == 2));
        let before = page.lines[1].id;
        assert_eq!(
            capture
                .page(HistoryPageRequest {
                    before_line_id: before,
                    ..request(&capture)
                })
                .unwrap()
                .lines
                .len(),
            1
        );
    }

    #[test]
    fn primary_history_keeps_its_geometry_while_resized_alternate_is_active() {
        let mut t = terminal(8, 30);
        feed(&mut t, "one\r\ntwo\r\nthree\r\nfour\r\nfive\r\n\x1b[?1049h");
        let before = HistoryCapture::capture(&t).unwrap();
        let expected = before.page(request(&before)).unwrap().encode(1).unwrap();
        t.apply_event(Event::Resize {
            source_sequence: t.next_source_sequence(),
            columns: 4,
            rows: 6,
        })
        .unwrap();
        let capture = HistoryCapture::capture(&t).unwrap();
        let page = capture.page(request(&capture)).unwrap();
        assert_eq!(page.columns, 8);
        assert_eq!(page.lines, before.page(request(&before)).unwrap().lines);
        let decoded = HistoryPage::decode(&page.encode(2).unwrap(), 2).unwrap();
        assert_eq!(decoded, page);
        let (viewport, _, _) = capture.viewport(1).unwrap();
        assert_eq!(u32_at(&viewport, 40), 8);
        assert_eq!(u32_at(&viewport, 44), 3);
        feed(&mut t, "\x1b[?1049l");
        assert_eq!(
            before.page(request(&before)).unwrap().encode(1).unwrap(),
            expected
        );
    }

    #[test]
    fn graphics_capture_shares_pixels_preserves_glyphs_and_uses_only_v2_envelopes() {
        use crate::{OutputProfile, WidthProfile, drcs_bank::GlyphLookup, render_frame_v5::Frame};
        let mut t = Terminal::new_with_graphics_profile(
            10,
            3,
            0,
            TerminalOptions {
                history_capacity_rows: 20,
            },
            OutputProfile::Utf8,
            WidthProfile::Unicode16,
        )
        .unwrap();
        feed(&mut t, "one\r\ntwo\r\nthree\r\nfour\r\nfive\r\n");
        let graphics = t.graphics.as_mut().unwrap();
        graphics
            .install_font(
                crate::drcs::decode(&[1, 1, 0, 5, 1, 2, 8, 0], b" @~").unwrap(),
                &[],
            )
            .unwrap();
        let GlyphLookup::Glyph(glyph) = graphics.fonts().lookup(b" @", false, 80, 24, 1) else {
            panic!("missing font");
        };
        let command = crate::kitty_graphics::TransferDecoder::default()
            .push(b"Ga=T,s=1,v=1,i=1,c=2,r=2;AQIDBA==")
            .unwrap()
            .unwrap();
        graphics
            .apply_kitty(
                command,
                crate::graphics_state::Anchor {
                    page: 1,
                    logical_line: t.main.row_metadata[0].logical_line_id,
                    cell_offset: 0,
                },
                crate::HostGeometryV1 {
                    cell_width: 10,
                    cell_height: 20,
                    ..Default::default()
                },
                &[glyph.resource],
            )
            .unwrap();
        t.main.cells[0].glyph = Some(glyph);
        t.main.cells[0].character = '\u{fffd}';
        let before = crate::graphics_resources::image_allocation_attempts();
        let capture = HistoryCapture::capture(&t).unwrap();
        assert_eq!(
            crate::graphics_resources::image_allocation_attempts(),
            before
        );
        assert!(std::ptr::eq(
            capture
                .graphics
                .as_ref()
                .unwrap()
                .resources
                .get(glyph.resource)
                .unwrap()
                .image(),
            t.graphics
                .as_ref()
                .unwrap()
                .resources()
                .get(glyph.resource)
                .unwrap()
                .image()
        ));
        assert_eq!(
            capture.viewport(1),
            Err(TerminalError::RenderFrameVersionRequired)
        );
        assert!(capture.page(request(&capture)).is_err());
        let page = capture.page_v5(request(&capture)).unwrap();
        let bytes = page.encode_v2(7).unwrap();
        assert_eq!(&bytes[..12], b"TLHPV2\0\0\x02\0\x60\0");
        assert_eq!(u32_at(&bytes, 72), 1);
        assert_eq!(HistoryPage::decode_v2(&bytes, 7).unwrap(), page);
        assert!(HistoryPage::decode(&bytes, 7).is_err());
        assert!(page.encode(7).is_err());
        let frame = Frame::decode(&page.frames[0]).unwrap();
        assert_eq!(frame.glyphs[0].glyph, glyph);
        assert_eq!(frame.images.len(), 1);
        let viewport = capture.viewport_v5(u32::MAX).unwrap().0;
        assert_eq!(Frame::decode(&viewport).unwrap().glyphs[0].glyph, glyph);
        feed(&mut t, "\x1b[3J");
        t.apply_event(Event::Resize {
            source_sequence: t.next_source_sequence(),
            columns: 5,
            rows: 4,
        })
        .unwrap();
        feed(&mut t, "\x1bc");
        assert_eq!(
            capture
                .page_v5(request(&capture))
                .unwrap()
                .encode_v2(7)
                .unwrap(),
            bytes
        );
        assert_eq!(capture.viewport_v5(u32::MAX).unwrap().0, viewport);
        for (offset, value) in [(72, 0), (72, 3), (76, 1)] {
            let mut bad = bytes.clone();
            bad[offset..offset + 4].copy_from_slice(&u32::to_le_bytes(value));
            let crc = crate::crc32c::checksum_parts(&[&bad[..92], &bad[96..]]);
            bad[92..96].copy_from_slice(&crc.to_le_bytes());
            assert!(HistoryPage::decode_v2(&bad, 7).is_err());
        }
    }
}

#[derive(Clone)]
struct SealedLine {
    line: HistoryLine,
    cells: u32,
    utf8_bytes: u32,
}

/// No wall clock, mutable parser, or external resource access exists in a capture.
pub struct HistoryCapture {
    frozen: Terminal,
    lines: Vec<SealedLine>,
    accounted_bytes: usize,
    graphics: Option<FrozenGraphics>,
}

struct FrozenGraphics {
    resources: crate::graphics_resources::GraphicsResources,
    placements: Vec<crate::graphics_state::Placement>,
}

impl HistoryCapture {
    /// Conservative allocation charge, including per-cell strings, row/line
    /// metadata, an empty alternate viewport, and bounded OSC tables. Evaluate
    /// before cloning so a registry can refuse the allocation in advance.
    /// Shared pixel bytes count as retained memory for each capture quota; the
    /// process pixel budget charges their Arc allocation only once.
    #[must_use]
    pub fn allocation_charge(terminal: &Terminal) -> usize {
        let viewport_cells = terminal.main.columns as usize * terminal.main.rows as usize;
        (terminal.main.cells.len() + 2 * viewport_cells) * (std::mem::size_of::<crate::Cell>() + 48)
            + (terminal.main.row_metadata.len() + 2 * terminal.main.rows as usize) * 128
            + std::mem::size_of::<Terminal>()
            + 2 * 1024 * 1024
            + terminal.graphics.as_ref().map_or(0, |g| {
                g.resources().pixel_bytes()
                    + std::mem::size_of_val(g.placements())
                    + std::mem::size_of_val(g.resources().entries())
            })
    }

    #[must_use]
    pub fn accounted_bytes(&self) -> usize {
        self.accounted_bytes
    }

    pub fn capture(terminal: &Terminal) -> Result<Self, TerminalError> {
        terminal.ensure_presentation_eligible()?;
        let accounted_bytes = Self::allocation_charge(terminal);
        if accounted_bytes
            > if terminal.graphics.is_some() {
                MAX_HISTORY_GRAPHICS_CAPTURE_BYTES
            } else {
                MAX_HISTORY_CAPTURE_BYTES
            }
        {
            return Err(TerminalError::LimitExceeded);
        }
        let history = &terminal.main;
        let end = history.viewport_top() as usize;
        let mut lines = Vec::new();
        lines
            .try_reserve_exact(end)
            .map_err(|_| TerminalError::LimitExceeded)?;
        let mut first = 0;
        while first < end {
            let metadata = history.row_metadata[first];
            let mut last = first + 1;
            while last < history.row_metadata.len()
                && history.row_metadata[last].logical_line_id == metadata.logical_line_id
            {
                last += 1;
            }
            if last <= end && metadata.sealed_revision != 0 && !history.soft_wrapped[last - 1] {
                let mut utf8_bytes = 0u32;
                for row in first..last {
                    for column in 0..history.meaningful_end[row] as usize {
                        let cell = &history.cells[row * history.columns as usize + column];
                        if !cell.continuation {
                            utf8_bytes += if cell.glyph.is_some() {
                                '\u{fffd}'.len_utf8() as u32
                            } else {
                                (cell.character.len_utf8() + cell.combining.len()) as u32
                            };
                        }
                    }
                }
                lines.push(SealedLine {
                    line: HistoryLine {
                        id: metadata.logical_line_id,
                        sealed_revision: metadata.sealed_revision,
                        first_row: first as u32,
                        row_count: (last - first) as u32,
                        first_cell_offset: metadata.logical_cell_offset,
                        head_truncated: metadata.is_head_truncated(),
                    },
                    cells: (last - first) as u32 * history.columns,
                    utf8_bytes,
                });
            }
            first = last;
        }
        // Retain only the state used to paint history, not macro/page/alternate state.
        // An inactive primary keeps its previous geometry until the alternate
        // screen exits. History belongs to that primary geometry, not the live alt.
        let mut frozen = Terminal::new(history.columns, history.rows)?;
        frozen.main = terminal.main.clone();
        frozen.osc = terminal.osc.clone();
        frozen.modes = terminal.modes;
        frozen.next_source_sequence = terminal.next_source_sequence;
        frozen.cursor_visible = false;
        let graphics = terminal
            .graphics
            .as_ref()
            .map(|graphics| {
                let placements: Vec<_> = graphics
                    .placements()
                    .iter()
                    .filter(|p| p.anchor.page == 1)
                    .cloned()
                    .collect();
                let mut resources = graphics.resources().clone();
                resources.retain_referenced(
                    placements.iter().map(|p| p.resource).chain(
                        history
                            .cells
                            .iter()
                            .filter_map(|c| c.glyph.map(|g| g.resource)),
                    ),
                )?;
                Ok::<_, TerminalError>(FrozenGraphics {
                    resources,
                    placements,
                })
            })
            .transpose()?;
        Ok(Self {
            frozen,
            lines,
            accounted_bytes,
            graphics,
        })
    }

    #[must_use]
    pub fn revision(&self) -> u64 {
        self.frozen.next_source_sequence
    }

    /// Resolve a primary-screen anchor in this frozen revision, never by ID ordering.
    pub fn offset_for_anchor(
        &self,
        line_id: u64,
        cell_offset: u32,
    ) -> Result<u32, HistoryPageError> {
        let screen = &self.frozen.main;
        for (row, metadata) in screen.row_metadata.iter().enumerate() {
            if metadata.logical_line_id != line_id || cell_offset < metadata.logical_cell_offset {
                continue;
            }
            let next = screen.row_metadata.get(row + 1);
            let continued = next.is_some_and(|m| m.logical_line_id == line_id);
            let end = if continued {
                next.unwrap().logical_cell_offset
            } else {
                metadata.logical_cell_offset + screen.line_renditions[row].columns(screen.columns)
            };
            if cell_offset < end || (!continued && cell_offset == end) {
                return Ok(screen.viewport_top().saturating_sub(row as u32));
            }
        }
        Err(HistoryPageError::StaleAnchor)
    }

    /// Pixel projection for smooth local browsing. The frozen live tail is
    /// visible here, but is never advertised as a sealed line in `page()`.
    pub fn viewport(&self, offset: u32) -> Result<(Vec<u8>, u32, u64), TerminalError> {
        if self.graphics.is_some() {
            return Err(TerminalError::RenderFrameVersionRequired);
        }
        let (rows, generation) = self.frozen.scrollback_info();
        Ok((
            self.frozen.export_render_frame_v4_at_scrollback(offset)?,
            rows,
            generation,
        ))
    }

    pub fn viewport_v5(&self, offset: u32) -> Result<(Vec<u8>, u32, u64), TerminalError> {
        let source = &self.frozen.main;
        let top = source.viewport_top() - offset.min(source.viewport_top());
        let window = source.render_range(top, source.rows);
        let (rows, generation) = self.frozen.scrollback_info();
        Ok((self.project_v5(&window, top)?, rows, generation))
    }

    fn project_v5(
        &self,
        window: &crate::screen::Screen,
        top: u32,
    ) -> Result<Vec<u8>, TerminalError> {
        let text = crate::render_frame::export_v4_text_layer(&self.frozen, window, false)?;
        crate::render_frame_v5::export_projection(
            &self.frozen.main,
            window,
            top,
            1,
            self.graphics.as_ref().map(|g| &g.resources),
            self.graphics.as_ref().map_or(&[], |g| &g.placements),
            text,
        )
    }

    pub fn page(&self, request: HistoryPageRequest) -> Result<HistoryPage, HistoryPageError> {
        if self.graphics.is_some() {
            return Err(TerminalError::RenderFrameVersionRequired.into());
        }
        self.page_version(request, false)
    }

    pub fn page_v5(&self, request: HistoryPageRequest) -> Result<HistoryPage, HistoryPageError> {
        self.page_version(request, true)
    }

    fn page_version(
        &self,
        request: HistoryPageRequest,
        graphics: bool,
    ) -> Result<HistoryPage, HistoryPageError> {
        if request.revision != self.revision() {
            return Err(HistoryPageError::StaleRevision);
        }
        if !(1..=MAX_HISTORY_PAGE_LINES).contains(&request.max_lines)
            || !(1..=MAX_HISTORY_PAGE_CELLS).contains(&request.max_cells)
            || !(1..=MAX_HISTORY_PAGE_UTF8).contains(&request.max_utf8_bytes)
        {
            return Err(HistoryPageError::InvalidRequest);
        }
        let end = if request.before_line_id == HISTORY_LATEST_ANCHOR {
            self.lines.len()
        } else {
            self.lines
                .iter()
                .position(|v| v.line.id == request.before_line_id)
                .ok_or(HistoryPageError::StaleAnchor)?
        };
        let mut start = end;
        let (mut cells, mut utf8_bytes) = (0, 0);
        while start > 0 && end - start < request.max_lines as usize {
            let line = &self.lines[start - 1];
            if cells + line.cells > request.max_cells
                || utf8_bytes + line.utf8_bytes > request.max_utf8_bytes
            {
                if start == end {
                    return Err(HistoryPageError::BudgetExceeded {
                        required_cells: line.cells,
                        required_utf8_bytes: line.utf8_bytes,
                    });
                }
                break;
            }
            cells += line.cells;
            utf8_bytes += line.utf8_bytes;
            start -= 1;
        }
        let mut page = HistoryPage {
            revision: self.revision(),
            requested_anchor: request.before_line_id,
            next_before_line_id: self
                .lines
                .get(start)
                .filter(|_| start < end)
                .map_or(request.before_line_id, |v| v.line.id),
            has_more_before: start > 0,
            head_truncated: self
                .lines
                .get(start)
                .filter(|_| start < end)
                .is_some_and(|v| v.line.head_truncated),
            columns: self.frozen.columns,
            cells,
            utf8_bytes,
            lines: Vec::new(),
            frames: Vec::new(),
        };
        let mut total_rows = 0;
        let mut wire_bytes = 96 + (end - start) * 40;
        for sealed in &self.lines[start..end] {
            let mut line = sealed.line.clone();
            line.first_row = total_rows;
            total_rows += line.row_count;
            page.lines.push(line);
            let mut row = sealed.line.first_row;
            let end_row = row + sealed.line.row_count;
            while row < end_row {
                let count = (end_row - row).min(crate::MAX_ROWS);
                let frame = if graphics {
                    self.project_v5(&self.frozen.main.render_range(row, count), row)?
                } else {
                    crate::render_frame::export_history_rows(&self.frozen, row, count)?
                };
                wire_bytes += 4 + frame.len();
                if wire_bytes > page_limit(graphics) {
                    return Err(HistoryPageError::WireBudgetExceeded);
                }
                page.frames.push(frame);
                row += count;
            }
        }
        Ok(page)
    }
}
