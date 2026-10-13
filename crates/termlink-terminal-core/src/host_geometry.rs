//! Geometry supplied by the selected display host through the ordered journal.
//! Zero pairs mean unavailable; output sequences cannot write this state.
use crate::{CsiSequence, EffectContext, Terminal, TerminalError};

pub const HOST_GEOMETRY_V1_BYTES: usize = 32;
pub const MAX_CELL_PIXELS: u32 = 1024;
pub const MAX_DISPLAY_PIXELS: u32 = 32768;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HostGeometryV1 {
    pub cell_width: u32,
    pub cell_height: u32,
    pub display_width: u32,
    pub display_height: u32,
    pub window_width: u32,
    pub window_height: u32,
}

impl HostGeometryV1 {
    pub fn validate(self) -> Result<(), TerminalError> {
        for (width, height, maximum) in [
            (self.cell_width, self.cell_height, MAX_CELL_PIXELS),
            (self.display_width, self.display_height, MAX_DISPLAY_PIXELS),
            (self.window_width, self.window_height, MAX_DISPLAY_PIXELS),
        ] {
            if (width == 0) != (height == 0) || width > maximum || height > maximum {
                return Err(TerminalError::InvalidDimensions);
            }
        }
        Ok(())
    }

    /// Eight little-endian u32 words: version, six dimensions, reserved zero.
    pub fn decode(bytes: &[u8]) -> Result<Self, TerminalError> {
        if bytes.len() != HOST_GEOMETRY_V1_BYTES {
            return Err(TerminalError::InvalidDimensions);
        }
        let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        if word(0) != 1 || word(28) != 0 {
            return Err(TerminalError::InvalidDimensions);
        }
        let value = Self {
            cell_width: word(4),
            cell_height: word(8),
            display_width: word(12),
            display_height: word(16),
            window_width: word(20),
            window_height: word(24),
        };
        value.validate()?;
        Ok(value)
    }

    #[must_use]
    pub fn encode(self) -> [u8; HOST_GEOMETRY_V1_BYTES] {
        let mut bytes = [0; HOST_GEOMETRY_V1_BYTES];
        for (target, value) in bytes.chunks_exact_mut(4).zip([
            1,
            self.cell_width,
            self.cell_height,
            self.display_width,
            self.display_height,
            self.window_width,
            self.window_height,
            0,
        ]) {
            target.copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}

impl Terminal {
    pub(super) fn report_geometry(
        &self,
        csi: &CsiSequence,
        effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        // Unknown numeric variants of these optional probes receive no reply.
        // Neither physical screen size nor window decoration is inferred from cells.
        let g = self.host_geometry;
        let (kind, width, height) = match csi.parameters() {
            [14] if g.cell_width != 0 => {
                (4, self.columns * g.cell_width, self.rows * g.cell_height)
            }
            [14, 2] => (4, g.window_width, g.window_height),
            [15] => (5, g.display_width, g.display_height),
            [16] => (6, g.cell_width, g.cell_height),
            [18] => (8, self.columns, self.rows),
            [19] if g.cell_width != 0 => (
                9,
                g.display_width / g.cell_width,
                g.display_height / g.cell_height,
            ),
            _ => return Ok(()),
        };
        if width == 0 || height == 0 {
            return Ok(());
        }
        Self::emit_host_reply_v2(effect, format!("\x1b[{kind};{height};{width}t").as_bytes())
    }
}

pub(crate) fn canonical_geometry_reply(payload: &[u8]) -> bool {
    let Some(body) = payload
        .strip_prefix(b"\x1b[")
        .and_then(|v| v.strip_suffix(b"t"))
    else {
        return false;
    };
    let mut words = body.split(|byte| *byte == b';');
    let maximum = match words.next() {
        Some(b"4") => 512 * MAX_CELL_PIXELS,
        Some(b"5" | b"9") => MAX_DISPLAY_PIXELS,
        Some(b"6") => MAX_CELL_PIXELS,
        Some(b"8") => 512,
        _ => return false,
    };
    for _ in 0..2 {
        let Some(bytes) = words.next() else {
            return false;
        };
        if bytes.is_empty() || bytes[0] == b'0' {
            return false;
        }
        let number = bytes.iter().try_fold(0u32, |n, b| {
            if !b.is_ascii_digit() {
                return None;
            }
            n.checked_mul(10)?.checked_add(u32::from(*b - b'0'))
        });
        if !number.is_some_and(|v| v <= maximum) {
            return false;
        }
    }
    words.next().is_none()
}
