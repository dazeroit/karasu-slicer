//! The Windows resource file that puts the application icon into `karasu.exe` (TD-141).
//!
//! The build script (`build.rs`) turns `assets/ui/icon/karasu.ico` into a 32-bit resource file
//! (`.res`), which the MSVC linker takes as an input. Explorer, the taskbar and shortcuts then
//! show the icon from the executable itself. No resource compiler or crate is involved: the
//! `.res` layout is a documented, simple format.
//!
//! **Layout** (Microsoft "Resource file formats"): an empty first entry, then one entry per
//! resource. Each entry is a header (data size, header size, ordinal type, ordinal name, data
//! version, memory flags, language, version, characteristics; 32 bytes with ordinals) followed by
//! the data, padded to four bytes. An icon file becomes:
//! - one `RT_ICON` (3) per image, named 1, 2, …, holding the image's bytes (PNG or DIB) as the
//!   `.ico` holds them;
//! - one `RT_GROUP_ICON` (14) named 1, the `.ico` directory with each entry's file offset
//!   replaced by its `RT_ICON` name. Windows shows the group with the lowest name.
//!
//! The same file is compiled into the build script (with `#[path]`) and, for its tests, into
//! the application.

/// `RT_ICON`: one image of an icon.
const RT_ICON: u16 = 3;
/// `RT_GROUP_ICON`: an icon's directory of images.
const RT_GROUP_ICON: u16 = 14;
/// `MOVEABLE | DISCARDABLE`, as resource compilers mark icon images.
const ICON_FLAGS: u16 = 0x1010;
/// `MOVEABLE | PURE | DISCARDABLE`, as resource compilers mark icon groups.
const GROUP_FLAGS: u16 = 0x1030;
/// Language neutral.
const NEUTRAL: u16 = 0;

/// One image of an `.ico` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconImage {
    /// The directory entry's first 12 bytes: width, height, colours, reserved, planes, bit count
    /// and size, as the `.ico` stores them.
    pub entry: [u8; 12],
    /// The image's bytes.
    pub data: Vec<u8>,
}

/// Read an `.ico` file's images.
///
/// # Errors
/// A message when the file is not an icon (reserved 0, type 1), has no image, or an entry's
/// bytes lie outside the file or disagree with its stated size.
pub fn read_ico(ico: &[u8]) -> Result<Vec<IconImage>, String> {
    let word = |at: usize| -> Result<u16, String> {
        ico.get(at..at + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .ok_or_else(|| "the icon file is truncated".to_owned())
    };
    let dword = |at: usize| -> Result<u32, String> {
        ico.get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| "the icon file is truncated".to_owned())
    };
    if word(0)? != 0 || word(2)? != 1 {
        return Err("not an icon file (reserved 0, type 1)".to_owned());
    }
    let count = usize::from(word(4)?);
    if count == 0 {
        return Err("the icon file holds no image".to_owned());
    }
    let mut images = Vec::with_capacity(count);
    for index in 0..count {
        let at = 6 + 16 * index;
        let size = usize::try_from(dword(at + 8)?).map_err(|e| e.to_string())?;
        let offset = usize::try_from(dword(at + 12)?).map_err(|e| e.to_string())?;
        let data = offset
            .checked_add(size)
            .and_then(|end| ico.get(offset..end))
            .ok_or_else(|| format!("image {} lies outside the icon file", index + 1))?;
        let mut entry = [0; 12];
        entry.copy_from_slice(&ico[at..at + 12]);
        images.push(IconImage {
            entry,
            data: data.to_vec(),
        });
    }
    Ok(images)
}

/// The resource file holding `images` as icon group 1.
///
/// # Errors
/// A message when there are more images than resource names (65,535) or an image is larger
/// than a resource can hold.
pub fn resource_file(images: &[IconImage]) -> Result<Vec<u8>, String> {
    let mut res = Vec::new();
    // The empty first entry that marks a 32-bit resource file.
    entry(&mut res, 0, 0, 0, &[])?;
    let mut group = Vec::with_capacity(6 + 14 * images.len());
    let count = u16::try_from(images.len()).map_err(|_| "too many icon images".to_owned())?;
    group.extend_from_slice(&0u16.to_le_bytes());
    group.extend_from_slice(&1u16.to_le_bytes());
    group.extend_from_slice(&count.to_le_bytes());
    for (index, image) in images.iter().enumerate() {
        let name = u16::try_from(index + 1).map_err(|_| "too many icon images".to_owned())?;
        entry(&mut res, RT_ICON, name, ICON_FLAGS, &image.data)?;
        group.extend_from_slice(&image.entry);
        group.extend_from_slice(&name.to_le_bytes());
    }
    entry(&mut res, RT_GROUP_ICON, 1, GROUP_FLAGS, &group)?;
    Ok(res)
}

/// Append one resource with an ordinal type and name, its data padded to four bytes.
fn entry(res: &mut Vec<u8>, kind: u16, name: u16, flags: u16, data: &[u8]) -> Result<(), String> {
    const HEADER: u32 = 32;
    let size = u32::try_from(data.len()).map_err(|_| "an icon image is too large".to_owned())?;
    res.extend_from_slice(&size.to_le_bytes());
    res.extend_from_slice(&HEADER.to_le_bytes());
    res.extend_from_slice(&[0xFF, 0xFF]);
    res.extend_from_slice(&kind.to_le_bytes());
    res.extend_from_slice(&[0xFF, 0xFF]);
    res.extend_from_slice(&name.to_le_bytes());
    res.extend_from_slice(&0u32.to_le_bytes()); // data version
    res.extend_from_slice(&flags.to_le_bytes());
    res.extend_from_slice(&NEUTRAL.to_le_bytes());
    res.extend_from_slice(&0u32.to_le_bytes()); // version
    res.extend_from_slice(&0u32.to_le_bytes()); // characteristics
    res.extend_from_slice(data);
    while !res.len().is_multiple_of(4) {
        res.push(0);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bundled icon, as the build script reads it.
    const ICO: &[u8] = include_bytes!("../../../assets/ui/icon/karasu.ico");

    /// One resource read back: type, name, flags, data; and the offset of the next.
    fn read_entry(res: &[u8], at: usize) -> (u16, u16, u16, &[u8], usize) {
        let dword = |i: usize| u32::from_le_bytes(res[i..i + 4].try_into().unwrap()) as usize;
        let word = |i: usize| u16::from_le_bytes(res[i..i + 2].try_into().unwrap());
        let (size, header) = (dword(at), dword(at + 4));
        assert_eq!(header, 32);
        assert_eq!((word(at + 8), word(at + 12)), (0xFFFF, 0xFFFF));
        let data = &res[at + header..at + header + size];
        (
            word(at + 10),
            word(at + 14),
            word(at + 20),
            data,
            (at + header + size).next_multiple_of(4),
        )
    }

    #[test]
    fn the_bundled_icon_becomes_a_group_of_its_images() {
        let images = read_ico(ICO).unwrap();
        assert_eq!(images.len(), 7);
        let res = resource_file(&images).unwrap();
        // The empty first entry.
        let (kind, name, _, data, mut at) = read_entry(&res, 0);
        assert_eq!((kind, name, data.len()), (0, 0, 0));
        for (index, image) in images.iter().enumerate() {
            let (kind, name, flags, data, next) = read_entry(&res, at);
            assert_eq!(
                (kind, usize::from(name), flags),
                (RT_ICON, index + 1, ICON_FLAGS)
            );
            assert_eq!(data, image.data.as_slice());
            at = next;
        }
        let (kind, name, flags, group, end) = read_entry(&res, at);
        assert_eq!(
            (kind, name, flags, end),
            (RT_GROUP_ICON, 1, GROUP_FLAGS, res.len())
        );
        assert_eq!(&group[..6], &[0, 0, 1, 0, 7, 0]);
        for (index, image) in images.iter().enumerate() {
            let entry = &group[6 + 14 * index..6 + 14 * (index + 1)];
            assert_eq!(&entry[..12], &image.entry);
            assert_eq!(
                usize::from(u16::from_le_bytes([entry[12], entry[13]])),
                index + 1
            );
        }
    }

    #[test]
    fn a_file_that_is_not_an_icon_is_refused() {
        assert!(read_ico(b"\x89PNG\r\n").is_err());
        assert!(read_ico(&[0, 0, 1, 0, 0, 0]).is_err());
        // One entry whose image lies past the end.
        let mut ico = vec![0, 0, 1, 0, 1, 0];
        ico.extend_from_slice(&[16, 16, 0, 0, 1, 0, 32, 0]);
        ico.extend_from_slice(&100u32.to_le_bytes());
        ico.extend_from_slice(&22u32.to_le_bytes());
        assert!(read_ico(&ico).unwrap_err().contains("outside"));
    }
}
