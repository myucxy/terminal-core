//! Selected display-host theme. It is journal state, never inferred from an OS.
use crate::osc::{OscBaseline, OscColor};
use crate::{EffectContext, Terminal, TerminalError};

pub const HOST_THEME_V1_BYTES: usize = 8 + 259 * 4;
pub(crate) const THEME_STATE_BYTES: usize = 40;

/// Version, scheme (1 dark / 2 light), then 259 opaque RGBx colors.
/// Slots 0..255 are the palette; 256/257/258 are foreground/background/cursor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostThemeV1 {
    pub scheme: u8,
    pub baseline: OscBaseline,
}

impl HostThemeV1 {
    pub fn decode(bytes: &[u8]) -> Result<Self, TerminalError> {
        if bytes.len() != HOST_THEME_V1_BYTES
            || bytes[..4] != 1u32.to_le_bytes()
            || !matches!(bytes[4], 1 | 2)
            || bytes[5..8] != [0; 3]
        {
            return Err(TerminalError::InvalidDimensions);
        }
        let mut colors = [None; 259];
        for (slot, rgb) in colors.iter_mut().zip(bytes[8..].chunks_exact(4)) {
            if rgb[3] != 0 {
                return Err(TerminalError::InvalidDimensions);
            }
            *slot = Some(OscColor::new(rgb[0], rgb[1], rgb[2]));
        }
        Ok(Self {
            scheme: bytes[4],
            baseline: OscBaseline {
                palette: colors[..256].try_into().unwrap(),
                foreground: colors[256],
                background: colors[257],
                cursor: colors[258],
            },
        })
    }

    pub fn encode(&self) -> Result<[u8; HOST_THEME_V1_BYTES], TerminalError> {
        if !matches!(self.scheme, 1 | 2) {
            return Err(TerminalError::InvalidDimensions);
        }
        let mut bytes = [0; HOST_THEME_V1_BYTES];
        bytes[..4].copy_from_slice(&1u32.to_le_bytes());
        bytes[4] = self.scheme;
        for (target, color) in
            bytes[8..]
                .chunks_exact_mut(4)
                .zip(self.baseline.palette.iter().copied().chain([
                    self.baseline.foreground,
                    self.baseline.background,
                    self.baseline.cursor,
                ]))
        {
            let color = color.ok_or(TerminalError::InvalidDimensions)?;
            target[..3].copy_from_slice(&[color.red, color.green, color.blue]);
        }
        Ok(bytes)
    }
}

impl Terminal {
    pub(super) fn apply_host_theme(
        &mut self,
        bytes: &[u8],
        capture: Option<&mut dyn crate::host_effect::CaptureSink>,
    ) -> Result<(), TerminalError> {
        let value = HostThemeV1::decode(bytes)?;
        let changed = self.osc.theme_scheme() != value.scheme;
        self.osc.apply_host_theme(&value);
        if changed && self.modes.color_scheme_reporting {
            let effect = capture.map(|capture| EffectContext {
                send_c1: self.modes.send_c1,
                capture,
                source_sequence: self.next_source_sequence,
                source_offset_end: HOST_THEME_V1_BYTES as u32,
            });
            self.report_theme(effect)?;
        }
        Ok(())
    }

    pub(super) fn report_theme(
        &self,
        effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        match self.osc.theme_scheme() {
            1 => Self::emit_host_reply_v2(effect, b"\x1b[?997;1n"),
            2 => Self::emit_host_reply_v2(effect, b"\x1b[?997;2n"),
            _ => Ok(()),
        }
    }
}

pub(crate) fn is_theme_reply(bytes: &[u8]) -> bool {
    matches!(bytes, b"\x1b[?997;1n" | b"\x1b[?997;2n")
}
