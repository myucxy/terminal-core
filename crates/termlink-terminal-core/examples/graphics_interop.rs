//! Public-API synthetic graphics fixtures for independent renderer decoders.
//! Usage: cargo run -p termlink-terminal-core --example graphics_interop -- OUTPUT_DIRECTORY
use std::{fmt::Write, fs, io, path::Path};
use termlink_terminal_core::{
    Event, HISTORY_LATEST_ANCHOR, HistoryCapture, HistoryPage, HistoryPageRequest, HostGeometryV1,
    HostThemeV1, MAX_HISTORY_PAGE_CELLS, MAX_HISTORY_PAGE_LINES, MAX_HISTORY_PAGE_UTF8,
    OscBaseline, OscColor, OutputProfile, Terminal, TerminalOptions, WidthProfile,
    graphics_resources::ImageFormat, render_frame_v5::Frame,
};

const KITTY: &[u8] = b"\x1b_Ga=T,f=32,s=2,v=2,i=17,x=1,y=0,w=1,h=2,c=2,r=3,z=-7,C=1;\
/wAA/wD/AP8AAP//////gA==\x1b\\";
const RGBA: &[u8] = &[
    255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 128,
];

fn terminal(columns: u32, rows: u32) -> Terminal {
    let mut terminal = Terminal::new_with_graphics_profile(
        columns,
        rows,
        0,
        TerminalOptions {
            history_capacity_rows: 64,
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
    let theme = HostThemeV1 {
        scheme: 1,
        baseline: OscBaseline {
            palette: [Some(OscColor::new(0, 0, 0)); 256],
            foreground: Some(OscColor::new(17, 34, 51)),
            background: Some(OscColor::new(68, 85, 102)),
            cursor: Some(OscColor::new(255, 255, 255)),
        },
    }
    .encode()
    .unwrap();
    terminal
        .apply_event(Event::HostTheme {
            source_sequence: terminal.next_source_sequence(),
            bytes: &theme,
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

fn hex(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(value, "{byte:02x}").unwrap();
    }
    value
}

fn save_frame(
    directory: &Path,
    name: &str,
    bytes: &[u8],
    metadata: &mut String,
) -> io::Result<Frame> {
    let frame = Frame::decode(bytes).unwrap();
    assert_eq!(frame.encode().unwrap(), bytes);
    fs::write(directory.join(format!("{name}.v5.bin")), bytes)?;
    writeln!(
        metadata,
        "\n{name}.v5.bin bytes={} revision={} columns={} rows={} images={} glyphs={} resources={}",
        bytes.len(),
        frame.revision,
        frame.columns,
        frame.rows,
        frame.images.len(),
        frame.glyphs.len(),
        frame.resources.entries().len()
    )
    .unwrap();
    for image in &frame.images {
        writeln!(metadata, "  image placement={} resource={} column={} row={} z={} image_id={} source={:?} clip={:?} allocation={}x{} destination={:?}/{:?}/{:?}/{:?}",
            image.placement, image.resource, image.column, image.row, image.z, image.image_id,
            image.layout.source, image.clip, image.layout.columns, image.layout.rows,
            image.layout.x, image.layout.y, image.layout.width, image.layout.height).unwrap();
    }
    for resource in frame.resources.entries() {
        let image = resource.image();
        writeln!(
            metadata,
            "  resource id={} format={:?} width={} height={} bytes={} first_32_bytes_hex={}",
            resource.id(),
            image.format(),
            image.width(),
            image.height(),
            image.pixels().len(),
            hex(&image.pixels()[..image.pixels().len().min(32)])
        )
        .unwrap();
    }
    for draw in &frame.glyphs {
        let glyph = draw.glyph;
        let image = frame.resources.get(glyph.resource).unwrap().image();
        assert_eq!(image.format(), ImageFormat::Alpha8);
        let mask: Vec<_> = (0..glyph.height)
            .flat_map(|row| {
                let first = (row * image.width() + glyph.x) as usize;
                image.pixels()[first..first + glyph.width as usize]
                    .iter()
                    .copied()
            })
            .collect();
        writeln!(
            metadata,
            "  glyph column={} row={} resource={} x={} width={} height={} mask_hex={}",
            draw.column,
            draw.row,
            glyph.resource,
            glyph.x,
            glyph.width,
            glyph.height,
            hex(&mask)
        )
        .unwrap();
    }
    Ok(frame)
}

fn save_terminal(
    directory: &Path,
    name: &str,
    terminal: &Terminal,
    metadata: &mut String,
) -> io::Result<Frame> {
    let snapshot = terminal.export_snapshot_v2().unwrap();
    let bytes = terminal.export_render_frame_v5(0).unwrap();
    let restored = Terminal::import_snapshot_v2(&snapshot).unwrap();
    assert_eq!(restored.export_snapshot_v2().unwrap(), snapshot);
    assert_eq!(restored.export_render_frame_v5(0).unwrap(), bytes);
    fs::write(directory.join(format!("{name}.snapshot.bin")), &snapshot)?;
    save_frame(directory, name, &bytes, metadata)
}

fn main() -> io::Result<()> {
    let directory = std::env::args_os()
        .nth(1)
        .expect("provide the fixture output directory");
    let directory = Path::new(&directory);
    fs::create_dir_all(directory)?;
    let mut metadata = String::from(
        "Self-authored synthetic fixtures; GPL-3.0-only.\n\
Generated by public Terminal Output -> Snapshot import -> Render V5 APIs.\n\
V5: features=7, images=144 bytes, glyphs=32 bytes, resources=TLGRS1.\n\
History V2: capture_id=37, required_features=1, 96-byte header, V5 fragments.\n\
Every .v5.bin is canonical Rust decode/encode checked; every .snapshot.bin roundtrips.\n\
RGBA oracle (2x2, row-major, straight alpha): ff0000ff00ff00ff0000ffffffffff80.\n\
Theme foreground=112233 background=445566 cursor=ffffff; palette black.\n\
DRCS A: 10x16 mask, rows 0..5 all ff, rows 6..15 all 00.\n\
DRCS B: 10x16 mask, row 1 all ff, all other rows 00.\n",
    );

    let empty = save_terminal(directory, "empty", &terminal(12, 4), &mut metadata)?;
    assert!(
        empty.images.is_empty() && empty.glyphs.is_empty() && empty.resources.entries().is_empty()
    );

    let mut kitty = terminal(12, 4);
    output(&mut kitty, KITTY);
    output(&mut kitty, b"\x1b[4;1H\n");
    let frame = save_terminal(directory, "kitty-crop-scrolled", &kitty, &mut metadata)?;
    assert_eq!(frame.images.len(), 1);
    let image = &frame.images[0];
    assert_eq!((image.z, image.row), (-7, -1));
    assert_eq!(
        (
            image.layout.source.x,
            image.layout.source.width,
            image.layout.source.height
        ),
        (1, 1, 2)
    );
    assert_eq!(
        frame
            .resources
            .get(image.resource)
            .unwrap()
            .image()
            .pixels(),
        RGBA
    );

    let mut sixel = terminal(12, 4);
    output(
        &mut sixel,
        b"\x1b[2;3r\x1b[3;1H\x1bP7;1q#1;2;100;0;0~-~-~-~-~\x1b\\",
    );
    let frame = save_terminal(directory, "sixel-region-clip", &sixel, &mut metadata)?;
    assert_eq!(frame.images.len(), 1);
    let image = &frame.images[0];
    assert!(image.row < 0 && image.clip.top > 0);
    assert_eq!(image.row + image.clip.top as i32, 1);
    assert_eq!(image.row + image.clip.bottom as i32, 3);
    assert!(
        frame
            .resources
            .get(image.resource)
            .unwrap()
            .image()
            .pixels()
            .chunks_exact(4)
            .all(|p| p == [255, 0, 0, 255])
    );

    let mut drcs = terminal(80, 24);
    output(&mut drcs, KITTY);
    output(
        &mut drcs,
        b"\x1bP1;1;0;10;1;2;16;0{ @~~~~~~~~~~\x1b\\\x1b( @!\x1b[2b",
    );
    output(
        &mut drcs,
        b"\x1bP1;1;0;10;1;2;16;0{ @AAAAAAAAAA\x1b\\!\x1b(B",
    );
    let frame = save_terminal(directory, "drcs-live", &drcs, &mut metadata)?;
    assert_eq!(frame.glyphs.len(), 4);
    assert_eq!(
        frame.glyphs[0].glyph.resource,
        frame.glyphs[2].glyph.resource
    );
    assert_ne!(
        frame.glyphs[0].glyph.resource,
        frame.glyphs[3].glyph.resource
    );
    for (index, draw) in frame.glyphs.iter().enumerate() {
        let glyph = draw.glyph;
        assert_eq!((glyph.width, glyph.height), (10, 16));
        let image = frame.resources.get(glyph.resource).unwrap().image();
        for row in 0..glyph.height {
            let first = (row * image.width() + glyph.x) as usize;
            let expected = if (index < 3 && row < 6) || (index == 3 && row == 1) {
                255
            } else {
                0
            };
            assert!(
                image.pixels()[first..first + glyph.width as usize]
                    .iter()
                    .all(|&pixel| pixel == expected)
            );
        }
    }
    output(&mut drcs, b"\r\n");
    for _ in 0..24 {
        output(&mut drcs, b"history\r\n");
    }
    save_terminal(directory, "drcs-scrolled-live", &drcs, &mut metadata)?;
    let capture = HistoryCapture::capture(&drcs).unwrap();
    let request = HistoryPageRequest {
        revision: capture.revision(),
        before_line_id: HISTORY_LATEST_ANCHOR,
        max_lines: MAX_HISTORY_PAGE_LINES,
        max_cells: MAX_HISTORY_PAGE_CELLS,
        max_utf8_bytes: MAX_HISTORY_PAGE_UTF8,
    };
    let page = capture.page_v5(request).unwrap();
    let page_bytes = page.encode_v2(37).unwrap();
    assert_eq!(HistoryPage::decode_v2(&page_bytes, 37).unwrap(), page);
    assert!(HistoryPage::decode(&page_bytes, 37).is_err());
    fs::write(directory.join("drcs-history.v2.bin"), &page_bytes)?;
    writeln!(metadata, "\ndrcs-history.v2.bin bytes={} capture_id=37 revision={} columns={} cells={} utf8_bytes={} lines={} frames={}",
        page_bytes.len(), page.revision, page.columns, page.cells, page.utf8_bytes, page.lines.len(), page.frames.len()).unwrap();
    let mut history_glyphs = 0;
    for (index, bytes) in page.frames.iter().enumerate() {
        history_glyphs += save_frame(
            directory,
            &format!("drcs-history-frame-{index:03}"),
            bytes,
            &mut metadata,
        )?
        .glyphs
        .len();
    }
    assert_eq!(history_glyphs, 4);
    let frozen = capture.viewport_v5(u32::MAX).unwrap().0;
    let frame = save_frame(directory, "drcs-frozen-viewport", &frozen, &mut metadata)?;
    assert_eq!(frame.glyphs.len(), 4);
    output(&mut drcs, b"\x1b[3J\x1bc");
    assert_eq!(
        capture.page_v5(request).unwrap().encode_v2(37).unwrap(),
        page_bytes
    );
    assert_eq!(capture.viewport_v5(u32::MAX).unwrap().0, frozen);
    metadata.push_str("\nFrozen history bytes remain identical after live ED3 and RIS.\n");
    fs::write(directory.join("metadata.txt"), metadata)?;
    println!(
        "Graphics interoperability fixtures written to {}",
        directory.display()
    );
    Ok(())
}
