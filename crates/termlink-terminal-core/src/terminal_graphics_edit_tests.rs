//! Parser-to-render coverage for graphic geometry during ordinary text edits.
use crate::{
    Event, HostGeometryV1, OutputProfile, Terminal, TerminalOptions, WidthProfile,
    graphics_state::{GraphicProtocol, Placement},
    render_frame_v5::Frame,
    terminal_graphics::GraphicsAnchorIndex,
};

fn terminal() -> Terminal {
    let mut terminal = Terminal::new_with_graphics_profile(
        8,
        4,
        0,
        TerminalOptions {
            history_capacity_rows: 32,
        },
        OutputProfile::Utf8,
        WidthProfile::Unicode16,
    )
    .unwrap();
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

fn images_at(terminal: &mut Terminal, row: u32, column: u32, columns: u32, rows: u32) {
    assert!((1..=2).contains(&rows));
    output(
        terminal,
        format!(
            "\x1b[{};{}H\x1b_Ga=T,s=1,v=1,i=1,p=1,c={columns},r={rows},C=1,q=2;AQIDBA==\x1b\\",
            row + 1,
            column + 1
        )
        .as_bytes(),
    );
    // Each physical cell is a uniform 2 x 6 pixel block: red in the first row, green in the second.
    let width = columns * 2;
    let mut sixel = format!("\x1bP7;1q\"1;1;{width};{}#1;2;100;0;0!{width}~", rows * 6);
    if rows == 2 {
        sixel.push_str(&format!("-#2;2;0;100;0!{width}~"));
    }
    sixel.push_str("\x1b\\");
    output(terminal, sixel.as_bytes());
}

fn frame(terminal: &Terminal, offset: u32) -> Frame {
    Frame::decode(&terminal.export_render_frame_v5(offset).unwrap()).unwrap()
}

fn roundtrip(terminal: &Terminal) {
    let bytes = terminal.export_snapshot_v2().unwrap();
    let restored = Terminal::import_snapshot_v2(&bytes).unwrap();
    assert_eq!(&restored, terminal);
    for offset in [0, terminal.scrollback_info().0] {
        assert_eq!(
            restored.export_render_frame_v5(offset).unwrap(),
            terminal.export_render_frame_v5(offset).unwrap()
        );
    }
}

fn kitty(terminal: &Terminal) -> Placement {
    terminal
        .graphics_state()
        .unwrap()
        .placements()
        .iter()
        .find(|placement| placement.protocol == GraphicProtocol::Kitty)
        .unwrap()
        .clone()
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PaintedCell {
    column: i32,
    row: i32,
    source_column: u32,
    source_row: u32,
    rgba: [u8; 4],
}

fn sixel_cells(frame: &Frame) -> Vec<PaintedCell> {
    let mut cells = Vec::new();
    for draw in frame
        .images
        .iter()
        .filter(|draw| draw.protocol == GraphicProtocol::Sixel)
    {
        let image = frame.resources.get(draw.resource).unwrap().image();
        assert_eq!(image.width(), draw.layout.columns * 2);
        assert_eq!(image.height(), draw.layout.rows * 6);
        for row in draw.clip.top..draw.clip.bottom {
            for column in draw.clip.left..draw.clip.right {
                let first = ((row * 6 * image.width() + column * 2) * 4) as usize;
                let rgba: [u8; 4] = image.pixels()[first..first + 4].try_into().unwrap();
                // Every pixel in this fixture's cell must keep the same original source color.
                for y in 0..6 {
                    for x in 0..2 {
                        let pixel = (((row * 6 + y) * image.width() + column * 2 + x) * 4) as usize;
                        assert_eq!(&image.pixels()[pixel..pixel + 4], &rgba);
                    }
                }
                cells.push(PaintedCell {
                    column: draw.column + column as i32,
                    row: draw.row + row as i32,
                    source_column: column,
                    source_row: row,
                    rgba,
                });
            }
        }
    }
    cells.sort_unstable();
    cells
}

fn expected_cells(columns: u32, rows: u32, erased: std::ops::Range<u32>) -> Vec<PaintedCell> {
    let mut cells = Vec::new();
    for column in 0..columns {
        for row in 0..rows {
            if row == 0 && erased.contains(&column) {
                continue;
            }
            cells.push(PaintedCell {
                column: column as i32,
                row: row as i32,
                source_column: column,
                source_row: row,
                rgba: if row == 0 {
                    [255, 0, 0, 255]
                } else {
                    [0, 255, 0, 255]
                },
            });
        }
    }
    cells.sort_unstable();
    cells
}

fn assert_anchor_positions(terminal: &Terminal, row: u32, column: u32) {
    let index = GraphicsAnchorIndex::new(terminal.active_screen()).unwrap();
    let placements = terminal.graphics_state().unwrap().placements();
    assert_eq!(placements.len(), 2);
    for placement in placements {
        let (anchor_row, anchor_column) = index
            .locate(placement.anchor.logical_line, placement.anchor.cell_offset)
            .expect("edit retained a valid logical anchor");
        assert_eq!(
            (
                i64::from(anchor_row) + i64::from(placement.origin_y),
                i64::from(anchor_column) + i64::from(placement.origin_x),
            ),
            (i64::from(row), i64::from(column)),
            "{:?} moved after row metadata changed",
            placement.protocol
        );
    }
}

#[test]
fn ech_crops_only_the_selected_sixel_cells_and_keeps_kitty_pixels() {
    let mut terminal = terminal();
    output(&mut terminal, b"ABCDEFGH");
    images_at(&mut terminal, 0, 0, 8, 2);
    let original_kitty = kitty(&terminal);
    let before = frame(&terminal, 0);
    assert_eq!(sixel_cells(&before), expected_cells(8, 2, 0..0));
    let original_kitty_draw = before
        .images
        .iter()
        .find(|draw| draw.protocol == GraphicProtocol::Kitty)
        .unwrap();

    output(&mut terminal, b"\x1b[1;3H\x1b[3X");
    for column in 2..5 {
        assert_eq!(terminal.cell(0, column).unwrap().character, ' ');
    }
    assert_eq!(terminal.cell(0, 1).unwrap().character, 'B');
    assert_eq!(terminal.cell(0, 5).unwrap().character, 'F');
    assert_eq!(kitty(&terminal), original_kitty);
    let after = frame(&terminal, 0);
    assert_eq!(sixel_cells(&after), expected_cells(8, 2, 2..5));
    assert_eq!(after.resources, before.resources);
    assert_eq!(
        after
            .images
            .iter()
            .find(|draw| draw.protocol == GraphicProtocol::Kitty)
            .unwrap(),
        original_kitty_draw
    );
    roundtrip(&terminal);
}

#[test]
fn ech_expands_wide_character_boundaries_and_scales_line_renditions() {
    for rendition in [b"\x1b#5".as_slice(), b"\x1b#6", b"\x1b#3", b"\x1b#4"] {
        let scale = if rendition == b"\x1b#5" { 1 } else { 2 };
        for cursor_column in [2, 3] {
            let mut terminal = terminal();
            output(&mut terminal, rendition);
            output(&mut terminal, "A界B".as_bytes());
            assert_eq!(terminal.cell(0, 1).unwrap().width, 2);
            assert!(terminal.cell(0, 2).unwrap().continuation);
            images_at(&mut terminal, 0, 0, 8, 2);
            let original_kitty = kitty(&terminal);
            let before = frame(&terminal, 0);
            output(
                &mut terminal,
                format!("\x1b[1;{cursor_column}H\x1b[X").as_bytes(),
            );
            assert_eq!(terminal.cell(0, 0).unwrap().character, 'A');
            assert_eq!(terminal.cell(0, 3).unwrap().character, 'B');
            for column in 1..3 {
                let cell = terminal.cell(0, column).unwrap();
                assert_eq!(cell.character, ' ');
                assert_eq!(cell.width, 1);
                assert!(!cell.continuation);
            }
            assert_eq!(kitty(&terminal), original_kitty);
            let after = frame(&terminal, 0);
            assert_eq!(
                sixel_cells(&after),
                expected_cells(8, 2, scale..3 * scale),
                "rendition {rendition:?}, cursor {cursor_column}"
            );
            assert_eq!(after.resources, before.resources);
            roundtrip(&terminal);
        }
    }
}

#[test]
fn horizontal_index_keeps_images_fixed_when_wrapped_line_offsets_change() {
    for (column, command) in [(1, b'6'), (8, b'9')] {
        let mut terminal = terminal();
        output(&mut terminal, "abcdefg界Q".as_bytes());
        images_at(&mut terminal, 1, 2, 2, 1);
        assert!(terminal.main.soft_wrapped[0]);
        assert_eq!(terminal.main.row_metadata[1].logical_cell_offset, 7);
        let before = frame(&terminal, 0);
        assert_anchor_positions(&terminal, 1, 2);
        let mut bytes = format!("\x1b[2;{column}H\x1b").into_bytes();
        bytes.push(command);
        output(&mut terminal, &bytes);
        assert!(terminal.main.soft_wrapped[0]);
        assert_eq!(terminal.main.row_metadata[1].logical_cell_offset, 8);
        assert_anchor_positions(&terminal, 1, 2);
        let after = frame(&terminal, 0);
        assert_eq!(after.images, before.images);
        assert_eq!(sixel_cells(&after), sixel_cells(&before));
        assert_eq!(after.resources, before.resources);
        roundtrip(&terminal);
    }
}

#[test]
fn rectangle_rendition_materialization_preserves_image_position_then_reflows_its_anchor() {
    for command in *b"rt" {
        let mut terminal = terminal();
        // The wide character wraps before the final column, leaving offset 7 rather than 8.
        output(&mut terminal, "abcdefg界Q".as_bytes());
        images_at(&mut terminal, 1, 2, 2, 1);
        assert!(terminal.main.soft_wrapped[0]);
        assert_eq!(terminal.main.meaningful_end[0], 7);
        assert_eq!(terminal.main.row_metadata[1].logical_cell_offset, 7);
        let before = frame(&terminal, 0);
        assert_anchor_positions(&terminal, 1, 2);
        let mut bytes = b"\x1b[2*x\x1b[1;8;1;8;1$".to_vec();
        bytes.push(command);
        output(&mut terminal, &bytes);
        assert!(terminal.cell(0, 7).unwrap().attributes.bold);
        assert_eq!(terminal.main.meaningful_end[0], 8);
        assert_eq!(terminal.main.row_metadata[1].logical_cell_offset, 8);
        assert_anchor_positions(&terminal, 1, 2);
        let after = frame(&terminal, 0);
        assert_eq!(after.images, before.images);
        assert_eq!(sixel_cells(&after), sixel_cells(&before));
        assert_eq!(after.resources, before.resources);
        roundtrip(&terminal);

        terminal
            .apply_event(Event::Resize {
                source_sequence: terminal.next_source_sequence(),
                columns: 4,
                rows: 4,
            })
            .unwrap();
        assert_anchor_positions(&terminal, 2, 2);
        let reflowed = frame(&terminal, terminal.scrollback_info().0);
        assert_eq!(reflowed.resources, before.resources);
        assert_eq!(reflowed.images.len(), before.images.len());
        for (draw, original) in reflowed.images.iter().zip(&before.images) {
            assert_eq!((draw.row, draw.column), (2, 2));
            assert_eq!(draw.resource, original.resource);
            assert_eq!(draw.layout, original.layout);
            assert_eq!(draw.clip, original.clip);
        }
        let expected: Vec<_> = sixel_cells(&before)
            .into_iter()
            .map(|mut cell| {
                cell.row += 1;
                cell
            })
            .collect();
        assert_eq!(sixel_cells(&reflowed), expected);
        roundtrip(&terminal);
    }
}
