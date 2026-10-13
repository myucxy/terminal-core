//! DEC presentation reports. Parse a complete restore before changing any state.
use std::fmt::Write;

use crate::charset::{CharsetSize, CharsetState, GraphicRegister};
use crate::{
    CsiSequence, DcsSequence, EffectContext, OscColor, Terminal, TerminalError, UnderlineStyle,
};

const REGISTERS: [GraphicRegister; 4] = [
    GraphicRegister::G0,
    GraphicRegister::G1,
    GraphicRegister::G2,
    GraphicRegister::G3,
];
const MALFORMED: TerminalError = TerminalError::Unsupported("malformed DEC state report");

impl Terminal {
    pub(super) fn report_terminal_state(
        &self,
        csi: &CsiSequence,
        effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        if csi.parameters().len() > 2 || csi.parameter(0, 0) != 2 {
            return Ok(());
        }
        let model = csi.parameter(1, 1);
        if !matches!(model, 1 | 2) {
            return Ok(());
        }
        let mut reply = String::from("\x1bP2$s");
        let mut separator = "";
        for index in 0..=255 {
            if let Some(color) = self.osc.palette(index) {
                let [x, y, z] = color_to_components(color, model);
                write!(reply, "{separator}{index};{model};{x};{y};{z}").expect("report string");
                separator = "/";
            }
        }
        reply.push_str("\x1b\\");
        Self::emit_host_reply_v2(effect, reply.as_bytes())
    }

    pub(super) fn report_presentation_state(
        &self,
        csi: &CsiSequence,
        effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        if csi.parameters().len() > 1 {
            return Ok(());
        }
        let reply = match csi.parameter(0, 0) {
            1 => {
                let screen = self.active_screen();
                let (top, bottom) = self.vertical_cursor_bounds();
                let (left, right) = self.horizontal_cursor_bounds();
                let a = screen.attributes;
                let rendition = 0x40
                    | u8::from(a.bold)
                    | (u8::from(a.is_underlined()) << 1)
                    | (u8::from(a.blink) << 2)
                    | (u8::from(a.inverse) << 3)
                    | (u8::from(a.conceal) << 4);
                let flags = 0x40
                    | u8::from(self.modes.origin_mode)
                    | (u8::from(self.charset.pending_single_shift() == Some(GraphicRegister::G2))
                        << 1)
                    | (u8::from(self.charset.pending_single_shift() == Some(GraphicRegister::G3))
                        << 2)
                    | (u8::from(screen.wrap_pending) << 3);
                let mut sizes = 0x40;
                let mut ids = String::new();
                for (index, register) in REGISTERS.iter().enumerate() {
                    let designation = self.charset.designation(*register);
                    sizes |= u8::from(designation.size == CharsetSize::Characters96) << index;
                    ids.push_str(designation.identifier());
                }
                format!(
                    "\x1bP1$u{};{};{};{};{};{};{};{};{};{ids}\x1b\\",
                    screen.cursor_row.clamp(top, bottom - 1) - top + 1,
                    screen.cursor_column.clamp(left, right - 1) - left + 1,
                    self.active_page(),
                    char::from(rendition),
                    char::from(0x40 | u8::from(screen.protected)),
                    char::from(flags),
                    register_index(self.charset.gl()),
                    register_index(self.charset.gr()),
                    char::from(sizes)
                )
            }
            2 => {
                let stops: Vec<_> = (0..self.columns)
                    .filter(|column| self.tabs.get(*column))
                    .map(|column| (column + 1).to_string())
                    .collect();
                format!("\x1bP2$u{}\x1b\\", stops.join("/"))
            }
            _ => return Ok(()),
        };
        Self::emit_host_reply_v2(effect, reply.as_bytes())
    }

    pub(super) fn restore_report(&mut self, sequence: &DcsSequence) -> Result<(), TerminalError> {
        let selector = match sequence.parameters() {
            [selector] => *selector,
            _ => return Err(MALFORMED),
        };
        match (sequence.final_byte(), selector) {
            (b'p', 2) => {
                let colors = parse_colors(sequence.payload())?;
                let mut osc = self.osc.clone();
                for (index, model, components) in colors {
                    let color = color_from_components(model, components);
                    let payload = format!(
                        "{index};#{:02x}{:02x}{:02x}",
                        color.red, color.green, color.blue
                    );
                    osc.apply(4, payload.as_bytes())
                        .map_err(crate::map_osc_error)?;
                }
                self.osc = osc;
            }
            (b't', 2) => {
                let stops = parse_tabs(sequence.payload())?;
                self.tabs.clear_all();
                for column in stops {
                    self.tabs.set(u32::from(column - 1), true);
                }
            }
            (b't', 1) => {
                let report = parse_cursor_with_soft(sequence.payload(), self.graphics.is_some())?;
                self.move_page(u32::from(report.page))?;
                self.modes.origin_mode = report.flags & 1 != 0;
                let (top, bottom) = self.vertical_cursor_bounds();
                let (left, right) = self.horizontal_cursor_bounds();
                let row = (top + u32::from(report.row - 1)).min(bottom - 1);
                let column = (left + u32::from(report.column - 1)).min(right - 1);
                let screen = self.active_screen_mut();
                screen.position_cursor(row, column);
                screen.wrap_pending = report.flags & 8 != 0 && column == right - 1;
                screen.attributes.bold = report.sgr & 1 != 0;
                screen.attributes.underline_style = if report.sgr & 2 != 0 {
                    UnderlineStyle::Single
                } else {
                    UnderlineStyle::None
                };
                screen.attributes.blink = report.sgr & 4 != 0;
                screen.attributes.inverse = report.sgr & 8 != 0;
                screen.attributes.conceal = report.sgr & 16 != 0;
                screen.set_protected(report.protected);
                self.charset = report.charset;
            }
            _ => return Err(TerminalError::Unsupported("DEC state restore selector")),
        }
        Ok(())
    }
}

fn register_index(register: GraphicRegister) -> usize {
    REGISTERS
        .iter()
        .position(|value| *value == register)
        .expect("graphic register")
}

struct CursorReport {
    row: u16,
    column: u16,
    page: u16,
    sgr: u8,
    protected: bool,
    flags: u8,
    charset: CharsetState,
}

fn number(bytes: &[u8], max: u16) -> Result<u16, TerminalError> {
    if bytes.is_empty() || bytes.len() > 5 {
        return Err(MALFORMED);
    }
    let mut value = 0_u16;
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return Err(MALFORMED);
        }
        value = value
            .checked_mul(10)
            .and_then(|v| v.checked_add(u16::from(byte - b'0')))
            .filter(|v| *v <= max)
            .ok_or(MALFORMED)?;
    }
    Ok(value)
}

fn flag(bytes: &[u8], mask: u8) -> Result<u8, TerminalError> {
    match bytes {
        [byte] if byte & !mask == 0x40 => Ok(byte & mask),
        _ => Err(MALFORMED),
    }
}

#[cfg(test)]
fn parse_cursor(bytes: &[u8]) -> Result<CursorReport, TerminalError> {
    parse_cursor_with_soft(bytes, false)
}

fn parse_cursor_with_soft(bytes: &[u8], allow_soft: bool) -> Result<CursorReport, TerminalError> {
    if bytes.len() > 128 {
        return Err(MALFORMED);
    }
    let fields: Vec<_> = bytes.splitn(10, |b| *b == b';').collect();
    if fields.len() != 10 {
        return Err(MALFORMED);
    }
    let row = number(fields[0], 512)?.max(1);
    let column = number(fields[1], 512)?.max(1);
    let page = number(fields[2], 65534)?.max(1);
    let sgr = flag(fields[3], 31)?;
    let protected = flag(fields[4], 1)? != 0;
    let flags = flag(fields[5], 15)?;
    if flags & 6 == 6 {
        return Err(MALFORMED);
    }
    let gl = number(fields[6], 3)?;
    let gr = number(fields[7], 3)?;
    let sizes = flag(fields[8], 15)?;
    let mut charset = CharsetState::default();
    let mut ids = fields[9];
    for (index, register) in REGISTERS.iter().enumerate() {
        let prefix_len = ids
            .iter()
            .take_while(|b| (0x20..=0x2f).contains(*b))
            .count();
        if prefix_len > 2 {
            return Err(MALFORMED);
        }
        let final_byte = *ids.get(prefix_len).ok_or(MALFORMED)?;
        let size = if sizes & (1 << index) == 0 {
            CharsetSize::Characters94
        } else {
            CharsetSize::Characters96
        };
        charset
            .designate_with_soft(*register, size, &ids[..prefix_len], final_byte, allow_soft)
            .map_err(|_| TerminalError::Unsupported("DECRSPS charset"))?;
        ids = &ids[prefix_len + 1..];
    }
    if !ids.is_empty() {
        return Err(MALFORMED);
    }
    charset.invoke_gl(REGISTERS[usize::from(gl)]);
    charset.invoke_gr(REGISTERS[usize::from(gr)]);
    if flags & 2 != 0 {
        charset.single_shift_2();
    }
    if flags & 4 != 0 {
        charset.single_shift_3();
    }
    Ok(CursorReport {
        row,
        column,
        page,
        sgr,
        protected,
        flags,
        charset,
    })
}

fn parse_tabs(bytes: &[u8]) -> Result<Vec<u16>, TerminalError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for field in bytes.split(|b| *b == b'/') {
        let column = number(field, 512)?;
        if column == 0 || result.last().is_some_and(|last| *last >= column) {
            return Err(MALFORMED);
        }
        result.push(column);
    }
    Ok(result)
}

type ColorRecord = (u8, u16, [u16; 3]);
fn parse_colors(bytes: &[u8]) -> Result<Vec<ColorRecord>, TerminalError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    let mut seen = [false; 256];
    for entry in bytes.split(|b| *b == b'/') {
        if result.len() == 256 {
            return Err(MALFORMED);
        }
        let fields: Vec<_> = entry.split(|b| *b == b';').collect();
        if fields.len() != 5 {
            return Err(MALFORMED);
        }
        let index = number(fields[0], 255)? as u8;
        if std::mem::replace(&mut seen[usize::from(index)], true) {
            return Err(MALFORMED);
        }
        let model = number(fields[1], 2)?;
        if model == 0 {
            return Err(MALFORMED);
        }
        let components = [
            number(fields[2], if model == 1 { 360 } else { 100 })?,
            number(fields[3], 100)?,
            number(fields[4], 100)?,
        ];
        result.push((index, model, components));
    }
    Ok(result)
}

// DEC hue is blue=0, red=120, green=240. Fixed bounded inputs; no host color APIs.
fn color_to_components(color: OscColor, model: u16) -> [u16; 3] {
    let [r, g, b] = [color.red, color.green, color.blue].map(|v| f64::from(v) / 255.0);
    if model == 2 {
        return [r, g, b].map(|v| (v * 100.0).round() as u16);
    }
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let light = (max + min) / 2.0;
    if delta == 0.0 {
        return [0, (light * 100.0).round() as u16, 0];
    }
    let hue = if max == r {
        (g - b) / delta
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    [
        ((hue * 60.0 + 120.0).rem_euclid(360.0)).round() as u16,
        (light * 100.0).round() as u16,
        (delta / (1.0 - (2.0 * light - 1.0).abs()) * 100.0).round() as u16,
    ]
}

fn color_from_components(model: u16, values: [u16; 3]) -> OscColor {
    let rgb = if model == 2 {
        values.map(|v| f64::from(v) / 100.0)
    } else {
        let hue = (f64::from(values[0]) + 240.0).rem_euclid(360.0) / 60.0;
        let light = f64::from(values[1]) / 100.0;
        let saturation = f64::from(values[2]) / 100.0;
        let chroma = (1.0 - (2.0 * light - 1.0).abs()) * saturation;
        let x = chroma * (1.0 - (hue.rem_euclid(2.0) - 1.0).abs());
        let rgb = match hue as u8 {
            0 => [chroma, x, 0.0],
            1 => [x, chroma, 0.0],
            2 => [0.0, chroma, x],
            3 => [0.0, x, chroma],
            4 => [x, 0.0, chroma],
            _ => [chroma, 0.0, x],
        };
        rgb.map(|v| v + light - chroma / 2.0)
    };
    let [r, g, b] = rgb.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8);
    OscColor::new(r, g, b)
}

pub(crate) fn canonical_report(payload: &[u8]) -> bool {
    let Some(body) = payload
        .strip_prefix(b"\x1bP")
        .and_then(|b| b.strip_suffix(b"\x1b\\"))
    else {
        return false;
    };
    if let Some(data) = body.strip_prefix(b"1$u") {
        return parse_cursor_with_soft(data, true).is_ok();
    }
    if let Some(data) = body.strip_prefix(b"2$u") {
        return parse_tabs(data).is_ok();
    }
    if let Some(data) = body.strip_prefix(b"2$s") {
        return parse_colors(data).is_ok();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Event, tests::host_effect_payloads};

    fn output(t: &mut Terminal, bytes: &[u8]) -> Vec<Vec<u8>> {
        let p = t
            .prepare_event_with_host_effects_v2(Event::Output {
                source_sequence: t.next_source_sequence(),
                bytes,
            })
            .unwrap();
        let replies = host_effect_payloads(p.host_effect_frame_v2())
            .into_iter()
            .map(<[u8]>::to_vec)
            .collect();
        p.publish(|_| Ok::<(), ()>(())).unwrap();
        replies
    }

    #[test]
    fn graphics_cursor_report_preserves_soft_names_aliases_and_semicolon_finals() {
        let body = b"1;1;1;@;@;@;0;2;L;1%;AA";
        let report = b"\x1bP1$u1;1;1;@;@;@;0;2;L;1%;AA\x1b\\";
        assert!(parse_cursor(body).is_err());
        let parsed = parse_cursor_with_soft(body, true).unwrap();
        assert_eq!(
            parsed.charset.designation(GraphicRegister::G0).designator(),
            b"1"
        );
        assert_eq!(
            parsed.charset.designation(GraphicRegister::G1).designator(),
            b"%;"
        );
        let shifted = parse_cursor_with_soft(b"1;1;1;@;@;D;0;2;N;1%;A %;", true).unwrap();
        assert_eq!(
            shifted.charset.pending_single_shift(),
            Some(GraphicRegister::G3)
        );
        for register in [GraphicRegister::G1, GraphicRegister::G3] {
            assert_eq!(
                shifted.charset.designation(register).size,
                CharsetSize::Characters96
            );
        }
        assert_eq!(
            shifted
                .charset
                .designation(GraphicRegister::G3)
                .designator(),
            b" %;"
        );
        assert!(canonical_report(report));
        let restore = b"\x1bP1$t1;1;1;@;@;@;0;2;L;1%;AA\x1b\\";
        for split in 0..=restore.len() {
            let mut t = Terminal::new_with_graphics_profile(
                80,
                24,
                0,
                crate::TerminalOptions::default(),
                crate::OutputProfile::Utf8,
                crate::WidthProfile::Unicode16,
            )
            .unwrap();
            output(&mut t, &restore[..split]);
            output(&mut t, &restore[split..]);
            assert_eq!(output(&mut t, b"\x1b[1$w"), [report]);
        }
    }

    #[test]
    fn cursor_report_rejects_overlong_or_non_scs_soft_names() {
        for body in [
            b"1;1;1;@;@;@;0;2;L;   @BAA".as_slice(),
            b"1;1;1;@;@;@;0;2;L;\x7fBAA",
            b"1;1;1;@;@;@;0;2;L; @BAAextra",
        ] {
            assert!(parse_cursor_with_soft(body, true).is_err());
        }
    }

    #[test]
    fn window_title_alias_preserves_icon_and_title_stack_at_every_split() {
        let bytes = "\x1b]21;中文标题\x1b\\".as_bytes();
        for split in 0..=bytes.len() {
            let mut t = Terminal::new(17, 4).unwrap();
            output(&mut t, b"\x1b]0;base\x07\x1b[22;2t");
            output(&mut t, &bytes[..split]);
            output(&mut t, &bytes[split..]);
            assert_eq!(t.osc.window_title(), "中文标题");
            assert_eq!(t.osc.icon_title(), "base");
            let snapshot = t.export_snapshot_v2().unwrap();
            let mut restored = Terminal::import_snapshot_v2(&snapshot).unwrap();
            assert_eq!(restored.osc.window_title(), "中文标题");
            output(&mut restored, b"\x1b[23;2t");
            assert_eq!(restored.osc.window_title(), "base");
        }
    }

    #[test]
    fn dec_report_oracles_survive_every_input_split() {
        for (query, expected) in [
            (
                b"\x1b[1$w".as_slice(),
                b"\x1bP1$u1;1;1;@;@;@;0;2;L;BBAA\x1b\\".as_slice(),
            ),
            (b"\x1b[2$w", b"\x1bP2$u9/17\x1b\\"),
            (b"\x1b[2;2$u", b"\x1bP2$s\x1b\\"),
        ] {
            for split in 0..=query.len() {
                let mut t = Terminal::new(17, 4).unwrap();
                let mut replies = output(&mut t, &query[..split]);
                replies.extend(output(&mut t, &query[split..]));
                assert_eq!(replies, [expected]);
                assert!(canonical_report(expected));
            }
        }
    }

    #[test]
    fn cursor_report_restores_only_defined_fields_and_survives_snapshot() {
        let mut t = Terminal::new(20, 6).unwrap();
        output(
            &mut t,
            b"\x1b[2;5r\x1b[?6h\x1b[2;4H\x1b[1;4;5;7;8;3;38;2;1;2;3m\x1b[1\"q\x1b)%6\x0e\x1bN",
        );
        let report = output(&mut t, b"\x1b[1$w").remove(0);
        assert_eq!(report, b"\x1bP1$u2;4;1;_;A;C;1;2;L;B%6AA\x1b\\");
        output(
            &mut t,
            b"\x1b[?6l\x1b[H\x1b[22;24;25;27;28m\x1b[0\"q\x0f\x1b)B",
        );
        let mut restore = report.clone();
        restore[4] = b't';
        output(&mut t, &restore);
        assert_eq!(output(&mut t, b"\x1b[1$w"), [report]);
        assert!(t.active_screen().attributes.italic);
        assert_eq!(
            t.active_screen().attributes.foreground,
            crate::Color::Rgb(1, 2, 3)
        );
        let snapshot = t.export_snapshot_v2().unwrap();
        let mut restored = Terminal::import_snapshot_v2(&snapshot).unwrap();
        assert_eq!(
            output(&mut t, b"x\x1b[1$w"),
            output(&mut restored, b"x\x1b[1$w")
        );
    }

    #[test]
    fn tab_restore_is_bounded_atomic_and_retains_hidden_stops() {
        let bytes = b"\x1bP2$t1/4/132/512\x1b\\";
        for split in 0..=bytes.len() {
            let mut t = Terminal::new(17, 4).unwrap();
            output(&mut t, &bytes[..split]);
            output(&mut t, &bytes[split..]);
            assert_eq!(output(&mut t, b"\x1b[2$w"), [b"\x1bP2$u1/4\x1b\\"]);
            assert!(t.tabs.get(131) && t.tabs.get(511));
            let original = t.tabs.clone();
            assert!(
                t.apply_event(Event::Output {
                    source_sequence: t.next_source_sequence(),
                    bytes: b"\x1bP2$t2/3/513\x1b\\"
                })
                .is_err()
            );
            assert_eq!(t.tabs, original);
        }
        for data in [b"4/4".as_slice(), b"4/3", b"0", b"/", b"9/", b"1/abc"] {
            assert!(parse_tabs(data).is_err());
        }
    }

    #[test]
    fn palette_reports_use_dec_hue_and_restore_all_entries_atomically() {
        for (rgb, hls) in [
            ([255, 0, 0], [120, 50, 100]),
            ([0, 255, 0], [240, 50, 100]),
            ([0, 0, 255], [0, 50, 100]),
            ([255, 255, 255], [0, 100, 0]),
            ([0, 0, 0], [0, 0, 0]),
        ] {
            let color = OscColor::new(rgb[0], rgb[1], rgb[2]);
            assert_eq!(color_to_components(color, 1), hls);
            assert_eq!(color_from_components(1, hls), color);
        }
        let mut t = Terminal::new(17, 4).unwrap();
        output(&mut t, b"\x1bP2$p0;2;100;0;0/255;1;0;50;100\x1b\\");
        assert_eq!(
            output(&mut t, b"\x1b[2;2$u"),
            [b"\x1bP2$s0;2;100;0;0/255;2;0;0;100\x1b\\"]
        );
        assert_eq!(
            output(&mut t, b"\x1b[2$u"),
            [b"\x1bP2$s0;1;120;50;100/255;1;0;50;100\x1b\\"]
        );
        let original = t.osc.clone();
        assert!(
            t.apply_event(Event::Output {
                source_sequence: t.next_source_sequence(),
                bytes: b"\x1bP2$p0;2;0;0;0/255;2;101;0;0\x1b\\"
            })
            .is_err()
        );
        assert_eq!(t.osc, original);
        let all = (0..256)
            .map(|i| format!("{i};2;100;100;100"))
            .collect::<Vec<_>>()
            .join("/");
        assert!(all.len() > 4096);
        let mut t = Terminal::new(8, 2).unwrap();
        output(&mut t, format!("\x1bP2$p{all}\x1b\\").as_bytes());
        assert_eq!(
            output(&mut t, b"\x1b[2;2$u"),
            [format!("\x1bP2$s{all}\x1b\\").as_bytes()]
        );
        for bad in [
            b"0;2;0;0;0/0;2;1;1;1".as_slice(),
            b"256;2;0;0;0",
            b"0;3;0;0;0",
            b"0;1;361;0;0",
            b"0;2;0;0",
        ] {
            assert!(parse_colors(bad).is_err());
        }
    }

    #[test]
    fn invalid_cursor_and_cancelled_restore_never_apply_partial_state() {
        for data in [
            b"1;1;1;@;@;@;0;2;L;BBAAextra".as_slice(),
            b"1;1;1;@;@;F;0;2;L;BBAA",
            b"1;1;1;@;@;@;4;2;L;BBAA",
            b"1;1;1;@;@;@;0;2;L;BB",
        ] {
            assert!(parse_cursor(data).is_err());
        }
        let mut t = Terminal::new(17, 4).unwrap();
        let original = t.tabs.clone();
        output(&mut t, b"\x1bP2$t1/2\x18X");
        assert_eq!(t.tabs, original);
        assert_eq!(t.cursor(), (0, 1));
    }
}
