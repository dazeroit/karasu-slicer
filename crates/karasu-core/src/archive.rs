//! ZIP packages read and written whole (TD-130): the project file and other karasu packages.
//!
//! - **Reading** goes through the bounded in-memory reader of the model formats
//!   ([`crate::mesh::formats`]): the central directory is authoritative, entry data must lie in
//!   the file without overlapping, sizes and CRC-32s are checked, and the entry count and sizes
//!   are capped up front. ZIP64 records are read.
//! - **Writing** is independently designed (APPNOTE 6.3.x subset): entries are deflated, or
//!   stored when deflating does not make them smaller; names are UTF-8 (general-purpose flag 11);
//!   no data descriptors, comments, encryption or extra fields other than ZIP64. A ZIP64 extra
//!   field is written for an entry whose size or offset does not fit 32 bits, and the ZIP64 end
//!   records when the entry count, the directory's size or its offset does not fit, so a package
//!   is not bounded at 4 GiB.
//! - **Deterministic:** every entry has the same DOS timestamp (1980-01-01 00:00) and the
//!   entries are written in the order they are added, so the same entries give the same bytes.
//!
//! The SL1 writer keeps its own stored ZIP framing, so its output is unchanged.
use crate::{
    digest::Crc32,
    mesh::formats::zip::{archive_error, Archive, Entry},
};
use std::io::{self, Read, Write};

pub use crate::mesh::formats::zip::{ArchiveError, ArchiveLimits};

/// A ZIP package parsed from bytes held in memory.
pub struct ArchiveReader<'a> {
    archive: Archive<'a>,
}

impl<'a> ArchiveReader<'a> {
    /// Parse the central directory of `bytes` within `limits`.
    ///
    /// # Errors
    /// Not an archive, a malformed or unsupported one, or one beyond the limits.
    pub fn open(bytes: &'a [u8], limits: ArchiveLimits) -> Result<Self, ArchiveError> {
        Archive::parse(bytes, limits).map(|archive| Self { archive })
    }

    /// The entries' names, in central-directory order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.archive.entries().iter().map(Entry::name)
    }

    /// The whole content of the entry named exactly `name`, or `None` without one.
    ///
    /// # Errors
    /// An entry that cannot be opened, or whose data does not decompress to its declared size
    /// and CRC-32.
    pub fn read(&self, name: &str) -> Result<Option<Vec<u8>>, ArchiveError> {
        let Some(entry) = self.archive.entries().iter().find(|e| e.name() == name) else {
            return Ok(None);
        };
        let size = usize::try_from(entry.declared_size())
            .map_err(|_| ArchiveError::Malformed("entry size out of range"))?;
        let mut out = Vec::new();
        out.try_reserve_exact(size)
            .map_err(|_| ArchiveError::Malformed("entry buffer allocation"))?;
        self.archive
            .open(entry)?
            .read_to_end(&mut out)
            .map_err(|e| archive_error(&e))?;
        Ok(Some(out))
    }
}

/// How hard entries are deflated (miniz's levels, 0 to 10). Measured on two binary STL models
/// (93.2 and 25.0 MB, TD-130): level 3 makes them 52 % and 75 % of their size, as level 6 does,
/// in 1.6 and 0.56 s instead of 2.3 and 0.69 s; level 1 is three times faster at 60 % and 76 %.
pub const DEFLATE_LEVEL: u8 = 3;

const LOCAL: u32 = 0x0403_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const EOCD: u32 = 0x0605_4b50;
const ZIP64_EOCD: u32 = 0x0606_4b50;
const ZIP64_LOCATOR: u32 = 0x0706_4b50;
/// 1980-01-01, the earliest DOS date; the time is 00:00:00.
const DOS_DATE: u16 = (1 << 5) | 1;
/// UTF-8 names.
const FLAGS: u16 = 1 << 11;
/// Version needed: 2.0 for deflate, 4.5 for ZIP64.
const VERSION_DEFLATE: u16 = 20;
const VERSION_ZIP64: u16 = 45;
const SATURATED: u32 = u32::MAX;

/// What the central directory records of one written entry.
struct Written {
    name: String,
    method: u16,
    crc: u32,
    compressed: u64,
    size: u64,
    offset: u64,
}

/// A ZIP package written entry by entry to `W`, then finished with its central directory.
pub struct ArchiveWriter<W: Write> {
    out: W,
    position: u64,
    entries: Vec<Written>,
    /// Values from this one up need ZIP64 records; lowered by tests to exercise them.
    zip64_from: u64,
}

impl<W: Write> ArchiveWriter<W> {
    /// A package written to `out` from its start.
    pub const fn new(out: W) -> Self {
        Self {
            out,
            position: 0,
            entries: Vec::new(),
            zip64_from: SATURATED as u64,
        }
    }

    /// The same writer with ZIP64 records for every value, as a package beyond 4 GiB has them:
    /// lets tests read such records without writing gigabytes.
    #[doc(hidden)]
    #[must_use]
    pub fn with_zip64_always(mut self) -> Self {
        self.zip64_from = 0;
        self
    }

    /// Add the entry `name` holding `data`, deflated unless that does not make it smaller.
    ///
    /// # Errors
    /// An empty or overlong name, a name already added (ignoring ASCII case, as readers compare
    /// them), more entries than ZIP64 can count, or a write failure.
    pub fn add(&mut self, name: &str, data: &[u8]) -> io::Result<()> {
        let invalid = |reason: &str| io::Error::new(io::ErrorKind::InvalidInput, reason);
        if name.is_empty() || name.len() > usize::from(u16::MAX) {
            return Err(invalid("a ZIP entry name must have 1 to 65535 bytes"));
        }
        if self
            .entries
            .iter()
            .any(|e| e.name.eq_ignore_ascii_case(name))
        {
            return Err(invalid("two ZIP entries would have the same name"));
        }
        let mut crc = Crc32::new();
        crc.update(data);
        let crc = crc.finish();
        let deflated = miniz_oxide::deflate::compress_to_vec(data, DEFLATE_LEVEL);
        let (method, stored): (u16, &[u8]) = if deflated.len() < data.len() {
            (8, &deflated)
        } else {
            (0, data)
        };
        let size = data.len() as u64;
        let compressed = stored.len() as u64;
        let zip64 = size >= self.zip64_from || compressed >= self.zip64_from;
        let mut header = Vec::with_capacity(30 + name.len() + 20);
        put32(&mut header, LOCAL);
        put16(
            &mut header,
            if zip64 {
                VERSION_ZIP64
            } else {
                VERSION_DEFLATE
            },
        );
        put16(&mut header, FLAGS);
        put16(&mut header, method);
        put16(&mut header, 0);
        put16(&mut header, DOS_DATE);
        put32(&mut header, crc);
        if zip64 {
            put32(&mut header, SATURATED);
            put32(&mut header, SATURATED);
        } else {
            put32(&mut header, narrow(compressed)?);
            put32(&mut header, narrow(size)?);
        }
        put16(&mut header, narrow16(name.len())?);
        put16(&mut header, if zip64 { 20 } else { 0 });
        header.extend_from_slice(name.as_bytes());
        if zip64 {
            put16(&mut header, 0x0001);
            put16(&mut header, 16);
            put64(&mut header, size);
            put64(&mut header, compressed);
        }
        let offset = self.position;
        self.write(&header)?;
        self.write(stored)?;
        self.entries.push(Written {
            name: name.to_owned(),
            method,
            crc,
            compressed,
            size,
            offset,
        });
        Ok(())
    }

    /// Write the central directory and the end records, and return the output.
    ///
    /// # Errors
    /// A write failure.
    pub fn finish(mut self) -> io::Result<W> {
        let start = self.position;
        let mut directory = Vec::new();
        for e in &self.entries {
            let zip64 = e.size >= self.zip64_from
                || e.compressed >= self.zip64_from
                || e.offset >= self.zip64_from;
            put32(&mut directory, CENTRAL);
            let version = if zip64 {
                VERSION_ZIP64
            } else {
                VERSION_DEFLATE
            };
            // Made by: MS-DOS attributes, the same version.
            put16(&mut directory, version);
            put16(&mut directory, version);
            put16(&mut directory, FLAGS);
            put16(&mut directory, e.method);
            put16(&mut directory, 0);
            put16(&mut directory, DOS_DATE);
            put32(&mut directory, e.crc);
            let field = |value: u64| if zip64 { Ok(SATURATED) } else { narrow(value) };
            put32(&mut directory, field(e.compressed)?);
            put32(&mut directory, field(e.size)?);
            put16(&mut directory, narrow16(e.name.len())?);
            put16(&mut directory, if zip64 { 28 } else { 0 });
            put16(&mut directory, 0); // Comment.
            put16(&mut directory, 0); // Disk.
            put16(&mut directory, 0); // Internal attributes.
            put32(&mut directory, 0); // External attributes.
            put32(&mut directory, field(e.offset)?);
            directory.extend_from_slice(e.name.as_bytes());
            if zip64 {
                // The saturated fields, in the order the specification gives.
                put16(&mut directory, 0x0001);
                put16(&mut directory, 24);
                put64(&mut directory, e.size);
                put64(&mut directory, e.compressed);
                put64(&mut directory, e.offset);
            }
        }
        let size = directory.len() as u64;
        let count = self.entries.len() as u64;
        let zip64 = count >= u64::from(u16::MAX).min(self.zip64_from)
            || size >= self.zip64_from
            || start >= self.zip64_from;
        let mut end = Vec::new();
        if zip64 {
            let record = start + size;
            put32(&mut end, ZIP64_EOCD);
            put64(&mut end, 44); // Size of the rest of this record.
            put16(&mut end, VERSION_ZIP64);
            put16(&mut end, VERSION_ZIP64);
            put32(&mut end, 0); // This disk.
            put32(&mut end, 0); // The directory's disk.
            put64(&mut end, count);
            put64(&mut end, count);
            put64(&mut end, size);
            put64(&mut end, start);
            put32(&mut end, ZIP64_LOCATOR);
            put32(&mut end, 0); // The record's disk.
            put64(&mut end, record);
            put32(&mut end, 1); // Disks.
        }
        put32(&mut end, EOCD);
        put16(&mut end, 0);
        put16(&mut end, 0);
        let entries = if zip64 {
            u16::MAX
        } else {
            narrow16(self.entries.len())?
        };
        put16(&mut end, entries);
        put16(&mut end, entries);
        put32(&mut end, if zip64 { SATURATED } else { narrow(size)? });
        put32(&mut end, if zip64 { SATURATED } else { narrow(start)? });
        put16(&mut end, 0); // Comment.
        self.write(&directory)?;
        self.write(&end)?;
        self.out.flush()?;
        Ok(self.out)
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.out.write_all(bytes)?;
        self.position += bytes.len() as u64;
        Ok(())
    }
}

fn put16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// A value below the ZIP64 threshold, which fits 32 bits.
fn narrow(value: u64) -> io::Result<u32> {
    u32::try_from(value)
        .ok()
        .filter(|&v| v != SATURATED)
        .ok_or_else(|| io::Error::other("a ZIP value needs ZIP64 records"))
}

fn narrow16(value: usize) -> io::Result<u16> {
    u16::try_from(value)
        .ok()
        .filter(|&v| v != u16::MAX)
        .ok_or_else(|| io::Error::other("a ZIP count needs ZIP64 records"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: ArchiveLimits = ArchiveLimits {
        max_entries: 16,
        max_entry_bytes: 1 << 20,
    };

    fn written(entries: &[(&str, &[u8])], zip64: bool) -> Vec<u8> {
        let mut writer = ArchiveWriter::new(Vec::new());
        if zip64 {
            writer = writer.with_zip64_always();
        }
        for (name, data) in entries {
            writer.add(name, data).unwrap();
        }
        writer.finish().unwrap()
    }

    #[test]
    fn entries_written_read_back_deflated_or_stored_with_or_without_zip64() {
        let text = b"karasu ".repeat(1000);
        let noise: Vec<u8> = (0..4096_u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13).to_le_bytes()[0])
            .collect();
        let entries: [(&str, &[u8]); 4] = [
            ("karasu-project.json", &text),
            ("models/noise.bin", &noise),
            ("empty", b""),
            ("models/città.stl", b"solid"),
        ];
        for zip64 in [false, true] {
            let bytes = written(&entries, zip64);
            let reader = ArchiveReader::open(&bytes, LIMITS).unwrap();
            let names: Vec<&str> = reader.names().collect();
            assert_eq!(names, entries.map(|(n, _)| n));
            for (name, data) in entries {
                assert_eq!(reader.read(name).unwrap().as_deref(), Some(data), "{name}");
            }
            assert_eq!(reader.read("missing").unwrap(), None);
            // Text deflates; noise is stored: the package is smaller than the text alone.
            assert!(bytes.len() < text.len(), "{}", bytes.len());
            // Deterministic.
            assert_eq!(written(&entries, zip64), bytes);
        }
        // ZIP64 records are there only when needed.
        let plain = written(&entries, false);
        assert!(!plain
            .windows(4)
            .any(|w| w == ZIP64_EOCD.to_le_bytes().as_slice()));
        let wide = written(&entries, true);
        assert!(wide
            .windows(4)
            .any(|w| w == ZIP64_EOCD.to_le_bytes().as_slice()));
    }

    #[test]
    fn names_that_readers_could_not_tell_apart_are_refused() {
        let mut writer = ArchiveWriter::new(Vec::new());
        writer.add("Model.stl", b"a").unwrap();
        assert!(writer.add("model.STL", b"b").is_err());
        assert!(writer.add("", b"c").is_err());
        // The entry that was refused left nothing behind.
        let bytes = writer.finish().unwrap();
        let reader = ArchiveReader::open(&bytes, LIMITS).unwrap();
        assert_eq!(reader.names().collect::<Vec<_>>(), ["Model.stl"]);
    }

    #[test]
    fn a_damaged_package_is_refused() {
        let bytes = written(&[("a.json", &b"{}".repeat(100))], false);
        // Truncated: no end record.
        assert!(ArchiveReader::open(&bytes[..bytes.len() - 10], LIMITS).is_err());
        // A flipped data byte fails its CRC-32 or its inflation.
        let mut flipped = bytes.clone();
        flipped[40] ^= 0x55;
        let reader = ArchiveReader::open(&flipped, LIMITS).unwrap();
        assert!(reader.read("a.json").is_err());
    }
}
