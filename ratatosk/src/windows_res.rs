//! The resources `rata bundle` links into a Windows `.exe`: its icon, the
//! one Explorer, the Start menu and the taskbar show, and its version
//! information, the name Task Manager shows and the details in its
//! Properties.
//!
//! Written as a compiled resource file (`.res`), which Microsoft's linker
//! takes as it takes an object file, so no resource compiler (`rc.exe`,
//! `windres`) is needed: `cargo rustc -- -C link-arg=app.res`.

/// What the version resource says about the app.
pub struct VersionInfo<'a> {
    /// `[bundle].name`: the file description (Task Manager's name for the
    /// process) and the product name.
    pub name: &'a str,
    /// `[bundle].version`, shown as it's written; its first three numbers
    /// are also the numeric file version.
    pub version: &'a str,
    /// `[bundle].publisher`, as the company name.
    pub publisher: Option<&'a str>,
    /// The executable's file name (`my_app.exe`).
    pub exe: &'a str,
}

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_VERSION: u16 = 16;
/// English (United States), as the version resource's translation says.
const LANGUAGE: u16 = 0x0409;
/// The code page of the strings: Unicode.
const CODE_PAGE: u16 = 0x04b0;

/// A `.res` file of `icons` (each image's size and data, from
/// [`crate::icon::windows_images`]) as one icon group, the app's, and of
/// `version`.
pub fn res(icons: &[(u32, Vec<u8>)], version: &VersionInfo) -> Vec<u8> {
    // A `.res` file starts with an empty entry, which says what it is.
    let mut out = entry(0, 0, 0, &[]);
    for (n, (_, data)) in icons.iter().enumerate() {
        out.extend(entry(RT_ICON, n as u16 + 1, 0x1010, data));
    }
    if !icons.is_empty() {
        out.extend(entry(RT_GROUP_ICON, 1, 0x1030, &icon_group(icons)));
    }
    out.extend(entry(RT_VERSION, 1, 0x0030, &version_info(version)));
    out
}

/// One resource: its header (type and name by number) and its data, each
/// padded to four bytes.
fn entry(kind: u16, name: u16, flags: u16, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(32 + data.len() + 3);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes()); // the header's size
    for id in [kind, name] {
        out.extend_from_slice(&0xffffu16.to_le_bytes()); // a number, not a name
        out.extend_from_slice(&id.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes()); // data version
    out.extend_from_slice(&flags.to_le_bytes());
    let language = if kind == 0 { 0 } else { LANGUAGE };
    out.extend_from_slice(&language.to_le_bytes());
    out.extend_from_slice(&[0; 8]); // version, characteristics
    out.extend_from_slice(data);
    pad(&mut out);
    out
}

/// The icon group: each image's directory entry, as in an `.ico`, with the
/// number of its `RT_ICON` resource where an `.ico` has an offset.
fn icon_group(icons: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    for value in [0u16, 1, icons.len() as u16] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    for (n, (size, data)) in icons.iter().enumerate() {
        out.extend_from_slice(&crate::icon::entry_head(*size));
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(n as u16 + 1).to_le_bytes());
    }
    out
}

/// The first four numbers of `version` (`1.2.3-beta.1` is 1, 2, 3, 0),
/// as the numeric file version has them.
pub fn numeric_version(version: &str) -> [u16; 4] {
    let core = version.split(['-', '+']).next().unwrap_or("");
    let mut out = [0u16; 4];
    for (slot, part) in out.iter_mut().zip(core.split('.')) {
        *slot = part.trim().parse().unwrap_or(0);
    }
    out
}

/// `VS_VERSIONINFO`: the numeric version (`VS_FIXEDFILEINFO`), the strings
/// Explorer and Task Manager read, and the translation they're in.
fn version_info(info: &VersionInfo) -> Vec<u8> {
    let [a, b, c, d] = numeric_version(info.version).map(u32::from);
    let (high, low) = ((a << 16) | b, (c << 16) | d);
    let mut fixed = Vec::with_capacity(52);
    for value in [
        0xfeef_04bd, // signature
        0x0001_0000, // structure version
        high,
        low, // file version
        high,
        low,         // product version
        0x3f,        // which flags are meaningful
        0,           // flags: none
        0x0004_0004, // for Windows NT
        1,           // an application
        0,
        0,
        0, // subtype, date
    ] {
        fixed.extend_from_slice(&u32::to_le_bytes(value));
    }

    let mut strings = vec![
        ("FileDescription", info.name),
        ("FileVersion", info.version),
        ("InternalName", info.exe.trim_end_matches(".exe")),
        ("OriginalFilename", info.exe),
        ("ProductName", info.name),
        ("ProductVersion", info.version),
    ];
    if let Some(publisher) = info.publisher {
        strings.insert(0, ("CompanyName", publisher));
    }
    let table: Vec<Vec<u8>> = strings
        .into_iter()
        .map(|(key, value)| block(key, Value::Text(value), &[]))
        .collect();
    let string_table = block(
        &format!("{LANGUAGE:04X}{CODE_PAGE:04X}"),
        Value::None,
        &table,
    );
    let string_file_info = block("StringFileInfo", Value::None, &[string_table]);

    let mut translation = Vec::new();
    translation.extend_from_slice(&LANGUAGE.to_le_bytes());
    translation.extend_from_slice(&CODE_PAGE.to_le_bytes());
    let var = block("Translation", Value::Binary(&translation), &[]);
    let var_file_info = block("VarFileInfo", Value::None, &[var]);

    block(
        "VS_VERSION_INFO",
        Value::Binary(&fixed),
        &[string_file_info, var_file_info],
    )
}

enum Value<'a> {
    None,
    Text(&'a str),
    Binary(&'a [u8]),
}

/// One block of the version resource: its length, its value's length and
/// kind, its key, its value, and its children, each started on four bytes.
fn block(key: &str, value: Value, children: &[Vec<u8>]) -> Vec<u8> {
    let (value_len, kind, bytes): (u16, u16, Vec<u8>) = match value {
        Value::None => (0, 1, Vec::new()),
        // A string's length is in characters, its terminator included.
        Value::Text(text) => {
            let wide = utf16z(text);
            ((wide.len() / 2) as u16, 1, wide)
        }
        Value::Binary(data) => (data.len() as u16, 0, data.to_vec()),
    };
    let mut out = vec![0, 0]; // the length, written last
    out.extend_from_slice(&value_len.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend(utf16z(key));
    pad(&mut out);
    out.extend(bytes);
    for child in children {
        pad(&mut out);
        out.extend_from_slice(child);
    }
    let len = out.len() as u16;
    out[..2].copy_from_slice(&len.to_le_bytes());
    out
}

/// `text` as UTF-16, little-endian, with its terminator.
fn utf16z(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}

fn pad(out: &mut Vec<u8>) {
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> VersionInfo<'static> {
        VersionInfo {
            name: "Félagi",
            version: "1.4.2-beta.1",
            publisher: Some("Gets AS"),
            exe: "felagi.exe",
        }
    }

    /// The resources in a `.res` file: (type, name, language, data).
    fn entries(file: &[u8]) -> Vec<(u16, u16, u16, Vec<u8>)> {
        let u16_at = |i: usize| u16::from_le_bytes([file[i], file[i + 1]]);
        let u32_at = |i: usize| u32::from_le_bytes(file[i..i + 4].try_into().unwrap()) as usize;
        let mut found = Vec::new();
        let mut at = 0;
        while at < file.len() {
            let (data_len, head_len) = (u32_at(at), u32_at(at + 4));
            assert_eq!(head_len, 32);
            assert_eq!((u16_at(at + 8), u16_at(at + 12)), (0xffff, 0xffff));
            let data = file[at + head_len..at + head_len + data_len].to_vec();
            found.push((u16_at(at + 10), u16_at(at + 14), u16_at(at + 22), data));
            at += (head_len + data_len).div_ceil(4) * 4;
        }
        found
    }

    fn contains(haystack: &[u8], text: &str) -> bool {
        let needle = utf16z(text);
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn a_res_file_has_the_icons_their_group_and_the_version() {
        let icons = vec![(16, vec![1; 7]), (256, b"\x89PNG....".to_vec())];
        let file = res(&icons, &info());
        assert_eq!(file.len() % 4, 0);
        let found = entries(&file);
        let kinds: Vec<_> = found.iter().map(|e| (e.0, e.1)).collect();
        assert_eq!(
            kinds,
            [
                (0, 0),
                (RT_ICON, 1),
                (RT_ICON, 2),
                (RT_GROUP_ICON, 1),
                (RT_VERSION, 1)
            ]
        );
        assert!(found[0].3.is_empty(), "the first entry is the empty one");
        assert_eq!(found[1].3, vec![1; 7]);
        assert_eq!(found[1].2, LANGUAGE);

        // The group: an icon (type 1) of two, each pointing at its RT_ICON.
        let group = &found[3].3;
        assert_eq!(&group[..6], &[0, 0, 1, 0, 2, 0]);
        assert_eq!(group.len(), 6 + 14 * 2);
        assert_eq!(group[6], 16, "16 px");
        assert_eq!(&group[6 + 8..6 + 12], &7u32.to_le_bytes());
        assert_eq!(&group[6 + 12..6 + 14], &1u16.to_le_bytes());
        assert_eq!(group[6 + 14], 0, "256 px is written 0");
        assert_eq!(&group[6 + 14 + 12..], &2u16.to_le_bytes());
    }

    #[test]
    fn without_an_icon_only_the_version_is_linked() {
        let found = entries(&res(&[], &info()));
        let kinds: Vec<_> = found.iter().map(|e| e.0).collect();
        assert_eq!(kinds, [0, RT_VERSION]);
    }

    #[test]
    fn the_version_resource_names_the_app() {
        let data = version_info(&info());
        let u16_at = |i: usize| u16::from_le_bytes([data[i], data[i + 1]]);
        let u32_at = |i: usize| u32::from_le_bytes(data[i..i + 4].try_into().unwrap());
        assert_eq!(u16_at(0) as usize, data.len(), "the length covers it all");
        assert_eq!(u16_at(2), 52, "VS_FIXEDFILEINFO");
        assert!(contains(&data, "VS_VERSION_INFO"));
        // The key, padded, then the fixed info: its signature, and 1.4.2.0.
        let fixed = (6 + utf16z("VS_VERSION_INFO").len()).div_ceil(4) * 4;
        assert_eq!(u32_at(fixed), 0xfeef_04bd);
        assert_eq!(u32_at(fixed + 8), (1 << 16) | 4);
        assert_eq!(u32_at(fixed + 12), 2 << 16);
        for text in [
            "StringFileInfo",
            "040904B0",
            "FileDescription",
            "Félagi",
            "1.4.2-beta.1",
            "CompanyName",
            "Gets AS",
            "felagi.exe",
            "VarFileInfo",
            "Translation",
        ] {
            assert!(contains(&data, text), "missing {text}");
        }

        let mut plain = info();
        plain.publisher = None;
        assert!(!contains(&version_info(&plain), "CompanyName"));
    }

    #[test]
    fn each_block_says_its_own_length() {
        let child = block("Key", Value::Text("Value"), &[]);
        // 6 + "Key\0" (8) = 14, padded to 16; then "Value\0" (12).
        assert_eq!(child.len(), 28);
        assert_eq!(u16::from_le_bytes([child[0], child[1]]), 28);
        assert_eq!(u16::from_le_bytes([child[2], child[3]]), 6, "in characters");
        let parent = block("P", Value::None, &[child.clone(), child]);
        // 6 + "P\0" (4) = 10, padded to 12; two children, each on four bytes.
        assert_eq!(parent.len(), 12 + 28 + 28);
    }

    #[test]
    fn the_numeric_version_is_its_first_four_numbers() {
        assert_eq!(numeric_version("1.2.3"), [1, 2, 3, 0]);
        assert_eq!(numeric_version("0.42.0-rc.1"), [0, 42, 0, 0]);
        assert_eq!(numeric_version("2.0.0+build.7"), [2, 0, 0, 0]);
        assert_eq!(numeric_version("1.2.3.4.5"), [1, 2, 3, 4]);
        assert_eq!(numeric_version("garbled"), [0, 0, 0, 0]);
    }
}
