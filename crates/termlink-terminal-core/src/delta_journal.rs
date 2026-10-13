//! Bounded retention of explicit legal checkpoints with shared immutable blocks/resources.
use crate::delta::{add, mul, reserved};
use crate::{DeltaCheckpoint, DeltaContext, DeltaError, DeltaStateInfo, Terminal, export_delta_v1};
use std::{collections::HashSet, sync::Arc};

const MAX_ENTRIES: usize = 256;
const MAX_BYTES: usize = 256 * 1024 * 1024;
const MAX_AGE_MILLIS: u64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeltaJournalConfig {
    pub max_entries: usize,
    /// Includes unique referenced payload capacities, Arc allocations, and checkpoint indexes.
    pub max_bytes: usize,
    /// Host-supplied monotonic time; maximum seven days. Idle expiry occurs on info/export/capture.
    pub max_age_millis: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeltaJournalInfo {
    pub entries: usize,
    pub retained_bytes: usize,
    pub oldest: Option<DeltaStateInfo>,
    pub newest: Option<DeltaStateInfo>,
}
struct Entry {
    captured_at: u64,
    checkpoint: DeltaCheckpoint,
}

pub struct DeltaJournal {
    context: DeltaContext,
    lineage: u64,
    config: DeltaJournalConfig,
    entries: Vec<Entry>,
    last_time: u64,
    retained_bytes: usize,
}
impl DeltaJournal {
    /// `lineage` identifies a local terminal ownership branch. New imports get a new lineage;
    /// it is never a wire identity or a substitute for the authenticated context.
    pub fn new(
        context: DeltaContext,
        lineage: u64,
        config: DeltaJournalConfig,
    ) -> Result<Self, DeltaError> {
        if context.validate().is_err() || lineage == 0 {
            return Err(DeltaError::Incompatible);
        }
        if config.max_entries == 0
            || config.max_entries > MAX_ENTRIES
            || config.max_bytes == 0
            || config.max_bytes > MAX_BYTES
            || config.max_age_millis == 0
            || config.max_age_millis > MAX_AGE_MILLIS
        {
            return Err(DeltaError::LimitExceeded);
        }
        let entries = reserved(config.max_entries)?;
        let retained_bytes = journal_overhead(entries.capacity())?;
        if retained_bytes > config.max_bytes {
            return Err(DeltaError::LimitExceeded);
        }
        Ok(Self {
            context,
            lineage,
            config,
            entries,
            last_time: 0,
            retained_bytes,
        })
    }
    pub fn capture(
        &mut self,
        terminal: &Terminal,
        lineage: u64,
        now_millis: u64,
    ) -> Result<DeltaStateInfo, DeltaError> {
        self.check_source(lineage, now_millis)?;
        // shortcut: snapshot export/hash still scans full state; a streaming encoder can reduce
        // this single temporary allocation after measured capture CPU/peak-memory demand.
        let bytes = terminal.export_snapshot_v2()?;
        self.capture_snapshot(&bytes, lineage, now_millis)
    }
    /// Failure leaves retention, time, and all previous checkpoints unchanged.
    pub fn capture_snapshot(
        &mut self,
        bytes: &[u8],
        lineage: u64,
        now_millis: u64,
    ) -> Result<DeltaStateInfo, DeltaError> {
        self.check_source(lineage, now_millis)?;
        let previous = self.entries.last().map(|e| &e.checkpoint);
        let checkpoint = DeltaCheckpoint::capture_shared(self.context, bytes, previous)?;
        if let Some(previous) = previous
            && checkpoint.info.next_source_sequence <= previous.info.next_source_sequence
        {
            if checkpoint.info == previous.info {
                let info = previous.info;
                let last = self.entries.len() - 1;
                let skip = self.entries[..last]
                    .iter()
                    .take_while(|entry| {
                        now_millis.saturating_sub(entry.captured_at) >= self.config.max_age_millis
                    })
                    .count();
                let retained_bytes = retained_size(
                    &self.entries[skip..],
                    None,
                    journal_overhead(self.entries.capacity())?,
                    self.config.max_bytes,
                )?
                .ok_or(DeltaError::LimitExceeded)?;
                self.entries.drain(..skip);
                self.entries.last_mut().unwrap().captured_at = now_millis;
                self.last_time = now_millis;
                self.retained_bytes = retained_bytes;
                return Ok(info);
            }
            return Err(DeltaError::NeedsFullSnapshot);
        }
        let info = checkpoint.info;
        let incoming = Entry {
            captured_at: now_millis,
            checkpoint,
        };
        let mut skip = self.expired_count(now_millis);
        skip = skip.max(
            self.entries
                .len()
                .saturating_add(1)
                .saturating_sub(self.config.max_entries),
        );
        let overhead = journal_overhead(self.entries.capacity())?;
        let retained_bytes = loop {
            match retained_size(
                &self.entries[skip..],
                Some(&incoming),
                overhead,
                self.config.max_bytes,
            )? {
                Some(bytes) => break bytes,
                None if skip < self.entries.len() => skip += 1,
                None => return Err(DeltaError::NeedsFullSnapshot),
            }
        };
        // All fallible validation, allocation and budget accounting is complete before mutation.
        self.entries.drain(..skip);
        self.entries.push(incoming);
        self.last_time = now_millis;
        self.retained_bytes = retained_bytes;
        Ok(info)
    }
    pub fn info(&mut self, now_millis: u64) -> Result<DeltaJournalInfo, DeltaError> {
        self.expire(now_millis)?;
        Ok(DeltaJournalInfo {
            entries: self.entries.len(),
            retained_bytes: self.retained_bytes,
            oldest: self.entries.first().map(|e| e.checkpoint.info),
            newest: self.entries.last().map(|e| e.checkpoint.info),
        })
    }
    /// Only actual retained (sequence, canonical hash, length) points are addressable.
    pub fn export(
        &mut self,
        base: DeltaStateInfo,
        target: DeltaStateInfo,
        now_millis: u64,
        max_bytes: usize,
    ) -> Result<Vec<u8>, DeltaError> {
        self.expire(now_millis)?;
        let find = |info| {
            self.entries
                .iter()
                .find(|e| e.checkpoint.info == info)
                .map(|e| &e.checkpoint)
                .ok_or(DeltaError::NeedsFullSnapshot)
        };
        export_delta_v1(find(base)?, find(target)?, max_bytes)
    }
    fn check_source(&self, lineage: u64, now: u64) -> Result<(), DeltaError> {
        if lineage != self.lineage || now < self.last_time {
            return Err(DeltaError::NeedsFullSnapshot);
        }
        Ok(())
    }
    fn expired_count(&self, now: u64) -> usize {
        self.entries
            .iter()
            .take_while(|e| now.saturating_sub(e.captured_at) >= self.config.max_age_millis)
            .count()
    }
    fn expire(&mut self, now: u64) -> Result<(), DeltaError> {
        self.check_source(self.lineage, now)?;
        let count = self.expired_count(now);
        if count != 0 {
            let retained_bytes = retained_size(
                &self.entries[count..],
                None,
                journal_overhead(self.entries.capacity())?,
                self.config.max_bytes,
            )?
            .ok_or(DeltaError::LimitExceeded)?;
            self.entries.drain(..count);
            self.retained_bytes = retained_bytes;
        }
        self.last_time = now;
        Ok(())
    }
}

fn journal_overhead(capacity: usize) -> Result<usize, DeltaError> {
    add(
        std::mem::size_of::<DeltaJournal>(),
        mul(capacity, std::mem::size_of::<Entry>())?,
    )
}
fn retained_size(
    entries: &[Entry],
    incoming: Option<&Entry>,
    overhead: usize,
    limit: usize,
) -> Result<Option<usize>, DeltaError> {
    let points = || entries.iter().chain(incoming);
    let mut size = overhead;
    let mut references = 0;
    for entry in points() {
        // Entry storage already accounts for the inline checkpoint itself.
        size = add(
            size,
            entry
                .checkpoint
                .metadata_bytes()?
                .saturating_sub(std::mem::size_of::<DeltaCheckpoint>()),
        )?;
        if size > limit {
            return Ok(None);
        }
        for object in &entry.checkpoint.objects {
            references = add(references, object.blocks.len())?;
        }
    }
    let mut seen = HashSet::new();
    seen.try_reserve(references)
        .map_err(|_| DeltaError::LimitExceeded)?;
    for entry in points() {
        for object in &entry.checkpoint.objects {
            for block in &object.blocks {
                if seen.insert(Arc::as_ptr(block)) {
                    size = add(
                        size,
                        add(
                            block.capacity(),
                            std::mem::size_of::<Vec<u8>>() + 2 * std::mem::size_of::<usize>(),
                        )?,
                    )?;
                    if size > limit {
                        return Ok(None);
                    }
                }
            }
        }
    }
    Ok(Some(size))
}
