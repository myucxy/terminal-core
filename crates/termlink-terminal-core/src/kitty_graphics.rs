//! Kitty static-image control/payload decoding. No terminal placement or filesystem side effects.
use crate::{
    TerminalError, base64, graphics_png,
    graphics_resources::{GraphicsScratch, ImageData, ImageFormat, MAX_IMAGE_BYTES},
    graphics_zlib,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub const MAX_CONTROL_BYTES: usize = 1024;
pub const MAX_CHUNK_BYTES: usize = 4096;
pub const MAX_TRANSFER_BYTES: usize = graphics_png::MAX_PNG_BYTES;
pub const MAX_TRANSFER_CHUNKS: usize = 16_384;
pub const MAX_BROKER_CAPTURE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_BROKER_REQUESTS_PER_OUTPUT: u16 = 64;
const KEYS: &[u8] = b"atfosvSOiIpqmxywhXYcrCzdN";
// Fixed control storage has no allocation or ambiguous duplicate-key semantics.
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct Control {
    values: [i64; KEYS.len()],
    present: u32,
}

fn invalid(input: &[u8], offset: usize, reason: &'static str) -> TerminalError {
    TerminalError::UnsupportedPayload {
        offset,
        reason,
        byte: input.get(offset).copied(),
    }
}
impl Control {
    fn index(key: u8) -> Option<usize> {
        KEYS.iter().position(|&k| k == key)
    }
    pub fn get(&self, key: u8) -> Option<i64> {
        Self::index(key)
            .filter(|&i| self.present & (1 << i) != 0)
            .map(|i| self.values[i])
    }
    pub fn number(&self, key: u8) -> u32 {
        self.get(key).unwrap_or(0) as u32
    }
    pub fn action(&self) -> u8 {
        self.get(b'a').unwrap_or(i64::from(b't')) as u8
    }
    pub fn medium(&self) -> u8 {
        self.get(b't').unwrap_or(i64::from(b'd')) as u8
    }
    pub fn format(&self) -> u32 {
        self.get(b'f').unwrap_or(32) as u32
    }
    pub fn quiet(&self) -> u32 {
        self.number(b'q')
    }
    pub(crate) fn set(&mut self, key: u8, value: i64) {
        let i = Self::index(key).expect("known Kitty key");
        self.values[i] = value;
        self.present |= 1 << i;
    }
    fn only_continuation_keys(&self) -> bool {
        KEYS.iter()
            .copied()
            .all(|key| matches!(key, b'm' | b'q') || self.get(key).is_none())
    }
    /// `input` is the APC body starting with G, excluding ESC _ / ST.
    pub fn parse(input: &[u8]) -> Result<(Self, &[u8]), TerminalError> {
        if input.len() > MAX_CONTROL_BYTES + 1 + MAX_CHUNK_BYTES {
            return Err(TerminalError::ProtocolResourceLimit(
                "Kitty APC body exceeds header/chunk budget",
            ));
        }
        if input.first() != Some(&b'G') {
            return Err(invalid(input, 0, "Kitty graphics APC must start with G"));
        }
        let end = input.iter().position(|&b| b == b';').unwrap_or(input.len());
        if end > MAX_CONTROL_BYTES {
            return Err(TerminalError::ProtocolResourceLimit(
                "Kitty control header exceeds 1024 bytes",
            ));
        }
        let payload = if end < input.len() {
            &input[end + 1..]
        } else {
            &[]
        };
        if payload.len() > MAX_CHUNK_BYTES {
            return Err(TerminalError::ProtocolResourceLimit(
                "Kitty Base64 chunk exceeds 4096 bytes",
            ));
        }
        let mut result = Self::default();
        let mut offset = 1;
        if end > 1 {
            for item in input[1..end].split(|&b| b == b',') {
                if item.len() < 3 || item[1] != b'=' {
                    return Err(invalid(
                        input,
                        offset,
                        "Kitty control must be a single key=value",
                    ));
                }
                let key = item[0];
                if Self::index(key).is_none() {
                    return Err(invalid(
                        input,
                        offset,
                        "Kitty control key is unsupported by the static image profile",
                    ));
                }
                if result.get(key).is_some() {
                    return Err(invalid(input, offset, "duplicate Kitty control key"));
                }
                let value = &item[2..];
                let number = if matches!(key, b'a' | b't' | b'o' | b'd') {
                    if value.len() != 1 {
                        return Err(invalid(
                            input,
                            offset + 2,
                            "Kitty character control must have one byte",
                        ));
                    }
                    let allowed: &[u8] = match key {
                        b'a' => b"tTqpd",
                        b't' => b"dfts",
                        b'o' => b"z",
                        b'd' => b"aAiInNcCpPqQxXyYzZrR",
                        _ => unreachable!(),
                    };
                    if !allowed.contains(&value[0]) {
                        return Err(invalid(
                            input,
                            offset + 2,
                            "Kitty action, medium, compression or deletion selector is unsupported",
                        ));
                    }
                    i64::from(value[0])
                } else {
                    let negative = key == b'z' && value.first() == Some(&b'-');
                    let digits = if negative { &value[1..] } else { value };
                    if digits.is_empty() {
                        return Err(invalid(input, offset + 2, "Kitty control integer is empty"));
                    }
                    let mut number = 0i64;
                    for (i, &digit) in digits.iter().enumerate() {
                        if !digit.is_ascii_digit() {
                            return Err(invalid(
                                input,
                                offset + 2 + usize::from(negative) + i,
                                "Kitty control requires decimal digits",
                            ));
                        }
                        number = number
                            .checked_mul(10)
                            .and_then(|n| n.checked_add(i64::from(digit - b'0')))
                            .filter(|&n| {
                                n <= if key == b'z' {
                                    i64::from(i32::MAX) + i64::from(negative)
                                } else {
                                    i64::from(u32::MAX)
                                }
                            })
                            .ok_or_else(|| {
                                invalid(
                                    input,
                                    offset + 2 + usize::from(negative) + i,
                                    "Kitty control integer exceeds 32 bits",
                                )
                            })?;
                    }
                    if negative { -number } else { number }
                };
                if (key == b'f' && !matches!(number, 24 | 32 | 100))
                    || (key == b'q' && number > 2)
                    || (matches!(key, b'm' | b'C') && number > 1)
                {
                    return Err(invalid(
                        input,
                        offset + 2,
                        "Kitty format, quiet, continuation or cursor policy value is invalid",
                    ));
                }
                result.set(key, number);
                offset += item.len() + 1;
            }
        }
        if result.number(b'i') != 0 && result.number(b'I') != 0 {
            return Err(invalid(
                input,
                1,
                "Kitty image ID and image number cannot both be nonzero",
            ));
        }
        Ok((result, payload))
    }
}

#[derive(Debug)]
pub enum Content {
    Image(ImageData),
    /// The Windows broker must validate ownership, path and offset before returning bytes.
    /// Merely decoding this request does not declare support for the medium.
    BrokerReference(Vec<u8>),
    Display,
    Delete,
}
#[derive(Debug)]
pub struct DecodedCommand {
    pub control: Control,
    pub content: Content,
}

/// A bounded, parser-produced request. The host authorizes the resource and supplies immutable
/// bytes; core never opens the name. Kitty defines Windows `t=s` as named shared memory and
/// `O`/`S` as the read offset/count: https://sw.kovidgoyal.net/kitty/graphics-protocol/#the-transmission-medium
#[derive(Clone, Eq, PartialEq)]
pub struct BrokerRequest {
    pub source_sequence: u64,
    pub source_offset_end: u32,
    pub ordinal: u16,
    pub control_digest: [u8; 32],
    pub medium: u8,
    pub reference_utf8: String,
    pub read_offset: u64,
    /// None means read to EOF, subject to the capture budget.
    pub read_length: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrokerFailure {
    Denied,
    Unavailable,
    UnsupportedMedium,
    OverLimit,
}

#[derive(Clone, Eq, PartialEq)]
pub enum BrokerOutcome {
    Bytes(Vec<u8>),
    Failure(BrokerFailure),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrokerCapture {
    pub request: BrokerRequest,
    pub outcome: BrokerOutcome,
}

impl std::fmt::Debug for BrokerRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrokerRequest")
            .field("source_sequence", &self.source_sequence)
            .field("source_offset_end", &self.source_offset_end)
            .field("ordinal", &self.ordinal)
            .field("control_digest", &self.control_digest)
            .field("medium", &self.medium)
            .field("reference_utf8", &"<redacted>")
            .field("read_offset", &self.read_offset)
            .field("read_length", &self.read_length)
            .finish()
    }
}

impl std::fmt::Debug for BrokerOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bytes(bytes) => f.debug_tuple("Bytes").field(&bytes.len()).finish(),
            Self::Failure(failure) => f.debug_tuple("Failure").field(failure).finish(),
        }
    }
}

impl BrokerRequest {
    pub fn from_command(
        command: &DecodedCommand,
        source_sequence: u64,
        source_offset_end: u32,
        ordinal: u16,
    ) -> Result<Self, TerminalError> {
        let Content::BrokerReference(reference) = &command.content else {
            return Err(TerminalError::Unsupported(
                "Kitty command has no broker reference",
            ));
        };
        if reference.is_empty() || reference.contains(&0) {
            return Err(TerminalError::Unsupported(
                "Kitty broker reference must be nonempty UTF-8 without U+0000",
            ));
        }
        let reference_utf8 = std::str::from_utf8(reference).map_err(|_| {
            TerminalError::Unsupported("Kitty broker reference must be valid UTF-8")
        })?;
        if ordinal >= MAX_BROKER_REQUESTS_PER_OUTPUT {
            return Err(TerminalError::ProtocolResourceLimit(
                "Kitty broker request count exceeds 64 per Output",
            ));
        }
        let medium = command.control.medium();
        if !matches!(medium, b'f' | b't' | b's') {
            return Err(TerminalError::Unsupported("Kitty broker medium is invalid"));
        }
        let read_length = command.control.number(b'S');
        if read_length as usize > MAX_BROKER_CAPTURE_BYTES {
            return Err(TerminalError::ProtocolResourceLimit(
                "Kitty broker capture exceeds 16 MiB",
            ));
        }
        // S is the byte count to read for file/shm media, not an inflated PNG size.
        if command.control.format() == 100 && command.control.get(b'o').is_some() {
            return Err(TerminalError::Unsupported(
                "Kitty compressed PNG broker has no bounded decompressed size",
            ));
        }
        let mut digest = Sha256::new();
        digest.update(b"TermLink Kitty broker control v1\0");
        for (index, key) in KEYS.iter().copied().enumerate() {
            if command.control.present & (1 << index) != 0 {
                digest.update([key]);
                digest.update(command.control.values[index].to_le_bytes());
            }
        }
        digest.update((reference.len() as u32).to_le_bytes());
        digest.update(reference);
        Ok(Self {
            source_sequence,
            source_offset_end,
            ordinal,
            control_digest: digest.finalize().into(),
            medium,
            reference_utf8: reference_utf8.to_owned(),
            read_offset: u64::from(command.control.number(b'O')),
            read_length: (read_length != 0).then_some(read_length),
        })
    }

    /// Validates the exact request identity before decoding captured bytes.
    pub fn resolve(
        &self,
        command: DecodedCommand,
        capture: &BrokerCapture,
    ) -> Result<Result<DecodedCommand, BrokerFailure>, TerminalError> {
        let expected = Self::from_command(
            &command,
            self.source_sequence,
            self.source_offset_end,
            self.ordinal,
        )?;
        if *self != expected || capture.request != *self {
            return Err(TerminalError::Unsupported(
                "Kitty broker capture request identity mismatch",
            ));
        }
        match &capture.outcome {
            BrokerOutcome::Failure(failure) => Ok(Err(*failure)),
            BrokerOutcome::Bytes(bytes) => {
                if bytes.len() > MAX_BROKER_CAPTURE_BYTES {
                    return Err(TerminalError::ProtocolResourceLimit(
                        "Kitty broker capture exceeds 16 MiB",
                    ));
                }
                if self
                    .read_length
                    .is_some_and(|length| bytes.len() != length as usize)
                {
                    return Err(TerminalError::Unsupported(
                        "Kitty broker capture length does not match S",
                    ));
                }
                Ok(Ok(DecodedCommand {
                    control: command.control.clone(),
                    content: Content::Image(decode_pixels(&command.control, bytes)?),
                }))
            }
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TransferDecoder {
    pending: Option<Pending>,
}
#[derive(Clone, Debug)]
struct Pending {
    control: Control,
    tail: Arc<Chunk>,
    chunks: usize,
    bytes: usize,
}
impl PartialEq for Pending {
    fn eq(&self, other: &Self) -> bool {
        if self.control != other.control || self.chunks != other.chunks || self.bytes != other.bytes
        {
            return false;
        }
        let (mut left, mut right) = (Some(&*self.tail), Some(&*other.tail));
        while let (Some(a), Some(b)) = (left, right) {
            if std::ptr::eq(a, b) {
                return true;
            }
            if a.data.bytes() != b.data.bytes() {
                return false;
            }
            left = a.previous.as_deref();
            right = b.previous.as_deref();
        }
        left.is_none() && right.is_none()
    }
}
impl Eq for Pending {}

// Persistent append: each output transaction shares its prefix instead of cloning up to 16K Arcs.
struct Chunk {
    previous: Option<Arc<Chunk>>,
    data: GraphicsScratch,
}
impl std::fmt::Debug for Chunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Chunk")
            .field("bytes", &self.data.bytes().len())
            .field("has_previous", &self.previous.is_some())
            .finish()
    }
}
impl Drop for Chunk {
    fn drop(&mut self) {
        // A completed/aborted maximum-size upload must not recurse through 16K stack frames.
        let mut previous = self.previous.take();
        while let Some(node) = previous {
            match Arc::try_unwrap(node) {
                Ok(mut chunk) => previous = chunk.previous.take(),
                Err(_) => break,
            }
        }
    }
}
impl TransferDecoder {
    pub(crate) fn pending_control(&self) -> Option<&Control> {
        self.pending.as_ref().map(|p| &p.control)
    }
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }
    pub fn abort(&mut self) {
        self.pending = None;
    }
    /// Prior pending bytes remain intact on error. The enclosing terminal transaction decides rollback/abort.
    /// Decoder errors on assembled image bytes use decoded-domain coordinates, never fake APC offsets.
    pub fn push(&mut self, input: &[u8]) -> Result<Option<DecodedCommand>, TerminalError> {
        let (request, payload) = Control::parse(input)?;
        let more = request.number(b'm') == 1;
        // The protocol explicitly allows deletion to cancel an in-flight upload.
        if request.action() == b'd' {
            if !payload.is_empty() || more {
                return Err(invalid(
                    input,
                    input.len() - payload.len(),
                    "Kitty delete cannot contain image data",
                ));
            }
            self.pending = None;
            return Ok(Some(DecodedCommand {
                control: request,
                content: Content::Delete,
            }));
        }
        let mut control = if let Some(pending) = &self.pending {
            if !request.only_continuation_keys() {
                return Err(invalid(
                    input,
                    1,
                    "Kitty continuation permits only m and q controls",
                ));
            }
            let mut inherited = pending.control.clone();
            if let Some(quiet) = request.get(b'q') {
                inherited.set(b'q', quiet);
            }
            inherited
        } else {
            request
        };
        control.set(b'm', i64::from(more));
        if matches!(control.action(), b'p' | b'd') {
            if !payload.is_empty() || more {
                return Err(invalid(
                    input,
                    input.len() - payload.len(),
                    "Kitty display/delete commands cannot contain image data",
                ));
            }
            let content = if control.action() == b'p' {
                Content::Display
            } else {
                Content::Delete
            };
            return Ok(Some(DecodedCommand { control, content }));
        }
        if more && (payload.is_empty() || payload.contains(&b'=')) {
            return Err(invalid(
                input,
                input.len() - payload.len(),
                "Kitty non-final chunk must make progress and cannot contain Base64 padding",
            ));
        }
        if control.medium() != b'd' && more {
            return Err(invalid(
                input,
                1,
                "Kitty file/shared-memory references cannot be chunked",
            ));
        }
        // Validate dimensions and declared decompression lengths before retaining even the first chunk.
        let expected = validate_transfer(&control)?;
        let payload_offset = input.len() - payload.len();
        let length = base64::decoded_length(payload, payload_offset)?;
        let existing = self.pending.as_ref().map_or(0, |p| p.bytes);
        let chunks = self.pending.as_ref().map_or(0, |p| p.chunks);
        let total = existing
            .checked_add(length)
            .filter(|&v| v <= MAX_TRANSFER_BYTES)
            .ok_or(TerminalError::ProtocolResourceLimit(
                "Kitty decoded transfer exceeds 32 MiB",
            ))?;
        if control.medium() == b'd'
            && control.format() != 100
            && control.get(b'o').is_none()
            && total > expected
        {
            return Err(TerminalError::ProtocolResourceLimit(
                "Kitty pixel chunks exceed declared dimensions",
            ));
        }
        if chunks >= MAX_TRANSFER_CHUNKS {
            return Err(TerminalError::ProtocolResourceLimit(
                "Kitty transfer exceeds 16384 chunks",
            ));
        }
        let mut decoded = GraphicsScratch::allocate(length, MAX_CHUNK_BYTES / 4 * 3)?;
        base64::decode_into(payload, payload_offset, decoded.bytes_mut())?;
        if more {
            self.pending = Some(Pending {
                control,
                tail: Arc::new(Chunk {
                    previous: self.pending.as_ref().map(|p| p.tail.clone()),
                    data: decoded,
                }),
                chunks: chunks + 1,
                bytes: total,
            });
            return Ok(None);
        }
        let mut assembled;
        let data = if let Some(pending) = &self.pending {
            assembled = GraphicsScratch::allocate(total, MAX_TRANSFER_BYTES)?;
            let mut offset = existing;
            let mut next = Some(&*pending.tail);
            while let Some(chunk) = next {
                let start = offset - chunk.data.bytes().len();
                assembled.bytes_mut()[start..offset].copy_from_slice(chunk.data.bytes());
                offset = start;
                next = chunk.previous.as_deref();
            }
            debug_assert_eq!(offset, 0);
            assembled.bytes_mut()[existing..].copy_from_slice(decoded.bytes());
            assembled.bytes()
        } else {
            decoded.bytes()
        };
        let content = if control.medium() == b'd' {
            Content::Image(decode_pixels(&control, data)?)
        } else {
            if data.is_empty() {
                return Err(TerminalError::Unsupported(
                    "Kitty broker reference must not be empty",
                ));
            }
            if let Some(offset) = data.iter().position(|&b| b == 0) {
                return Err(TerminalError::UnsupportedDecodedPayload {
                    domain: "Kitty broker reference",
                    reason: "Kitty broker reference contains U+0000",
                    offset,
                    byte: 0,
                });
            }
            if let Err(error) = std::str::from_utf8(data) {
                let offset = error.valid_up_to();
                return Err(TerminalError::UnsupportedDecodedPayload {
                    domain: "Kitty broker reference",
                    reason: "Kitty broker reference is not UTF-8",
                    offset,
                    byte: data[offset],
                });
            }
            Content::BrokerReference(data.to_vec())
        };
        self.pending = None;
        Ok(Some(DecodedCommand { control, content }))
    }
}

fn validate_transfer(control: &Control) -> Result<usize, TerminalError> {
    if control.format() == 100 {
        let size = if control.medium() == b'd' {
            control.number(b'S') as usize
        } else {
            0
        };
        if control.medium() == b'd'
            && (size > graphics_png::MAX_PNG_BYTES || (control.get(b'o').is_some() && size == 0))
        {
            return Err(TerminalError::ProtocolResourceLimit(
                "Kitty compressed PNG requires S between 1 and 33554432",
            ));
        }
        Ok(size)
    } else {
        let rgba = ImageFormat::Rgba8888.byte_count(control.number(b's'), control.number(b'v'))?;
        Ok(if control.format() == 24 {
            rgba / 4 * 3
        } else {
            rgba
        })
    }
}
fn decoded_error(error: TerminalError, domain: &'static str) -> TerminalError {
    match error {
        TerminalError::UnsupportedPayload {
            reason,
            offset,
            byte: Some(byte),
        } => TerminalError::UnsupportedDecodedPayload {
            domain,
            reason,
            offset,
            byte,
        },
        // End-of-input has no byte to report. Preserve the reason without inventing a raw-stream coordinate.
        TerminalError::UnsupportedPayload {
            reason, byte: None, ..
        } => TerminalError::Unsupported(reason),
        other => other,
    }
}
/// Broker data must already be bounded and captured into the ordered stream. This function never opens a path.
pub fn decode_pixels(control: &Control, input: &[u8]) -> Result<ImageData, TerminalError> {
    let expected = validate_transfer(control)?;
    if control.medium() != b'd' && control.format() == 100 && control.get(b'o').is_some() {
        return Err(TerminalError::Unsupported(
            "Kitty compressed PNG broker has no bounded decompressed size",
        ));
    }
    if input.len() > MAX_TRANSFER_BYTES {
        return Err(TerminalError::ProtocolResourceLimit(
            "Kitty image payload exceeds 32 MiB",
        ));
    }
    let mut inflated;
    let bytes = if control.get(b'o').is_some() {
        inflated = GraphicsScratch::allocate(expected, MAX_TRANSFER_BYTES)?;
        graphics_zlib::inflate_exact(input, inflated.bytes_mut())
            .map_err(|e| decoded_error(e, "Kitty Base64 payload (zlib)"))?;
        inflated.bytes()
    } else {
        input
    };
    if control.format() == 100 {
        graphics_png::decode(bytes).map_err(|e| decoded_error(e, "Kitty image PNG container"))
    } else {
        if bytes.len() != expected {
            return Err(TerminalError::Unsupported(
                "Kitty RGB/RGBA payload length does not match dimensions",
            ));
        }
        let mut image = ImageData::allocate(
            control.number(b's'),
            control.number(b'v'),
            ImageFormat::Rgba8888,
        )?;
        if control.format() == 32 {
            image.pixels_mut().copy_from_slice(bytes);
        } else {
            for (rgb, rgba) in bytes
                .chunks_exact(3)
                .zip(image.pixels_mut().chunks_exact_mut(4))
            {
                rgba[..3].copy_from_slice(rgb);
                rgba[3] = 255;
            }
        }
        debug_assert!(image.pixels().len() <= MAX_IMAGE_BYTES);
        Ok(image)
    }
}

/// Deterministic acknowledgement bytes. Only the authority may send them to the PTY.
pub(crate) fn canonical_response(bytes: &[u8]) -> bool {
    let Some(body) = bytes
        .strip_prefix(b"\x1b_G")
        .and_then(|b| b.strip_suffix(b"\x1b\\"))
    else {
        return false;
    };
    let Some(separator) = body.iter().position(|b| *b == b';') else {
        return false;
    };
    let (header, message) = (&body[..separator], &body[separator + 1..]);
    if message != b"OK"
        && (message.len() > 240
            || !message.iter().all(|b| (0x20..=0x7e).contains(b))
            || ![b"EINVAL: ".as_slice(), b"ENOTSUP: ", b"ENOSPC: "]
                .iter()
                .any(|prefix| message.starts_with(prefix) && message.len() > prefix.len()))
    {
        return false;
    }
    let mut previous = 0;
    let mut identity = false;
    for item in header.split(|b| *b == b',') {
        if item.len() < 3 || item[1] != b'=' {
            return false;
        }
        let order = match item[0] {
            b'i' => 1,
            b'I' => 2,
            b'p' => 3,
            _ => return false,
        };
        let digits = &item[2..];
        if order <= previous
            || digits.len() > 10
            || digits[0] == b'0'
            || !digits.iter().all(u8::is_ascii_digit)
        {
            return false;
        }
        if std::str::from_utf8(digits)
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .is_none()
        {
            return false;
        }
        previous = order;
        identity |= order <= 2;
    }
    identity
}

pub fn response(control: &Control, success: bool, message: &str) -> Result<Vec<u8>, TerminalError> {
    if control.quiet() == 2
        || (success && control.quiet() == 1)
        || (control.number(b'i') == 0 && control.number(b'I') == 0)
    {
        return Ok(Vec::new());
    }
    if message.len() > 240 || !message.bytes().all(|b| (0x20..=0x7e).contains(&b)) {
        return Err(TerminalError::Unsupported(
            "Kitty response must be at most 240 printable ASCII bytes",
        ));
    }
    let mut text = String::from("\x1b_G");
    use std::fmt::Write;
    if control.number(b'i') != 0 {
        write!(text, "i={}", control.number(b'i')).expect("string");
    }
    if control.number(b'I') != 0 {
        if control.number(b'i') != 0 {
            text.push(',');
        }
        write!(text, "I={}", control.number(b'I')).expect("string");
    }
    if control.number(b'p') != 0 {
        write!(text, ",p={}", control.number(b'p')).expect("string");
    }
    write!(text, ";{message}\x1b\\").expect("string");
    Ok(text.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn image(result: DecodedCommand) -> ImageData {
        match result.content {
            Content::Image(image) => image,
            _ => panic!("expected pixels"),
        }
    }
    fn encode(input: &[u8]) -> Vec<u8> {
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        for c in input.chunks(3) {
            let a = c[0];
            let b = c.get(1).copied().unwrap_or(0);
            let d = c.get(2).copied().unwrap_or(0);
            out.extend_from_slice(&[
                alphabet[(a >> 2) as usize],
                alphabet[(((a & 3) << 4) | (b >> 4)) as usize],
                if c.len() > 1 {
                    alphabet[(((b & 15) << 2) | (d >> 6)) as usize]
                } else {
                    b'='
                },
                if c.len() > 2 {
                    alphabet[(d & 63) as usize]
                } else {
                    b'='
                },
            ]);
        }
        out
    }
    #[test]
    fn controls_are_typed_bounded_and_report_exact_invalid_bytes() {
        let (c, p) = Control::parse(b"Ga=T,f=24,s=1,v=1,i=4294967295,z=-2147483648;AQID").unwrap();
        assert_eq!(
            (c.action(), c.format(), c.number(b'i'), c.get(b'z')),
            (b'T', 24, u32::MAX, Some(i64::from(i32::MIN)))
        );
        assert_eq!(p, b"AQID");
        for bad in [
            b"Gf=16".as_slice(),
            b"Gs=4294967296",
            b"Gz=2147483648",
            b"Gi=-1",
            b"Ga=T,a=t",
            b"Gm=2",
            b"Gq=3",
            b"Gt=x",
            b"Go=g",
            b"Gi=1,I=2",
            b"G,",
            b"Gx=1,",
            b"GU=1",
            b"Ga=f",
        ] {
            assert!(Control::parse(bad).is_err(), "{bad:?}");
        }
        assert!(matches!(
            Control::parse(b"GX=1!"),
            Err(TerminalError::UnsupportedPayload {
                offset: 4,
                byte: Some(b'!'),
                ..
            })
        ));
    }
    #[test]
    fn rgba_chunking_padding_and_transaction_clones_preserve_pending_data() {
        let mut decoder = TransferDecoder::default();
        assert!(
            decoder
                .push(b"Ga=T,s=2,v=1,i=9,m=1;AQID")
                .unwrap()
                .is_none()
        );
        let mut frozen = decoder.clone();
        assert!(decoder.push(b"Gm=1,f=32;BAUG").is_err());
        assert!(decoder.push(b"Gm=1;BA==").is_err());
        assert!(decoder.push(b"Gm=1;").is_err());
        assert!(decoder.is_pending());
        let result = decoder.push(b"Gm=0,q=1;BAUGBwg=").unwrap().unwrap();
        assert_eq!(result.control.quiet(), 1);
        assert_eq!(result.control.number(b'm'), 0);
        assert_eq!(image(result).pixels(), &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(!decoder.is_pending());
        assert!(frozen.is_pending());
        assert_eq!(
            image(frozen.push(b"G;BAUGBwg=").unwrap().unwrap()).pixels(),
            &[1, 2, 3, 4, 5, 6, 7, 8]
        );
        assert_eq!(
            image(decoder.push(b"Gf=24,s=1,v=1;AQID").unwrap().unwrap()).pixels(),
            &[1, 2, 3, 255]
        );
    }
    #[test]
    fn maximum_chunk_journal_clones_constant_prefix_and_drops_without_recursion() {
        std::thread::Builder::new()
            .stack_size(128 * 1024)
            .spawn(|| {
                let mut decoder = TransferDecoder::default();
                decoder.push(b"Gf=24,s=4096,v=4,m=1;AQID").unwrap();
                let frozen = decoder.clone();
                let root = Arc::downgrade(&frozen.pending.as_ref().unwrap().tail);
                for _ in 1..MAX_TRANSFER_CHUNKS - 1 {
                    let old = decoder.clone();
                    assert!(Arc::ptr_eq(
                        &old.pending.as_ref().unwrap().tail,
                        &decoder.pending.as_ref().unwrap().tail
                    ));
                    decoder.push(b"Gm=1;BAUG").unwrap();
                    assert!(Arc::ptr_eq(
                        decoder
                            .pending
                            .as_ref()
                            .unwrap()
                            .tail
                            .previous
                            .as_ref()
                            .unwrap(),
                        &old.pending.as_ref().unwrap().tail
                    ));
                }
                let result = image(decoder.push(b"G;BwgJ").unwrap().unwrap());
                assert_eq!(&result.pixels()[..4], &[1, 2, 3, 255]);
                assert_eq!(&result.pixels()[4..8], &[4, 5, 6, 255]);
                assert_eq!(
                    &result.pixels()[result.pixels().len() - 4..],
                    &[7, 8, 9, 255]
                );
                assert!(root.upgrade().is_some());
                drop(frozen);
                assert!(root.upgrade().is_none());
                let mut abort = TransferDecoder::default();
                abort.push(b"Gf=100,m=1;AQID").unwrap();
                for _ in 1..MAX_TRANSFER_CHUNKS {
                    abort.push(b"Gm=1;AQID").unwrap();
                }
                assert!(abort.push(b"Gm=1;AQID").is_err());
                let root = Arc::downgrade(&abort.pending.as_ref().unwrap().tail);
                abort.abort();
                assert!(root.upgrade().is_none());
            })
            .unwrap()
            .join()
            .unwrap();
    }
    #[test]
    fn png_and_zlib_data_decode_without_executing_broker_references() {
        let vector = include_str!("../tests/fixtures/graphics-png.txt")
            .lines()
            .find(|s| s.starts_with("c6-d16-i1-1x1|"))
            .unwrap();
        let fields: Vec<_> = vector.split('|').collect();
        let hex = |s: &str| {
            s.as_bytes()
                .chunks_exact(2)
                .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
                .collect::<Vec<_>>()
        };
        let png = hex(fields[3]);
        let mut input = b"Ga=q,f=100,i=77;".to_vec();
        input.extend(encode(&png));
        assert_eq!(
            image(TransferDecoder::default().push(&input).unwrap().unwrap()).pixels(),
            hex(fields[4])
        );
        let mut compressed = vec![0x78, 1, 1, 4, 0, 0xfb, 0xff, 1, 2, 3, 4, 0, 0x18, 0, 0x0b];
        let mut request = b"Gs=1,v=1,o=z;".to_vec();
        request.extend(encode(&compressed));
        assert_eq!(
            image(TransferDecoder::default().push(&request).unwrap().unwrap()).pixels(),
            &[1, 2, 3, 4]
        );
        compressed[14] ^= 1;
        let (c, _) = Control::parse(b"Gs=1,v=1,o=z").unwrap();
        assert!(matches!(
            decode_pixels(&c, &compressed),
            Err(TerminalError::UnsupportedDecodedPayload {
                domain: "Kitty Base64 payload (zlib)",
                ..
            })
        ));
        let result = TransferDecoder::default()
            .push(b"Gf=100,t=f;QzovbWlzc2luZy5wbmc=")
            .unwrap()
            .unwrap();
        assert!(matches!(result.content,Content::BrokerReference(p) if p==b"C:/missing.png"));
    }
    #[test]
    fn broker_request_binds_control_reference_and_exact_capture_range() {
        let mut decoder = TransferDecoder::default();
        let command = decoder
            .push(b"Gt=f,f=32,s=1,v=1,S=4,O=2,i=7;QzovYS5yZ2Jh")
            .unwrap()
            .unwrap();
        let request = BrokerRequest::from_command(&command, 7, 61, 0).unwrap();
        assert_eq!(
            (request.medium, request.read_offset, request.read_length),
            (b'f', 2, Some(4))
        );
        assert_eq!(request.reference_utf8, "C:/a.rgba");
        let capture = BrokerCapture {
            request: request.clone(),
            outcome: BrokerOutcome::Bytes(vec![1, 2, 3, 4]),
        };
        let diagnostic = format!("{capture:?}");
        assert!(!diagnostic.contains("C:/a.rgba"));
        assert!(!diagnostic.contains("[1, 2, 3, 4]"));
        let resolved = request.resolve(command, &capture).unwrap().unwrap();
        assert_eq!(image(resolved).pixels(), &[1, 2, 3, 4]);

        let same = decoder
            .push(b"Gt=f,f=32,s=1,v=1,S=4,O=2,i=7;QzovYS5yZ2Jh")
            .unwrap()
            .unwrap();
        let short = BrokerCapture {
            request: request.clone(),
            outcome: BrokerOutcome::Bytes(vec![1, 2, 3]),
        };
        assert!(request.resolve(same, &short).is_err());

        let other = decoder
            .push(b"Gt=f,f=32,s=1,v=1,S=4,O=2,i=7;QzovYi5yZ2Jh")
            .unwrap()
            .unwrap();
        assert!(request.resolve(other, &capture).is_err());
        let wrong_order = BrokerRequest::from_command(
            &decoder
                .push(b"Gt=f,f=32,s=1,v=1,S=4,O=2,i=7;QzovYS5yZ2Jh")
                .unwrap()
                .unwrap(),
            7,
            61,
            1,
        )
        .unwrap();
        assert_ne!(request, wrong_order);
        assert!(
            BrokerRequest::from_command(
                &decoder
                    .push(b"Gt=f,f=32,s=1,v=1,S=4,O=2,i=7;QzovYS5yZ2Jh")
                    .unwrap()
                    .unwrap(),
                7,
                61,
                MAX_BROKER_REQUESTS_PER_OUTPUT,
            )
            .is_err()
        );
        let too_large = decoder
            .push(b"Gt=f,f=32,s=1,v=1,S=16777217;QzovYS5yZ2Jh")
            .unwrap()
            .unwrap();
        assert!(matches!(
            BrokerRequest::from_command(&too_large, 7, 61, 0),
            Err(TerminalError::ProtocolResourceLimit(_))
        ));
    }

    #[test]
    fn broker_s_is_read_size_and_never_png_inflated_size() {
        let mut decoder = TransferDecoder::default();
        let command = decoder
            .push(b"Gt=t,f=100,S=12,O=3;QzovYS5wbmc=")
            .unwrap()
            .unwrap();
        let request = BrokerRequest::from_command(&command, 1, 30, 0).unwrap();
        assert_eq!((request.read_offset, request.read_length), (3, Some(12)));
        let compressed = decoder
            .push(b"Gt=f,f=100,o=z,S=12;QzovYS5wbmc=")
            .unwrap()
            .unwrap();
        assert!(matches!(
            BrokerRequest::from_command(&compressed, 1, 30, 0),
            Err(TerminalError::Unsupported(
                "Kitty compressed PNG broker has no bounded decompressed size"
            ))
        ));
        let shm = decoder
            .push(b"Gt=s,f=32,s=1,v=1;L25hbWU=")
            .unwrap()
            .unwrap();
        let request = BrokerRequest::from_command(&shm, 1, 30, 0).unwrap();
        let failure = BrokerCapture {
            request: request.clone(),
            outcome: BrokerOutcome::Failure(BrokerFailure::UnsupportedMedium),
        };
        assert!(matches!(
            request.resolve(shm, &failure),
            Ok(Err(BrokerFailure::UnsupportedMedium))
        ));
    }
    #[test]
    fn broker_shared_memory_uses_only_exact_host_capture_bytes() {
        let mut decoder = TransferDecoder::default();
        let input = b"Gt=s,f=32,s=1,v=1,S=4,O=2;L25hbWU=";
        let command = decoder.push(input).unwrap().unwrap();
        let request = BrokerRequest::from_command(&command, 7, 42, 0).unwrap();
        assert_eq!((request.read_offset, request.read_length), (2, Some(4)));
        let capture = BrokerCapture {
            request: request.clone(),
            outcome: BrokerOutcome::Bytes(vec![1, 2, 3, 4]),
        };
        assert_eq!(
            image(request.resolve(command, &capture).unwrap().unwrap()).pixels(),
            &[1, 2, 3, 4]
        );
        let command = decoder.push(input).unwrap().unwrap();
        let mut wrong = capture.clone();
        wrong.request.read_offset += 1;
        assert!(request.resolve(command, &wrong).is_err());
        let command = decoder.push(input).unwrap().unwrap();
        let short = BrokerCapture {
            request: request.clone(),
            outcome: BrokerOutcome::Bytes(vec![1, 2, 3]),
        };
        assert!(request.resolve(command, &short).is_err());
        let command = decoder.push(input).unwrap().unwrap();
        let denied = BrokerCapture {
            request: request.clone(),
            outcome: BrokerOutcome::Failure(BrokerFailure::Denied),
        };
        assert_eq!(
            request.resolve(command, &denied).unwrap().unwrap_err(),
            BrokerFailure::Denied
        );
        let oversized = decoder
            .push(b"Gt=s,f=32,s=1,v=1,S=16777217;L25hbWU=")
            .unwrap()
            .unwrap();
        assert!(matches!(
            BrokerRequest::from_command(&oversized, 7, 42, 0),
            Err(TerminalError::ProtocolResourceLimit(_))
        ));
    }
    #[test]
    fn final_error_is_atomic_abort_releases_state_and_all_limits_are_checked() {
        let mut decoder = TransferDecoder::default();
        decoder.push(b"Gs=1,v=1,m=1;AQID").unwrap();
        assert!(decoder.push(b"G;BAUG").is_err());
        assert!(decoder.is_pending());
        assert_eq!(
            image(decoder.push(b"G;BA==").unwrap().unwrap()).pixels(),
            &[1, 2, 3, 4]
        );
        for input in [
            b"Gs=4096,v=4096,m=1;AQID".as_slice(),
            b"Gf=100,o=z;AAAA",
            b"Ga=p,i=1;AAAA",
            b"Gf=100,t=s;AA==",
            b"Gs=1,v=1;AQI=",
            b"Gs=1,v=1;AA_A",
        ] {
            assert!(TransferDecoder::default().push(input).is_err(), "{input:?}");
        }
        decoder.push(b"Gs=1,v=1,m=1;AQID").unwrap();
        decoder.abort();
        assert!(!decoder.is_pending());
        decoder.push(b"Gs=1,v=1,m=1;AQID").unwrap();
        assert!(matches!(
            decoder.push(b"Ga=d").unwrap().unwrap().content,
            Content::Delete
        ));
        assert!(!decoder.is_pending());
        let mut too_long = b"Gs=1,v=1;".to_vec();
        too_long.extend(vec![b'A'; 4097]);
        assert!(decoder.push(&too_long).is_err());
    }
    #[test]
    fn quiet_responses_and_display_delete_commands_are_inert() {
        let (c, _) = Control::parse(b"Ga=p,i=42,p=7").unwrap();
        for reply in [
            b"\x1b_Gi=1;OK\x1b\\".as_slice(),
            b"\x1b_Gi=4294967295,I=9,p=2;OK\x1b\\",
        ] {
            assert!(canonical_response(reply));
        }
        for reply in [
            b"\x1b_Gp=1;OK\x1b\\".as_slice(),
            b"\x1b_Gi=0;OK\x1b\\",
            b"\x1b_Gi=01;OK\x1b\\",
            b"\x1b_Gi=4294967296;OK\x1b\\",
            b"\x1b_Gi=1,i=2;OK\x1b\\",
            b"\x1b_GI=2,i=1;OK\x1b\\",
            b"\x1b_Gi=1;OK\x1b[2J\x1b\\",
        ] {
            assert!(!canonical_response(reply));
        }
        assert_eq!(
            response(&c, true, "OK").unwrap(),
            b"\x1b_Gi=42,p=7;OK\x1b\\"
        );
        assert!(response(&c, false, "bad\x1b[2J").is_err());
        for q in 0..=2 {
            let mut c = c.clone();
            c.set(b'q', q);
            assert_eq!(response(&c, true, "OK").unwrap().is_empty(), q != 0);
            assert_eq!(
                response(&c, false, "ENOENT: image not found")
                    .unwrap()
                    .is_empty(),
                q == 2
            );
        }
        let mut decoder = TransferDecoder::default();
        assert!(matches!(
            decoder.push(b"Ga=p,i=42").unwrap().unwrap().content,
            Content::Display
        ));
        assert!(matches!(
            decoder.push(b"Ga=d,d=I,i=42").unwrap().unwrap().content,
            Content::Delete
        ));
    }
}
