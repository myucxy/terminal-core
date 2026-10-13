//! End-to-end soft-font tests use terminal output, never synthetic Cell references.
use crate::{
    Color, Event, HISTORY_LATEST_ANCHOR, HistoryCapture, HistoryPage, HistoryPageRequest,
    HostGeometryV1, OutputProfile, Terminal, TerminalOptions, WidthProfile,
    charset::GraphicRegister,
    drcs_bank::GlyphReference,
    graphics_resources::{GraphicsResources, ImageFormat},
    render_frame_v5::Frame,
};

const PROFILES: [WidthProfile; 2] = [WidthProfile::LegacyV1, WidthProfile::Unicode16];

fn terminal(profile: WidthProfile) -> Terminal {
    let mut terminal = Terminal::new_with_graphics_profile(
        80,
        24,
        0,
        TerminalOptions {
            history_capacity_rows: 256,
        },
        OutputProfile::Utf8,
        profile,
    )
    .unwrap();
    terminal
        .apply_event(Event::HostGeometry {
            source_sequence: terminal.next_source_sequence(),
            geometry: HostGeometryV1 {
                cell_width: 10,
                cell_height: 16,
                ..Default::default()
            },
        })
        .unwrap();
    terminal
}

fn output(terminal: &mut Terminal, bytes: &[u8]) {
    let pending = terminal
        .prepare_event_with_host_effects_v2(Event::Output {
            source_sequence: terminal.next_source_sequence(),
            bytes,
        })
        .unwrap();
    pending.publish(|_| Ok::<(), ()>(())).unwrap();
}

fn download(
    terminal: &mut Terminal,
    slot: u8,
    start: u8,
    erase: u8,
    characters96: bool,
    name: &[u8],
    mask: &[u8],
) {
    // Repository-owned DECDLD fixtures: 5 x 8 matrix, full 10 x 16 cell, 80 x 24 rendition.
    let mut bytes = format!(
        "\x1bP{slot};{start};{erase};5;1;2;8;{}{{",
        u8::from(characters96)
    )
    .into_bytes();
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(mask);
    bytes.extend_from_slice(b"\x1b\\");
    output(terminal, &bytes);
}

fn glyph(terminal: &Terminal, row: u32, column: u32) -> GlyphReference {
    let cell = terminal.cell(row, column).unwrap();
    assert_eq!(cell.character, '\u{fffd}');
    assert_eq!(cell.width, 1);
    assert!(!cell.continuation);
    assert!(cell.combining.is_empty());
    cell.glyph.expect("downloaded glyph at requested cell")
}

fn pixels(resources: &GraphicsResources, glyph: GlyphReference) -> Vec<u8> {
    let image = resources.get(glyph.resource).unwrap().image();
    assert_eq!(image.format(), ImageFormat::Alpha8);
    (0..glyph.height as usize)
        .flat_map(|row| {
            let first = row * image.width() as usize + glyph.x as usize;
            image.pixels()[first..first + glyph.width as usize]
                .iter()
                .copied()
        })
        .collect()
}

fn frame(terminal: &Terminal, offset: u32) -> Frame {
    Frame::decode(&terminal.export_render_frame_v5(offset).unwrap()).unwrap()
}

fn roundtrip(terminal: &Terminal) -> Terminal {
    let bytes = terminal.export_snapshot_v2().unwrap();
    let restored = Terminal::import_snapshot_v2(&bytes).unwrap();
    assert_eq!(restored, *terminal);
    assert_eq!(restored.export_snapshot_v2().unwrap(), bytes);
    restored
}

fn history_request(capture: &HistoryCapture) -> HistoryPageRequest {
    HistoryPageRequest {
        revision: capture.revision(),
        before_line_id: HISTORY_LATEST_ANCHOR,
        max_lines: 128,
        max_cells: 65_536,
        max_utf8_bytes: 4 * 1024 * 1024,
    }
}

fn assert_frame_pixels(frame: &Frame, resource: u64, expected: &[u8]) {
    let draws: Vec<_> = frame
        .glyphs
        .iter()
        .filter(|draw| draw.glyph.resource == resource)
        .collect();
    assert!(
        !draws.is_empty(),
        "missing frozen glyph resource {resource}"
    );
    for draw in draws {
        assert_eq!(pixels(&frame.resources, draw.glyph), expected);
    }
}

fn text_colors(frame: &Frame, row: u32, column: u32) -> (u32, u32) {
    let bytes = &frame.text_v4;
    let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let rows = word(96 + 4 * 24 + 8) as usize;
    let record = rows + 8 + frame.rows as usize * 8 + (row * frame.columns + column) as usize * 12;
    let rendition_id = word(record + 4) as usize;
    let renditions = word(96 + 24 + 8) as usize;
    (
        word(renditions + rendition_id * 16 + 4),
        word(renditions + rendition_id * 16 + 8),
    )
}

#[test]
fn arbitrary_viewport_download_and_resize_keep_soft_glyphs_in_live_and_frozen_frames() {
    let mut direct = terminal(WidthProfile::Unicode16);
    direct
        .apply_event(Event::Resize {
            source_sequence: direct.next_source_sequence(),
            columns: 106,
            rows: 32,
        })
        .unwrap();
    download(&mut direct, 1, 1, 0, false, b" @", b"~");
    output(&mut direct, b"\x1b( @!");
    let direct_glyph = glyph(&direct, 0, 0);
    let direct_pixels = pixels(direct.graphics_state().unwrap().resources(), direct_glyph);
    let direct_frame = frame(&direct, 0);
    assert!(direct_frame.images.is_empty());
    assert_frame_pixels(&direct_frame, direct_glyph.resource, &direct_pixels);

    let mut resized = terminal(WidthProfile::Unicode16);
    download(&mut resized, 1, 1, 0, false, b" @", b"~");
    output(&mut resized, b"\x1b( @!");
    let old = glyph(&resized, 0, 0);
    let old_pixels = pixels(resized.graphics_state().unwrap().resources(), old);
    resized
        .apply_event(Event::Resize {
            source_sequence: resized.next_source_sequence(),
            columns: 106,
            rows: 32,
        })
        .unwrap();
    assert!(
        resized
            .main
            .cells
            .iter()
            .any(|cell| cell.glyph == Some(old))
    );
    output(&mut resized, b"\x1b[2;1H!");
    assert_eq!(glyph(&resized, 1, 0), old);
    let live = frame(&resized, 0);
    assert!(live.images.is_empty());
    assert_frame_pixels(&live, old.resource, &old_pixels);
    resized = roundtrip(&resized);

    output(&mut resized, b"\x1b(B\x1b[1;12H\r\n\r\n");
    output(&mut resized, &b"tail\r\n".repeat(40));
    let capture = HistoryCapture::capture(&resized).unwrap();
    let page = capture.page_v5(history_request(&capture)).unwrap();
    let frozen = page
        .frames
        .iter()
        .map(|bytes| Frame::decode(bytes).unwrap())
        .find(|frame| {
            frame
                .glyphs
                .iter()
                .any(|draw| draw.glyph.resource == old.resource)
        })
        .expect("resized glyph survived in frozen history");
    assert!(frozen.images.is_empty());
    assert_frame_pixels(&frozen, old.resource, &old_pixels);
}

#[test]
fn downloaded_atlases_survive_editing_reflow_snapshot_and_frozen_history() {
    for profile in PROFILES {
        let mut terminal = terminal(profile);
        download(&mut terminal, 1, 1, 0, false, b" @", b"~");
        output(
            &mut terminal,
            b"\x1b( @\x1b[38;2;10;20;30;48;2;40;50;60m!\x1b[2b",
        );
        let old = glyph(&terminal, 0, 0);
        assert_eq!(glyph(&terminal, 0, 1), old);
        assert_eq!(glyph(&terminal, 0, 2), old);
        let old_pixels = pixels(terminal.graphics_state().unwrap().resources(), old);
        assert_eq!(old_pixels.iter().filter(|pixel| **pixel == 255).count(), 6);

        // DECSC/DECRC must restore the exact soft designation and rendition.
        output(&mut terminal, b"\x1b7\x1b(BX\x1b8!");
        assert_eq!(glyph(&terminal, 0, 3), old);
        assert_eq!(
            terminal
                .charset
                .designation(GraphicRegister::G0)
                .designator(),
            b" @"
        );
        assert_eq!(
            terminal.cell(0, 3).unwrap().attributes.foreground,
            Color::Rgb(10, 20, 30)
        );
        output(&mut terminal, b"\x1b[33;2;1;2;3$x");
        for column in 0..3 {
            assert_eq!(glyph(&terminal, 1, column), old);
        }

        // DECCRA copies both rows to page 2, including their frozen references.
        output(&mut terminal, b"\x1b[1;1;2;4;1;1;1;2$v\x1b[2 P");
        assert_eq!(glyph(&terminal, 0, 0), old);
        assert_eq!(glyph(&terminal, 1, 2), old);
        output(&mut terminal, b"\x1b[1 P\x1b[1;9H!");
        assert_eq!(glyph(&terminal, 0, 8), old);

        download(&mut terminal, 1, 1, 0, false, b" @", b"@");
        output(&mut terminal, b"!\x1b[1b");
        let new = glyph(&terminal, 0, 9);
        assert_ne!(new.resource, old.resource);
        assert_eq!(glyph(&terminal, 0, 10), new);
        assert_eq!(glyph(&terminal, 0, 0), old);
        let new_pixels = pixels(terminal.graphics_state().unwrap().resources(), new);
        assert_eq!(new_pixels.iter().filter(|pixel| **pixel == 255).count(), 1);
        assert_eq!(
            pixels(terminal.graphics_state().unwrap().resources(), old),
            old_pixels
        );
        let live = frame(&terminal, 0);
        assert_frame_pixels(&live, old.resource, &old_pixels);
        assert_frame_pixels(&live, new.resource, &new_pixels);
        assert_eq!(text_colors(&live, 0, 0), (0x020a_141e, 0x0228_323c));
        assert_eq!(text_colors(&live, 0, 9), (0x020a_141e, 0x0228_323c));
        terminal = roundtrip(&terminal);

        // Seal the glyph rows, create a wrapped line, and scroll all of them into history.
        output(&mut terminal, b"\x1b(B\x1b[1;12H\r\n\r\n");
        output(&mut terminal, &[b'x'; 100]);
        output(&mut terminal, b"\r\n");
        output(&mut terminal, &b"tail\r\n".repeat(30));
        assert!(terminal.scrollback_info().0 > 0);
        terminal
            .apply_event(Event::Resize {
                source_sequence: terminal.next_source_sequence(),
                columns: 40,
                rows: 24,
            })
            .unwrap();
        assert!(
            terminal
                .main
                .cells
                .iter()
                .any(|cell| cell.glyph == Some(old))
        );
        assert!(
            terminal
                .main
                .cells
                .iter()
                .any(|cell| cell.glyph == Some(new))
        );
        terminal = roundtrip(&terminal);
        let offset = terminal.scrollback_info().0;
        let live_history = frame(&terminal, offset);
        assert_frame_pixels(&live_history, old.resource, &old_pixels);
        assert_frame_pixels(&live_history, new.resource, &new_pixels);

        let capture = HistoryCapture::capture(&terminal).unwrap();
        let frozen_view = capture.viewport_v5(offset).unwrap().0;
        let frozen_frame = Frame::decode(&frozen_view).unwrap();
        assert_frame_pixels(&frozen_frame, old.resource, &old_pixels);
        assert_frame_pixels(&frozen_frame, new.resource, &new_pixels);
        let page = capture.page_v5(history_request(&capture)).unwrap();
        let frozen_page = page.encode_v2(17).unwrap();
        assert_eq!(HistoryPage::decode_v2(&frozen_page, 17).unwrap(), page);
        for (resource, expected) in [(old.resource, &old_pixels), (new.resource, &new_pixels)] {
            let frames: Vec<_> = page
                .frames
                .iter()
                .map(|bytes| Frame::decode(bytes).unwrap())
                .collect();
            let containing = frames
                .iter()
                .find(|frame| {
                    frame
                        .glyphs
                        .iter()
                        .any(|draw| draw.glyph.resource == resource)
                })
                .expect("glyph survived in a sealed history line");
            assert_frame_pixels(containing, resource, expected);
        }

        // Each operation runs a different graphics collection path while history and page 2 pin A.
        for bytes in [
            b"\x1b[?1049h\x1b[?1049l".as_slice(),
            b"\x1b_Ga=T,s=1,v=1,i=7,C=1;AQIDBA==\x1b\\",
            b"\x1b_Ga=d,d=A\x1b\\",
            b"\x1bP0;1q#1;2;100;0;0#1~\x1b\\",
        ] {
            output(&mut terminal, bytes);
            assert_eq!(
                pixels(terminal.graphics_state().unwrap().resources(), old),
                old_pixels
            );
            assert_eq!(
                pixels(terminal.graphics_state().unwrap().resources(), new),
                new_pixels
            );
        }
        terminal = roundtrip(&terminal);

        // A has no bank entry now. Its final live references disappear only after both pages/history clear.
        output(&mut terminal, b"\x1b[2 P\x1b[2J\x1b[1 P\x1b[2J\x1b[3J");
        assert!(!terminal.cell_graphic_resources().contains(&old.resource));
        assert!(
            terminal
                .graphics_state()
                .unwrap()
                .resources()
                .get(old.resource)
                .is_none()
        );
        assert!(
            terminal
                .graphics_state()
                .unwrap()
                .resources()
                .get(new.resource)
                .is_some()
        );
        assert_eq!(capture.viewport_v5(offset).unwrap().0, frozen_view);
        assert_eq!(
            capture
                .page_v5(history_request(&capture))
                .unwrap()
                .encode_v2(17)
                .unwrap(),
            frozen_page
        );
        assert_frame_pixels(
            &Frame::decode(&frozen_view).unwrap(),
            old.resource,
            &old_pixels,
        );
        roundtrip(&terminal);
    }
}

#[test]
fn output_mapping_covers_alias_shadowing_94_96_gl_gr_delete_and_single_shifts() {
    for profile in PROFILES {
        let mut terminal = terminal(profile);
        download(&mut terminal, 1, 1, 0, false, b"1", b"~");
        output(&mut terminal, b"\x1b(B!\x1b(1!\"\x1b(;!");
        assert_eq!(terminal.cell(0, 0).unwrap().character, '!');
        assert!(terminal.cell(0, 0).unwrap().glyph.is_none());
        let font94 = glyph(&terminal, 0, 1);
        for column in [2, 3] {
            assert_eq!(terminal.cell(0, column).unwrap().character, ' ');
            assert!(terminal.cell(0, column).unwrap().glyph.is_none());
        }
        output(&mut terminal, b"\x1b(B\x1b*1\x1bN ");
        assert_eq!(
            terminal.charset.pending_single_shift(),
            Some(GraphicRegister::G2)
        );
        output(&mut terminal, b"!x\x1b%@\x1b}\xa1\x1bN\xa0");
        assert_eq!(glyph(&terminal, 0, 5), font94);
        assert_eq!(terminal.cell(0, 6).unwrap().character, 'x');
        assert_eq!(glyph(&terminal, 0, 7), font94);
        assert_eq!(terminal.cell(0, 8).unwrap().character, '\u{a0}');
        assert!(terminal.cell(0, 8).unwrap().glyph.is_none());
        assert_eq!(
            terminal.charset.pending_single_shift(),
            Some(GraphicRegister::G2)
        );
        output(&mut terminal, b"\xa1\x7fQ");
        assert_eq!(glyph(&terminal, 0, 9), font94);
        assert_eq!(terminal.cell(0, 10).unwrap().character, 'Q');
        assert_eq!(terminal.cursor(), (0, 11));
        assert_eq!(terminal.charset.pending_single_shift(), None);

        download(&mut terminal, 2, 0, 0, true, b"1", b"@");
        download(&mut terminal, 2, 95, 1, true, b"1", b"~");
        output(
            &mut terminal,
            b"\x1b/1\x1bO \x1bO\x7f\x1bo\x7f\x0f\x1b|\xa0\xff",
        );
        let font96_space = glyph(&terminal, 0, 11);
        let font96_delete = glyph(&terminal, 0, 12);
        assert_ne!(font96_space.resource, font94.resource);
        assert_eq!(font96_space.resource, font96_delete.resource);
        assert_eq!(font96_space.x, 0);
        assert_eq!(font96_delete.x, 95 * font96_delete.width);
        assert_eq!(glyph(&terminal, 0, 13), font96_delete);
        assert_eq!(glyph(&terminal, 0, 14), font96_space);
        assert_eq!(glyph(&terminal, 0, 15), font96_delete);
        assert_eq!(terminal.cursor(), (0, 16));
        assert_eq!(terminal.charset.pending_single_shift(), None);
        assert_eq!(frame(&terminal, 0).glyphs.len(), 9);
        roundtrip(&terminal);
    }
}

#[test]
fn unicode_extensions_never_merge_into_or_widen_downloaded_glyphs() {
    for profile in PROFILES {
        for suffix in ["\u{0301}", "\u{fe0f}", "\u{200d}"] {
            let mut terminal = terminal(profile);
            download(&mut terminal, 1, 1, 0, false, b" @", b"~");
            output(&mut terminal, b"\x1b( @!");
            let original = glyph(&terminal, 0, 0);
            output(&mut terminal, suffix.as_bytes());
            assert_eq!(glyph(&terminal, 0, 0), original, "{profile:?}, {suffix:?}");
            assert_eq!(terminal.cursor(), (0, 2), "{profile:?}, {suffix:?}");
            let standalone = terminal.cell(0, 1).unwrap();
            assert!(standalone.glyph.is_none());
            assert_eq!(standalone.width, 1);
            assert!(!standalone.continuation);
            assert_eq!(frame(&terminal, 0).glyphs.len(), 1);
            roundtrip(&terminal);
        }
    }
}
