//! Bounded read-only ZIP archives held in memory (APPNOTE 6.3.x subset), for package formats
//! such as 3MF.
//!
//! - **Structure:** the central directory is authoritative. Local headers only locate entry data,
//!   so streamed entries with zero local sizes are read correctly. ZIP64 records and fields are
//!   supported. Encrypted, multi-disk and patched entries, and methods other than stored (0) and
//!   deflate (8), are rejected when they are opened.
//! - **Safety:** entry data must lie inside the file without overlapping other entries. A reader
//!   never produces more bytes than the central directory declares, fails if fewer arrive, and
//!   checks the CRC-32 at the end. Entry count and declared sizes are capped up front, so a crafted
//!   archive cannot make the reader allocate beyond its limits.

use crate::digest::Crc32;
use miniz_oxide::inflate::stream::{inflate, InflateState};
use miniz_oxide::{DataFormat, MZFlush, MZStatus};
use std::{error::Error, fmt, io};

/// Why an archive or one of its entries cannot be read.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ArchiveError {
    /// No end-of-central-directory record: not a ZIP archive, or truncated.
    NotAnArchive,
    /// A structural record is truncated, inconsistent or out of range.
    Malformed(&'static str),
    /// A feature this reader deliberately does not support.
    Unsupported(&'static str),
    /// More entries than the limit.
    TooManyEntries {
        /// Configured cap.
        limit: usize,
    },
    /// An entry declares more uncompressed bytes than the limit.
    EntryTooLarge {
        /// Configured cap in bytes.
        limit: u64,
    },
    /// Two entries have the same name, ignoring ASCII case.
    DuplicateName,
    /// Entry data does not decompress to exactly its declared size and CRC-32.
    Corrupt(&'static str),
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnArchive => f.write_str("not a ZIP archive (no end of central directory)"),
            Self::Malformed(what) => write!(f, "malformed ZIP archive: {what}"),
            Self::Unsupported(what) => write!(f, "unsupported ZIP feature: {what}"),
            Self::TooManyEntries { limit } => {
                write!(f, "ZIP archive has more than {limit} entries")
            }
            Self::EntryTooLarge { limit } => {
                write!(f, "ZIP entry declares more than {limit} uncompressed bytes")
            }
            Self::DuplicateName => f.write_str("ZIP archive has two entries with the same name"),
            Self::Corrupt(what) => write!(f, "corrupt ZIP entry: {what}"),
        }
    }
}

impl Error for ArchiveError {}

/// Archive resource caps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveLimits {
    /// Maximum entries in the central directory.
    pub max_entries: usize,
    /// Maximum declared uncompressed size of one entry.
    pub max_entry_bytes: u64,
}

/// One central-directory entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    name: String,
    method: u16,
    flags: u16,
    crc: u32,
    compressed: u64,
    size: u64,
    /// Offset of the entry's local header.
    local: u64,
}

impl Entry {
    /// Declared uncompressed size.
    #[cfg(test)]
    pub(crate) const fn size(&self) -> u64 {
        self.size
    }

    /// Declared uncompressed size, checked when the entry is read.
    pub(crate) const fn declared_size(&self) -> u64 {
        self.size
    }

    /// The entry's name.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
}

/// A parsed archive borrowing its bytes.
pub(crate) struct Archive<'a> {
    bytes: &'a [u8],
    entries: Vec<Entry>,
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16, ArchiveError> {
    bytes
        .get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or(ArchiveError::Malformed("record truncated"))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, ArchiveError> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(ArchiveError::Malformed("record truncated"))
}

fn u64_at(bytes: &[u8], at: usize) -> Result<u64, ArchiveError> {
    bytes
        .get(at..at + 8)
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
        .ok_or(ArchiveError::Malformed("record truncated"))
}

fn offset(value: u64) -> Result<usize, ArchiveError> {
    usize::try_from(value).map_err(|_| ArchiveError::Malformed("offset out of range"))
}

const EOCD: u32 = 0x0605_4b50;
const ZIP64_LOCATOR: u32 = 0x0706_4b50;
const ZIP64_EOCD: u32 = 0x0606_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const LOCAL: u32 = 0x0403_4b50;

/// Where and how many central-directory records there are.
struct Directory {
    entries: u64,
    size: u64,
    offset: u64,
}

impl<'a> Archive<'a> {
    /// Parse the central directory of `bytes`.
    pub(crate) fn parse(bytes: &'a [u8], limits: ArchiveLimits) -> Result<Self, ArchiveError> {
        let directory = Self::directory(bytes)?;
        let count = usize::try_from(directory.entries)
            .ok()
            .filter(|&n| n <= limits.max_entries)
            .ok_or(ArchiveError::TooManyEntries {
                limit: limits.max_entries,
            })?;
        let start = offset(directory.offset)?;
        let end = offset(
            directory
                .offset
                .checked_add(directory.size)
                .ok_or(ArchiveError::Malformed("central directory out of range"))?,
        )?;
        let records = bytes
            .get(start..end)
            .ok_or(ArchiveError::Malformed("central directory out of range"))?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(count)
            .map_err(|_| ArchiveError::Malformed("entry table allocation"))?;
        let mut at = 0;
        for _ in 0..count {
            let (entry, next) = Self::central_entry(records, at, limits)?;
            entries.push(entry);
            at = next;
        }
        if at != records.len() {
            return Err(ArchiveError::Malformed("central directory size mismatch"));
        }
        let archive = Self { bytes, entries };
        archive.check_names_and_ranges(start)?;
        Ok(archive)
    }

    /// Locate the (ZIP64) end of central directory, searching back over a possible comment.
    fn directory(bytes: &[u8]) -> Result<Directory, ArchiveError> {
        if bytes.len() < 22 {
            return Err(ArchiveError::NotAnArchive);
        }
        let lowest = bytes.len().saturating_sub(22 + 0xffff);
        let eocd = (lowest..=bytes.len() - 22)
            .rev()
            .find(|&at| {
                u32_at(bytes, at) == Ok(EOCD)
                    && u16_at(bytes, at + 20).is_ok_and(|n| at + 22 + usize::from(n) == bytes.len())
            })
            .ok_or(ArchiveError::NotAnArchive)?;
        let disk = u16_at(bytes, eocd + 4)?;
        let directory_disk = u16_at(bytes, eocd + 6)?;
        let on_disk = u16_at(bytes, eocd + 8)?;
        let total = u16_at(bytes, eocd + 10)?;
        let mut directory = Directory {
            entries: u64::from(total),
            size: u64::from(u32_at(bytes, eocd + 12)?),
            offset: u64::from(u32_at(bytes, eocd + 16)?),
        };
        let zip64 = eocd >= 20 && u32_at(bytes, eocd - 20) == Ok(ZIP64_LOCATOR);
        if zip64 {
            if u32_at(bytes, eocd - 16)? != 0 || u32_at(bytes, eocd - 4)? > 1 {
                return Err(ArchiveError::Unsupported("multi-disk archive"));
            }
            let record = offset(u64_at(bytes, eocd - 12)?)?;
            if u32_at(bytes, record)? != ZIP64_EOCD {
                return Err(ArchiveError::Malformed("ZIP64 end of central directory"));
            }
            if u32_at(bytes, record + 16)? != 0 || u32_at(bytes, record + 20)? != 0 {
                return Err(ArchiveError::Unsupported("multi-disk archive"));
            }
            if u64_at(bytes, record + 24)? != u64_at(bytes, record + 32)? {
                return Err(ArchiveError::Unsupported("multi-disk archive"));
            }
            directory = Directory {
                entries: u64_at(bytes, record + 32)?,
                size: u64_at(bytes, record + 40)?,
                offset: u64_at(bytes, record + 48)?,
            };
        } else if disk != 0 || directory_disk != 0 || on_disk != total {
            return Err(ArchiveError::Unsupported("multi-disk archive"));
        }
        Ok(directory)
    }

    /// Parse the central record at `at`; returns it and the next record's offset.
    fn central_entry(
        records: &[u8],
        at: usize,
        limits: ArchiveLimits,
    ) -> Result<(Entry, usize), ArchiveError> {
        if u32_at(records, at)? != CENTRAL {
            return Err(ArchiveError::Malformed(
                "central directory record signature",
            ));
        }
        let flags = u16_at(records, at + 8)?;
        let method = u16_at(records, at + 10)?;
        let crc = u32_at(records, at + 16)?;
        let mut compressed = u64::from(u32_at(records, at + 20)?);
        let mut size = u64::from(u32_at(records, at + 24)?);
        let name_length = usize::from(u16_at(records, at + 28)?);
        let extra_length = usize::from(u16_at(records, at + 30)?);
        let comment_length = usize::from(u16_at(records, at + 32)?);
        let disk = u16_at(records, at + 34)?;
        let mut local = u64::from(u32_at(records, at + 42)?);
        let name_start = at + 46;
        let extra_start = name_start + name_length;
        let next = extra_start + extra_length + comment_length;
        if next > records.len() {
            return Err(ArchiveError::Malformed(
                "central directory record truncated",
            ));
        }
        // ZIP64 extended information: only the saturated fields are present, in this order.
        let mut extra = &records[extra_start..extra_start + extra_length];
        let mut disk = u32::from(disk);
        while extra.len() >= 4 {
            let id = u16_at(extra, 0)?;
            let length = usize::from(u16_at(extra, 2)?);
            let data = extra
                .get(4..4 + length)
                .ok_or(ArchiveError::Malformed("extra field truncated"))?;
            if id == 0x0001 {
                let mut field = 0;
                for value in [&mut size, &mut compressed, &mut local] {
                    if *value == u64::from(u32::MAX) {
                        *value = u64_at(data, field)?;
                        field += 8;
                    }
                }
                if disk == u32::from(u16::MAX) {
                    disk = u32_at(data, field)?;
                }
            }
            extra = &extra[4 + length..];
        }
        if disk != 0 {
            return Err(ArchiveError::Unsupported("multi-disk archive"));
        }
        if size > limits.max_entry_bytes {
            return Err(ArchiveError::EntryTooLarge {
                limit: limits.max_entry_bytes,
            });
        }
        let name = std::str::from_utf8(&records[name_start..extra_start])
            .map_err(|_| ArchiveError::Unsupported("entry name that is not UTF-8"))?
            .to_owned();
        Ok((
            Entry {
                name,
                method,
                flags,
                crc,
                compressed,
                size,
                local,
            },
            next,
        ))
    }

    /// Reject duplicate names, and entry data outside the file, overlapping another entry or the
    /// central directory.
    fn check_names_and_ranges(&self, directory: usize) -> Result<(), ArchiveError> {
        let mut names: Vec<String> = self
            .entries
            .iter()
            .map(|e| e.name.to_ascii_lowercase())
            .collect();
        names.sort_unstable();
        if names.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(ArchiveError::DuplicateName);
        }
        let mut ranges = Vec::new();
        ranges
            .try_reserve_exact(self.entries.len())
            .map_err(|_| ArchiveError::Malformed("entry range allocation"))?;
        for entry in &self.entries {
            let start = offset(entry.local)?;
            let end = self
                .data_start(entry)?
                .checked_add(offset(entry.compressed)?)
                .filter(|&end| end <= directory)
                .ok_or(ArchiveError::Malformed("entry data out of range"))?;
            ranges.push((start, end));
        }
        ranges.sort_unstable();
        if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
            return Err(ArchiveError::Malformed("overlapping entries"));
        }
        Ok(())
    }

    /// Offset of an entry's data, after its local header.
    fn data_start(&self, entry: &Entry) -> Result<usize, ArchiveError> {
        let local = offset(entry.local)?;
        if u32_at(self.bytes, local)? != LOCAL {
            return Err(ArchiveError::Malformed("local header signature"));
        }
        let name = usize::from(u16_at(self.bytes, local + 26)?);
        let extra = usize::from(u16_at(self.bytes, local + 28)?);
        Ok(local + 30 + name + extra)
    }

    /// Every entry, in central-directory order.
    pub(crate) fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The entry named `name`, ignoring ASCII case and a leading slash (OPC part names).
    pub(crate) fn find(&self, name: &str) -> Option<&Entry> {
        let name = name.strip_prefix('/').unwrap_or(name);
        self.entries
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
    }

    /// A reader of `entry`'s uncompressed bytes.
    pub(crate) fn open(&self, entry: &Entry) -> Result<EntryReader<'a>, ArchiveError> {
        if entry.flags & 0x0001 != 0 {
            return Err(ArchiveError::Unsupported("encrypted entry"));
        }
        if entry.flags & 0x0020 != 0 {
            return Err(ArchiveError::Unsupported("patched entry"));
        }
        let method = match entry.method {
            0 => Method::Stored,
            8 => Method::Deflate(InflateState::new_boxed(DataFormat::Raw)),
            _ => return Err(ArchiveError::Unsupported("compression method")),
        };
        if matches!(method, Method::Stored) && entry.compressed != entry.size {
            return Err(ArchiveError::Malformed("stored entry sizes differ"));
        }
        let start = self.data_start(entry)?;
        let input = &self.bytes[start..start + offset(entry.compressed)?];
        Ok(EntryReader {
            input,
            method,
            expected: entry.size,
            produced: 0,
            crc: Crc32::new(),
            expected_crc: entry.crc,
            finished: false,
        })
    }

    /// The whole uncompressed content of `entry`.
    #[cfg(test)]
    pub(crate) fn read(&self, entry: &Entry) -> Result<Vec<u8>, ArchiveError> {
        let mut out = Vec::new();
        out.try_reserve_exact(offset(entry.size)?)
            .map_err(|_| ArchiveError::Malformed("entry buffer allocation"))?;
        io::Read::read_to_end(&mut self.open(entry)?, &mut out).map_err(|e| archive_error(&e))?;
        Ok(out)
    }
}

/// The archive error behind an I/O error from an [`EntryReader`].
pub(crate) fn archive_error(error: &io::Error) -> ArchiveError {
    error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<ArchiveError>())
        .cloned()
        .unwrap_or(ArchiveError::Corrupt("unreadable entry data"))
}

enum Method {
    Stored,
    Deflate(Box<InflateState>),
}

/// Streaming reader of one entry, checking size and CRC-32.
pub(crate) struct EntryReader<'a> {
    input: &'a [u8],
    method: Method,
    expected: u64,
    produced: u64,
    crc: Crc32,
    expected_crc: u32,
    finished: bool,
}

fn corrupt(what: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, ArchiveError::Corrupt(what))
}

impl EntryReader<'_> {
    fn finish(&mut self) -> io::Result<()> {
        if self.produced != self.expected {
            return Err(corrupt("fewer bytes than declared"));
        }
        if self.crc.finish() != self.expected_crc {
            return Err(corrupt("CRC-32 mismatch"));
        }
        self.finished = true;
        Ok(())
    }
}

impl io::Read for EntryReader<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.finished || out.is_empty() {
            return Ok(0);
        }
        // Never produce beyond the declared size: one spare byte detects overlong data.
        let room = usize::try_from(self.expected - self.produced)
            .unwrap_or(usize::MAX)
            .saturating_add(1);
        let out_len = out.len().min(room);
        let out = &mut out[..out_len];
        let written = match &mut self.method {
            Method::Stored => {
                let count = out.len().min(self.input.len());
                out[..count].copy_from_slice(&self.input[..count]);
                self.input = &self.input[count..];
                count
            }
            Method::Deflate(state) => {
                let result = inflate(state, self.input, out, MZFlush::None);
                self.input = &self.input[result.bytes_consumed..];
                match result.status {
                    Ok(MZStatus::Ok | MZStatus::StreamEnd) => {}
                    Ok(MZStatus::NeedDict) | Err(_) => return Err(corrupt("invalid deflate data")),
                }
                if result.bytes_written == 0 && result.status != Ok(MZStatus::StreamEnd) {
                    if self.input.is_empty() {
                        return Err(corrupt("truncated deflate data"));
                    }
                    return Err(corrupt("deflate stream makes no progress"));
                }
                result.bytes_written
            }
        };
        self.produced += written as u64;
        if self.produced > self.expected {
            return Err(corrupt("more bytes than declared"));
        }
        self.crc.update(&out[..written]);
        if written == 0 {
            self.finish()?;
        }
        Ok(written)
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)] // Test archives are small.
pub(crate) mod tests {
    use super::*;
    use std::io::Read;

    pub(crate) const LIMITS: ArchiveLimits = ArchiveLimits {
        max_entries: 64,
        max_entry_bytes: 1 << 24,
    };

    /// A test archive writer: each entry stored or raw-deflated, optionally with ZIP64 fields and
    /// a streamed (zero-size) local header.
    pub(crate) fn build(entries: &[(&str, &[u8], bool)], zip64: bool) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for &(name, data, deflate) in entries {
            let mut crc = Crc32::new();
            crc.update(data);
            let crc = crc.finish();
            let payload = if deflate {
                miniz_oxide::deflate::compress_to_vec(data, 6)
            } else {
                data.to_vec()
            };
            let local = out.len() as u64;
            let method: u16 = if deflate { 8 } else { 0 };
            out.extend(LOCAL.to_le_bytes());
            out.extend(20u16.to_le_bytes());
            out.extend(8u16.to_le_bytes()); // Streaming flag: sizes after the data.
            out.extend(method.to_le_bytes());
            out.extend([0; 4]);
            out.extend([0; 12]); // Zero CRC and sizes, as streaming writers emit.
            out.extend((name.len() as u16).to_le_bytes());
            out.extend(0u16.to_le_bytes());
            out.extend(name.as_bytes());
            out.extend(&payload);
            central.extend(CENTRAL.to_le_bytes());
            central.extend([20, 0, 20, 0]);
            central.extend(8u16.to_le_bytes());
            central.extend(method.to_le_bytes());
            central.extend([0; 4]);
            central.extend(crc.to_le_bytes());
            let mut extra = Vec::new();
            if zip64 {
                central.extend(u32::MAX.to_le_bytes());
                central.extend(u32::MAX.to_le_bytes());
                extra.extend(1u16.to_le_bytes());
                extra.extend(24u16.to_le_bytes());
                extra.extend((data.len() as u64).to_le_bytes());
                extra.extend((payload.len() as u64).to_le_bytes());
                extra.extend(local.to_le_bytes());
            } else {
                central.extend((payload.len() as u32).to_le_bytes());
                central.extend((data.len() as u32).to_le_bytes());
            }
            central.extend((name.len() as u16).to_le_bytes());
            central.extend((extra.len() as u16).to_le_bytes());
            central.extend([0; 10]); // Comment length, disk, internal and external attributes.
            central.extend(if zip64 { u32::MAX } else { local as u32 }.to_le_bytes());
            central.extend(name.as_bytes());
            central.extend(extra);
        }
        let start = out.len() as u64;
        out.extend(&central);
        let count = entries.len() as u64;
        if zip64 {
            let record = out.len() as u64;
            out.extend(ZIP64_EOCD.to_le_bytes());
            out.extend(44u64.to_le_bytes());
            out.extend([45, 0, 45, 0]);
            out.extend([0; 8]);
            out.extend(count.to_le_bytes());
            out.extend(count.to_le_bytes());
            out.extend((central.len() as u64).to_le_bytes());
            out.extend(start.to_le_bytes());
            out.extend(ZIP64_LOCATOR.to_le_bytes());
            out.extend(0u32.to_le_bytes());
            out.extend(record.to_le_bytes());
            out.extend(1u32.to_le_bytes());
        }
        out.extend(EOCD.to_le_bytes());
        out.extend([0; 4]);
        let saturate = |n: u64| if zip64 { u16::MAX } else { n as u16 };
        out.extend(saturate(count).to_le_bytes());
        out.extend(saturate(count).to_le_bytes());
        out.extend((central.len() as u32).to_le_bytes());
        out.extend(if zip64 { u32::MAX } else { start as u32 }.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out
    }

    #[test]
    fn stored_deflated_streamed_and_zip64_entries_read_back() {
        let big: Vec<u8> = (0..200_000u32)
            .flat_map(|i| (i % 251).to_le_bytes())
            .collect();
        for zip64 in [false, true] {
            let bytes = build(
                &[
                    ("3D/3dmodel.model", &big, true),
                    ("_rels/.rels", b"<r/>", false),
                ],
                zip64,
            );
            let archive = Archive::parse(&bytes, LIMITS).unwrap();
            assert_eq!(archive.entries().len(), 2);
            let model = archive.find("/3d/3DMODEL.model").unwrap();
            assert_eq!(model.size(), big.len() as u64);
            assert_eq!(archive.read(model).unwrap(), big);
            // Small reads through the streaming reader give the same bytes.
            let mut reader = archive.open(model).unwrap();
            let mut streamed = Vec::new();
            let mut chunk = [0u8; 7];
            loop {
                let n = reader.read(&mut chunk).unwrap();
                if n == 0 {
                    break;
                }
                streamed.extend_from_slice(&chunk[..n]);
            }
            assert_eq!(streamed, big);
            assert_eq!(
                archive.read(archive.find("_rels/.rels").unwrap()).unwrap(),
                b"<r/>"
            );
            assert!(archive.find("missing").is_none());
        }
    }

    #[test]
    fn corrupt_and_hostile_archives_are_refused() {
        let good = build(&[("a", b"hello hello hello", true)], false);
        assert_eq!(
            Archive::parse(b"plain text", LIMITS).err(),
            Some(ArchiveError::NotAnArchive)
        );
        // A flipped data byte fails the CRC (or the deflate stream) on read.
        let mut flipped = good.clone();
        flipped[31 + 2] ^= 0x40;
        let archive = Archive::parse(&flipped, LIMITS).unwrap();
        assert!(matches!(
            archive.read(&archive.entries()[0]),
            Err(ArchiveError::Corrupt(_))
        ));
        // A declared size larger than the data, or above the limit.
        let lying = build(&[("a", b"abc", false)], false);
        let mut lying = lying;
        let central = lying.len() - 22 - 47;
        lying[central + 24..central + 28].copy_from_slice(&4u32.to_le_bytes());
        let archive = Archive::parse(&lying, LIMITS).unwrap();
        assert_eq!(
            archive.read(&archive.entries()[0]).err(),
            Some(ArchiveError::Malformed("stored entry sizes differ"))
        );
        let tight = ArchiveLimits {
            max_entries: 64,
            max_entry_bytes: 4,
        };
        assert_eq!(
            Archive::parse(&good, tight).err(),
            Some(ArchiveError::EntryTooLarge { limit: 4 })
        );
        let few = ArchiveLimits {
            max_entries: 1,
            max_entry_bytes: 1 << 20,
        };
        let two = build(&[("a", b"1", false), ("b", b"2", false)], false);
        assert_eq!(
            Archive::parse(&two, few).err(),
            Some(ArchiveError::TooManyEntries { limit: 1 })
        );
        let twins = build(&[("Part", b"1", false), ("part", b"2", false)], false);
        assert_eq!(
            Archive::parse(&twins, LIMITS).err(),
            Some(ArchiveError::DuplicateName)
        );
        // Truncation removes the end of central directory.
        assert!(Archive::parse(&good[..good.len() - 5], LIMITS).is_err());
    }
}
