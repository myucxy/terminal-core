//! Additive state delta V1. Checkpoints retain shared canonical blocks, never event replay.
use crate::{Terminal, TerminalError, graphics_state::GraphicsState, snapshot};
use sha2::{Digest, Sha256};
use std::{fmt, sync::Arc};

/// Graphics Snapshot 2.22, LegacyV1 width (1), UTF-8 output (0), resources V2.
pub const STATE_DELTA_PROFILE_V1: u32 = 1;
/// Graphics Snapshot 2.22, Unicode16 width (2), UTF-8 output (0), resources V2.
pub const STATE_DELTA_PROFILE_UNICODE16_V1: u32 = 2;
/// Graphics Snapshot 2.22, LegacyV1 width (1), standalone C1 output (1), resources V2.
pub const STATE_DELTA_PROFILE_STANDALONE_C1_V1: u32 = 3;
/// Graphics Snapshot 2.22, Unicode16 width (2), standalone C1 output (1), resources V2.
pub const STATE_DELTA_PROFILE_UNICODE16_STANDALONE_C1_V1: u32 = 4;
pub const MAX_STATE_DELTA_BYTES: usize = crate::MAX_GRAPHICS_SNAPSHOT_BYTES;
pub(crate) const BLOCK: usize = 4096;
const HEADER: usize = 256;
const SNAP_HEADER: usize = 104;
const MAX_ENVELOPE: usize = SNAP_HEADER + 21 * 32;
const GRAPHICS: u32 = 0x500;
const RESOURCE: u8 = 3;
const MAX_OBJECTS: usize = 21 + 3 + crate::graphics_resources::MAX_IMAGE_RESOURCES;
const MAX_OPERATIONS: usize = MAX_OBJECTS + crate::graphics_resources::MAX_IMAGE_RESOURCES;
const KEEP: u8 = 0;
const PATCH: u8 = 1;
const ADD: u8 = 2;
const DELETE: u8 = 3;
const DOMAIN: &[u8] = b"TermLink state delta V1\0";
const TAGS: [u32; 21] = [
    1, 2, 3, 4, 8, 0x10, 0x18, 0x20, 0x40, 0x100, 0x101, 0x200, 0x201, 0x210, 0x211, 0x300, 0x400,
    0x401, 0x402, 0x403, GRAPHICS,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeltaContext {
    /// Globally unique session/stream/branch identity, fixed by the authenticated caller.
    pub id: [u8; 32],
    pub profile: u32,
}
impl DeltaContext {
    pub(crate) fn validate(self) -> Result<(), DeltaError> {
        if !matches!(
            self.profile,
            STATE_DELTA_PROFILE_V1
                | STATE_DELTA_PROFILE_UNICODE16_V1
                | STATE_DELTA_PROFILE_STANDALONE_C1_V1
                | STATE_DELTA_PROFILE_UNICODE16_STANDALONE_C1_V1
        ) || self.id == [0; 32]
        {
            return Err(DeltaError::Incompatible);
        }
        Ok(())
    }
    fn validate_width(self, width: crate::WidthProfile) -> Result<(), DeltaError> {
        if width
            != if matches!(
                self.profile,
                STATE_DELTA_PROFILE_UNICODE16_V1 | STATE_DELTA_PROFILE_UNICODE16_STANDALONE_C1_V1
            ) {
                crate::WidthProfile::Unicode16
            } else {
                crate::WidthProfile::LegacyV1
            }
        {
            return Err(DeltaError::Incompatible);
        }
        Ok(())
    }
    fn validate_output(self, output: crate::OutputProfile) -> Result<(), DeltaError> {
        if output
            != if matches!(
                self.profile,
                STATE_DELTA_PROFILE_STANDALONE_C1_V1
                    | STATE_DELTA_PROFILE_UNICODE16_STANDALONE_C1_V1
            ) {
                crate::OutputProfile::Utf8WithStandaloneC1
            } else {
                crate::OutputProfile::Utf8
            }
        {
            return Err(DeltaError::Incompatible);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeltaStateInfo {
    /// Exclusive next source sequence: delta covers [base, target).
    pub next_source_sequence: u64,
    pub snapshot_bytes: u64,
    pub sha256: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeltaError {
    Malformed,
    Incompatible,
    LimitExceeded,
    /// Missing checkpoint, different branch/context, expired base, or no bandwidth benefit.
    NeedsFullSnapshot,
    Terminal(TerminalError),
}
impl fmt::Display for DeltaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed => f.write_str("malformed state delta"),
            Self::Incompatible => f.write_str("incompatible state delta profile or features"),
            Self::LimitExceeded => f.write_str("state delta allocation or configuration limit"),
            Self::NeedsFullSnapshot => f.write_str("full snapshot required"),
            Self::Terminal(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for DeltaError {}
impl From<TerminalError> for DeltaError {
    fn from(error: TerminalError) -> Self {
        Self::Terminal(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct Key {
    tag: u32,
    kind: u8,
    id: u64,
}
pub(crate) struct Object {
    key: Key,
    len: usize,
    pub(crate) blocks: Vec<Arc<Vec<u8>>>,
}
impl Object {
    fn matches(&self, bytes: &[u8]) -> bool {
        self.len == bytes.len()
            && self
                .blocks
                .iter()
                .zip(bytes.chunks(self.chunk_size()))
                .all(|(block, other)| block.as_slice() == other)
    }
    fn chunk_size(&self) -> usize {
        if self.key.kind == RESOURCE {
            self.len
        } else {
            BLOCK
        }
    }
    fn equals(&self, other: &Self) -> bool {
        self.len == other.len
            && self.blocks.len() == other.blocks.len()
            && self
                .blocks
                .iter()
                .zip(&other.blocks)
                .all(|(a, b)| Arc::ptr_eq(a, b) || a == b)
    }
    fn write(&self, bytes: &mut [u8]) -> Result<(), DeltaError> {
        if bytes.len() != self.len {
            return Err(DeltaError::Malformed);
        }
        let mut at = 0;
        for block in &self.blocks {
            let end = add(at, block.len())?;
            bytes
                .get_mut(at..end)
                .ok_or(DeltaError::Malformed)?
                .copy_from_slice(block);
            at = end;
        }
        if at != bytes.len() {
            return Err(DeltaError::Malformed);
        }
        Ok(())
    }
}

/// An immutable legal checkpoint; cloning the terminal is not required for retention.
/// Snapshot input is fully validated before capture, without materializing image pixels.
pub struct DeltaCheckpoint {
    pub(crate) context: DeltaContext,
    pub(crate) info: DeltaStateInfo,
    pub(crate) envelope: Vec<u8>,
    pub(crate) objects: Vec<Object>,
}
impl DeltaCheckpoint {
    pub fn capture(context: DeltaContext, terminal: &Terminal) -> Result<Self, DeltaError> {
        context.validate()?;
        Self::from_snapshot(context, &terminal.export_snapshot_v2()?)
    }
    pub fn from_snapshot(context: DeltaContext, bytes: &[u8]) -> Result<Self, DeltaError> {
        Self::capture_shared(context, bytes, None)
    }
    pub fn info(&self) -> DeltaStateInfo {
        self.info
    }
    pub fn context(&self) -> DeltaContext {
        self.context
    }
    /// Materialize only on explicit request; the journal never retains this full buffer.
    pub fn snapshot_bytes(&self) -> Result<Vec<u8>, DeltaError> {
        assemble(&self.envelope, &self.objects, self.info.snapshot_bytes)
    }
    pub(crate) fn capture_shared(
        context: DeltaContext,
        bytes: &[u8],
        previous: Option<&Self>,
    ) -> Result<Self, DeltaError> {
        context.validate()?;
        let view = snapshot::delta_view(bytes)?;
        context.validate_width(view.width_profile)?;
        context.validate_output(view.output_profile)?;
        let info = DeltaStateInfo {
            next_source_sequence: view.sequence,
            snapshot_bytes: bytes.len() as u64,
            sha256: hash(bytes),
        };
        let mut objects = reserved(MAX_OBJECTS)?;
        for (tag, section) in view.sections {
            if tag != GRAPHICS {
                objects.push(capture_object(
                    Key {
                        tag,
                        kind: 0,
                        id: 0,
                    },
                    section,
                    previous,
                )?);
            } else {
                let resources = GraphicsState::inspect_resources(section)?;
                let resource_len = read32(section, 36)? as usize;
                let end = add(64, resource_len)?;
                objects.push(capture_object(
                    Key {
                        tag,
                        kind: 1,
                        id: 0,
                    },
                    &section[..64],
                    previous,
                )?);
                objects.push(capture_object(
                    Key {
                        tag,
                        kind: 2,
                        id: 0,
                    },
                    &section[64..112],
                    previous,
                )?);
                for (id, record) in resources.encoded_records() {
                    objects.push(capture_object(
                        Key {
                            tag,
                            kind: RESOURCE,
                            id,
                        },
                        record,
                        previous,
                    )?);
                }
                objects.push(capture_object(
                    Key {
                        tag,
                        kind: 4,
                        id: 0,
                    },
                    &section[end..],
                    previous,
                )?);
            }
        }
        let checkpoint = Self {
            context,
            info,
            envelope: copy(view.envelope)?,
            objects,
        };
        if let Some(base) = previous {
            validate_resources(base, &checkpoint)?;
        }
        Ok(checkpoint)
    }
    fn object(&self, key: Key) -> Option<&Object> {
        self.objects
            .binary_search_by_key(&key, |object| object.key)
            .ok()
            .map(|i| &self.objects[i])
    }
    pub(crate) fn metadata_bytes(&self) -> Result<usize, DeltaError> {
        let mut size = add(std::mem::size_of::<Self>(), self.envelope.capacity())?;
        size = add(
            size,
            mul(self.objects.capacity(), std::mem::size_of::<Object>())?,
        )?;
        for object in &self.objects {
            size = add(
                size,
                mul(
                    object.blocks.capacity(),
                    std::mem::size_of::<Arc<Vec<u8>>>(),
                )?,
            )?;
        }
        Ok(size)
    }
}

fn capture_object(
    key: Key,
    bytes: &[u8],
    previous: Option<&DeltaCheckpoint>,
) -> Result<Object, DeltaError> {
    if bytes.is_empty() {
        return Err(DeltaError::Malformed);
    }
    let base = previous.and_then(|point| point.object(key));
    if key.kind == RESOURCE && base.is_some_and(|base| !base.matches(bytes)) {
        return Err(DeltaError::NeedsFullSnapshot);
    }
    let chunk = if key.kind == RESOURCE {
        bytes.len()
    } else {
        BLOCK
    };
    let mut blocks = reserved(bytes.len().div_ceil(chunk))?;
    for (index, bytes) in bytes.chunks(chunk).enumerate() {
        blocks.push(
            match base
                .and_then(|base| base.blocks.get(index))
                .filter(|b| b.as_slice() == bytes)
            {
                Some(block) => Arc::clone(block),
                None => Arc::new(copy(bytes)?),
            },
        );
    }
    Ok(Object {
        key,
        len: bytes.len(),
        blocks,
    })
}

fn resource_next(point: &DeltaCheckpoint) -> Result<u64, DeltaError> {
    let header = point
        .object(Key {
            tag: GRAPHICS,
            kind: 2,
            id: 0,
        })
        .ok_or(DeltaError::Malformed)?;
    read64(&header.blocks[0], 16)
}
fn validate_resources(base: &DeltaCheckpoint, target: &DeltaCheckpoint) -> Result<(), DeltaError> {
    let next = resource_next(base)?;
    if resource_next(target)? < next {
        return Err(DeltaError::NeedsFullSnapshot);
    }
    for object in target.objects.iter().filter(|o| o.key.kind == RESOURCE) {
        match base.object(object.key) {
            Some(old) if !old.equals(object) => return Err(DeltaError::NeedsFullSnapshot),
            None if object.key.id < next => return Err(DeltaError::NeedsFullSnapshot),
            _ => {}
        }
    }
    Ok(())
}

/// No mutation, event replay, host effects, or embedded full-snapshot fallback.
pub fn export_delta_v1(
    base: &DeltaCheckpoint,
    target: &DeltaCheckpoint,
    max_bytes: usize,
) -> Result<Vec<u8>, DeltaError> {
    if base.context != target.context
        || base.info.next_source_sequence >= target.info.next_source_sequence
    {
        return Err(DeltaError::NeedsFullSnapshot);
    }
    validate_resources(base, target)?;
    let target_size = usize_from(target.info.snapshot_bytes)?;
    let limit = max_bytes
        .min(MAX_STATE_DELTA_BYTES)
        .min(target_size.saturating_sub(1));
    let mut out = Writer {
        bytes: Vec::new(),
        limit,
    };
    out.zeros(HEADER)?;
    out.append(&target.envelope)?;
    let mut i = 0;
    let mut j = 0;
    let mut count = 0u32;
    while i < base.objects.len() || j < target.objects.len() {
        let old = base.objects.get(i);
        let new = target.objects.get(j);
        let key = match (old, new) {
            (Some(a), Some(b)) => a.key.min(b.key),
            (Some(a), None) => a.key,
            (None, Some(b)) => b.key,
            _ => unreachable!(),
        };
        let old = old.filter(|o| o.key == key);
        let new = new.filter(|o| o.key == key);
        let action = match (old, new) {
            (Some(a), Some(b)) if a.equals(b) => KEEP,
            (Some(_), Some(_)) => PATCH,
            (None, Some(_)) => ADD,
            (Some(_), None) => DELETE,
            _ => unreachable!(),
        };
        let op = out.bytes.len();
        out.zeros(32)?;
        put32(&mut out.bytes, op, key.tag);
        out.bytes[op + 4] = key.kind;
        out.bytes[op + 5] = action;
        put64(&mut out.bytes, op + 8, key.id);
        put64(&mut out.bytes, op + 16, new.map_or(0, |o| o.len as u64));
        let mut patches = 0;
        if matches!(action, PATCH | ADD) {
            let new = new.ok_or(DeltaError::Malformed)?;
            for (index, block) in new.blocks.iter().enumerate() {
                if old
                    .and_then(|o| o.blocks.get(index))
                    .is_some_and(|b| b == block)
                {
                    continue;
                }
                let offset = mul(index, new.chunk_size())?;
                out.append(&(offset as u64).to_le_bytes())?;
                out.append(&(block.len() as u32).to_le_bytes())?;
                out.append(&[0; 4])?;
                out.append(block)?;
                patches += 1;
            }
        }
        put32(&mut out.bytes, op + 24, patches);
        if old.is_some() {
            i += 1;
        }
        if new.is_some() {
            j += 1;
        }
        count += 1;
    }
    let size = out.bytes.len() as u64;
    let bytes = &mut out.bytes;
    bytes[..8].copy_from_slice(b"TLDELTA1");
    bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
    bytes[10..12].copy_from_slice(&(HEADER as u16).to_le_bytes());
    put32(bytes, 12, BLOCK as u32);
    put32(bytes, 16, base.context.profile);
    put32(bytes, 20, 1);
    bytes[24..56].copy_from_slice(&base.context.id);
    put64(bytes, 56, base.info.next_source_sequence);
    put64(bytes, 64, target.info.next_source_sequence);
    put64(bytes, 72, base.info.snapshot_bytes);
    put64(bytes, 80, target.info.snapshot_bytes);
    bytes[88..120].copy_from_slice(&base.info.sha256);
    bytes[120..152].copy_from_slice(&target.info.sha256);
    put32(bytes, 184, target.envelope.len() as u32);
    put32(bytes, 188, count);
    put64(bytes, 192, size);
    let digest = packet_hash(bytes);
    bytes[152..184].copy_from_slice(&digest);
    Ok(out.bytes)
}

// The detached terminal is already owned; keep the apply result allocation-free.
#[allow(clippy::large_enum_variant)]
pub enum DeltaApply {
    Applied {
        terminal: Terminal,
        info: DeltaStateInfo,
    },
    AlreadyApplied {
        info: DeltaStateInfo,
    },
}
impl fmt::Debug for DeltaApply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Applied { info, .. } => f.debug_tuple("Applied").field(info).finish(),
            Self::AlreadyApplied { info } => f.debug_tuple("AlreadyApplied").field(info).finish(),
        }
    }
}

/// Fully validates the packet before idempotency checks. Success returns a detached terminal;
/// ownership publication and durable ACK remain the caller's installation transaction.
pub fn apply_delta_v1(
    current: &DeltaCheckpoint,
    bytes: &[u8],
    max_bytes: usize,
) -> Result<DeltaApply, DeltaError> {
    let packet = Packet::parse(bytes, max_bytes)?;
    if packet.context != current.context {
        return Err(DeltaError::NeedsFullSnapshot);
    }
    if packet.target == current.info {
        packet.verify_target(current)?;
        return Ok(DeltaApply::AlreadyApplied { info: current.info });
    }
    if packet.base != current.info {
        return Err(DeltaError::NeedsFullSnapshot);
    }
    let objects = packet.reconstruct(current)?;
    let target = DeltaCheckpoint {
        context: current.context,
        info: packet.target,
        envelope: copy(packet.envelope)?,
        objects,
    };
    validate_resources(current, &target)?;
    let snapshot = target.snapshot_bytes()?;
    if hash(&snapshot) != target.info.sha256 {
        return Err(DeltaError::Malformed);
    }
    // Reuse the complete native profile/reference validation before allocating decoded pixels.
    let view = snapshot::delta_view(&snapshot)?;
    current.context.validate_width(view.width_profile)?;
    current.context.validate_output(view.output_profile)?;
    if view.sequence != target.info.next_source_sequence {
        return Err(DeltaError::Malformed);
    }
    let terminal = Terminal::import_snapshot_v2(&snapshot)?;
    Ok(DeltaApply::Applied {
        terminal,
        info: target.info,
    })
}

struct Patch<'a> {
    offset: usize,
    bytes: &'a [u8],
}
struct Operation<'a> {
    key: Key,
    action: u8,
    len: usize,
    patches: Vec<Patch<'a>>,
}
struct Packet<'a> {
    context: DeltaContext,
    base: DeltaStateInfo,
    target: DeltaStateInfo,
    envelope: &'a [u8],
    operations: Vec<Operation<'a>>,
}
impl<'a> Packet<'a> {
    fn parse(bytes: &'a [u8], limit: usize) -> Result<Self, DeltaError> {
        if bytes.len() > limit.min(MAX_STATE_DELTA_BYTES) {
            return Err(DeltaError::NeedsFullSnapshot);
        }
        if bytes.len() < HEADER {
            return Err(DeltaError::Malformed);
        }
        if &bytes[..12] != b"TLDELTA1\x01\x00\x00\x01"
            || read32(bytes, 12)? != BLOCK as u32
            || read32(bytes, 20)? != 1
        {
            return Err(DeltaError::Incompatible);
        }
        if read64(bytes, 192)? != bytes.len() as u64
            || bytes[200..HEADER].iter().any(|&b| b != 0)
            || packet_hash(bytes).as_slice() != &bytes[152..184]
        {
            return Err(DeltaError::Malformed);
        }
        let context = DeltaContext {
            id: bytes[24..56]
                .try_into()
                .map_err(|_| DeltaError::Malformed)?,
            profile: read32(bytes, 16)?,
        };
        context.validate()?;
        let base = DeltaStateInfo {
            next_source_sequence: read64(bytes, 56)?,
            snapshot_bytes: read64(bytes, 72)?,
            sha256: bytes[88..120]
                .try_into()
                .map_err(|_| DeltaError::Malformed)?,
        };
        let target = DeltaStateInfo {
            next_source_sequence: read64(bytes, 64)?,
            snapshot_bytes: read64(bytes, 80)?,
            sha256: bytes[120..152]
                .try_into()
                .map_err(|_| DeltaError::Malformed)?,
        };
        if base.next_source_sequence >= target.next_source_sequence
            || base.snapshot_bytes < SNAP_HEADER as u64
            || base.snapshot_bytes > MAX_STATE_DELTA_BYTES as u64
            || target.snapshot_bytes > MAX_STATE_DELTA_BYTES as u64
            || target.snapshot_bytes <= bytes.len() as u64
        {
            return Err(DeltaError::Malformed);
        }
        let envelope_len = read32(bytes, 184)? as usize;
        if !(SNAP_HEADER..=MAX_ENVELOPE).contains(&envelope_len) {
            return Err(DeltaError::Malformed);
        }
        let mut at = add(HEADER, envelope_len)?;
        let envelope = bytes.get(HEADER..at).ok_or(DeltaError::Malformed)?;
        let descriptors = directory(envelope, target.snapshot_bytes)?;
        let count = read32(bytes, 188)? as usize;
        if count == 0 || count > MAX_OPERATIONS || count > bytes.len().saturating_sub(at) / 32 {
            return Err(DeltaError::Malformed);
        }
        let mut operations: Vec<Operation<'a>> = reserved(count)?;
        for _ in 0..count {
            let record = take(bytes, &mut at, 32)?;
            let key = Key {
                tag: read32(record, 0)?,
                kind: record[4],
                id: read64(record, 8)?,
            };
            let action = record[5];
            let len = usize_from(read64(record, 16)?)?;
            let patches = read32(record, 24)? as usize;
            if TAGS.binary_search(&key.tag).is_err()
                || action > DELETE
                || record[6..8] != [0; 2]
                || read32(record, 28)? != 0
                || operations.last().is_some_and(|o| o.key >= key)
                || (key.tag != GRAPHICS && (key.kind != 0 || key.id != 0))
                || (key.tag == GRAPHICS
                    && (!(1..=4).contains(&key.kind) || (key.kind == RESOURCE) != (key.id != 0)))
                || (key.kind == RESOURCE && action == PATCH)
                || len > MAX_STATE_DELTA_BYTES
                || (action == DELETE) != (len == 0)
                || (matches!(action, KEEP | DELETE) && patches != 0)
                || patches > bytes.len().saturating_sub(at) / 16
            {
                return Err(DeltaError::Malformed);
            }
            if action != DELETE {
                let desc = descriptors
                    .iter()
                    .find(|d| d.tag == key.tag)
                    .ok_or(DeltaError::Malformed)?;
                if len > desc.len
                    || (key.kind == 0 && len != desc.len)
                    || (key.kind == 1 && len != 64)
                    || (key.kind == 2 && len != 48)
                {
                    return Err(DeltaError::Malformed);
                }
            }
            let chunk = if key.kind == RESOURCE {
                len.max(1)
            } else {
                BLOCK
            };
            if patches > len.div_ceil(chunk)
                || (action == ADD && patches != len.div_ceil(chunk))
                || (action == PATCH && patches == 0 && len == 0)
            {
                return Err(DeltaError::Malformed);
            }
            let mut changes: Vec<Patch<'a>> = reserved(patches)?;
            for index in 0..patches {
                let patch = take(bytes, &mut at, 16)?;
                let offset = usize_from(read64(patch, 0)?)?;
                let length = read32(patch, 8)? as usize;
                let end = add(offset, length)?;
                if read32(patch, 12)? != 0
                    || offset >= len
                    || offset % chunk != 0
                    || length != chunk.min(len - offset)
                    || end > len
                    || changes.last().is_some_and(|p| p.offset >= offset)
                    || (action == ADD && offset != mul(index, chunk)?)
                {
                    return Err(DeltaError::Malformed);
                }
                changes.push(Patch {
                    offset,
                    bytes: take(bytes, &mut at, length)?,
                });
            }
            operations.push(Operation {
                key,
                action,
                len,
                patches: changes,
            });
        }
        if at != bytes.len() {
            return Err(DeltaError::Malformed);
        }
        // Every target section and graphics subobject is explicit and totals its declared length.
        for desc in descriptors {
            let ops = operations
                .iter()
                .filter(|o| o.key.tag == desc.tag && o.action != DELETE);
            let mut size = 0;
            let mut kinds = 0u8;
            let mut resources = 0;
            for op in ops {
                size = add(size, op.len)?;
                kinds |= 1 << op.key.kind;
                if op.key.kind == RESOURCE {
                    resources += 1;
                }
            }
            if size != desc.len
                || (desc.tag != GRAPHICS && kinds != 1)
                || (desc.tag == GRAPHICS && kinds & !8 != 0b10110)
                || resources > crate::graphics_resources::MAX_IMAGE_RESOURCES
            {
                return Err(DeltaError::Malformed);
            }
        }
        Ok(Self {
            context,
            base,
            target,
            envelope,
            operations,
        })
    }
    fn reconstruct(&self, base: &DeltaCheckpoint) -> Result<Vec<Object>, DeltaError> {
        for object in &base.objects {
            if self
                .operations
                .binary_search_by_key(&object.key, |op| op.key)
                .is_err()
            {
                return Err(DeltaError::Malformed);
            }
        }
        let mut objects = reserved(self.operations.len())?;
        for op in &self.operations {
            let old = base.object(op.key);
            if (op.action == ADD) != old.is_none() {
                return Err(DeltaError::Malformed);
            }
            if op.action == DELETE {
                continue;
            }
            if op.action == KEEP && old.is_some_and(|o| o.len != op.len) {
                return Err(DeltaError::Malformed);
            }
            let chunk = if op.key.kind == RESOURCE {
                op.len
            } else {
                BLOCK
            };
            let mut blocks = reserved(op.len.div_ceil(chunk))?;
            let mut change = op.patches.iter().peekable();
            for index in 0..op.len.div_ceil(chunk) {
                let offset = mul(index, chunk)?;
                let length = chunk.min(op.len - offset);
                if change.peek().is_some_and(|patch| patch.offset == offset) {
                    blocks.push(Arc::new(copy(
                        change.next().ok_or(DeltaError::Malformed)?.bytes,
                    )?));
                } else {
                    let block = old
                        .and_then(|old| old.blocks.get(index))
                        .filter(|b| b.len() == length)
                        .ok_or(DeltaError::Malformed)?;
                    blocks.push(Arc::clone(block));
                }
            }
            objects.push(Object {
                key: op.key,
                len: op.len,
                blocks,
            });
        }
        Ok(objects)
    }
    fn verify_target(&self, current: &DeltaCheckpoint) -> Result<(), DeltaError> {
        if self.envelope != current.envelope {
            return Err(DeltaError::Malformed);
        }
        let mut seen = 0;
        for op in &self.operations {
            let object = current.object(op.key);
            if op.action == DELETE {
                if object.is_some() {
                    return Err(DeltaError::Malformed);
                }
                continue;
            }
            let object = object
                .filter(|o| o.len == op.len)
                .ok_or(DeltaError::Malformed)?;
            for patch in &op.patches {
                let block = object
                    .blocks
                    .get(patch.offset / object.chunk_size())
                    .ok_or(DeltaError::Malformed)?;
                if block.as_slice() != patch.bytes {
                    return Err(DeltaError::Malformed);
                }
            }
            seen += 1;
        }
        if seen != current.objects.len() {
            return Err(DeltaError::Malformed);
        }
        Ok(())
    }
}

struct Descriptor {
    tag: u32,
    offset: usize,
    len: usize,
}
fn directory(envelope: &[u8], total: u64) -> Result<Vec<Descriptor>, DeltaError> {
    if envelope.len() < SNAP_HEADER
        || envelope.len() > MAX_ENVELOPE
        || &envelope[..18] != b"TLSNP2S\0\x02\0\x16\0\x16\0\x68\0\x20\0"
        || read64(envelope, 24)? != total
        || read64(envelope, 32)? != SNAP_HEADER as u64
        || !matches!(read32(envelope, 48)?, 1 | 2)
        || envelope[18..24] != [0; 6]
        || read32(envelope, 100)? != crate::crc32c::checksum(&envelope[..100])
    {
        return Err(DeltaError::Incompatible);
    }
    let count = read32(envelope, 40)? as usize;
    if !(13..=TAGS.len()).contains(&count) || envelope.len() != add(SNAP_HEADER, mul(count, 32)?)? {
        return Err(DeltaError::Incompatible);
    }
    let mut result = reserved(count)?;
    let mut end = envelope.len();
    let mut previous = 0;
    for index in 0..count {
        let record = &envelope[SNAP_HEADER + index * 32..SNAP_HEADER + (index + 1) * 32];
        let tag = read32(record, 0)?;
        let offset = usize_from(read64(record, 8)?)?;
        let len = usize_from(read64(record, 16)?)?;
        if tag <= previous
            || TAGS.binary_search(&tag).is_err()
            || record[4..6] != [1, 0]
            || record[6..8] == [0, 0]
            || read32(record, 28)? != 0
            || len == 0
            || offset != end
        {
            return Err(DeltaError::Malformed);
        }
        end = add(offset, len)?
            .checked_next_multiple_of(8)
            .ok_or(DeltaError::Malformed)?;
        if end as u64 > total {
            return Err(DeltaError::Malformed);
        }
        result.push(Descriptor { tag, offset, len });
        previous = tag;
    }
    if end as u64 != total {
        return Err(DeltaError::Malformed);
    }
    Ok(result)
}

fn assemble(envelope: &[u8], objects: &[Object], total: u64) -> Result<Vec<u8>, DeltaError> {
    let descriptors = directory(envelope, total)?;
    let length = usize_from(total)?;
    if length > MAX_STATE_DELTA_BYTES {
        return Err(DeltaError::LimitExceeded);
    }
    let mut bytes = reserved(length)?;
    bytes.resize(length, 0);
    bytes[..envelope.len()].copy_from_slice(envelope);
    let mut index = 0;
    for desc in descriptors {
        let mut at = desc.offset;
        while let Some(object) = objects.get(index).filter(|o| o.key.tag == desc.tag) {
            let end = add(at, object.len)?;
            if end > add(desc.offset, desc.len)? {
                return Err(DeltaError::Malformed);
            }
            object.write(bytes.get_mut(at..end).ok_or(DeltaError::Malformed)?)?;
            at = end;
            index += 1;
        }
        if at != add(desc.offset, desc.len)? {
            return Err(DeltaError::Malformed);
        }
    }
    if index != objects.len() {
        return Err(DeltaError::Malformed);
    }
    Ok(bytes)
}

struct Writer {
    bytes: Vec<u8>,
    limit: usize,
}
impl Writer {
    fn room(&mut self, len: usize) -> Result<(), DeltaError> {
        if add(self.bytes.len(), len)? > self.limit {
            return Err(DeltaError::NeedsFullSnapshot);
        }
        self.bytes
            .try_reserve_exact(len)
            .map_err(|_| DeltaError::LimitExceeded)
    }
    fn append(&mut self, bytes: &[u8]) -> Result<(), DeltaError> {
        self.room(bytes.len())?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
    fn zeros(&mut self, len: usize) -> Result<(), DeltaError> {
        self.room(len)?;
        self.bytes.resize(self.bytes.len() + len, 0);
        Ok(())
    }
}
pub(crate) fn reserved<T>(count: usize) -> Result<Vec<T>, DeltaError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| DeltaError::LimitExceeded)?;
    Ok(result)
}
fn copy(bytes: &[u8]) -> Result<Vec<u8>, DeltaError> {
    let mut result = reserved(bytes.len())?;
    result.extend_from_slice(bytes);
    Ok(result)
}
pub(crate) fn add(a: usize, b: usize) -> Result<usize, DeltaError> {
    a.checked_add(b).ok_or(DeltaError::LimitExceeded)
}
pub(crate) fn mul(a: usize, b: usize) -> Result<usize, DeltaError> {
    a.checked_mul(b).ok_or(DeltaError::LimitExceeded)
}
fn usize_from(value: u64) -> Result<usize, DeltaError> {
    usize::try_from(value).map_err(|_| DeltaError::Malformed)
}
fn take<'a>(bytes: &'a [u8], at: &mut usize, len: usize) -> Result<&'a [u8], DeltaError> {
    let end = add(*at, len)?;
    let result = bytes.get(*at..end).ok_or(DeltaError::Malformed)?;
    *at = end;
    Ok(result)
}
fn read32(bytes: &[u8], at: usize) -> Result<u32, DeltaError> {
    Ok(u32::from_le_bytes(
        bytes
            .get(at..add(at, 4)?)
            .ok_or(DeltaError::Malformed)?
            .try_into()
            .map_err(|_| DeltaError::Malformed)?,
    ))
}
fn read64(bytes: &[u8], at: usize) -> Result<u64, DeltaError> {
    Ok(u64::from_le_bytes(
        bytes
            .get(at..add(at, 8)?)
            .ok_or(DeltaError::Malformed)?
            .try_into()
            .map_err(|_| DeltaError::Malformed)?,
    ))
}
fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn put64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
pub(crate) fn hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn packet_hash(bytes: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(DOMAIN);
    digest.update(&bytes[..152]);
    digest.update([0; 32]);
    digest.update(&bytes[184..]);
    digest.finalize().into()
}

#[cfg(test)]
pub(crate) fn reseal_for_test(bytes: &mut [u8]) {
    let digest = packet_hash(bytes);
    bytes[152..184].copy_from_slice(&digest);
}
