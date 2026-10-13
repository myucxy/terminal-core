//! Frozen data-backed supplemental and national character sets.
use crate::charset::CharsetSize;
use crate::legacy_charset_data::{DESCRIPTORS, TABLES};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ExtendedCharset(u8);

impl ExtendedCharset {
    pub(crate) fn from_id(id: u8) -> Option<Self> {
        (usize::from(id) < DESCRIPTORS.len()).then_some(Self(id))
    }
    pub(crate) fn id(self) -> u8 {
        self.0
    }
    pub(crate) fn designator(self) -> &'static str {
        DESCRIPTORS[usize::from(self.0)].0
    }
    pub(crate) fn size(self) -> CharsetSize {
        if DESCRIPTORS[usize::from(self.0)].1 {
            CharsetSize::Characters96
        } else {
            CharsetSize::Characters94
        }
    }
    pub(crate) fn decode(size: CharsetSize, prefix: &[u8], final_byte: u8) -> Option<Self> {
        (0..DESCRIPTORS.len()).find_map(|id| {
            let set = Self(id as u8);
            let designator = set.designator().as_bytes();
            (set.size() == size
                && designator.last() == Some(&final_byte)
                && &designator[..designator.len() - 1] == prefix)
                .then_some(set)
        })
    }
    pub(crate) fn map(self, byte: u8, national: bool) -> char {
        if !(0x20..=0x7f).contains(&byte) || (DESCRIPTORS[usize::from(self.0)].2 && !national) {
            return char::from(byte);
        }
        TABLES[usize::from(self.0)][usize::from(byte - 0x20)]
    }
}
