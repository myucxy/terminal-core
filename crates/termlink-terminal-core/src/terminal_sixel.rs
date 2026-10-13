use crate::{
    Color, Terminal, TerminalError,
    graphics_layout::{self, ImageLayout},
    graphics_state::{Anchor, GraphicClip},
    parser::DcsSequence,
    sixel::SixelImage,
};

impl Terminal {
    pub(crate) fn dispatch_sixel(&mut self, sequence: &DcsSequence) -> Result<(), TerminalError> {
        let decoded = self.decode_terminal_sixel(sequence)?;
        let scrolling = self
            .graphics
            .as_ref()
            .expect("validated profile")
            .sixel_scrolling();
        let layout = decoded
            .image
            .as_ref()
            .map(|image| {
                graphics_layout::sixel(
                    image.width(),
                    image.height(),
                    decoded.aspect_numerator,
                    decoded.aspect_denominator,
                    self.host_geometry,
                )
            })
            .transpose()?;
        if layout.is_none() && decoded.cursor_x == 0 && decoded.cursor_y == 0 {
            return self.install_terminal_sixel(decoded, None, 0, 0, None);
        }
        self.host_geometry.validate()?;
        if self.host_geometry.cell_width == 0 {
            return Err(TerminalError::Unsupported(
                "SIXEL cursor requires ordered cell pixel geometry from the selected display host",
            ));
        }
        let cursor_columns = u64::from(decoded.cursor_x) / u64::from(self.host_geometry.cell_width);
        let cursor_rows = u64::from(decoded.cursor_y) * u64::from(decoded.aspect_numerator)
            / (u64::from(self.host_geometry.cell_height) * u64::from(decoded.aspect_denominator));
        if cursor_columns > u64::from(graphics_layout::MAX_PLACEMENT_CELLS)
            || cursor_rows > u64::from(graphics_layout::MAX_PLACEMENT_CELLS)
        {
            return Err(TerminalError::ProtocolResourceLimit(
                "SIXEL cursor exceeds 4096 placement cells",
            ));
        }
        let screen = self.active_screen();
        let stored_row = screen.visible_stored_row(screen.cursor_row) as usize;
        let scale = if screen.line_renditions[stored_row] == crate::LineRendition::SingleWidth {
            1
        } else {
            2
        };
        // VT340 chapter 14: scrolling starts at the text position; display mode starts at page home.
        let (column, row) = if scrolling {
            (screen.cursor_column * scale, screen.cursor_row)
        } else {
            (0, 0)
        };
        let (margin_left, margin_right) = self.scrolling_horizontal_bounds();
        let in_margins = scrolling
            && row >= self.margins.top
            && row < self.margins.bottom
            && column >= margin_left
            && column < margin_right;
        let (left, top, right, bottom) = if in_margins {
            (
                margin_left,
                self.margins.top,
                margin_right,
                self.margins.bottom,
            )
        } else {
            (0, 0, self.columns, self.rows)
        };
        let scroll = if in_margins {
            let image_bottom = row + layout.map_or(0, |layout| layout.rows);
            image_bottom
                .saturating_sub(bottom)
                .max((row + cursor_rows as u32).saturating_sub(bottom - 1))
        } else {
            0
        };
        // Install after scrolling so the incoming part below the old page is not prematurely clipped.
        let mut remaining = scroll;
        while remaining != 0 {
            let count = remaining.min(bottom - top);
            self.graphics_scroll_up(count, top, bottom, left, right)?;
            remaining -= count;
        }
        let image_row = row as i32 - scroll as i32;
        let clip = layout
            .map(|layout| {
                let oldest_row = if (left, top, right, bottom) == (0, 0, self.columns, self.rows) {
                    -(self.active_screen().viewport_top() as i32)
                } else {
                    top as i32
                };
                GraphicClip {
                    left: left.saturating_sub(column).min(layout.columns),
                    top: (oldest_row - image_row).max(0) as u32,
                    right: right.saturating_sub(column).min(layout.columns),
                    bottom: (bottom as i32 - image_row).max(0) as u32,
                }
            })
            .map(|clip| {
                let layout = layout.expect("clip has layout");
                GraphicClip {
                    top: clip.top.min(layout.rows),
                    bottom: clip.bottom.min(layout.rows),
                    ..clip
                }
            });
        self.install_terminal_sixel(decoded, layout, column, image_row, clip)?;
        if scrolling {
            let cursor_row = (i64::from(row) + cursor_rows as i64 - i64::from(scroll))
                .clamp(i64::from(top), i64::from(bottom - 1)) as u32;
            let stored_row = self.active_screen().visible_stored_row(cursor_row) as usize;
            let scale = if self.active_screen().line_renditions[stored_row]
                == crate::LineRendition::SingleWidth
            {
                1
            } else {
                2
            };
            let cursor_column = (column + cursor_columns as u32).min(right - 1) / scale;
            self.active_screen_mut()
                .position_cursor(cursor_row, cursor_column);
        }
        Ok(())
    }

    fn install_terminal_sixel(
        &mut self,
        mut decoded: SixelImage,
        mut layout: Option<ImageLayout>,
        column: u32,
        row: i32,
        mut clip: Option<GraphicClip>,
    ) -> Result<(), TerminalError> {
        if clip.is_some_and(|clip| clip.left >= clip.right || clip.top >= clip.bottom) {
            decoded.image = None;
            layout = None;
            clip = None;
        }
        let screen = self.active_screen();
        let stored_position = screen.viewport_top() as i32 + row;
        let stored_row = stored_position.clamp(0, screen.row_metadata.len() as i32 - 1) as usize;
        let metadata = &screen.row_metadata[stored_row];
        let anchor = Anchor {
            page: if self.active_alternate {
                7
            } else {
                self.active_page()
            },
            logical_line: metadata.logical_line_id,
            cell_offset: metadata.logical_cell_offset,
        };
        let cell_resources = self.cell_graphic_resources();
        self.graphics
            .as_mut()
            .expect("validated profile")
            .apply_sixel_layout(
                decoded,
                anchor,
                layout,
                (column as i32, stored_position - stored_row as i32),
                clip,
                &cell_resources,
            )?;
        Ok(())
    }

    fn sixel_color(&self, foreground: bool) -> Option<[u8; 3]> {
        // VT340 chapter 14 defines P2 against the current background. This profile uses current
        // text ink before an explicit '#' selection; the independent SIXEL registers stay intact.
        let attributes = self.active_screen().attributes;
        let foreground = foreground ^ attributes.inverse ^ self.modes.reverse_video;
        let color = if foreground {
            attributes.foreground
        } else {
            attributes.background
        };
        if let Color::Rgb(red, green, blue) = color {
            return Some([red, green, blue]);
        }
        let color = match color {
            Color::Indexed(index) => self.osc.palette(index),
            Color::Default => {
                let slot = usize::from(!foreground);
                match self.osc.color_aliases()[slot] {
                    Some(index @ 0..=255) => self.osc.palette(index as u8),
                    Some(256) => self.osc.foreground(),
                    Some(257) => self.osc.background(),
                    None if foreground => self.osc.foreground(),
                    None => self.osc.background(),
                    _ => unreachable!("validated OSC color alias"),
                }
            }
            Color::Rgb(..) => unreachable!(),
        }?;
        Some([color.red, color.green, color.blue])
    }

    fn decode_terminal_sixel(
        &mut self,
        sequence: &DcsSequence,
    ) -> Result<crate::sixel::SixelImage, TerminalError> {
        let graphics = self.graphics.as_ref().ok_or(TerminalError::Unsupported(
            "SIXEL requires the graphics session profile",
        ))?;
        let parameters = sequence.parameters();
        for (index, &value) in parameters.iter().enumerate() {
            let value = if value == u16::MAX { 0 } else { value };
            let reason = match index {
                0 if value > 9 => Some("SIXEL P1 aspect macro must be 0..9"),
                1 if value > 2 => Some("SIXEL P2 background mode must be 0..2"),
                3.. => Some("SIXEL permits only P1, P2 and P3 header parameters"),
                _ => None,
            };
            if let Some(reason) = reason {
                self.diagnostics.trace.reject_parameter(index);
                return Err(TerminalError::Unsupported(reason));
            }
        }
        crate::sixel::decode_with_colors(
            parameters,
            sequence.payload(),
            graphics.palette(),
            self.sixel_color(true),
            self.sixel_color(false),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Event, HostGeometryV1, OutputProfile, TerminalOptions, WidthProfile};

    fn terminal() -> Terminal {
        Terminal::new_with_graphics_profile(
            8,
            4,
            0,
            TerminalOptions::default(),
            OutputProfile::Utf8,
            WidthProfile::Unicode16,
        )
        .unwrap()
    }

    fn configured() -> Terminal {
        let mut terminal = terminal();
        terminal
            .apply_event(Event::HostGeometry {
                source_sequence: 0,
                geometry: HostGeometryV1 {
                    cell_width: 2,
                    cell_height: 6,
                    ..Default::default()
                },
            })
            .unwrap();
        output(&mut terminal, b"\x1b]10;#112233\x07\x1b]11;#445566\x07");
        terminal
    }

    fn output(terminal: &mut Terminal, bytes: &[u8]) {
        terminal
            .apply_event(Event::Output {
                source_sequence: terminal.next_source_sequence(),
                bytes,
            })
            .unwrap();
    }

    fn rendered(terminal: &Terminal) -> crate::render_frame_v5::Frame {
        crate::render_frame_v5::Frame::decode(&terminal.export_render_frame_v5(0).unwrap()).unwrap()
    }

    fn roundtrip(terminal: &Terminal) {
        let restored =
            Terminal::import_snapshot_v2(&terminal.export_snapshot_v2().unwrap()).unwrap();
        assert_eq!(&restored, terminal);
        assert_eq!(
            restored.export_render_frame_v5(0).unwrap(),
            terminal.export_render_frame_v5(0).unwrap()
        );
    }

    #[test]
    fn sixel_output_uses_ordered_sgr_osc_colors_and_preserves_register_zero() {
        let mut terminal = configured();
        output(&mut terminal, b"\x1bP7q@\x1b\\");
        let image = rendered(&terminal);
        assert_eq!(image.images.len(), 1);
        let pixels = image.resources.entries()[0].image().pixels();
        assert_eq!(&pixels[..4], &[0x11, 0x22, 0x33, 255]);
        assert!(
            pixels[4..]
                .chunks_exact(4)
                .all(|p| p == [0x44, 0x55, 0x66, 255])
        );
        assert_eq!(terminal.graphics_state().unwrap().palette()[0], [0; 3]);
        output(
            &mut terminal,
            b"\x1b[38;2;1;2;3;48;2;4;5;6;7m\x1bP7q@\x1b\\",
        );
        let image = rendered(&terminal);
        let pixels = image.resources.entries().last().unwrap().image().pixels();
        assert_eq!(&pixels[..4], &[4, 5, 6, 255]);
        assert_eq!(&pixels[4..8], &[1, 2, 3, 255]);
        output(
            &mut terminal,
            b"\x1b]4;7;#abcdef\x07\x1b[1;7;256,|\x1b[0m\x1bP7q@\x1b\\",
        );
        let image = rendered(&terminal);
        let pixels = image.resources.entries().last().unwrap().image().pixels();
        assert_eq!(&pixels[..4], &[0xab, 0xcd, 0xef, 255]);
        assert_eq!(&pixels[4..8], &[0x11, 0x22, 0x33, 255]);
        roundtrip(&terminal);
    }

    #[test]
    fn sixel_palette_only_transparency_and_unknown_theme_are_distinct() {
        let mut terminal = terminal();
        output(&mut terminal, b"\x1bPq#3;2;100;0;0\x1b\\");
        assert_eq!(terminal.graphics_state().unwrap().palette()[3], [255, 0, 0]);
        assert!(terminal.graphics_state().unwrap().placements().is_empty());
        terminal
            .apply_event(Event::HostGeometry {
                source_sequence: terminal.next_source_sequence(),
                geometry: HostGeometryV1 {
                    cell_width: 2,
                    cell_height: 6,
                    ..Default::default()
                },
            })
            .unwrap();
        output(&mut terminal, b"\x1bP7;1q\"1;1;2;6#3@\x1b\\");
        let image = rendered(&terminal);
        assert_eq!(
            &image.resources.entries()[0].image().pixels()[..8],
            &[255, 0, 0, 255, 0, 0, 0, 0]
        );
        roundtrip(&terminal);
        let before = terminal.graphics_state().unwrap().clone();
        assert_eq!(
            terminal.apply_event(Event::Output {
                source_sequence: terminal.next_source_sequence(),
                bytes: b"\x1bP7q#3@\x1b\\"
            }),
            Err(TerminalError::Unsupported(
                "opaque SIXEL requires an ordered terminal background color"
            ))
        );
        assert_eq!(terminal.graphics_state(), Some(&before));
    }

    #[test]
    fn sixel_every_split_commits_only_at_st_and_roundtrips_pixels_palette_cursor() {
        let bytes = b"\x1bP7;1;99q\"1;1;3;6#1;2;100;0;0!2@$#2;2;0;100;0A-\x1b\\";
        for split in 0..=bytes.len() {
            let mut terminal = configured();
            output(&mut terminal, b"\x1b[2;3H");
            output(&mut terminal, &bytes[..split]);
            if split != bytes.len() {
                assert!(
                    terminal
                        .graphics_state()
                        .unwrap()
                        .resources()
                        .entries()
                        .is_empty()
                );
            }
            output(&mut terminal, &bytes[split..]);
            let frame = rendered(&terminal);
            assert_eq!(frame.images.len(), 1);
            let pixels = frame.resources.entries()[0].image().pixels();
            assert_eq!(&pixels[..12], &[255, 0, 0, 255, 255, 0, 0, 255, 0, 0, 0, 0]);
            assert_eq!(&pixels[12..16], &[0, 255, 0, 255]);
            assert_eq!(terminal.graphics_state().unwrap().palette()[2], [0, 255, 0]);
            assert_eq!(
                (
                    terminal.active_screen().cursor_row,
                    terminal.active_screen().cursor_column
                ),
                (2, 2)
            );
            roundtrip(&terminal);
        }
    }

    #[test]
    fn sixel_dec_sdm_page_home_preserves_cursor_and_pending_wrap_then_restores_scrolling() {
        let mut terminal = configured();
        output(&mut terminal, b"\x1b[3;8HX\x1b[?80l");
        assert!(terminal.active_screen().wrap_pending);
        output(&mut terminal, b"\x1bP7;1q#1!20~-~-~-~-~\x1b\\");
        assert_eq!(
            (
                terminal.active_screen().cursor_row,
                terminal.active_screen().cursor_column
            ),
            (2, 7)
        );
        assert!(terminal.active_screen().wrap_pending);
        let frame = rendered(&terminal);
        assert_eq!((frame.images[0].column, frame.images[0].row), (0, 0));
        assert_eq!(frame.images[0].clip.right, 8);
        assert_eq!(frame.images[0].clip.bottom, 4);
        roundtrip(&terminal);
        output(&mut terminal, b"\x1b[?80h\x1b[1;3H\x1bP7;1q#1!4@-\x1b\\");
        assert_eq!(
            (
                terminal.active_screen().cursor_row,
                terminal.active_screen().cursor_column
            ),
            (1, 2)
        );
        assert!(!terminal.active_screen().wrap_pending);
    }

    #[test]
    fn sixel_scrolling_clips_and_reanchors_without_history_and_obeys_margins() {
        let mut terminal = configured();
        output(&mut terminal, b"\x1b[4;1H\x1bP7;1q#1~-~-~\x1b\\");
        let frame = rendered(&terminal);
        assert_eq!(frame.images.len(), 1);
        assert_eq!((frame.images[0].column, frame.images[0].row), (0, 1));
        assert_eq!(
            (
                terminal.active_screen().cursor_row,
                terminal.active_screen().cursor_column
            ),
            (3, 0)
        );
        roundtrip(&terminal);

        let mut terminal = configured();
        output(
            &mut terminal,
            b"\x1b[2;3r\x1b[3;1H\x1bP7;1q#1~-~-~-~-~\x1b\\",
        );
        let frame = rendered(&terminal);
        assert_eq!(frame.images.len(), 1);
        assert!(frame.images[0].row < 0);
        assert_eq!(frame.images[0].clip.top as i32 + frame.images[0].row, 1);
        assert_eq!(frame.images[0].clip.bottom as i32 + frame.images[0].row, 3);
        assert_eq!(terminal.active_screen().cursor_row, 2);
        roundtrip(&terminal);
    }

    #[test]
    fn sixel_cancellation_and_prepared_completion_never_publish_half_an_image() {
        let mut terminal = configured();
        output(&mut terminal, b"\x1bP7;1q#3;2;100;0;0!4~");
        assert!(terminal.is_parser_incomplete());
        assert_eq!(
            terminal.export_snapshot_v2(),
            Err(TerminalError::SnapshotNotEligible)
        );
        let before = terminal.clone();
        let prepared = terminal
            .prepare_event_with_host_effects_v2(Event::Output {
                source_sequence: terminal.next_source_sequence(),
                bytes: b"\x1b\\",
            })
            .unwrap();
        drop(prepared);
        assert_eq!(terminal, before);
        output(&mut terminal, b"\x18");
        assert!(!terminal.is_parser_incomplete());
        assert!(
            terminal
                .graphics_state()
                .unwrap()
                .resources()
                .entries()
                .is_empty()
        );
        assert_eq!(
            terminal.graphics_state().unwrap().palette()[3],
            crate::sixel::default_palette()[3]
        );
        output(&mut terminal, b"\x1bP7;1q#3~\x1a");
        assert!(
            terminal
                .graphics_state()
                .unwrap()
                .resources()
                .entries()
                .is_empty()
        );
        roundtrip(&terminal);
    }

    #[test]
    fn sixel_header_payload_and_limits_reject_atomically_with_precise_diagnostics() {
        for (bytes, diagnostic) in [
            (
                b"\x1bP10q#1~\x1b\\".as_slice(),
                "rejected_parameter_index=0\nrejected_parameter=10\n",
            ),
            (
                b"\x1bP7;3q#1~\x1b\\",
                "rejected_parameter_index=1\nrejected_parameter=3\n",
            ),
            (
                b"\x1bP7;1;0;4q#1~\x1b\\",
                "rejected_parameter_index=3\nrejected_parameter=4\n",
            ),
            (
                b"\x1bP7;1q#1;3;0;0;0@\x1b\\",
                "retained payload byte 3: 0x33",
            ),
            (
                b"\x1bP7;1q#1;2;100;0;0!4097@\x1b\\",
                "SIXEL horizontal position exceeds 4096",
            ),
        ] {
            let mut terminal = configured();
            let before = terminal.graphics_state().unwrap().clone();
            let sequence = terminal.next_source_sequence();
            assert!(
                terminal
                    .apply_event(Event::Output {
                        source_sequence: sequence,
                        bytes
                    })
                    .is_err()
            );
            assert_eq!(terminal.next_source_sequence(), sequence);
            assert_eq!(terminal.graphics_state(), Some(&before));
            let actual = terminal.failure_diagnostic_v2().unwrap();
            assert!(
                actual.contains(diagnostic),
                "expected {diagnostic:?} in {actual}"
            );
        }
    }

    #[test]
    fn sixel_geometry_is_ordered_and_legacy_profile_still_fails_closed() {
        let mut terminal = terminal();
        let before = terminal.graphics_state().unwrap().clone();
        assert!(
            terminal
                .apply_event(Event::Output {
                    source_sequence: 0,
                    bytes: b"\x1bP7;1q#1~\x1b\\"
                })
                .is_err()
        );
        assert_eq!(terminal.graphics_state(), Some(&before));
        for bytes in [b"\x1bP7;1q#1~\x1b\\".as_slice(), b"\x1b[?80h"] {
            let mut legacy = Terminal::new(8, 4).unwrap();
            assert!(matches!(
                legacy.apply_event(Event::Output {
                    source_sequence: 0,
                    bytes
                }),
                Err(TerminalError::Unsupported(_))
            ));
            assert!(!legacy.has_graphics_profile());
        }
        let mut terminal = configured();
        output(&mut terminal, b"\x1bPq#1~\x1b\\");
        let first = terminal.graphics_state().unwrap().placements()[0].layout;
        terminal
            .apply_event(Event::HostGeometry {
                source_sequence: terminal.next_source_sequence(),
                geometry: HostGeometryV1 {
                    cell_width: 4,
                    cell_height: 12,
                    ..Default::default()
                },
            })
            .unwrap();
        output(&mut terminal, b"\x1bPq#1~\x1b\\");
        let placements = terminal.graphics_state().unwrap().placements();
        assert_eq!(placements[0].layout, first);
        assert_eq!(first.width, graphics_layout::Ratio::new(1, 2));
        assert_eq!(first.height, graphics_layout::Ratio::new(2, 1));
        assert_eq!(
            placements[1].layout.width,
            graphics_layout::Ratio::new(1, 4)
        );
        assert_eq!(
            placements[1].layout.height,
            graphics_layout::Ratio::new(1, 1)
        );
        roundtrip(&terminal);
    }

    #[test]
    fn sixel_late_resource_failure_rolls_back_scrolling_text_palette_and_ids() {
        let mut terminal = configured();
        output(&mut terminal, b"one\r\ntwo\r\nthree\r\nfour\x1b[4;1H");
        output(
            &mut terminal,
            &b"\x1bP7;1q#1@\x1b\\".repeat(crate::graphics_resources::MAX_IMAGE_RESOURCES),
        );
        let screen = terminal.active_screen().clone();
        let graphics = terminal.graphics_state().unwrap().clone();
        let sequence = terminal.next_source_sequence();
        assert!(matches!(
            terminal.apply_event(Event::Output {
                source_sequence: sequence,
                bytes: b"\x1bP7;1q#1;2;100;0;0~-~-~\x1b\\",
            }),
            Err(TerminalError::ProtocolResourceLimit(_))
        ));
        assert_eq!(terminal.next_source_sequence(), sequence);
        assert_eq!(terminal.active_screen(), &screen);
        assert_eq!(terminal.graphics_state(), Some(&graphics));
    }
}
