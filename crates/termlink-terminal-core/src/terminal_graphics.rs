use crate::{
    OutputProfile, Terminal, TerminalError, TerminalOptions, WidthProfile,
    graphics_state::GraphicsState,
};

/// A bounded row index shared by snapshot validation and viewport projection. Row IDs need not be
/// in screen order after insert/delete/scroll; offsets within a wrapped logical line remain ordered.
pub(crate) struct GraphicsAnchorIndex {
    rows: Vec<AnchorRow>,
}
struct AnchorRow {
    line: u64,
    start: u32,
    end: u64,
    row: u32,
    scale: u32,
}
impl GraphicsAnchorIndex {
    pub(crate) fn new(screen: &crate::screen::Screen) -> Result<Self, TerminalError> {
        let mut rows = Vec::new();
        rows.try_reserve_exact(screen.row_metadata.len())
            .map_err(|_| TerminalError::LimitExceeded)?;
        for (index, metadata) in screen.row_metadata.iter().enumerate() {
            let rendition = screen.line_renditions[index];
            let end = u64::from(metadata.logical_cell_offset)
                + u64::from(rendition.columns(screen.columns));
            let end = screen
                .row_metadata
                .get(index + 1)
                .filter(|next| next.logical_line_id == metadata.logical_line_id)
                .map_or(end, |next| end.min(u64::from(next.logical_cell_offset)));
            rows.push(AnchorRow {
                line: metadata.logical_line_id,
                start: metadata.logical_cell_offset,
                end,
                row: index as u32,
                scale: if rendition == crate::LineRendition::SingleWidth {
                    1
                } else {
                    2
                },
            });
        }
        rows.sort_unstable_by_key(|r| (r.line, r.start));
        Ok(Self { rows })
    }
    pub(crate) fn locate(&self, line: u64, offset: u32) -> Option<(u32, u32)> {
        let end = self
            .rows
            .partition_point(|r| (r.line, r.start) <= (line, offset));
        let row = self.rows.get(end.checked_sub(1)?)?;
        (row.line == line && u64::from(offset) < row.end)
            .then(|| (row.row, (offset - row.start) * row.scale))
    }
}

impl Terminal {
    pub(crate) fn erase_characters_with_graphics(
        &mut self,
        count: u16,
    ) -> Result<(), TerminalError> {
        let margins = self.margins;
        let (left, right) = self.scrolling_horizontal_bounds();
        let screen = self.active_screen();
        let row = screen.cursor_row;
        let column = screen.cursor_column;
        let motion = if (margins.top..margins.bottom).contains(&row)
            && (left..right).contains(&column)
        {
            let stored = screen.visible_stored_row(row) as usize;
            let scale = if screen.line_renditions[stored] == crate::LineRendition::SingleWidth {
                1
            } else {
                2
            };
            let row_start = stored * screen.columns as usize;
            let start = column - u32::from(screen.cells[row_start + column as usize].continuation);
            let mut end = (start + u32::from(count.max(1))).min(right);
            if end < right && screen.cells[row_start + end as usize].continuation {
                end += 1;
            }
            crate::GraphicMotion::EraseSixel {
                top: row,
                bottom: row + 1,
                left: start * scale,
                right: (end * scale).min(screen.columns),
            }
        } else {
            crate::GraphicMotion::Preserve
        };
        self.with_graphics_motion(motion, |screen| {
            screen.erase_characters_in_rectangle(count, margins.top, margins.bottom, left, right);
            Ok(())
        })
    }

    pub(crate) fn erase_display_with_graphics(&mut self, mode: u16) -> Result<(), TerminalError> {
        if mode == 2 && self.graphics.is_some() {
            // Resolve before ED changes line rendition metadata. Other text erasures do not erase
            // Kitty placements; ED3 history eviction is part of the remaining scroll lifecycle.
            let mut control = crate::kitty_graphics::Control::default();
            control.set(b'a', i64::from(b'd'));
            control.set(b'd', i64::from(b'A'));
            self.delete_kitty_in_viewport(&control)?;
        }
        let screen = self.active_screen();
        let row = screen.cursor_row;
        let stored = screen.visible_stored_row(row) as usize;
        let scale = if screen.line_renditions[stored] == crate::LineRendition::SingleWidth {
            1
        } else {
            2
        };
        let column = screen.cursor_column * scale;
        let (rows, columns) = (self.rows, self.columns);
        match mode {
            0 | 1 => {
                let (top, bottom) = if mode == 0 { (row + 1, rows) } else { (0, row) };
                if top < bottom {
                    self.with_graphics_motion(
                        crate::GraphicMotion::EraseSixel {
                            top,
                            bottom,
                            left: 0,
                            right: columns,
                        },
                        |_| Ok(()),
                    )?;
                }
                let (left, right) = if mode == 0 {
                    (column, columns)
                } else {
                    (0, (column + scale).min(columns))
                };
                self.with_graphics_motion(
                    crate::GraphicMotion::EraseSixel {
                        top: row,
                        bottom: row + 1,
                        left,
                        right,
                    },
                    |s| s.erase_display(mode),
                )
            }
            2 => self.with_graphics_motion(
                crate::GraphicMotion::EraseSixel {
                    top: 0,
                    bottom: rows,
                    left: 0,
                    right: columns,
                },
                |s| s.erase_display(mode),
            ),
            3 => self.with_graphics_motion(crate::GraphicMotion::ClearHistory, |s| {
                s.erase_display(mode)
            }),
            _ => self.active_screen_mut().erase_display(mode),
        }
    }

    pub(crate) fn erase_line_with_graphics(&mut self, mode: u16) -> Result<(), TerminalError> {
        let screen = self.active_screen();
        let row = screen.cursor_row;
        let stored = screen.visible_stored_row(row) as usize;
        let scale = if screen.line_renditions[stored] == crate::LineRendition::SingleWidth {
            1
        } else {
            2
        };
        let column = screen.cursor_column * scale;
        let (left, right) = match mode {
            0 => (column, self.columns),
            1 => (0, (column + scale).min(self.columns)),
            2 => (0, self.columns),
            _ => return self.active_screen_mut().erase_line(mode),
        };
        self.with_graphics_motion(
            crate::GraphicMotion::EraseSixel {
                top: row,
                bottom: row + 1,
                left,
                right,
            },
            |s| s.erase_line(mode),
        )
    }
    fn delete_kitty_in_viewport(
        &mut self,
        control: &crate::kitty_graphics::Control,
    ) -> Result<(), TerminalError> {
        let screen = self.active_screen();
        let row = screen.visible_stored_row(screen.cursor_row) as usize;
        let viewport = crate::graphics_state::GraphicsViewport {
            page: if self.active_alternate {
                7
            } else {
                self.active_page()
            },
            columns: self.columns,
            rows: self.rows,
            cursor_column: screen.cursor_column
                * if screen.line_renditions[row] == crate::LineRendition::SingleWidth {
                    1
                } else {
                    2
                },
            cursor_row: screen.cursor_row,
        };
        let top = screen.viewport_top() as i32;
        let index = GraphicsAnchorIndex::new(screen)?;
        let cell_resources = self.cell_graphic_resources();
        self.graphics
            .as_mut()
            .expect("graphics-only caller")
            .delete_kitty_in_viewport(
                control,
                viewport,
                |a| {
                    index
                        .locate(a.logical_line, a.cell_offset)
                        .map(|(r, c)| (c as i32, r as i32 - top))
                },
                &cell_resources,
            )
    }
    pub(crate) fn dispatch_kitty(
        &mut self,
        bytes: &[u8],
        effect: Option<crate::EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        use crate::graphics_state::Anchor;
        use crate::kitty_graphics::{self, BrokerFailure, Content};
        if self.graphics.is_none() {
            return Err(TerminalError::Unsupported(
                "Kitty requires the graphics session profile",
            ));
        }
        let (request, _) = kitty_graphics::Control::parse(bytes)?;
        let mut query = self
            .kitty_transfer
            .pending_control()
            .unwrap_or(&request)
            .clone();
        if let Some(quiet) = request.get(b'q') {
            query.set(b'q', quiet);
        }
        let decoded = match self.kitty_transfer.push(bytes) {
            Ok(command) => command,
            Err(error) if query.action() == b'q' => {
                self.kitty_transfer.abort();
                return Self::reject_kitty_query(&query, &error, effect, None);
            }
            Err(error) => return Err(error),
        };
        let Some(mut command) = decoded else {
            return Ok(());
        };
        let replay_broker = if let (Content::BrokerReference(_), Some(replay)) =
            (&command.content, self.broker_replay.as_mut())
        {
            let (source_sequence, source_offset_end) = effect
                .as_ref()
                .map(|effect| (effect.source_sequence, effect.source_offset_end))
                .ok_or(TerminalError::HostEffectVersionRequired)?;
            let request = match kitty_graphics::BrokerRequest::from_command(
                &command,
                source_sequence,
                source_offset_end,
                replay.next_ordinal,
            ) {
                Ok(request) => request,
                Err(error) if command.control.action() == b'q' => {
                    return Self::reject_kitty_query(&command.control, &error, effect, None);
                }
                Err(error) => return Err(error),
            };
            let Some(capture) = replay
                .captures
                .get(usize::from(replay.next_ordinal))
                .cloned()
            else {
                replay.pending = Some(request);
                return Err(TerminalError::BrokerCaptureRequired);
            };
            replay.next_ordinal += 1;
            Some((request, capture))
        } else {
            None
        };
        let broker = replay_broker
            .as_ref()
            .map(|(request, capture)| (request, capture.as_ref()));
        if matches!(command.content, Content::BrokerReference(_)) {
            if let Some((request, capture)) = broker {
                let source = effect
                    .as_ref()
                    .ok_or(TerminalError::HostEffectVersionRequired)?;
                if source.source_sequence != request.source_sequence
                    || source.source_offset_end != request.source_offset_end
                {
                    return Err(TerminalError::Unsupported(
                        "Kitty broker capture source position mismatch",
                    ));
                }
                let control = command.control.clone();
                match request.resolve(command, capture)? {
                    Ok(resolved) => command = resolved,
                    Err(failure) => {
                        let error = match failure {
                            BrokerFailure::OverLimit => TerminalError::ProtocolResourceLimit(
                                "Kitty broker capture exceeds 16 MiB",
                            ),
                            BrokerFailure::UnsupportedMedium => {
                                TerminalError::Unsupported(if control.medium() == b's' {
                                    "Kitty shared-memory broker is unsupported"
                                } else {
                                    "Kitty file broker medium is unsupported"
                                })
                            }
                            // Keep missing, inaccessible and short reads indistinguishable to
                            // the program; the host records the local failure detail separately.
                            BrokerFailure::Denied | BrokerFailure::Unavailable => {
                                TerminalError::Unsupported(
                                    "Kitty broker resource could not be read",
                                )
                            }
                        };
                        let unsupported_request =
                            (failure == BrokerFailure::UnsupportedMedium).then_some(request);
                        return Self::reject_kitty_query(
                            &control,
                            &error,
                            effect,
                            unsupported_request,
                        );
                    }
                }
            } else {
                let error = if command.control.medium() == b's' {
                    TerminalError::Unsupported("Kitty shared-memory broker is unsupported")
                } else {
                    TerminalError::Unsupported("Kitty file/shared-memory broker is not connected")
                };
                if command.control.action() == b'q' || command.control.medium() == b's' {
                    return Self::reject_kitty_query(&command.control, &error, effect, None);
                }
                return Err(error);
            }
        }
        let page = if self.active_alternate {
            7
        } else {
            self.active_page()
        };
        let screen = self.active_screen();
        let row = screen.visible_stored_row(screen.cursor_row) as usize;
        let anchor = Anchor {
            page,
            logical_line: screen.row_metadata[row].logical_line_id,
            cell_offset: screen.row_metadata[row].logical_cell_offset + screen.cursor_column,
        };
        let control = if matches!(command.content, Content::Delete) {
            self.delete_kitty_in_viewport(&command.control)?;
            command.control
        } else {
            let cell_resources = self.cell_graphic_resources();
            let result = self
                .graphics
                .as_mut()
                .expect("checked graphics profile")
                .apply_kitty(command, anchor, self.host_geometry, &cell_resources)?;
            if result.cursor_columns != 0 || result.cursor_rows != 0 {
                // Kitty leaves out-of-screen cursor policy to the implementation; clamp without
                // scrolling, so placement itself cannot trigger an implicit graphics scroll.
                let screen = self.active_screen();
                let row = screen
                    .cursor_row
                    .saturating_add(result.cursor_rows)
                    .min(self.rows - 1);
                let column = screen.cursor_column.saturating_add(result.cursor_columns);
                self.active_screen_mut().position_cursor(row, column);
            }
            result.control
        };
        let reply = kitty_graphics::response(&control, true, "OK")?;
        if !reply.is_empty() {
            Self::emit_host_reply_v2(effect, &reply)?;
        }
        Ok(())
    }
    fn reject_kitty_query(
        control: &crate::kitty_graphics::Control,
        error: &TerminalError,
        effect: Option<crate::EffectContext<'_>>,
        unsupported_request: Option<&crate::kitty_graphics::BrokerRequest>,
    ) -> Result<(), TerminalError> {
        let code = match error {
            TerminalError::Unsupported(_) => "ENOTSUP",
            TerminalError::ProtocolResourceLimit(_) | TerminalError::LimitExceeded => "ENOSPC",
            _ => "EINVAL",
        };
        // Reasons are structured core errors, never echoed file paths or image payloads.
        let mut message = format!("{code}: {}", error.to_string().escape_default());
        if let Some(request) = unsupported_request {
            // The offset identifies the completed APC, not the physical position of the t value.
            use std::fmt::Write;
            write!(message,
                "; unsupported_control=t={}; byte=0x{:02X}; source_sequence={}; source_offset_end={}; ordinal={}; source_offset_kind=APC_end",
                char::from(request.medium), request.medium, request.source_sequence,
                request.source_offset_end, request.ordinal,
            ).expect("writing Kitty broker diagnostic");
        }
        if message.len() > 240 {
            message.truncate(237);
            message.push_str("...");
        }
        let reply = crate::kitty_graphics::response(control, false, &message)?;
        if !reply.is_empty() {
            Self::emit_host_reply_v2(effect, &reply)?;
        }
        Ok(())
    }
    /// Staged native graphics profile. Not exposed by existing C/JNI constructors; Render V5 and
    /// the product capability handshake must be integrated before applications may opt in.
    pub fn new_with_graphics_profile(
        columns: u32,
        rows: u32,
        initial_source_sequence: u64,
        options: TerminalOptions,
        output_profile: OutputProfile,
        width_profile: WidthProfile,
    ) -> Result<Self, TerminalError> {
        let mut terminal = Self::new_with_profiles(
            columns,
            rows,
            initial_source_sequence,
            options,
            output_profile,
            width_profile,
        )?;
        terminal.graphics = Some(GraphicsState::default());
        Ok(terminal)
    }
    pub fn has_graphics_profile(&self) -> bool {
        self.graphics.is_some()
    }
    pub fn graphics_state(&self) -> Option<&GraphicsState> {
        self.graphics.as_ref()
    }
    pub(crate) fn validate_graphics_anchors(&self) -> Result<(), TerminalError> {
        self.validate_graphic_cells()?;
        let Some(graphics) = &self.graphics else {
            return Ok(());
        };
        if graphics
            .placements()
            .iter()
            .any(|p| !(1..=7).contains(&p.anchor.page))
        {
            return Err(TerminalError::MalformedSnapshot);
        }
        for page in 1..=7 {
            if !graphics.placements().iter().any(|p| p.anchor.page == page) {
                continue;
            }
            let screen = match page {
                1 => Some(&self.main),
                7 => Some(&self.alternate),
                2..=6 => self.pages.extra[usize::from(page - 2)].as_ref(),
                _ => None,
            }
            .ok_or(TerminalError::MalformedSnapshot)?;
            let index = GraphicsAnchorIndex::new(screen)?;
            for placement in graphics
                .placements()
                .iter()
                .filter(|p| p.anchor.page == page)
            {
                let anchor = placement.anchor;
                if index
                    .locate(anchor.logical_line, anchor.cell_offset)
                    .is_none()
                {
                    return Err(TerminalError::MalformedSnapshot);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Event, HostGeometryV1,
        graphics_state::Anchor,
        kitty_graphics::{BrokerCapture, BrokerFailure, BrokerOutcome, TransferDecoder},
    };
    fn terminal() -> Terminal {
        Terminal::new_with_graphics_profile(
            20,
            4,
            0,
            TerminalOptions::default(),
            OutputProfile::Utf8,
            WidthProfile::Unicode16,
        )
        .unwrap()
    }
    fn graphics_terminal() -> Terminal {
        let mut terminal = terminal();
        terminal
            .apply_event(Event::HostGeometry {
                source_sequence: 0,
                geometry: HostGeometryV1 {
                    cell_width: 10,
                    cell_height: 20,
                    ..Default::default()
                },
            })
            .unwrap();
        terminal
    }
    fn output(t: &mut Terminal, bytes: &[u8]) -> Vec<Vec<u8>> {
        let pending = t
            .prepare_event_with_host_effects_v2(Event::Output {
                source_sequence: t.next_source_sequence(),
                bytes,
            })
            .unwrap();
        let replies = crate::tests::host_effect_payloads(pending.host_effect_frame_v2())
            .into_iter()
            .map(<[u8]>::to_vec)
            .collect();
        pending.publish(|_| Ok::<(), ()>(())).unwrap();
        replies
    }
    #[test]
    fn kitty_output_roundtrips_through_parser_state_snapshot_and_frame_at_every_split() {
        let bytes = b"\x1b_Ga=T,s=1,v=1,i=8,c=2,r=2,C=1;AQIDBA==\x1b\\";
        for split in 0..=bytes.len() {
            let mut t = graphics_terminal();
            let mut replies = output(&mut t, &bytes[..split]);
            replies.extend(output(&mut t, &bytes[split..]));
            assert_eq!(replies, vec![b"\x1b_Gi=8;OK\x1b\\".to_vec()]);
            assert_eq!(
                (
                    t.active_screen().cursor_row,
                    t.active_screen().cursor_column
                ),
                (0, 0)
            );
            let frame =
                crate::render_frame_v5::Frame::decode(&t.export_render_frame_v5(0).unwrap())
                    .unwrap();
            assert_eq!(frame.images.len(), 1);
            assert_eq!(frame.resources.entries()[0].image().pixels(), &[1, 2, 3, 4]);
            let restored = Terminal::import_snapshot_v2(&t.export_snapshot_v2().unwrap()).unwrap();
            assert_eq!(restored, t);
            assert_eq!(
                restored.export_render_frame_v5(0).unwrap(),
                t.export_render_frame_v5(0).unwrap()
            );
        }
    }
    #[test]
    fn incomplete_upload_blocks_checkpoint_and_prepared_completion_is_atomic() {
        let mut t = graphics_terminal();
        assert!(output(&mut t, b"\x1b_Ga=T,f=24,s=2,v=1,i=9,m=1;AQID\x1b\\").is_empty());
        assert!(t.graphics_state().unwrap().placements().is_empty());
        assert!(t.is_parser_incomplete());
        assert_eq!(
            t.export_snapshot_v2().unwrap_err(),
            TerminalError::SnapshotNotEligible
        );
        let before = t.clone();
        let pending = t
            .prepare_event_with_host_effects_v2(Event::Output {
                source_sequence: t.next_source_sequence(),
                bytes: b"\x1b_G;BAUG\x1b\\",
            })
            .unwrap();
        assert_eq!(
            crate::tests::host_effect_payloads(pending.host_effect_frame_v2()),
            vec![b"\x1b_Gi=9;OK\x1b\\".as_slice()]
        );
        drop(pending);
        assert_eq!(t, before);
        assert_eq!(
            output(&mut t, b"\x1b_G;BAUG\x1b\\"),
            vec![b"\x1b_Gi=9;OK\x1b\\".to_vec()]
        );
        assert!(!t.is_parser_incomplete());
        assert_eq!(t.graphics_state().unwrap().placements().len(), 1);
        assert!(t.export_snapshot_v2().is_ok());
    }
    #[test]
    fn kitty_query_delete_and_reset_follow_terminal_transactions() {
        let mut t = graphics_terminal();
        output(&mut t, b"\x1b_Ga=q,s=1,v=1,i=5;AQIDBA==\x1b\\");
        assert!(t.graphics_state().unwrap().resources().entries().is_empty());
        output(&mut t, b"\x1b_Ga=T,s=1,v=1,i=5;AQIDBA==\x1b\\");
        assert_eq!(
            (
                t.active_screen().cursor_row,
                t.active_screen().cursor_column
            ),
            (1, 1)
        );
        output(&mut t, b"\x1b_Ga=d,d=A\x1b\\");
        assert!(t.graphics_state().unwrap().placements().is_empty());
        assert!(t.graphics_state().unwrap().resources().entries().is_empty());
        output(&mut t, b"\x1b_Gs=1,v=1,m=1;AQID\x1b\\");
        assert!(t.is_parser_incomplete());
        output(&mut t, b"\x1b_Ga=d\x1b\\");
        assert!(!t.is_parser_incomplete());
        output(&mut t, b"\x1b_Gs=1,v=1,m=1;AQID\x1b\\\x1bc");
        assert!(!t.is_parser_incomplete());
        assert!(t.export_snapshot_v2().is_ok());
    }
    #[test]
    fn unsupported_kitty_token_preserves_header_and_redacts_payload_without_ack() {
        let mut t = graphics_terminal();
        let before = t.graphics_state().unwrap().clone();
        let seq = t.next_source_sequence();
        let error = t
            .apply_event(Event::Output {
                source_sequence: seq,
                bytes: b"\x1b_Ga=T,s=1,v=1,U=1;SEVDUkVU\x1b\\",
            })
            .unwrap_err();
        assert!(matches!(
            error,
            TerminalError::UnsupportedPayload {
                byte: Some(b'U'),
                ..
            }
        ));
        assert_eq!(t.next_source_sequence(), seq);
        assert_eq!(t.graphics_state(), Some(&before));
        let diagnostic = t.failure_diagnostic_v2().unwrap();
        assert!(diagnostic.contains("U=1"), "{diagnostic}");
        assert!(!diagnostic.contains("SEVDUkVU"));
        assert!(!diagnostic.contains("SECRET"));
    }
    #[test]
    fn interrupted_apc_diagnostic_identifies_the_new_escape_without_private_payload() {
        let mut t = graphics_terminal();
        let seq = t.next_source_sequence();
        assert!(
            t.prepare_event_with_host_effects_v2(Event::Output {
                source_sequence: seq,
                bytes: b"\x1b_Ga=T;private\x1b\x07!z"
            })
            .is_err()
        );
        let diagnostic = t.failure_diagnostic_v2().unwrap();
        assert!(diagnostic.contains("family=ESC\n"), "{diagnostic}");
        assert!(
            diagnostic.contains("sequence=<ESC>\\x07!z\n"),
            "{diagnostic}"
        );
        assert!(!diagnostic.contains("private"));
        assert!(!diagnostic.contains("Ga=T"));
        assert_eq!(t.next_source_sequence(), seq);
    }
    #[test]
    fn downloaded_font_is_stateful_only_in_the_graphics_profile() {
        let bytes = b"\x1bP1;1;0;5;1;2;8;0{ @@\x1b\\";
        let mut t = graphics_terminal();
        output(&mut t, bytes);
        assert_eq!(
            t.graphics_state()
                .unwrap()
                .fonts()
                .referenced_resources()
                .count(),
            1
        );
        assert_eq!(
            Terminal::import_snapshot_v2(&t.export_snapshot_v2().unwrap()).unwrap(),
            t
        );
        let mut legacy = Terminal::new(20, 4).unwrap();
        assert_eq!(
            legacy.apply_event(Event::Output {
                source_sequence: 0,
                bytes
            }),
            Err(TerminalError::Unsupported("DECDLD/DRCS"))
        );
    }
    #[test]
    fn valid_optional_graphics_queries_reply_with_precise_failure_and_keep_running() {
        let mut t = graphics_terminal();
        for (query, expected) in [
            (b"\x1b_Ga=q,f=100,i=8;AQID\x1b\\".as_slice(), "PNG"),
            (
                b"\x1b_Ga=q,f=100,t=f,i=8;QzovbWlzc2luZy5wbmc=\x1b\\",
                "broker is not connected",
            ),
            (b"\x1b_Ga=q,s=4096,v=4096,i=8;AQID\x1b\\", "16 MiB"),
        ] {
            let replies = output(&mut t, query);
            assert_eq!(replies.len(), 1);
            let reply = std::str::from_utf8(&replies[0]).unwrap();
            assert!(reply.starts_with("\x1b_Gi=8;E"), "{reply:?}");
            assert!(reply.contains(expected), "{reply:?}");
            assert!(!reply.contains("C:/missing.png"));
            assert!(crate::kitty_graphics::canonical_response(&replies[0]));
            assert!(t.graphics_state().unwrap().resources().entries().is_empty());
            assert!(t.export_snapshot_v2().is_ok());
        }
        output(&mut t, b"\x1b_Ga=q,f=24,s=2,v=1,i=8,m=1;AQID\x1b\\");
        assert!(t.is_parser_incomplete());
        let replies = output(&mut t, b"\x1b_G;BA==\x1b\\");
        assert_eq!(replies.len(), 1);
        assert!(!t.is_parser_incomplete());
        output(&mut t, b"still running");
        assert!(t.export_snapshot_v2().is_ok());
    }
    #[test]
    fn shared_memory_without_broker_reports_unsupported_and_keeps_session_usable() {
        let mut t = graphics_terminal();
        let replies = output(&mut t, b"\x1b_Gt=s,f=32,s=1,v=1,i=9;L25hbWU=\x1b\\");
        assert_eq!(replies.len(), 1);
        let reply = std::str::from_utf8(&replies[0]).unwrap();
        assert!(reply.contains("ENOTSUP:"), "{reply:?}");
        assert!(
            reply.contains("shared-memory broker is unsupported"),
            "{reply:?}"
        );
        assert!(t.graphics_state().unwrap().resources().entries().is_empty());
        output(&mut t, b"still alive");
        assert!(t.export_snapshot_v2().is_ok());
    }

    #[test]
    fn broker_output_candidate_replays_in_order_and_publishes_only_after_commit() {
        let mut t = graphics_terminal();
        let original = t.export_snapshot_v2().unwrap();
        let bytes =
            b"\x1b[5n\x1b_Ga=T,t=f,f=32,s=1,v=1,i=31;QzovYS5yZ2Jh\x1b\\\x1b_Ga=d,d=I,i=31\x1b\\";
        let sequence = t.next_source_sequence();
        let mut preparation = t.prepare_output_with_broker_v1(sequence, bytes).unwrap();
        let request = preparation.request().unwrap().clone();
        assert_eq!(request.ordinal, 0);
        let mut wrong = request.clone();
        wrong.source_offset_end += 1;
        assert!(
            preparation
                .supply(BrokerCapture {
                    request: wrong,
                    outcome: BrokerOutcome::Bytes(vec![1, 2, 3, 4]),
                })
                .is_err()
        );
        assert_eq!(preparation.request(), Some(&request));
        assert!(
            preparation
                .supply(BrokerCapture {
                    request: request.clone(),
                    outcome: BrokerOutcome::Bytes(vec![1, 2, 3]),
                })
                .is_err()
        );
        assert_eq!(preparation.request(), Some(&request));
        preparation
            .supply(BrokerCapture {
                request,
                outcome: BrokerOutcome::Bytes(vec![1, 2, 3, 4]),
            })
            .unwrap();
        assert!(preparation.request().is_none());
        let ready = preparation.finish().unwrap();
        let replies = crate::tests::host_effect_payloads(ready.host_effect_frame_v2());
        assert_eq!(replies.len(), 3);
        assert_eq!(replies[0], b"\x1b[0n");
        assert_eq!(replies[1], b"\x1b_Gi=31;OK\x1b\\");
        ready.commit_with(|_| Err::<(), _>(())).unwrap_err();
        assert_eq!(t.export_snapshot_v2().unwrap(), original);
        assert_eq!(t.next_source_sequence(), sequence);

        let mut preparation = t.prepare_output_with_broker_v1(sequence, bytes).unwrap();
        let request = preparation.request().unwrap().clone();
        preparation
            .supply(BrokerCapture {
                request,
                outcome: BrokerOutcome::Bytes(vec![1, 2, 3, 4]),
            })
            .unwrap();
        preparation
            .finish()
            .unwrap()
            .commit_with(|_| Ok::<(), ()>(()))
            .unwrap();
        assert_eq!(t.next_source_sequence(), sequence + 1);
        assert!(t.graphics_state().unwrap().placements().is_empty());
        assert!(t.graphics_state().unwrap().resources().entries().is_empty());
    }

    #[test]
    fn broker_batch_accepts_prefix_and_complete_capture_list_in_order() {
        let mut t = graphics_terminal();
        let before = t.export_snapshot_v2().unwrap();
        let sequence = t.next_source_sequence();
        let bytes = b"\x1b_Ga=T,t=f,f=32,s=1,v=1,i=3;QzovYS5yZ2Jh\x1b\\\x1b_Ga=T,t=f,f=32,s=1,v=1,i=4;QzovYi5yZ2Jh\x1b\\";
        let mut preparation = t.prepare_output_with_broker_v1(sequence, bytes).unwrap();
        let first = BrokerCapture {
            request: preparation.request().unwrap().clone(),
            outcome: BrokerOutcome::Bytes(vec![1, 2, 3, 4]),
        };
        preparation.supply(first.clone()).unwrap();
        let second = BrokerCapture {
            request: preparation.request().unwrap().clone(),
            outcome: BrokerOutcome::Bytes(vec![5, 6, 7, 8]),
        };
        drop(preparation);

        let zero = t
            .prepare_output_with_broker_captures_v1(sequence, bytes, &[])
            .unwrap();
        assert_eq!(zero.request(), Some(&first.request));
        drop(zero);
        let mut prefix = t
            .prepare_output_with_broker_captures_v1(sequence, bytes, std::slice::from_ref(&first))
            .unwrap();
        assert_eq!(prefix.request(), Some(&second.request));
        prefix.supply(second.clone()).unwrap();
        assert!(prefix.request().is_none());
        drop(prefix);
        assert_eq!(t.export_snapshot_v2().unwrap(), before);

        let captures = [first, second];
        let complete = t
            .prepare_output_with_broker_captures_v1(sequence, bytes, &captures)
            .unwrap();
        assert!(complete.request().is_none());
        complete
            .finish()
            .unwrap()
            .commit_with(|_| Err::<(), ()>(()))
            .unwrap_err();
        assert_eq!(t.export_snapshot_v2().unwrap(), before);
        assert_eq!(t.next_source_sequence(), sequence);

        t.prepare_output_with_broker_captures_v1(sequence, bytes, &captures)
            .unwrap()
            .finish()
            .unwrap()
            .commit_with(|_| Ok::<(), ()>(()))
            .unwrap();
        assert_eq!(t.next_source_sequence(), sequence + 1);
        assert_eq!(t.graphics_state().unwrap().placements().len(), 2);
    }

    #[test]
    fn broker_batch_rejects_bad_identity_and_extra_capture_without_committing() {
        let mut t = graphics_terminal();
        let before = t.export_snapshot_v2().unwrap();
        let sequence = t.next_source_sequence();
        let bytes = b"\x1b_Ga=T,t=f,f=32,s=1,v=1,i=3;QzovYS5yZ2Jh\x1b\\";
        let first = t.prepare_output_with_broker_v1(sequence, bytes).unwrap();
        let capture = BrokerCapture {
            request: first.request().unwrap().clone(),
            outcome: BrokerOutcome::Bytes(vec![1, 2, 3, 4]),
        };
        drop(first);
        let mut wrong = capture.clone();
        wrong.request.source_offset_end += 1;
        assert!(matches!(
            t.prepare_output_with_broker_captures_v1(sequence, bytes, &[wrong]),
            Err(TerminalError::Unsupported(
                "Kitty broker capture request identity mismatch"
            ))
        ));
        assert!(matches!(
            t.prepare_output_with_broker_captures_v1(sequence, bytes, &[capture.clone(), capture]),
            Err(TerminalError::Unsupported(
                "Kitty broker capture count exceeds parsed requests"
            ))
        ));
        assert_eq!(t.export_snapshot_v2().unwrap(), before);
        assert_eq!(t.next_source_sequence(), sequence);
    }

    #[test]
    fn broker_batch_checks_capture_count_and_byte_budgets_before_replay() {
        let mut t = graphics_terminal();
        let sequence = t.next_source_sequence();
        let request = t
            .prepare_output_with_broker_v1(
                sequence,
                b"\x1b_Ga=T,t=f,f=32,s=1,v=1,i=3;QzovYS5yZ2Jh\x1b\\",
            )
            .unwrap()
            .request()
            .unwrap()
            .clone();
        let small = BrokerCapture {
            request: request.clone(),
            outcome: BrokerOutcome::Bytes(vec![0]),
        };
        let over_count = vec![
            small.clone();
            usize::from(crate::kitty_graphics::MAX_BROKER_REQUESTS_PER_OUTPUT) + 1
        ];
        assert!(matches!(
            t.prepare_output_with_broker_captures_v1(sequence, b"", &over_count),
            Err(TerminalError::ProtocolResourceLimit(
                "Kitty broker request count exceeds 64 per Output"
            ))
        ));
        let large = BrokerCapture {
            request,
            outcome: BrokerOutcome::Bytes(vec![0; crate::kitty_graphics::MAX_BROKER_CAPTURE_BYTES]),
        };
        let mut too_large = large.clone();
        if let BrokerOutcome::Bytes(bytes) = &mut too_large.outcome {
            bytes.push(0);
        }
        assert!(matches!(
            t.prepare_output_with_broker_captures_v1(sequence, b"", &[too_large]),
            Err(TerminalError::ProtocolResourceLimit(
                "Kitty broker transaction exceeds 32 MiB"
            ))
        ));
        assert!(matches!(
            t.prepare_output_with_broker_captures_v1(sequence, b"", &[large.clone(), large, small]),
            Err(TerminalError::ProtocolResourceLimit(
                "Kitty broker transaction exceeds 32 MiB"
            ))
        ));
        assert_eq!(t.next_source_sequence(), sequence);
    }

    #[test]
    fn broker_request_across_output_boundary_blocks_checkpoint_until_resolved() {
        let mut t = graphics_terminal();
        let sequence = t.next_source_sequence();
        let preparation = t
            .prepare_output_with_broker_v1(sequence, b"\x1b_Ga=T,t=f,f=32,s=1,v=1,i=2;Qzov")
            .unwrap();
        assert!(preparation.request().is_none());
        preparation
            .finish()
            .unwrap()
            .commit_with(|_| Ok::<(), ()>(()))
            .unwrap();
        assert_eq!(
            t.export_snapshot_v2(),
            Err(TerminalError::SnapshotNotEligible)
        );

        let sequence = t.next_source_sequence();
        let mut preparation = t
            .prepare_output_with_broker_v1(sequence, b"YS5yZ2Jh\x1b\\")
            .unwrap();
        let request = preparation.request().unwrap().clone();
        assert_eq!(request.source_sequence, sequence);
        preparation
            .supply(BrokerCapture {
                request,
                outcome: BrokerOutcome::Bytes(vec![1, 2, 3, 4]),
            })
            .unwrap();
        preparation
            .finish()
            .unwrap()
            .commit_with(|_| Ok::<(), ()>(()))
            .unwrap();
        assert!(t.export_snapshot_v2().is_ok());
        assert_eq!(t.graphics_state().unwrap().placements().len(), 1);
    }

    #[test]
    fn broker_macro_requests_share_source_offset_but_not_ordinal() {
        let mut t = graphics_terminal();
        let body = b"\x1b_Ga=T,t=f,f=32,s=1,v=1,i=3;QzovYS5yZ2Jh\x1b\\\x1b_Ga=T,t=f,f=32,s=1,v=1,i=4;QzovYi5yZ2Jh\x1b\\";
        let hex: String = body.iter().map(|b| format!("{b:02X}")).collect();
        let definition = format!("\x1bP0;0;1!z{hex}\x1b\\");
        output(&mut t, definition.as_bytes());
        let sequence = t.next_source_sequence();
        let mut preparation = t
            .prepare_output_with_broker_v1(sequence, b"\x1b[0*z")
            .unwrap();
        let first = preparation.request().unwrap().clone();
        preparation
            .supply(BrokerCapture {
                request: first.clone(),
                outcome: BrokerOutcome::Bytes(vec![1, 2, 3, 4]),
            })
            .unwrap();
        let second = preparation.request().unwrap().clone();
        assert_eq!(first.source_offset_end, second.source_offset_end);
        assert_eq!((first.ordinal, second.ordinal), (0, 1));
        assert_ne!(first.control_digest, second.control_digest);
        preparation
            .supply(BrokerCapture {
                request: second,
                outcome: BrokerOutcome::Bytes(vec![5, 6, 7, 8]),
            })
            .unwrap();
        preparation
            .finish()
            .unwrap()
            .commit_with(|_| Ok::<(), ()>(()))
            .unwrap();
        assert_eq!(t.graphics_state().unwrap().placements().len(), 2);
    }

    #[test]
    fn broker_shared_memory_accepts_host_bytes_and_freezes_snapshot() {
        let mut t = graphics_terminal();
        let sequence = t.next_source_sequence();
        let mut preparation = t
            .prepare_output_with_broker_v1(
                sequence,
                b"\x1b_Ga=T,t=s,f=32,s=1,v=1,i=17;L25hbWU=\x1b\\",
            )
            .unwrap();
        let request = preparation.request().unwrap().clone();
        assert_eq!(request.medium, b's');
        let captured = BrokerCapture {
            request,
            outcome: BrokerOutcome::Bytes(vec![1, 2, 3, 4]),
        };
        preparation.supply(captured.clone()).unwrap();
        let ready = preparation.finish().unwrap();
        let replies = crate::tests::host_effect_payloads(ready.host_effect_frame_v2());
        assert_eq!(replies.len(), 1);
        let reply = std::str::from_utf8(replies[0]).unwrap();
        assert!(reply.contains("i=17;OK"));
        ready.commit_with(|_| Ok::<(), ()>(())).unwrap();
        assert_eq!(t.graphics_state().unwrap().resources().entries().len(), 1);
        let snapshot = t.export_snapshot_v2().unwrap();
        let restored = Terminal::import_snapshot_v2(&snapshot).unwrap();
        assert_eq!(restored.export_snapshot_v2().unwrap(), snapshot);
        let mut replay = graphics_terminal();
        replay
            .prepare_output_with_broker_captures_v1(
                sequence,
                b"\x1b_Ga=T,t=s,f=32,s=1,v=1,i=17;L25hbWU=\x1b\\",
                &[captured],
            )
            .unwrap()
            .finish()
            .unwrap()
            .commit_with(|_| Ok::<(), ()>(()))
            .unwrap();
        assert_eq!(replay.export_snapshot_v2().unwrap(), snapshot);

        let sequence = t.next_source_sequence();
        let preparation = t
            .prepare_output_with_broker_v1(
                sequence,
                b"\x1b_Ga=q,t=f,f=100,o=z,S=12,i=18;QzovYS5wbmc=\x1b\\",
            )
            .unwrap();
        assert!(preparation.request().is_none());
        let ready = preparation.finish().unwrap();
        let replies = crate::tests::host_effect_payloads(ready.host_effect_frame_v2());
        assert_eq!(replies.len(), 1);
        assert!(
            std::str::from_utf8(replies[0])
                .unwrap()
                .contains("i=18;ENOTSUP:")
        );
    }
    #[test]
    fn broker_late_protocol_rejection_discards_prior_image_and_reply() {
        let mut t = graphics_terminal();
        let before = t.export_snapshot_v2().unwrap();
        let sequence = t.next_source_sequence();
        let mut preparation = t
            .prepare_output_with_broker_v1(
                sequence,
                b"\x1b[5n\x1b_Ga=T,t=f,f=32,s=1,v=1,i=19;QzovYS5yZ2Jh\x1b\\\x1b_Ga=x,i=20;\x1b\\",
            )
            .unwrap();
        let request = preparation.request().unwrap().clone();
        assert!(matches!(
            preparation.supply(BrokerCapture {
                request: request.clone(),
                outcome: BrokerOutcome::Bytes(vec![1, 2, 3, 4]),
            }),
            Err(TerminalError::UnsupportedPayload { .. })
        ));
        assert_eq!(preparation.request(), Some(&request));
        drop(preparation);
        assert_eq!(t.export_snapshot_v2().unwrap(), before);
        assert_eq!(t.next_source_sequence(), sequence);
        assert!(t.graphics_state().unwrap().resources().entries().is_empty());
    }

    #[test]
    fn broker_shared_memory_split_apc_replays_captured_bytes_without_a_mapping() {
        let mut t = graphics_terminal();
        let first = b"\x1b_Ga=T,t=s,f=32,s=1,v=1,S=4,O=2,i=26;L25h";
        let sequence = t.next_source_sequence();
        t.prepare_output_with_broker_v1(sequence, first)
            .unwrap()
            .finish()
            .unwrap()
            .commit_with(|_| Ok::<(), ()>(()))
            .unwrap();
        let mut replay = t.clone();
        let last = b"bWU=\x1b\\";
        let sequence = t.next_source_sequence();
        let mut preparation = t.prepare_output_with_broker_v1(sequence, last).unwrap();
        let request = preparation.request().unwrap().clone();
        assert_eq!(request.source_offset_end, last.len() as u32);
        let capture = BrokerCapture {
            request,
            outcome: BrokerOutcome::Bytes(vec![1, 2, 3, 4]),
        };
        preparation.supply(capture.clone()).unwrap();
        let ready = preparation.finish().unwrap();
        ready.commit_with(|_| Ok::<(), ()>(())).unwrap();
        replay
            .prepare_output_with_broker_captures_v1(sequence, last, &[capture])
            .unwrap()
            .finish()
            .unwrap()
            .commit_with(|_| Ok::<(), ()>(()))
            .unwrap();
        assert_eq!(
            replay.export_snapshot_v2().unwrap(),
            t.export_snapshot_v2().unwrap()
        );
        assert_eq!(t.graphics_state().unwrap().resources().entries().len(), 1);
    }

    #[test]
    fn broker_shared_memory_diagnostic_uses_completed_output_identity_without_reference() {
        let mut t = graphics_terminal();
        let first = b"\x1b_Ga=T,t=s,f=32,s=1,v=1,i=17;L25h";
        let sequence = t.next_source_sequence();
        let partial = t.prepare_output_with_broker_v1(sequence, first).unwrap();
        assert!(partial.request().is_none());
        partial
            .finish()
            .unwrap()
            .commit_with(|_| Ok::<(), ()>(()))
            .unwrap();

        let last = b"bWU=\x1b\\";
        let sequence = t.next_source_sequence();
        let mut preparation = t.prepare_output_with_broker_v1(sequence, last).unwrap();
        let request = preparation.request().unwrap().clone();
        assert_eq!(request.source_sequence, sequence);
        assert_eq!(request.source_offset_end, last.len() as u32);
        preparation
            .supply(BrokerCapture {
                request,
                outcome: BrokerOutcome::Failure(BrokerFailure::UnsupportedMedium),
            })
            .unwrap();
        let ready = preparation.finish().unwrap();
        let replies = crate::tests::host_effect_payloads(ready.host_effect_frame_v2());
        assert_eq!(replies.len(), 1);
        let reply = std::str::from_utf8(replies[0]).unwrap();
        assert!(reply.contains("i=17;ENOTSUP:"), "{reply}");
        assert!(
            reply.contains("unsupported_control=t=s; byte=0x73"),
            "{reply}"
        );
        assert!(reply.contains(&format!(
            "source_sequence={sequence}; source_offset_end={}; ordinal=0; source_offset_kind=APC_end",
            last.len()
        )), "{reply}");
        assert!(
            !reply.contains("/name") && !reply.contains("L25hbWU="),
            "{reply}"
        );
        ready.commit_with(|_| Ok::<(), ()>(())).unwrap();
        assert!(t.graphics_state().unwrap().resources().entries().is_empty());
    }

    #[test]
    fn broker_shared_memory_quiet_two_stays_silent() {
        let mut t = graphics_terminal();
        let sequence = t.next_source_sequence();
        let mut preparation = t
            .prepare_output_with_broker_v1(
                sequence,
                b"\x1b_Ga=T,t=s,f=32,s=1,v=1,i=17,q=2;L25hbWU=\x1b\\",
            )
            .unwrap();
        let request = preparation.request().unwrap().clone();
        preparation
            .supply(BrokerCapture {
                request,
                outcome: BrokerOutcome::Failure(BrokerFailure::UnsupportedMedium),
            })
            .unwrap();
        let ready = preparation.finish().unwrap();
        assert!(crate::tests::host_effect_payloads(ready.host_effect_frame_v2()).is_empty());
        ready.commit_with(|_| Ok::<(), ()>(())).unwrap();
        assert_eq!(t.next_source_sequence(), sequence + 1);
        assert!(t.graphics_state().unwrap().resources().entries().is_empty());
    }

    #[test]
    fn broker_shared_memory_query_success_quiet_two_stays_silent() {
        let mut t = graphics_terminal();
        let sequence = t.next_source_sequence();
        let mut preparation = t
            .prepare_output_with_broker_v1(
                sequence,
                b"\x1b_Ga=q,t=s,f=32,s=1,v=1,i=17,q=2,S=4,O=2;L25hbWU=\x1b\\",
            )
            .unwrap();
        let request = preparation.request().unwrap().clone();
        assert_eq!((request.read_offset, request.read_length), (2, Some(4)));
        preparation
            .supply(BrokerCapture {
                request,
                outcome: BrokerOutcome::Bytes(vec![1, 2, 3, 4]),
            })
            .unwrap();
        let ready = preparation.finish().unwrap();
        assert!(crate::tests::host_effect_payloads(ready.host_effect_frame_v2()).is_empty());
        ready.commit_with(|_| Ok::<(), ()>(())).unwrap();
        assert_eq!(t.next_source_sequence(), sequence + 1);
        assert!(t.graphics_state().unwrap().resources().entries().is_empty());
    }

    #[test]
    fn broker_file_denial_is_redacted_and_does_not_create_an_image() {
        let mut t = graphics_terminal();
        let sequence = t.next_source_sequence();
        let mut preparation = t
            .prepare_output_with_broker_v1(
                sequence,
                b"\x1b_Ga=T,t=f,f=32,s=1,v=1,i=23;QzovYS5yZ2Jh\x1b\\",
            )
            .unwrap();
        let request = preparation.request().unwrap().clone();
        preparation
            .supply(BrokerCapture {
                request,
                outcome: BrokerOutcome::Failure(BrokerFailure::Denied),
            })
            .unwrap();
        let ready = preparation.finish().unwrap();
        let replies = crate::tests::host_effect_payloads(ready.host_effect_frame_v2());
        let reply = std::str::from_utf8(replies[0]).unwrap();
        assert!(reply.contains("i=23;ENOTSUP:"));
        assert!(reply.contains("resource could not be read"));
        assert!(!reply.contains("unsupported_control=t=s"));
        assert!(!reply.contains("C:/a.rgba"));
        ready.commit_with(|_| Ok::<(), ()>(())).unwrap();
        assert!(t.graphics_state().unwrap().resources().entries().is_empty());
    }

    #[test]
    fn broker_read_failures_have_one_redacted_reply_for_file_and_shared_memory() {
        for medium in *b"fs" {
            let mut replies = Vec::new();
            for failure in [BrokerFailure::Denied, BrokerFailure::Unavailable] {
                let mut t = graphics_terminal();
                let output = format!(
                    "\x1b_Ga=q,t={},f=32,s=1,v=1,i=25;L25hbWU=\x1b\\",
                    char::from(medium)
                );
                let sequence = t.next_source_sequence();
                let mut preparation = t
                    .prepare_output_with_broker_v1(sequence, output.as_bytes())
                    .unwrap();
                let request = preparation.request().unwrap().clone();
                preparation
                    .supply(BrokerCapture {
                        request,
                        outcome: BrokerOutcome::Failure(failure),
                    })
                    .unwrap();
                let ready = preparation.finish().unwrap();
                let effects = crate::tests::host_effect_payloads(ready.host_effect_frame_v2());
                replies.push(effects[0].to_vec());
                ready.commit_with(|_| Ok::<(), ()>(())).unwrap();
                assert!(t.graphics_state().unwrap().resources().entries().is_empty());
            }
            assert_eq!(replies[0], replies[1]);
            let reply = std::str::from_utf8(&replies[0]).unwrap();
            assert!(reply.contains("i=25;ENOTSUP:"));
            assert!(reply.contains("Kitty broker resource could not be read"));
            assert!(!reply.contains("/name"));
        }
    }

    #[test]
    fn broker_file_unsupported_medium_reports_file_control_not_shared_memory() {
        let mut t = graphics_terminal();
        let bytes = b"\x1b_Ga=T,t=f,f=32,s=1,v=1,i=24;QzovYS5yZ2Jh\x1b\\";
        let sequence = t.next_source_sequence();
        let mut preparation = t.prepare_output_with_broker_v1(sequence, bytes).unwrap();
        let request = preparation.request().unwrap().clone();
        preparation
            .supply(BrokerCapture {
                request,
                outcome: BrokerOutcome::Failure(BrokerFailure::UnsupportedMedium),
            })
            .unwrap();
        let ready = preparation.finish().unwrap();
        let replies = crate::tests::host_effect_payloads(ready.host_effect_frame_v2());
        assert_eq!(replies.len(), 1);
        let reply = std::str::from_utf8(replies[0]).unwrap();
        assert!(
            reply.contains("unsupported_control=t=f; byte=0x66"),
            "{reply}"
        );
        assert!(reply.contains("source_offset_kind=APC_end"), "{reply}");
        assert!(!reply.contains("unsupported_control=t=s") && !reply.contains("C:/a.rgba"));
    }

    #[test]
    fn graphics_c1_replies_and_legacy_projection_do_not_leak_apc_to_outer_host() {
        let mut t = graphics_terminal();
        output(&mut t, b"\x1b G");
        assert_eq!(
            output(&mut t, b"\x1b_Ga=q,s=1,v=1,i=5;AQIDBA==\x1b\\"),
            vec![b"\x9fGi=5;OK\x9c".to_vec()]
        );
        output(&mut t, b"\x1b F");
        let pending = t
            .prepare_event_with_legacy_projection(Event::Output {
                source_sequence: t.next_source_sequence(),
                bytes: b"\x1b_Ga=T,s=1,v=1,i=7,C=1;AQIDBA==\x1b\\hello",
            })
            .unwrap();
        assert_eq!(pending.legacy_projection, b"hello");
        pending.publish(|_| Ok::<(), ()>(())).unwrap();
    }
    #[test]
    fn ed2_and_alternate_reset_remove_only_the_corresponding_placements() {
        let mut t = graphics_terminal();
        output(&mut t, b"\x1b_Ga=T,s=1,v=1,i=1,C=1;AQIDBA==\x1b\\");
        output(&mut t, b"\x1b[0K\x1b[0J");
        assert_eq!(t.graphics_state().unwrap().placements().len(), 1);
        output(
            &mut t,
            b"\x1b[?1049h\x1b_Ga=T,s=1,v=1,i=2,C=1;AQIDBA==\x1b\\",
        );
        assert_eq!(t.graphics_state().unwrap().placements().len(), 2);
        output(&mut t, b"\x1b[2J");
        assert_eq!(t.graphics_state().unwrap().placements().len(), 1);
        assert_eq!(t.graphics_state().unwrap().placements()[0].anchor.page, 1);
        output(
            &mut t,
            b"\x1b_Ga=T,s=1,v=1,i=3,C=1;AQIDBA==\x1b\\\x1b[?1049l",
        );
        assert_eq!(t.graphics_state().unwrap().placements().len(), 1);
        output(&mut t, b"\x1b[2J");
        assert!(t.graphics_state().unwrap().placements().is_empty());
        assert!(t.export_snapshot_v2().is_ok());
        output(&mut t, b"\x1b_Ga=T,s=1,v=1,m=1;AQID\x1b\\\x1b[2J");
        assert!(
            t.is_parser_incomplete(),
            "text clear is not a Kitty upload-cancel command"
        );
        output(&mut t, b"\x1b_G;BA==\x1b\\");
        assert_eq!(t.graphics_state().unwrap().placements().len(), 1);
    }
    #[test]
    fn anchor_index_resolves_wrapped_wide_cells_scaled_lines_and_reordered_rows() {
        let mut screen = crate::screen::Screen::new(4, 3);
        for c in "abc界de".chars() {
            screen.print(c).unwrap();
        }
        let line = screen.row_metadata[0].logical_line_id;
        let index = GraphicsAnchorIndex::new(&screen).unwrap();
        assert_eq!(index.locate(line, 2), Some((0, 2)));
        assert_eq!(index.locate(line, 3), Some((1, 0)));
        assert_eq!(index.locate(line, 6), Some((1, 3)));
        assert_eq!(index.locate(line, 7), None);
        assert_eq!(index.locate(u64::MAX, 0), None);
        let mut screen = crate::screen::Screen::new(10, 3);
        screen
            .set_line_rendition(crate::LineRendition::DoubleWidth)
            .unwrap();
        let line = screen.row_metadata[0].logical_line_id;
        let index = GraphicsAnchorIndex::new(&screen).unwrap();
        assert_eq!(index.locate(line, 4), Some((0, 8)));
        assert_eq!(index.locate(line, 5), None);
        screen.row_metadata.swap(1, 2);
        let index = GraphicsAnchorIndex::new(&screen).unwrap();
        assert_eq!(index.locate(2, 0), Some((1, 0)));
        assert_eq!(index.locate(1, 0), Some((2, 0)));
    }
    #[test]
    fn graphics_snapshot_is_required_versioned_state_and_old_renderers_refuse_it() {
        let mut terminal = terminal();
        let g = HostGeometryV1 {
            cell_width: 10,
            cell_height: 20,
            ..Default::default()
        };
        terminal
            .apply_event(Event::HostGeometry {
                source_sequence: 0,
                geometry: g,
            })
            .unwrap();
        terminal
            .apply_event(Event::Output {
                source_sequence: 1,
                bytes: b"hello",
            })
            .unwrap();
        let line = terminal.main.row_metadata[0].logical_line_id;
        let command = TransferDecoder::default()
            .push(b"Ga=T,s=1,v=1,i=4,p=2;AQIDBA==")
            .unwrap()
            .unwrap();
        terminal
            .graphics
            .as_mut()
            .unwrap()
            .apply_kitty(
                command,
                Anchor {
                    page: 1,
                    logical_line: line,
                    cell_offset: 5,
                },
                g,
                &[],
            )
            .unwrap();
        let bytes = terminal.export_snapshot_v2().unwrap();
        assert_eq!(u16::from_le_bytes([bytes[10], bytes[11]]), 22);
        let mut restored = Terminal::import_snapshot_v2(&bytes).unwrap();
        assert_eq!(restored.graphics_state(), terminal.graphics_state());
        assert_eq!(restored.export_snapshot_v2().unwrap(), bytes);
        for t in [&mut terminal, &mut restored] {
            t.apply_event(Event::Output {
                source_sequence: 2,
                bytes: b" world",
            })
            .unwrap();
        }
        assert_eq!(
            terminal.export_snapshot_v2().unwrap(),
            restored.export_snapshot_v2().unwrap()
        );
        assert!(matches!(
            terminal.export_render_frame_v4(),
            Err(TerminalError::RenderFrameVersionRequired)
        ));
    }
    #[test]
    fn graphics_capability_survives_empty_state_and_cannot_be_downgraded_by_changing_minor() {
        let terminal = terminal();
        let bytes = terminal.export_snapshot_v2().unwrap();
        assert!(
            Terminal::import_snapshot_v2(&bytes)
                .unwrap()
                .has_graphics_profile()
        );
        let legacy = Terminal::new(20, 4).unwrap();
        assert_eq!(
            u16::from_le_bytes(
                legacy.export_snapshot_v2().unwrap()[10..12]
                    .try_into()
                    .unwrap()
            ),
            0
        );
        let mut bad = bytes.clone();
        bad[10..12].copy_from_slice(&21u16.to_le_bytes());
        crate::snapshot::refresh_crc_for_test(&mut bad);
        assert!(matches!(
            Terminal::import_snapshot_v2(&bad),
            Err(TerminalError::IncompatibleSnapshot)
        ));
    }
    #[test]
    fn invalid_graphics_anchors_are_rejected_and_reset_preserves_profile_and_monotonic_ids() {
        let mut terminal = terminal();
        let g = HostGeometryV1 {
            cell_width: 10,
            cell_height: 20,
            ..Default::default()
        };
        let command = || {
            TransferDecoder::default()
                .push(b"Ga=T,s=1,v=1;AQIDBA==")
                .unwrap()
                .unwrap()
        };
        terminal
            .graphics
            .as_mut()
            .unwrap()
            .apply_kitty(
                command(),
                Anchor {
                    page: 6,
                    logical_line: 0,
                    cell_offset: 0,
                },
                g,
                &[],
            )
            .unwrap();
        assert!(matches!(
            terminal.export_snapshot_v2(),
            Err(TerminalError::MalformedSnapshot)
        ));
        let bad = crate::snapshot::export_unchecked_for_test(&terminal).unwrap();
        assert!(matches!(
            Terminal::import_snapshot_v2(&bad),
            Err(TerminalError::MalformedSnapshot)
        ));
        let old = terminal.graphics_state().unwrap().placements()[0].id;
        terminal
            .apply_event(Event::Output {
                source_sequence: 0,
                bytes: b"\x1bc",
            })
            .unwrap();
        assert!(terminal.has_graphics_profile());
        assert!(
            terminal
                .graphics_state()
                .unwrap()
                .resources()
                .entries()
                .is_empty()
        );
        terminal
            .graphics
            .as_mut()
            .unwrap()
            .apply_kitty(
                command(),
                Anchor {
                    page: 1,
                    logical_line: 0,
                    cell_offset: 0,
                },
                g,
                &[],
            )
            .unwrap();
        assert!(terminal.graphics_state().unwrap().placements()[0].id > old);
        assert!(terminal.export_snapshot_v2().is_ok());
    }
}
