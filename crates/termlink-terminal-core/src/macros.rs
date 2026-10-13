//! DEC text/hex macros. No filesystem, process or host side effects are executed here.
use std::sync::Arc;

use crate::{CsiSequence, DcsSequence, EffectContext, Terminal, TerminalError};

pub(crate) const CAPACITY: usize = 64;
pub(crate) const MAX_BYTES: usize = 256 * 1024;
pub(crate) const MAX_ENCODED_BYTES: usize = MAX_BYTES * 2;
pub(crate) const MAX_DEPTH: u8 = 16;
pub(crate) const MAX_EXPANDED_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_BATCH_EXPANDED_BYTES: usize = 4 * MAX_EXPANDED_BYTES;
pub(crate) const MAX_SNAPSHOT_BYTES: usize = 8 + CAPACITY * 8 + MAX_BYTES;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MacroStore {
    entries: [Option<Arc<Vec<u8>>>; CAPACITY],
}
impl Default for MacroStore {
    fn default() -> Self {
        Self {
            entries: std::array::from_fn(|_| None),
        }
    }
}
impl MacroStore {
    pub(crate) fn entries(&self) -> impl Iterator<Item = (usize, &[u8])> {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(id, bytes)| bytes.as_ref().map(|b| (id, b.as_slice())))
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.iter().all(Option::is_none)
    }
    pub(crate) fn used(&self) -> usize {
        self.entries().map(|(_, b)| b.len()).sum()
    }
    pub(crate) fn checksum(&self) -> u16 {
        self.entries()
            .flat_map(|(_, b)| b)
            .fold(0_u16, |sum, &b| sum.wrapping_sub(u16::from(b)))
    }
    pub(crate) fn install(
        &mut self,
        id: usize,
        bytes: Vec<u8>,
        delete_all: bool,
    ) -> Result<(), TerminalError> {
        if id >= CAPACITY {
            return Err(TerminalError::Unsupported("DECDMAC macro identifier"));
        }
        let retained = if delete_all {
            0
        } else {
            self.used() - self.entries[id].as_ref().map_or(0, |v| v.len())
        };
        if bytes.len() > MAX_BYTES - retained {
            return Err(TerminalError::ProtocolResourceLimit(
                "DECDMAC total storage exceeds 262144 bytes",
            ));
        }
        if delete_all {
            *self = Self::default();
        }
        self.entries[id] = if bytes.is_empty() {
            None
        } else {
            Some(Arc::new(bytes))
        };
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct MacroExecution {
    pub(crate) depth: u8,
    pub(crate) expanded_bytes: usize,
    pub(crate) batch_expanded_bytes: usize,
    // RIS within an invocation cancels the remainder, including outer invocations.
    pub(crate) cancelled: bool,
}

impl Terminal {
    pub(super) fn define_macro(&mut self, sequence: &DcsSequence) -> Result<(), TerminalError> {
        if self.macro_execution.depth != 0 {
            return Err(TerminalError::Unsupported(
                "DECDMAC definition inside macro invocation",
            ));
        }
        if sequence.parameters().len() > 3 {
            return Err(TerminalError::Unsupported("DECDMAC parameter count"));
        }
        let parameter = |index, default| {
            sequence
                .parameters()
                .get(index)
                .copied()
                .filter(|&p| p != crate::parser::PARAM_MISSING)
                .unwrap_or(default)
        };
        let id = usize::from(parameter(0, 0));
        let delete_all = parameter(1, 0);
        let encoding = parameter(2, 0);
        for (index, invalid, reason) in [
            (0, id >= CAPACITY, "DECDMAC macro identifier"),
            (1, delete_all > 1, "DECDMAC delete control"),
            (2, encoding > 1, "DECDMAC encoding"),
        ] {
            if invalid {
                self.diagnostics.trace.reject_parameter(index);
                return Err(TerminalError::Unsupported(reason));
            }
        }
        let bytes = decode(sequence.payload(), encoding == 1)?;
        self.macros.install(id, bytes, delete_all == 1)
    }

    pub(super) fn invoke_macro(
        &mut self,
        csi: &CsiSequence,
        mut effect: Option<EffectContext<'_>>,
    ) -> Result<(), TerminalError> {
        crate::require_max_parameters(csi, 1, "DECINVM parameter count")?;
        let id = usize::from(csi.parameter(0, 0));
        if id >= CAPACITY {
            self.diagnostics.trace.reject_parameter(0);
            return Err(TerminalError::Unsupported("DECINVM macro identifier"));
        }
        let Some(bytes) = self.macros.entries[id].clone() else {
            return Ok(());
        };
        if self.macro_execution.depth == MAX_DEPTH {
            return Err(TerminalError::ProtocolResourceLimit(
                "DECINVM nesting exceeds 16 invocations",
            ));
        }
        if bytes.len() > MAX_EXPANDED_BYTES - self.macro_execution.expanded_bytes {
            return Err(TerminalError::ProtocolResourceLimit(
                "DECINVM expansion exceeds 1048576 bytes per source event",
            ));
        }
        if bytes.len() > MAX_BATCH_EXPANDED_BYTES - self.macro_execution.batch_expanded_bytes {
            return Err(TerminalError::ProtocolResourceLimit(
                "DECINVM expansion exceeds 4194304 bytes per transaction",
            ));
        }
        self.macro_execution.depth += 1;
        self.macro_execution.expanded_bytes += bytes.len();
        self.macro_execution.batch_expanded_bytes += bytes.len();
        if self.diagnostics.physical_trace.is_none() {
            self.diagnostics.physical_trace = Some(self.diagnostics.trace.clone());
        }
        let result = (|| {
            for (offset, &byte) in bytes.iter().enumerate() {
                if self.macro_execution.cancelled {
                    break;
                }
                let sensitive = !self.parser.is_public_control_payload()
                    && (self.parser.is_string_payload()
                        || matches!(
                            self.parser,
                            crate::ParserState::OscEscape(_)
                                | crate::ParserState::DcsEscape(_)
                                | crate::ParserState::ApcEscape(_)
                                | crate::ParserState::IgnoredStringEscape(_)
                        ));
                let restart = (self.parser.is_ground() && self.utf8.len == 0)
                    || (byte == 0x1b && !self.parser.is_string_state());
                self.diagnostics.trace.observe_macro(
                    byte,
                    id as u64,
                    offset as u32,
                    restart,
                    sensitive,
                );
                let mut context = effect.as_mut().map(EffectContext::reborrow);
                if let Some(context) = &mut context {
                    context.send_c1 = self.modes.send_c1 && !self.modes.vt52;
                }
                self.process_byte(byte, context)?;
            }
            Ok(())
        })();
        self.macro_execution.depth -= 1;
        if self.macro_execution.depth == 0 {
            self.macro_execution.cancelled = false;
        }
        result
    }
}

fn append(output: &mut Vec<u8>, bytes: &[u8], count: usize) -> Result<(), TerminalError> {
    let additional = bytes
        .len()
        .checked_mul(count)
        .ok_or(TerminalError::LimitExceeded)?;
    if additional > MAX_BYTES - output.len() {
        return Err(TerminalError::ProtocolResourceLimit(
            "DECDMAC decoded definition exceeds 262144 bytes",
        ));
    }
    if additional == 0 {
        return Ok(());
    }
    output
        .try_reserve(additional)
        .map_err(|_| TerminalError::LimitExceeded)?;
    for _ in 0..count {
        output.extend_from_slice(bytes);
    }
    Ok(())
}

fn hex(byte: u8, offset: usize) -> Result<u8, TerminalError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(TerminalError::UnsupportedPayload {
            reason: "DECDMAC invalid hex digit",
            offset,
            byte: Some(byte),
        }),
    }
}

fn decode(payload: &[u8], hexadecimal: bool) -> Result<Vec<u8>, TerminalError> {
    let mut output = Vec::new();
    if !hexadecimal {
        append(&mut output, payload, 1)?;
        return Ok(output);
    }
    let mut offset = 0;
    while offset < payload.len() {
        if payload[offset] == b'!' {
            offset += 1;
            let start = offset;
            let mut count = 0usize;
            while let Some(&digit) = payload.get(offset).filter(|b| b.is_ascii_digit()) {
                count = count
                    .checked_mul(10)
                    .and_then(|v| v.checked_add(usize::from(digit - b'0')))
                    .filter(|&n| n <= 65534)
                    .ok_or(TerminalError::ProtocolResourceLimit(
                        "DECDMAC repeat count exceeds 65534",
                    ))?;
                offset += 1;
            }
            if offset == start || payload.get(offset) != Some(&b';') {
                return Err(TerminalError::UnsupportedPayload {
                    reason: "DECDMAC repeat count syntax",
                    offset,
                    byte: payload.get(offset).copied(),
                });
            }
            offset += 1;
            let end = payload[offset..]
                .iter()
                .position(|&b| b == b';')
                .map_or(payload.len(), |v| v + offset);
            let mut repeated = Vec::new();
            decode_pairs(&payload[offset..end], offset, &mut repeated)?;
            append(&mut output, &repeated, count.max(1))?;
            offset = end + usize::from(end < payload.len());
        } else {
            let first = hex(payload[offset], offset)?;
            let second =
                payload
                    .get(offset + 1)
                    .copied()
                    .ok_or(TerminalError::UnsupportedPayload {
                        reason: "DECDMAC incomplete hex pair",
                        offset: offset + 1,
                        byte: None,
                    })?;
            append(&mut output, &[first * 16 + hex(second, offset + 1)?], 1)?;
            offset += 2;
        }
    }
    Ok(output)
}

fn decode_pairs(payload: &[u8], base: usize, output: &mut Vec<u8>) -> Result<(), TerminalError> {
    for (index, pair) in payload.chunks(2).enumerate() {
        let offset = base + index * 2;
        let first = hex(pair[0], offset)?;
        let second = pair
            .get(1)
            .copied()
            .ok_or(TerminalError::UnsupportedPayload {
                reason: "DECDMAC incomplete hex pair",
                offset: offset + 1,
                byte: None,
            })?;
        append(output, &[first * 16 + hex(second, offset + 1)?], 1)?;
    }
    Ok(())
}
