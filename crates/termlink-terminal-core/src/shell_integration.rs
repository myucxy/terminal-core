//! Bounded, untrusted shell annotations. They never execute commands or access paths.
use crate::{Terminal, TerminalError};
use std::collections::VecDeque;

pub const MAX_COMMANDS: usize = 64;
pub const MAX_CWD_BYTES: usize = 4096;
pub const MAX_SHELL_STATE_BYTES: usize = 32 + MAX_CWD_BYTES + MAX_COMMANDS * (116 + MAX_CWD_BYTES);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ShellPoint {
    pub source_sequence: u64,
    pub line_id: u64,
    pub cell_offset: u32,
    pub page: u8,
    pub alternate: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellCommand {
    pub id: u64,
    /// 0 prompt, 1 input, 2 output, 3 completed, 4 interrupted by another prompt.
    pub phase: u8,
    pub prompt: ShellPoint,
    pub input: Option<ShellPoint>,
    pub output: Option<ShellPoint>,
    pub end: Option<ShellPoint>,
    pub exit_code: Option<i32>,
    pub cwd_uri: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellIntegration {
    pub cwd_uri: String,
    pub commands: VecDeque<ShellCommand>,
    pub next_command_id: u64,
}

impl Default for ShellIntegration {
    fn default() -> Self {
        Self {
            cwd_uri: String::new(),
            commands: VecDeque::new(),
            next_command_id: 1,
        }
    }
}

impl ShellIntegration {
    pub(crate) fn has_state(&self) -> bool {
        !self.cwd_uri.is_empty() || !self.commands.is_empty() || self.next_command_id != 1
    }

    pub(crate) fn reset(&mut self) {
        self.cwd_uri.clear();
        self.commands.clear();
    }

    fn start(&mut self, point: ShellPoint) -> Result<(), TerminalError> {
        let next = self
            .next_command_id
            .checked_add(1)
            .ok_or(TerminalError::LimitExceeded)?;
        if let Some(command) = self.commands.back_mut()
            && command.phase < 3
        {
            command.phase = 4;
        }
        if self.commands.len() == MAX_COMMANDS {
            self.commands.pop_front();
        }
        self.commands
            .try_reserve(1)
            .map_err(|_| TerminalError::LimitExceeded)?;
        self.commands.push_back(ShellCommand {
            id: self.next_command_id,
            phase: 0,
            prompt: point,
            input: None,
            output: None,
            end: None,
            exit_code: None,
            cwd_uri: self.cwd_uri.clone(),
        });
        self.next_command_id = next;
        Ok(())
    }

    pub(crate) fn apply(
        &mut self,
        selector: u32,
        payload: &[u8],
        point: ShellPoint,
    ) -> Result<(), TerminalError> {
        if selector == 7 {
            let uri = validate_cwd(payload)?;
            self.cwd_uri = uri.to_owned();
            if let Some(command) = self.commands.back_mut().filter(|c| c.phase < 2) {
                command.cwd_uri.clone_from(&self.cwd_uri);
            }
            return Ok(());
        }
        let value = std::str::from_utf8(payload)
            .map_err(|_| TerminalError::Unsupported("OSC 133 invalid UTF-8"))?;
        let mut fields = value.split(';');
        let marker = fields.next().unwrap_or("");
        if !matches!(marker, "A" | "B" | "C" | "D") {
            return Err(TerminalError::Unsupported("OSC 133 marker"));
        }
        let mut exit_code = None;
        for (index, field) in fields.enumerate() {
            if field.is_empty() {
                continue;
            }
            if marker == "D" && index == 0 && !field.contains('=') {
                exit_code = Some(
                    field
                        .parse::<i32>()
                        .map_err(|_| TerminalError::Unsupported("OSC 133 exit status"))?,
                );
            } else if !field.contains('=')
                || field.len() > 256
                || field.chars().any(char::is_control)
            {
                return Err(TerminalError::Unsupported("OSC 133 property"));
            }
            // Optional properties do not grant authority and are not interpreted as commands.
        }
        if marker == "A" {
            return self.start(point);
        }
        if marker != "D" && self.commands.back().is_none_or(|c| c.phase >= 3) {
            self.start(point)?;
        }
        let Some(command) = self.commands.back_mut().filter(|c| c.phase < 3) else {
            return Ok(());
        };
        match marker {
            "B" if command.phase < 2 => {
                command.input.get_or_insert(point);
                command.phase = 1;
            }
            "C" => {
                command.output.get_or_insert(point);
                command.phase = 2;
            }
            "D" => {
                command.end = Some(point);
                command.exit_code = exit_code;
                command.phase = 3;
            }
            _ => {}
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, TerminalError> {
        if self.commands.len() > MAX_COMMANDS
            || self.cwd_uri.len() > MAX_CWD_BYTES
            || self
                .commands
                .iter()
                .any(|c| c.cwd_uri.len() > MAX_CWD_BYTES)
        {
            return Err(TerminalError::LimitExceeded);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(MAX_SHELL_STATE_BYTES.min(
                32 + self.cwd_uri.len()
                    + self.commands.len() * 116
                    + self.commands.iter().map(|c| c.cwd_uri.len()).sum::<usize>(),
            ))
            .map_err(|_| TerminalError::LimitExceeded)?;
        bytes.extend_from_slice(b"TLSHV1\0\0");
        bytes.extend_from_slice(&self.next_command_id.to_le_bytes());
        bytes.extend_from_slice(&(self.cwd_uri.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(self.commands.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(self.cwd_uri.as_bytes());
        for command in &self.commands {
            bytes.extend_from_slice(&command.id.to_le_bytes());
            bytes.push(command.phase);
            bytes.push(
                u8::from(command.input.is_some())
                    | u8::from(command.output.is_some()) << 1
                    | u8::from(command.end.is_some()) << 2
                    | u8::from(command.exit_code.is_some()) << 3,
            );
            bytes.extend_from_slice(&[0; 2]);
            bytes.extend_from_slice(&command.exit_code.unwrap_or(0).to_le_bytes());
            for point in [
                Some(command.prompt),
                command.input,
                command.output,
                command.end,
            ] {
                let point = point.unwrap_or_default();
                bytes.extend_from_slice(&point.source_sequence.to_le_bytes());
                bytes.extend_from_slice(&point.line_id.to_le_bytes());
                bytes.extend_from_slice(&point.cell_offset.to_le_bytes());
                bytes.push(point.page);
                bytes.push(u8::from(point.alternate));
                bytes.extend_from_slice(&[0; 2]);
            }
            bytes.extend_from_slice(&(command.cwd_uri.len() as u32).to_le_bytes());
            bytes.extend_from_slice(command.cwd_uri.as_bytes());
        }
        if bytes.len() > MAX_SHELL_STATE_BYTES {
            return Err(TerminalError::LimitExceeded);
        }
        Ok(bytes)
    }

    pub(crate) fn validate(bytes: &[u8], next_source_sequence: u64) -> Result<bool, TerminalError> {
        let state = Self::decode_impl(bytes, next_source_sequence, false)?;
        Ok(state.next_command_id != 1 || bytes[16..24] != [0; 8])
    }

    pub fn decode(bytes: &[u8], next_source_sequence: u64) -> Result<Self, TerminalError> {
        Self::decode_impl(bytes, next_source_sequence, true)
    }

    fn decode_impl(
        bytes: &[u8],
        next_source_sequence: u64,
        retain: bool,
    ) -> Result<Self, TerminalError> {
        let bad = || TerminalError::MalformedSnapshot;
        if bytes.len() < 32
            || bytes.len() > MAX_SHELL_STATE_BYTES
            || &bytes[..8] != b"TLSHV1\0\0"
            || bytes[24..32] != [0; 8]
        {
            return Err(bad());
        }
        let next = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
        let count = u32::from_le_bytes(bytes[20..24].try_into().unwrap()) as usize;
        if next == 0 || count > MAX_COMMANDS {
            return Err(bad());
        }
        let mut offset = 32;
        let cwd_uri = read_cwd(
            bytes,
            &mut offset,
            u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize,
            retain,
        )?;
        let mut result = Self {
            cwd_uri,
            next_command_id: next,
            commands: VecDeque::new(),
        };
        let mut previous_id = 0;
        let mut previous_sequence = 0;
        for _ in 0..count {
            let record = bytes.get(offset..offset + 116).ok_or_else(bad)?;
            offset += 116;
            let id = u64::from_le_bytes(record[..8].try_into().unwrap());
            let phase = record[8];
            let flags = record[9];
            let code = i32::from_le_bytes(record[12..16].try_into().unwrap());
            if id <= previous_id
                || id >= next
                || phase > 4
                || flags & !15 != 0
                || record[10..12] != [0; 2]
                || (flags & 8 == 0 && code != 0)
                || (flags & 8 != 0 && flags & 4 == 0)
                || (phase == 3) != (flags & 4 != 0)
                || (phase == 0 && flags != 0)
                || (phase == 1 && flags != 1)
                || (phase == 2 && flags & 2 == 0)
            {
                return Err(bad());
            }
            let mut points = [None; 4];
            for (index, target) in points.iter_mut().enumerate() {
                let raw = &record[16 + index * 24..40 + index * 24];
                if index != 0 && flags & (1 << (index - 1)) == 0 {
                    if raw != [0; 24] {
                        return Err(bad());
                    }
                    continue;
                }
                let point = ShellPoint {
                    source_sequence: u64::from_le_bytes(raw[..8].try_into().unwrap()),
                    line_id: u64::from_le_bytes(raw[8..16].try_into().unwrap()),
                    cell_offset: u32::from_le_bytes(raw[16..20].try_into().unwrap()),
                    page: raw[20],
                    alternate: raw[21] != 0,
                };
                if point.source_sequence >= next_source_sequence
                    || point.source_sequence < previous_sequence
                    || !(1..=6).contains(&point.page)
                    || raw[21] > 1
                    || raw[22..24] != [0; 2]
                    || (point.alternate && point.page != 1)
                {
                    return Err(bad());
                }
                previous_sequence = point.source_sequence;
                *target = Some(point);
            }
            let cwd_uri = read_cwd(
                bytes,
                &mut offset,
                u32::from_le_bytes(record[112..116].try_into().unwrap()) as usize,
                retain,
            )?;
            if retain {
                result.commands.push_back(ShellCommand {
                    id,
                    phase,
                    prompt: points[0].unwrap(),
                    input: points[1],
                    output: points[2],
                    end: points[3],
                    exit_code: (flags & 8 != 0).then_some(code),
                    cwd_uri,
                });
            }
            previous_id = id;
        }
        if offset != bytes.len() {
            return Err(bad());
        }
        Ok(result)
    }
}

fn read_cwd(
    bytes: &[u8],
    offset: &mut usize,
    length: usize,
    retain: bool,
) -> Result<String, TerminalError> {
    if length > MAX_CWD_BYTES {
        return Err(TerminalError::MalformedSnapshot);
    }
    let value = bytes
        .get(*offset..*offset + length)
        .ok_or(TerminalError::MalformedSnapshot)?;
    *offset += length;
    if value.is_empty() {
        return Ok(String::new());
    }
    validate_cwd(value)
        .map(|value| {
            if retain {
                value.to_owned()
            } else {
                String::new()
            }
        })
        .map_err(|_| TerminalError::MalformedSnapshot)
}

fn validate_cwd(bytes: &[u8]) -> Result<&str, TerminalError> {
    if bytes.len() > MAX_CWD_BYTES {
        return Err(TerminalError::LimitExceeded);
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| TerminalError::Unsupported("OSC 7 invalid UTF-8"))?;
    let authority_path = text
        .strip_prefix("file://")
        .ok_or(TerminalError::Unsupported("OSC 7 requires file URI"))?;
    let slash = authority_path
        .find('/')
        .ok_or(TerminalError::Unsupported("OSC 7 requires absolute path"))?;
    if authority_path[..slash]
        .bytes()
        .any(|b| !b.is_ascii_alphanumeric() && !matches!(b, b'.' | b'-' | b'_'))
        || text.chars().any(char::is_control)
        || text.contains(['\\', '?', '#'])
    {
        return Err(TerminalError::Unsupported("OSC 7 URI syntax"));
    }
    let mut decoded = [0u8; MAX_CWD_BYTES];
    let mut decoded_length = 0;
    let path = &authority_path.as_bytes()[slash..];
    let mut index = 0;
    while index < path.len() {
        let byte = if path[index] == b'%' {
            let hex = path
                .get(index + 1..index + 3)
                .ok_or(TerminalError::Unsupported("OSC 7 percent escape"))?;
            let mut value = 0;
            for byte in hex {
                value = value * 16
                    + char::from(*byte)
                        .to_digit(16)
                        .ok_or(TerminalError::Unsupported("OSC 7 percent escape"))?
                        as u8;
            }
            index += 3;
            value
        } else {
            let byte = path[index];
            index += 1;
            byte
        };
        decoded[decoded_length] = byte;
        decoded_length += 1;
    }
    if std::str::from_utf8(&decoded[..decoded_length])
        .map_or(true, |s| s.chars().any(char::is_control))
    {
        return Err(TerminalError::Unsupported("OSC 7 decoded path"));
    }
    Ok(text)
}

impl Terminal {
    pub fn shell_integration(&self) -> &ShellIntegration {
        &self.shell_integration
    }

    pub(super) fn apply_shell_annotation(
        &mut self,
        selector: u32,
        payload: &[u8],
    ) -> Result<(), TerminalError> {
        let screen = self.active_screen();
        let (line_id, cell_offset) = screen.shell_cursor_anchor();
        let point = ShellPoint {
            source_sequence: self.next_source_sequence,
            line_id,
            cell_offset,
            page: self.active_page(),
            alternate: self.active_alternate,
        };
        self.shell_integration.apply(selector, payload, point)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Event;

    fn output(terminal: &mut Terminal, bytes: &[u8]) {
        terminal
            .apply_event(Event::Output {
                source_sequence: terminal.next_source_sequence(),
                bytes,
            })
            .unwrap();
    }

    #[test]
    fn shell_anchors_resolve_in_frozen_history_after_resize_but_not_after_eviction() {
        let mut terminal = Terminal::new_with_options(
            12,
            3,
            1,
            crate::TerminalOptions {
                history_capacity_rows: 100,
            },
        )
        .unwrap();
        output(&mut terminal, b"\x1b]133;A\x1b\\prompt>\x1b]133;B\x1b\\abcdefghijklmnop\r\n\x1b]133;C\x1b\\out\r\n\x1b]133;D;0\x1b\\more\r\nmore\r\n");
        let anchor = terminal.shell_integration.commands[0].input.unwrap();
        terminal
            .apply_event(Event::Resize {
                source_sequence: terminal.next_source_sequence(),
                columns: 6,
                rows: 3,
            })
            .unwrap();
        let capture = crate::HistoryCapture::capture(&terminal).unwrap();
        let offset = capture
            .offset_for_anchor(anchor.line_id, anchor.cell_offset)
            .unwrap();
        assert!(offset > 0);
        output(&mut terminal, b"\x1b[3J");
        assert_eq!(
            capture.offset_for_anchor(anchor.line_id, anchor.cell_offset),
            Ok(offset)
        );
        let current = crate::HistoryCapture::capture(&terminal).unwrap();
        assert_eq!(
            current.offset_for_anchor(anchor.line_id, anchor.cell_offset),
            Err(crate::HistoryPageError::StaleAnchor)
        );
    }

    #[test]
    fn cwd_and_command_phases_survive_every_split_snapshot_and_resize() {
        let bytes = "\x1b]7;file:///C:/work/%E4%B8%AD%20test\x1b\\\x1b]133;A;cl=m\x1b\\PS> \x1b]133;B\x1b\\echo hi\r\n\x1b]133;C\x1b\\hi\r\n\x1b]133;D;0\x1b\\".as_bytes();
        for split in 0..=bytes.len() {
            let mut terminal = Terminal::new(30, 5).unwrap();
            output(&mut terminal, &bytes[..split]);
            output(&mut terminal, &bytes[split..]);
            let state = terminal.shell_integration();
            assert_eq!(state.cwd_uri, "file:///C:/work/%E4%B8%AD%20test");
            let command = state.commands.back().unwrap();
            assert_eq!(command.phase, 3);
            assert_eq!(command.exit_code, Some(0));
            assert_eq!(command.prompt.cell_offset, 0);
            assert_eq!(command.input.unwrap().cell_offset, 4);
            assert_ne!(command.prompt.line_id, command.output.unwrap().line_id);
            let snapshot = terminal.export_snapshot_v2().unwrap();
            assert_eq!(u16::from_le_bytes(snapshot[10..12].try_into().unwrap()), 20);
            let mut restored = Terminal::import_snapshot_v2(&snapshot).unwrap();
            assert_eq!(restored.export_snapshot_v2().unwrap(), snapshot);
            assert_eq!(restored.shell_integration(), state);
            restored
                .apply_event(Event::Resize {
                    source_sequence: restored.next_source_sequence(),
                    columns: 10,
                    rows: 3,
                })
                .unwrap();
            assert_eq!(restored.shell_integration(), state);
            let snapshot = restored.export_snapshot_v2().unwrap();
            assert_eq!(
                Terminal::import_snapshot_v2(&snapshot)
                    .unwrap()
                    .shell_integration(),
                state
            );
        }
    }

    #[test]
    fn capture_and_discard_replay_have_identical_annotations() {
        let mut live = Terminal::new(10, 3).unwrap();
        let mut replay = live.clone();
        let commands = [
            b"\x1b]133;A\x1b\\\x1b]133;B\x1b\\".as_slice(),
            b"echo\r\n\x1b]133;C\x1b\\",
            b"\x1b]133;D;-1\x1b\\",
        ];
        for bytes in commands {
            let event = Event::Output {
                source_sequence: live.next_source_sequence(),
                bytes,
            };
            let prepared = live.prepare_event_with_host_effects_v2(event).unwrap();
            assert_eq!(
                u32::from_le_bytes(prepared.host_effect_frame_v2()[16..20].try_into().unwrap()),
                0
            );
            prepared.publish(|_| Ok::<(), ()>(())).unwrap();
            replay.apply_event(event).unwrap();
            assert_eq!(
                replay.export_snapshot_v2().unwrap(),
                live.export_snapshot_v2().unwrap()
            );
        }
    }

    #[test]
    fn bounded_command_history_partial_markers_alt_pages_and_reset() {
        let mut terminal = Terminal::new(10, 3).unwrap();
        output(&mut terminal, b"\x1b]133;D;0\x1b\\");
        assert!(!terminal.shell_integration.has_state());
        for _ in 0..70 {
            output(
                &mut terminal,
                b"\x1b]133;A\x1b\\\x1b]133;B\x1b\\\x1b]133;B\x1b\\",
            );
        }
        assert_eq!(terminal.shell_integration.commands.len(), MAX_COMMANDS);
        assert_eq!(terminal.shell_integration.commands.front().unwrap().id, 7);
        assert!(
            terminal
                .shell_integration
                .commands
                .iter()
                .take(63)
                .all(|c| c.phase == 4)
        );
        output(
            &mut terminal,
            b"\x1b[2 P\x1b]133;C\x1b\\\x1b[?1049h\x1b]133;D;7\x1b\\",
        );
        let command = terminal.shell_integration.commands.back().unwrap();
        assert_eq!(command.output.unwrap().page, 2);
        assert!(command.end.unwrap().alternate);
        let snapshot = terminal.export_snapshot_v2().unwrap();
        assert_eq!(
            Terminal::import_snapshot_v2(&snapshot)
                .unwrap()
                .shell_integration,
            terminal.shell_integration
        );
        output(&mut terminal, b"\x1bc\x1b]133;C\x1b\\");
        assert_eq!(terminal.shell_integration.commands.len(), 1);
        assert_eq!(terminal.shell_integration.commands[0].id, 71);
        assert_eq!(terminal.shell_integration.commands[0].phase, 2);
    }

    #[test]
    fn invalid_annotations_and_corrupt_serialized_state_do_not_commit() {
        for bytes in [
            b"\x1b]7;https://host/secret\x1b\\".as_slice(),
            b"\x1b]7;file://user@host/a\x1b\\",
            b"\x1b]7;file:///C:/bad%00\x1b\\",
            b"\x1b]7;file:///C:/bad%ff\x1b\\",
            b"\x1b]7;file:///a%Q0\x1b\\",
            b"\x1b]133;Z\x1b\\",
            b"\x1b]133;D;2147483648\x1b\\",
        ] {
            let mut terminal = Terminal::new(10, 3).unwrap();
            assert!(
                terminal
                    .apply_event(Event::Output {
                        source_sequence: terminal.next_source_sequence(),
                        bytes
                    })
                    .is_err()
            );
            assert!(!terminal.shell_integration.has_state());
        }
        let mut terminal = Terminal::new(10, 3).unwrap();
        output(
            &mut terminal,
            b"\x1b]7;file:///C:/work\x1b\\\x1b]133;A\x1b\\",
        );
        let encoded = terminal.shell_integration.encode().unwrap();
        for end in 0..encoded.len() {
            assert!(
                ShellIntegration::decode(&encoded[..end], terminal.next_source_sequence()).is_err()
            );
        }
        assert!(ShellIntegration::decode(&encoded, 0).is_err());
        let mut corrupted = encoded.clone();
        corrupted[24] = 1;
        assert!(ShellIntegration::decode(&corrupted, terminal.next_source_sequence()).is_err());
        let mut corrupted = encoded.clone();
        corrupted.extend_from_slice(b"extra");
        assert!(ShellIntegration::decode(&corrupted, terminal.next_source_sequence()).is_err());
    }
}
