//! Independently designed classic ZIP framing (APPNOTE 6.3.x subset) with fixed DOS timestamps
//! and checked offsets. Members are stored, or deflated with the crate's pinned encoder at
//! [`crate::archive::DEFLATE_LEVEL`] (TD-133); either way the header records the CRC-32 and the
//! size of the member's own bytes.
use super::machine::{CodecError, EncoderLimits};
use crate::{
    cancellation::{Cancellation, Checkpoint},
    export::WriteSeek,
};

/// ZIP method numbers.
const STORED: u16 = 0;
const DEFLATED: u16 = 8;

struct Entry {
    name: String,
    method: u16,
    crc: u32,
    compressed: u32,
    size: u32,
    offset: u32,
}

/// One member's bytes as they go into the archive, prepared on any thread: stored, or deflated
/// when that makes them smaller (TD-133). Its CRC-32 and size are those of the member's own
/// bytes.
#[derive(Debug)]
pub(crate) struct Member {
    method: u16,
    crc: u32,
    size: usize,
    data: Vec<u8>,
}

impl Member {
    /// `data` deflated, or stored when deflating does not make it smaller. Deterministic: the
    /// same bytes give the same member.
    pub(crate) fn deflated(
        data: Vec<u8>,
        cancel: &mut Cancellation<'_>,
    ) -> Result<Self, CodecError> {
        let crc = crc32(&data, cancel)?;
        cancel.check(Checkpoint::ExportLayer)?;
        let packed = miniz_oxide::deflate::compress_to_vec(&data, crate::archive::DEFLATE_LEVEL);
        let size = data.len();
        Ok(if packed.len() < size {
            Self {
                method: DEFLATED,
                crc,
                size,
                data: packed,
            }
        } else {
            Self {
                method: STORED,
                crc,
                size,
                data,
            }
        })
    }

    /// The bytes written into the archive.
    #[cfg(test)]
    pub(crate) fn written(&self) -> &[u8] {
        &self.data
    }
}

/// Internal bounded ZIP container; caller owns successful whole-file publication.
pub(crate) struct ClassicZip {
    entries: Vec<Entry>,
    position: u64,
    limits: EncoderLimits,
    name_bytes: usize,
}
impl ClassicZip {
    pub(crate) const fn new(limits: EncoderLimits) -> Self {
        Self {
            entries: Vec::new(),
            position: 0,
            limits,
            name_bytes: 0,
        }
    }
    /// Add `data` stored as member `name`.
    pub(crate) fn add(
        &mut self,
        writer: &mut dyn WriteSeek,
        name: &str,
        data: &[u8],
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), CodecError> {
        cancel.check(Checkpoint::ExportLayer)?;
        if data.len() > self.limits.max_layer_bytes {
            return Err(CodecError::Limit("ZIP member size/count"));
        }
        let crc = crc32(data, cancel)?;
        self.write(writer, name, (STORED, crc, data.len()), data, cancel)
    }

    /// Add `member`, prepared beforehand (possibly on another thread), as member `name`.
    pub(crate) fn add_member(
        &mut self,
        writer: &mut dyn WriteSeek,
        name: &str,
        member: &Member,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), CodecError> {
        let header = (member.method, member.crc, member.size);
        self.write(writer, name, header, &member.data, cancel)
    }

    /// Write member `name`: its method, CRC-32 and own size, then `data` as it goes into the
    /// archive.
    fn write(
        &mut self,
        writer: &mut dyn WriteSeek,
        name: &str,
        (method, crc, own_size): (u16, u32, usize),
        data: &[u8],
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), CodecError> {
        cancel.check(Checkpoint::ExportLayer)?;
        if name.is_empty()
            || name.len() > 1024
            || name.starts_with('/')
            || name.contains('\\')
            || name.contains('\0')
            || name
                .split('/')
                .any(|s| s.is_empty() || s == "." || s == "..")
            || self.entries.iter().any(|e| e.name == name)
        {
            return Err(CodecError::Invalid(
                "invalid or duplicate generated ZIP member name",
            ));
        }
        if own_size > self.limits.max_layer_bytes
            || data.len() > self.limits.max_layer_bytes
            || self.entries.len() >= usize::from(u16::MAX)
        {
            return Err(CodecError::Limit("ZIP member size/count"));
        }
        if writer.stream_position()? != self.position {
            return Err(CodecError::Invalid(
                "ZIP writer position changed outside encoder",
            ));
        }
        let name_len =
            u16::try_from(name.len()).map_err(|_| CodecError::Limit("ZIP member name"))?;
        let compressed =
            u32::try_from(data.len()).map_err(|_| CodecError::Limit("ZIP32 member size"))?;
        let size = u32::try_from(own_size).map_err(|_| CodecError::Limit("ZIP32 member size"))?;
        let offset =
            u32::try_from(self.position).map_err(|_| CodecError::Limit("ZIP32 member offset"))?;
        let end = self
            .position
            .checked_add(30 + u64::from(name_len) + u64::from(compressed))
            .ok_or(CodecError::Limit("ZIP length overflow"))?;
        self.check_end(end)?;
        let names = self
            .name_bytes
            .checked_add(name.len())
            .ok_or(CodecError::Limit("ZIP name storage"))?;
        let capacity = if self.entries.len() == self.entries.capacity() {
            self.entries
                .capacity()
                .max(2)
                .checked_mul(2)
                .ok_or(CodecError::Limit("ZIP directory capacity"))?
                .min(usize::from(u16::MAX))
        } else {
            self.entries.capacity()
        };
        let requested = capacity
            .checked_mul(std::mem::size_of::<Entry>())
            .and_then(|n| n.checked_add(names))
            .ok_or(CodecError::Limit("ZIP directory storage"))?;
        if requested > self.limits.max_metadata_bytes {
            return Err(CodecError::Limit("ZIP retained directory metadata"));
        }
        if capacity > self.entries.capacity() {
            self.entries
                .try_reserve_exact(capacity - self.entries.len())
                .map_err(|_| CodecError::Limit("ZIP directory allocation"))?;
        }
        let mut owned = String::new();
        owned
            .try_reserve_exact(name.len())
            .map_err(|_| CodecError::Limit("ZIP name allocation"))?;
        owned.push_str(name);
        let mut header = [0u8; 30];
        header[0..4].copy_from_slice(&0x0403_4b50u32.to_le_bytes());
        header[4..6].copy_from_slice(&20u16.to_le_bytes());
        header[6..8].copy_from_slice(&0x0800u16.to_le_bytes());
        header[8..10].copy_from_slice(&method.to_le_bytes());
        header[12..14].copy_from_slice(&0x0021u16.to_le_bytes());
        header[14..18].copy_from_slice(&crc.to_le_bytes());
        header[18..22].copy_from_slice(&compressed.to_le_bytes());
        header[22..26].copy_from_slice(&size.to_le_bytes());
        header[26..28].copy_from_slice(&name_len.to_le_bytes());
        writer.write_all(&header)?;
        writer.write_all(name.as_bytes())?;
        for chunk in data.chunks(65536) {
            cancel.check(Checkpoint::ExportLayer)?;
            writer.write_all(chunk)?;
        }
        self.position = end;
        self.name_bytes = names;
        self.entries.push(Entry {
            name: owned,
            method,
            crc,
            compressed,
            size,
            offset,
        });
        Ok(())
    }
    fn check_end(&self, end: u64) -> Result<(), CodecError> {
        if end > self.limits.max_output_bytes || end > u64::from(u32::MAX) {
            Err(CodecError::Limit(
                "ZIP output budget or classic ZIP32 address space",
            ))
        } else {
            Ok(())
        }
    }
    pub(crate) fn finish(
        self,
        writer: &mut dyn WriteSeek,
        cancel: &mut Cancellation<'_>,
    ) -> Result<(), CodecError> {
        cancel.check(Checkpoint::ExportFinish)?;
        if writer.stream_position()? != self.position {
            return Err(CodecError::Invalid("ZIP final writer position changed"));
        }
        let mut end = self.position;
        for entry in &self.entries {
            end = end
                .checked_add(
                    46 + u64::try_from(entry.name.len())
                        .map_err(|_| CodecError::Limit("ZIP name length"))?,
                )
                .ok_or(CodecError::Limit("ZIP directory length"))?;
        }
        end = end
            .checked_add(22)
            .ok_or(CodecError::Limit("ZIP final length"))?;
        self.check_end(end)?;
        let central_size = u32::try_from(end - 22 - self.position)
            .map_err(|_| CodecError::Limit("ZIP directory size"))?;
        for (index, entry) in self.entries.iter().enumerate() {
            cancel.poll(Checkpoint::ExportFinish, index)?;
            let mut header = [0u8; 46];
            header[0..4].copy_from_slice(&0x0201_4b50u32.to_le_bytes());
            header[4..6].copy_from_slice(&20u16.to_le_bytes());
            header[6..8].copy_from_slice(&20u16.to_le_bytes());
            header[8..10].copy_from_slice(&0x0800u16.to_le_bytes());
            header[10..12].copy_from_slice(&entry.method.to_le_bytes());
            header[14..16].copy_from_slice(&0x0021u16.to_le_bytes());
            header[16..20].copy_from_slice(&entry.crc.to_le_bytes());
            header[20..24].copy_from_slice(&entry.compressed.to_le_bytes());
            header[24..28].copy_from_slice(&entry.size.to_le_bytes());
            header[28..30].copy_from_slice(
                &u16::try_from(entry.name.len())
                    .map_err(|_| CodecError::Limit("ZIP name length"))?
                    .to_le_bytes(),
            );
            header[42..46].copy_from_slice(&entry.offset.to_le_bytes());
            writer.write_all(&header)?;
            writer.write_all(entry.name.as_bytes())?;
        }
        let count =
            u16::try_from(self.entries.len()).map_err(|_| CodecError::Limit("ZIP member count"))?;
        let mut tail = [0u8; 22];
        tail[0..4].copy_from_slice(&0x0605_4b50u32.to_le_bytes());
        tail[8..10].copy_from_slice(&count.to_le_bytes());
        tail[10..12].copy_from_slice(&count.to_le_bytes());
        tail[12..16].copy_from_slice(&central_size.to_le_bytes());
        tail[16..20].copy_from_slice(
            &u32::try_from(self.position)
                .map_err(|_| CodecError::Limit("ZIP directory offset"))?
                .to_le_bytes(),
        );
        writer.write_all(&tail)?;
        cancel.check(Checkpoint::ExportFinish)?;
        Ok(())
    }
}
const fn table() -> [u32; 256] {
    let mut result = [0; 256];
    let mut i = 0;
    let mut value = 0u32;
    while i < 256 {
        let mut crc = value;
        let mut bit = 0;
        while bit < 8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0xedb8_8320 } else { 0 };
            bit += 1;
        }
        result[i] = crc;
        i += 1;
        value += 1;
    }
    result
}
const TABLE: [u32; 256] = table();
pub(crate) fn crc32(bytes: &[u8], cancel: &mut Cancellation<'_>) -> Result<u32, CodecError> {
    let mut crc = u32::MAX;
    for (i, &b) in bytes.iter().enumerate() {
        cancel.poll(Checkpoint::ExportLayer, i)?;
        crc = (crc >> 8) ^ TABLE[usize::from(crc.to_le_bytes()[0] ^ b)];
    }
    Ok(!crc)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn limits() -> EncoderLimits {
        EncoderLimits {
            max_output_bytes: 4096,
            max_layer_bytes: 1024,
            max_metadata_bytes: 1024,
        }
    }
    #[test]
    fn empty_archive_and_crc_checkvector() {
        let mut writer = Cursor::new(Vec::new());
        ClassicZip::new(limits())
            .finish(&mut writer, &mut Cancellation::never())
            .unwrap();
        assert_eq!(
            writer.into_inner(),
            [&0x0605_4b50u32.to_le_bytes()[..], &[0u8; 18]].concat()
        );
        assert_eq!(
            crc32(b"123456789", &mut Cancellation::never()).unwrap(),
            0xcbf4_3926
        );
    }
    #[test]
    fn member_offsets_and_central_directory() {
        let mut writer = Cursor::new(Vec::new());
        let mut zip = ClassicZip::new(limits());
        zip.add(&mut writer, "a.txt", b"hello", &mut Cancellation::never())
            .unwrap();
        zip.add(&mut writer, "dir/b", b"world", &mut Cancellation::never())
            .unwrap();
        zip.finish(&mut writer, &mut Cancellation::never()).unwrap();
        let bytes = writer.into_inner();
        assert_eq!(&bytes[30..35], b"a.txt");
        assert_eq!(&bytes[35..40], b"hello");
        assert_eq!(&bytes[40..44], &0x0403_4b50u32.to_le_bytes());
        assert_eq!(&bytes[80..84], &0x0201_4b50u32.to_le_bytes());
        assert_eq!(
            &bytes[bytes.len() - 6..bytes.len() - 2],
            &80u32.to_le_bytes()
        );
    }
    #[test]
    fn bad_names_counts_and_limits_fail() {
        let mut writer = Cursor::new(Vec::new());
        let mut zip = ClassicZip::new(limits());
        assert!(zip
            .add(&mut writer, "../bad", b"", &mut Cancellation::never())
            .is_err());
        zip.add(&mut writer, "valid", b"1", &mut Cancellation::never())
            .unwrap();
        assert!(zip
            .add(&mut writer, "valid", b"2", &mut Cancellation::never())
            .is_err());
        let mut small = ClassicZip::new(EncoderLimits {
            max_output_bytes: 10,
            ..limits()
        });
        assert!(small
            .add(
                &mut Cursor::new(Vec::new()),
                "a",
                b"hello",
                &mut Cancellation::never()
            )
            .is_err());
    }
    #[test]
    fn deflated_members_read_back_and_incompressible_ones_stay_stored() {
        // TD-133: repeated runs deflate; noise does not, and is stored.
        let text: Vec<u8> = b"runs of the same bytes ".repeat(40);
        let mut noise = vec![0u8; 512];
        let mut state = 0x2545_f491u32;
        for byte in &mut noise {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *byte = state.to_le_bytes()[0];
        }
        let never = &mut Cancellation::never();
        let packed = Member::deflated(text.clone(), never).unwrap();
        let kept = Member::deflated(noise.clone(), never).unwrap();
        assert!(packed.written().len() < text.len());
        assert_eq!(kept.written(), &noise[..]);
        let mut writer = Cursor::new(Vec::new());
        let mut zip = ClassicZip::new(limits());
        zip.add_member(&mut writer, "text", &packed, never).unwrap();
        zip.add_member(&mut writer, "noise", &kept, never).unwrap();
        zip.finish(&mut writer, never).unwrap();
        let bytes = writer.into_inner();
        // The first local header: deflated, the text's CRC and size, the packed length.
        let field = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        assert_eq!(&bytes[8..10], &8u16.to_le_bytes());
        assert_eq!(field(14), crc32(&text, never).unwrap());
        assert_eq!(field(18) as usize, packed.written().len());
        assert_eq!(field(22) as usize, text.len());
        let start = 30 + 4;
        let end = start + packed.written().len();
        assert_eq!(
            miniz_oxide::inflate::decompress_to_vec(&bytes[start..end]).unwrap(),
            text
        );
        // The second: stored as it is.
        assert_eq!(&bytes[end + 8..end + 10], &0u16.to_le_bytes());
        assert_eq!(&bytes[end + 30 + 5..end + 30 + 5 + noise.len()], &noise[..]);
        // The same bytes give the same member.
        assert_eq!(
            Member::deflated(text.clone(), never).unwrap().written(),
            packed.written()
        );
    }
    #[test]
    fn cancelled_encoding_never_reports_success() {
        let mut predicate = |_| true;
        assert!(matches!(
            ClassicZip::new(limits()).add(
                &mut Cursor::new(Vec::new()),
                "a",
                b"hello",
                &mut Cancellation::new(&mut predicate)
            ),
            Err(CodecError::Cancelled(_))
        ));
    }
}
