//! Immutable pixels shared by transactions, snapshots and frozen history.
use crate::{TerminalError, crc32c};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicUsize, Ordering},
};

pub const MAX_IMAGE_DIMENSION: u32 = 4096;
pub const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_SESSION_IMAGE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_PROCESS_IMAGE_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_IMAGE_RESOURCES: usize = 256;
pub const MAX_GRAPHICS_RESOURCE_FRAME_BYTES: usize =
    48 + MAX_SESSION_IMAGE_BYTES + MAX_IMAGE_RESOURCES * 35;

#[cfg(test)]
std::thread_local! {
    static IMAGE_ALLOCATION_ATTEMPTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
#[cfg(test)]
pub(crate) fn image_allocation_attempts() -> usize {
    IMAGE_ALLOCATION_ATTEMPTS.with(std::cell::Cell::get)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ImageFormat {
    Rgba8888 = 1,
    Alpha8 = 2,
}

impl ImageFormat {
    pub(crate) fn byte_count(self, width: u32, height: u32) -> Result<usize, TerminalError> {
        if width == 0 || height == 0 || width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION
        {
            return Err(TerminalError::ProtocolResourceLimit(
                "image dimensions must be 1..4096",
            ));
        }
        let bytes = (width as usize)
            .checked_mul(height as usize)
            .and_then(|size| size.checked_mul(if self == Self::Rgba8888 { 4 } else { 1 }))
            .filter(|&size| size <= MAX_IMAGE_BYTES)
            .ok_or(TerminalError::ProtocolResourceLimit(
                "image pixels exceed 16 MiB",
            ))?;
        Ok(bytes)
    }
}

#[derive(Debug)]
struct ProcessBudget {
    used: AtomicUsize,
    limit: usize,
}
impl ProcessBudget {
    fn global() -> Arc<Self> {
        static BUDGET: OnceLock<Arc<ProcessBudget>> = OnceLock::new();
        BUDGET
            .get_or_init(|| {
                Arc::new(Self {
                    used: AtomicUsize::new(0),
                    limit: MAX_PROCESS_IMAGE_BYTES,
                })
            })
            .clone()
    }
    fn reserve(self: &Arc<Self>, bytes: usize) -> Result<Reservation, TerminalError> {
        self.used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|&total| total <= self.limit)
            })
            .map_err(|_| {
                TerminalError::ProtocolResourceLimit("process image memory budget exhausted")
            })?;
        Ok(Reservation {
            budget: self.clone(),
            bytes,
        })
    }
}
#[derive(Debug)]
struct Reservation {
    budget: Arc<ProcessBudget>,
    bytes: usize,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

/// Decoder scratch shares the pixel budget; temporary compressed and inflated copies count too.
#[derive(Debug)]
pub(crate) struct GraphicsScratch {
    bytes: Vec<u8>,
    _reservation: Reservation,
}
impl GraphicsScratch {
    pub(crate) fn allocate(length: usize, limit: usize) -> Result<Self, TerminalError> {
        Self::allocate_with_budget(length, limit, &ProcessBudget::global())
    }
    fn allocate_with_budget(
        length: usize,
        limit: usize,
        budget: &Arc<ProcessBudget>,
    ) -> Result<Self, TerminalError> {
        if length > limit {
            return Err(TerminalError::ProtocolResourceLimit(
                "image decoder scratch limit exceeded",
            ));
        }
        let reservation = budget.reserve(length)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| TerminalError::LimitExceeded)?;
        bytes.resize(length, 0);
        Ok(Self {
            bytes,
            _reservation: reservation,
        })
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub(crate) fn bytes_mut(&mut self) -> &mut [u8] {
        &mut self.bytes
    }
}

/// Mutable only while decoding. Its allocation is charged before any pixels are allocated.
#[derive(Debug)]
pub struct ImageData {
    width: u32,
    height: u32,
    format: ImageFormat,
    pixels: Vec<u8>,
    _reservation: Reservation,
}
impl ImageData {
    pub fn allocate(width: u32, height: u32, format: ImageFormat) -> Result<Self, TerminalError> {
        Self::allocate_with_budget(width, height, format, &ProcessBudget::global())
    }
    fn allocate_with_budget(
        width: u32,
        height: u32,
        format: ImageFormat,
        budget: &Arc<ProcessBudget>,
    ) -> Result<Self, TerminalError> {
        #[cfg(test)]
        IMAGE_ALLOCATION_ATTEMPTS.with(|attempts| attempts.set(attempts.get() + 1));
        let bytes = format.byte_count(width, height)?;
        let reservation = budget.reserve(bytes)?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(bytes)
            .map_err(|_| TerminalError::LimitExceeded)?;
        pixels.resize(bytes, 0);
        Ok(Self {
            width,
            height,
            format,
            pixels,
            _reservation: reservation,
        })
    }
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn format(&self) -> ImageFormat {
        self.format
    }
}
impl PartialEq for ImageData {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width
            && self.height == other.height
            && self.format == other.format
            && self.pixels == other.pixels
    }
}
impl Eq for ImageData {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageResource {
    id: u64,
    image: Arc<ImageData>,
}
impl ImageResource {
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn image(&self) -> &ImageData {
        &self.image
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphicsResources {
    next_id: u64,
    pixel_bytes: usize,
    entries: Vec<ImageResource>,
}

/// Validated borrowed pixels allow enclosing frames to check references before allocating images.
pub(crate) struct ResourceFrame<'a> {
    next_id: u64,
    pixel_bytes: usize,
    entries: Vec<ResourceView<'a>>,
}
struct ResourceView<'a> {
    id: u64,
    width: u32,
    height: u32,
    format: ImageFormat,
    pixels: &'a [u8],
    encoded: &'a [u8],
}
impl ResourceFrame<'_> {
    pub(crate) fn encoded_records(&self) -> impl Iterator<Item = (u64, &[u8])> + '_ {
        self.entries.iter().map(|entry| (entry.id, entry.encoded))
    }
    pub(crate) fn metadata(&self, id: u64) -> Option<(u32, u32, ImageFormat)> {
        self.entries
            .binary_search_by_key(&id, |entry| entry.id)
            .ok()
            .map(|index| {
                let entry = &self.entries[index];
                (entry.width, entry.height, entry.format)
            })
    }
    pub(crate) fn ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.entries.iter().map(|entry| entry.id)
    }
    pub(crate) fn materialize(self) -> Result<GraphicsResources, TerminalError> {
        self.materialize_with_budget(&ProcessBudget::global())
    }
    fn materialize_with_budget(
        self,
        budget: &Arc<ProcessBudget>,
    ) -> Result<GraphicsResources, TerminalError> {
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(self.entries.len())
            .map_err(|_| TerminalError::LimitExceeded)?;
        for entry in self.entries {
            let mut image =
                ImageData::allocate_with_budget(entry.width, entry.height, entry.format, budget)?;
            image.pixels.copy_from_slice(entry.pixels);
            entries.push(ImageResource {
                id: entry.id,
                image: Arc::new(image),
            });
        }
        Ok(GraphicsResources {
            next_id: self.next_id,
            pixel_bytes: self.pixel_bytes,
            entries,
        })
    }
}
impl Default for GraphicsResources {
    fn default() -> Self {
        Self {
            next_id: 1,
            pixel_bytes: 0,
            entries: Vec::new(),
        }
    }
}
impl GraphicsResources {
    pub fn entries(&self) -> &[ImageResource] {
        &self.entries
    }
    pub fn pixel_bytes(&self) -> usize {
        self.pixel_bytes
    }
    pub fn get(&self, id: u64) -> Option<&ImageResource> {
        self.entries
            .binary_search_by_key(&id, |entry| entry.id)
            .ok()
            .map(|index| &self.entries[index])
    }
    pub fn insert(&mut self, image: ImageData) -> Result<u64, TerminalError> {
        let total = self
            .pixel_bytes
            .checked_add(image.pixels.len())
            .filter(|&total| {
                total <= MAX_SESSION_IMAGE_BYTES && self.entries.len() < MAX_IMAGE_RESOURCES
            })
            .ok_or(TerminalError::ProtocolResourceLimit(
                "session image resource budget exhausted",
            ))?;
        let next = self
            .next_id
            .checked_add(1)
            .ok_or(TerminalError::ProtocolResourceLimit(
                "image resource IDs exhausted",
            ))?;
        self.entries
            .try_reserve(1)
            .map_err(|_| TerminalError::LimitExceeded)?;
        let id = self.next_id;
        self.entries.push(ImageResource {
            id,
            image: Arc::new(image),
        });
        self.next_id = next;
        self.pixel_bytes = total;
        Ok(id)
    }
    pub fn remove(&mut self, id: u64) -> bool {
        let Ok(index) = self.entries.binary_search_by_key(&id, |entry| entry.id) else {
            return false;
        };
        self.pixel_bytes -= self.entries.remove(index).image.pixels.len();
        true
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.pixel_bytes = 0;
    }

    /// Caller supplies the union of current cells, placements and font references. Frozen captures
    /// own their Arc clones independently. A dangling live reference aborts collection atomically.
    pub fn retain_referenced(
        &mut self,
        ids: impl IntoIterator<Item = u64>,
    ) -> Result<(), TerminalError> {
        let mut keep = [false; MAX_IMAGE_RESOURCES];
        for id in ids {
            let index = self
                .entries
                .binary_search_by_key(&id, |entry| entry.id)
                .map_err(|_| TerminalError::MalformedSnapshot)?;
            keep[index] = true;
        }
        let mut index = 0;
        self.entries.retain(|_| {
            let retained = keep[index];
            index += 1;
            retained
        });
        self.pixel_bytes = self
            .entries
            .iter()
            .map(|entry| entry.image.pixels.len())
            .sum();
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, TerminalError> {
        let size = 48
            + self
                .entries
                .iter()
                .map(|entry| 32 + entry.image.pixels.len().next_multiple_of(4))
                .sum::<usize>();
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|_| TerminalError::LimitExceeded)?;
        bytes.resize(48, 0);
        bytes[..8].copy_from_slice(b"TLGRS1\0\0");
        bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
        bytes[10..12].copy_from_slice(&48u16.to_le_bytes());
        bytes[12..16].copy_from_slice(&(size as u32).to_le_bytes());
        bytes[16..24].copy_from_slice(&self.next_id.to_le_bytes());
        bytes[24..28].copy_from_slice(&(self.entries.len() as u32).to_le_bytes());
        bytes[32..40].copy_from_slice(&(self.pixel_bytes as u64).to_le_bytes());
        for entry in &self.entries {
            let image = &entry.image;
            bytes.extend_from_slice(&entry.id.to_le_bytes());
            bytes.extend_from_slice(&image.width.to_le_bytes());
            bytes.extend_from_slice(&image.height.to_le_bytes());
            bytes.extend_from_slice(&[image.format as u8, 0, 0, 0]);
            bytes.extend_from_slice(&(image.pixels.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&crc32c::checksum(&image.pixels).to_le_bytes());
            bytes.extend_from_slice(&[0; 4]);
            bytes.extend_from_slice(&image.pixels);
            bytes.resize(bytes.len().next_multiple_of(4), 0);
        }
        let crc = crc32c::checksum_parts(&[&bytes[..40], &bytes[44..]]);
        bytes[40..44].copy_from_slice(&crc.to_le_bytes());
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, TerminalError> {
        Self::decode_with_budget(bytes, &ProcessBudget::global())
    }
    fn decode_with_budget(
        bytes: &[u8],
        budget: &Arc<ProcessBudget>,
    ) -> Result<Self, TerminalError> {
        Self::inspect(bytes)?.materialize_with_budget(budget)
    }

    pub(crate) fn inspect(bytes: &[u8]) -> Result<ResourceFrame<'_>, TerminalError> {
        let malformed = TerminalError::MalformedSnapshot;
        if bytes.len() < 48
            || bytes.len() > MAX_GRAPHICS_RESOURCE_FRAME_BYTES
            || &bytes[..12] != b"TLGRS1\0\0\x01\0\x30\0"
        {
            return Err(malformed);
        }
        let u32_at = |offset| {
            u32::from_le_bytes(
                bytes[offset..offset + 4]
                    .try_into()
                    .expect("checked resource header"),
            )
        };
        let u64_at = |offset| {
            u64::from_le_bytes(
                bytes[offset..offset + 8]
                    .try_into()
                    .expect("checked resource header"),
            )
        };
        let next_id = u64_at(16);
        let count = u32_at(24) as usize;
        let declared_bytes = u64_at(32);
        if u32_at(12) as usize != bytes.len()
            || next_id == 0
            || count > MAX_IMAGE_RESOURCES
            || u32_at(28) != 0
            || u32_at(44) != 0
            || declared_bytes > MAX_SESSION_IMAGE_BYTES as u64
            || u32_at(40) != crc32c::checksum_parts(&[&bytes[..40], &bytes[44..]])
        {
            return Err(malformed);
        }
        // Validate every directory entry before allocation, including payload CRC/padding.
        let mut offset = 48;
        let mut last_id = 0;
        let mut total = 0usize;
        let mut directory = Vec::new();
        directory
            .try_reserve_exact(count)
            .map_err(|_| TerminalError::LimitExceeded)?;
        for _ in 0..count {
            if bytes.len() - offset < 32 {
                return Err(malformed);
            }
            let id = u64_at(offset);
            let width = u32_at(offset + 8);
            let height = u32_at(offset + 12);
            let format = match bytes[offset + 16] {
                1 => ImageFormat::Rgba8888,
                2 => ImageFormat::Alpha8,
                _ => return Err(malformed),
            };
            let length = u32_at(offset + 20) as usize;
            if id <= last_id
                || id >= next_id
                || bytes[offset + 17..offset + 20] != [0; 3]
                || u32_at(offset + 28) != 0
                || format.byte_count(width, height).ok() != Some(length)
            {
                return Err(malformed);
            }
            let start = offset + 32;
            let end = start.checked_add(length).ok_or(malformed.clone())?;
            let padded = end.checked_next_multiple_of(4).ok_or(malformed.clone())?;
            if padded > bytes.len()
                || bytes[end..padded].iter().any(|&byte| byte != 0)
                || u32_at(offset + 24) != crc32c::checksum(&bytes[start..end])
            {
                return Err(malformed);
            }
            total = total
                .checked_add(length)
                .filter(|&value| value <= MAX_SESSION_IMAGE_BYTES)
                .ok_or(malformed.clone())?;
            directory.push(ResourceView {
                id,
                width,
                height,
                format,
                pixels: &bytes[start..end],
                encoded: &bytes[offset..padded],
            });
            last_id = id;
            offset = padded;
        }
        if offset != bytes.len() || total as u64 != declared_bytes {
            return Err(malformed);
        }
        Ok(ResourceFrame {
            next_id,
            pixel_bytes: total,
            entries: directory,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn budget(limit: usize) -> Arc<ProcessBudget> {
        Arc::new(ProcessBudget {
            used: AtomicUsize::new(0),
            limit,
        })
    }
    fn alpha(budget: &Arc<ProcessBudget>, width: u32) -> ImageData {
        ImageData::allocate_with_budget(width, 1, ImageFormat::Alpha8, budget).unwrap()
    }
    fn seal(bytes: &mut [u8]) {
        let crc = crc32c::checksum_parts(&[&bytes[..40], &bytes[44..]]);
        bytes[40..44].copy_from_slice(&crc.to_le_bytes());
    }

    #[test]
    fn frozen_pixels_survive_deletion_without_double_charging_or_id_reuse() {
        let quota = budget(32);
        let mut store = GraphicsResources::default();
        let mut image = alpha(&quota, 3);
        image.pixels_mut().copy_from_slice(&[1, 2, 3]);
        let id = store.insert(image).unwrap();
        let frozen = store.clone();
        assert_eq!(quota.used.load(Ordering::Acquire), 3);
        assert!(store.remove(id));
        assert!(!store.remove(id));
        assert_eq!(frozen.get(id).unwrap().image().pixels(), &[1, 2, 3]);
        assert_eq!(quota.used.load(Ordering::Acquire), 3);
        let second = store.insert(alpha(&quota, 2)).unwrap();
        store.clear();
        let third = store.insert(alpha(&quota, 1)).unwrap();
        assert!(id < second && second < third);
        drop(frozen);
        assert_eq!(quota.used.load(Ordering::Acquire), 1);
        drop(store);
        assert_eq!(quota.used.load(Ordering::Acquire), 0);
    }
    #[test]
    fn reference_collection_is_atomic_and_frozen_captures_keep_retired_pixels() {
        let quota = budget(64);
        let mut store = GraphicsResources::default();
        let first = store.insert(alpha(&quota, 3)).unwrap();
        let second = store.insert(alpha(&quota, 5)).unwrap();
        let frozen = store.clone();
        let before = store.encode().unwrap();
        assert!(store.retain_referenced([first, 999]).is_err());
        assert_eq!(store.encode().unwrap(), before);
        store.retain_referenced([second, second]).unwrap();
        assert_eq!(store.pixel_bytes(), 5);
        assert_eq!(quota.used.load(Ordering::Acquire), 8);
        assert!(store.get(first).is_none());
        assert_eq!(frozen.get(first).unwrap().image().pixels().len(), 3);
        drop(frozen);
        assert_eq!(quota.used.load(Ordering::Acquire), 5);
        store.retain_referenced([]).unwrap();
        assert_eq!(quota.used.load(Ordering::Acquire), 0);
        assert!(store.insert(alpha(&quota, 1)).unwrap() > second);
    }
    #[test]
    fn dimensions_budget_and_failed_decode_release_all_reservations() {
        let quota = budget(4);
        assert!(ImageData::allocate_with_budget(4097, 1, ImageFormat::Alpha8, &quota).is_err());
        assert!(
            ImageData::allocate_with_budget(4096, 4096, ImageFormat::Rgba8888, &quota).is_err()
        );
        let first = alpha(&quota, 3);
        assert!(ImageData::allocate_with_budget(2, 1, ImageFormat::Alpha8, &quota).is_err());
        drop(first);
        assert_eq!(quota.used.load(Ordering::Acquire), 0);
        let source_quota = budget(100);
        let mut store = GraphicsResources::default();
        store.insert(alpha(&source_quota, 3)).unwrap();
        store.insert(alpha(&source_quota, 2)).unwrap();
        assert!(GraphicsResources::decode_with_budget(&store.encode().unwrap(), &quota).is_err());
        assert_eq!(quota.used.load(Ordering::Acquire), 0);
    }
    #[test]
    fn scratch_and_pixels_share_quota_and_all_error_paths_release_it() {
        let quota = budget(8);
        let image = alpha(&quota, 3);
        assert!(GraphicsScratch::allocate_with_budget(6, 16, &quota).is_err());
        assert!(GraphicsScratch::allocate_with_budget(2, 1, &quota).is_err());
        assert_eq!(quota.used.load(Ordering::Acquire), 3);
        let scratch = GraphicsScratch::allocate_with_budget(5, 5, &quota).unwrap();
        assert_eq!(scratch.bytes(), &[0; 5]);
        assert!(ImageData::allocate_with_budget(1, 1, ImageFormat::Alpha8, &quota).is_err());
        drop(scratch);
        assert_eq!(quota.used.load(Ordering::Acquire), 3);
        drop(image);
        assert_eq!(quota.used.load(Ordering::Acquire), 0);
    }
    #[test]
    fn resource_codec_is_canonical_and_rejects_corruption_before_allocating() {
        let quota = budget(1024);
        let mut store = GraphicsResources::default();
        store.insert(alpha(&quota, 3)).unwrap();
        store.insert(alpha(&quota, 2)).unwrap();
        let bytes = store.encode().unwrap();
        let restored = GraphicsResources::decode_with_budget(&bytes, &quota).unwrap();
        assert_eq!(restored, store);
        assert_eq!(restored.encode().unwrap(), bytes);
        drop(restored);
        let baseline = quota.used.load(Ordering::Acquire);
        for offset in [
            0, 8, 10, 12, 16, 24, 28, 32, 40, 44, 48, 56, 60, 64, 65, 68, 72, 76, 83, 84, 116,
        ] {
            let mut bad = bytes.clone();
            bad[offset] ^= 1;
            if offset >= 44 {
                seal(&mut bad);
            }
            assert!(
                GraphicsResources::decode_with_budget(&bad, &quota).is_err(),
                "accepted bad offset {offset}"
            );
            assert_eq!(quota.used.load(Ordering::Acquire), baseline);
        }
        for end in 0..bytes.len() {
            assert!(GraphicsResources::decode_with_budget(&bytes[..end], &quota).is_err());
        }
        let mut trailing = bytes;
        trailing.push(0);
        assert!(GraphicsResources::decode(&trailing).is_err());
    }
    #[test]
    fn entry_limit_and_exhausted_ids_do_not_commit_partial_insertions() {
        let quota = budget(1024);
        let mut store = GraphicsResources::default();
        for _ in 0..MAX_IMAGE_RESOURCES {
            store.insert(alpha(&quota, 1)).unwrap();
        }
        let before = store.clone();
        assert!(store.insert(alpha(&quota, 1)).is_err());
        assert_eq!(store, before);
        assert_eq!(quota.used.load(Ordering::Acquire), MAX_IMAGE_RESOURCES);
        store.clear();
        store.next_id = u64::MAX;
        assert!(store.insert(alpha(&quota, 1)).is_err());
        assert!(store.entries.is_empty());
        assert_eq!(store.next_id, u64::MAX);
    }
}
