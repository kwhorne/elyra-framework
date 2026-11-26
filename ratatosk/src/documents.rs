//! Document types (RFC 0052): what `[[bundle.documents]]` in `elyra.toml`
//! declares, and what `rata bundle` writes for each platform so the OS offers
//! the app for those files: macOS's `CFBundleDocumentTypes` and type
//! declarations, Linux's `.desktop` `MimeType` and shared-mime-info XML, and
//! the Windows installer's ProgIDs.
//!
//! Everything is written here as text, tested on every platform; the bundle
//! for each host puts it where it goes.

use std::path::PathBuf;

use serde::Deserialize;

/// `[[bundle.documents]]`, as written.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct RawDocument {
    name: String,
    extensions: Vec<String>,
    mime: Option<String>,
    role: Option<String>,
    icon: Option<String>,
    uti: Option<String>,
    owned: Option<bool>,
}

/// Whether the app edits a document type or only shows it (macOS's
/// `CFBundleTypeRole`; the other platforms have no equivalent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Editor,
    Viewer,
}

/// A document type, resolved: defaults filled in, extensions checked.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentType {
    /// What the OS calls it ("Kind: Kver document").
    pub name: String,
    /// Lowercase, without the dot; at least one.
    pub extensions: Vec<String>,
    pub mime: String,
    pub role: Role,
    /// A document icon (resolved against the project root), if one exists.
    pub icon: Option<PathBuf>,
    /// macOS's type identifier: given, or `<identifier>.<first extension>`
    /// for a type the app owns. `None` for a type it doesn't own and gave no
    /// identifier for: macOS matches it by extension.
    pub uti: Option<String>,
    /// The app defines the type (exported, the default handler), or only
    /// opens a type defined elsewhere (`md`, `txt`: offered in "Open With").
    pub owned: bool,
}

impl DocumentType {
    /// The Windows ProgID: `<identifier>.<first extension>`.
    pub fn prog_id(&self, identifier: &str) -> String {
        format!("{identifier}.{}", self.extensions[0])
    }

    // Linux packaging: exercised by unit tests on every platform.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    /// The MIME type as an icon name (`application/x-kver` →
    /// `application-x-kver`), which is how Linux finds a type's icon.
    pub fn mime_icon_name(&self) -> String {
        self.mime.replace('/', "-")
    }
}

/// Resolve `[[bundle.documents]]`: defaults, and the mistakes that would make
/// a broken registration said up front.
pub fn resolve(
    raw: Vec<RawDocument>,
    root: &std::path::Path,
    identifier: &str,
    app_crate: &str,
) -> Result<Vec<DocumentType>, String> {
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for doc in raw {
        let what = format!("[[bundle.documents]] \"{}\"", doc.name);
        if doc.name.trim().is_empty() {
            return Err("[[bundle.documents]]: `name` is empty".into());
        }
        if doc.extensions.is_empty() {
            return Err(format!("{what}: `extensions` is empty"));
        }
        let mut extensions = Vec::new();
        for ext in &doc.extensions {
            let valid = !ext.is_empty()
                && ext
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
            if !valid {
                return Err(format!(
                    "{what}: extension \"{ext}\" should be lowercase letters, digits, `-` or `_`, \
                     without the dot (\"kver\", not \".kver\")"
                ));
            }
            if seen.contains(ext) {
                return Err(format!("{what}: extension \"{ext}\" is declared twice"));
            }
            seen.push(ext.clone());
            extensions.push(ext.clone());
        }
        let role = match doc.role.as_deref() {
            None | Some("editor") => Role::Editor,
            Some("viewer") => Role::Viewer,
            Some(other) => {
                return Err(format!(
                    "{what}: role \"{other}\" should be \"editor\" or \"viewer\""
                ))
            }
        };
        let owned = doc.owned.unwrap_or(true);
        let mime = match doc.mime {
            Some(mime) if mime.split('/').count() == 2 && !mime.contains(char::is_whitespace) => {
                mime
            }
            Some(mime) => return Err(format!("{what}: mime \"{mime}\" isn't `type/subtype`")),
            None => format!(
                "application/x-{}-{}",
                app_crate.replace('_', "-"),
                extensions[0]
            ),
        };
        let uti = doc
            .uti
            .or_else(|| owned.then(|| format!("{identifier}.{}", extensions[0])));
        // A path that isn't there is a mistake to say, not to bundle past.
        let icon = match doc.icon {
            Some(path) => {
                let full = root.join(&path);
                if !full.is_file() {
                    return Err(format!(
                        "{what}: icon \"{path}\" isn't a file (relative to {})",
                        root.display()
                    ));
                }
                Some(full)
            }
            None => None,
        };
        out.push(DocumentType {
            name: doc.name,
            extensions,
            mime,
            role,
            icon,
            uti,
            owned,
        });
    }
    Ok(out)
}

/// `text` as XML character data or an attribute value.
fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// macOS: the `Info.plist` keys (indented for its top `<dict>`), empty
/// without document types. `icons[i]` is the icon file stem written for
/// `docs[i]`, if one was.
pub fn plist(docs: &[DocumentType], icons: &[Option<String>]) -> String {
    if docs.is_empty() {
        return String::new();
    }
    let mut s = String::from("\n    <key>CFBundleDocumentTypes</key>\n    <array>");
    for (i, doc) in docs.iter().enumerate() {
        s.push_str("\n        <dict>");
        s.push_str(&format!(
            "\n            <key>CFBundleTypeName</key><string>{}</string>",
            xml(&doc.name)
        ));
        s.push_str(&format!(
            "\n            <key>CFBundleTypeRole</key><string>{}</string>",
            match doc.role {
                Role::Editor => "Editor",
                Role::Viewer => "Viewer",
            }
        ));
        s.push_str(&format!(
            "\n            <key>LSHandlerRank</key><string>{}</string>",
            if doc.owned { "Owner" } else { "Alternate" }
        ));
        match &doc.uti {
            Some(uti) => s.push_str(&format!(
                "\n            <key>LSItemContentTypes</key>\n            <array><string>{}</string></array>",
                xml(uti)
            )),
            // A type the app doesn't own and named no identifier for: matched
            // by extension.
            None => {
                s.push_str("\n            <key>CFBundleTypeExtensions</key>\n            <array>");
                for ext in &doc.extensions {
                    s.push_str(&format!("<string>{}</string>", xml(ext)));
                }
                s.push_str("</array>");
            }
        }
        if let Some(Some(icon)) = icons.get(i) {
            s.push_str(&format!(
                "\n            <key>CFBundleTypeIconFile</key><string>{}</string>",
                xml(icon)
            ));
        }
        s.push_str("\n        </dict>");
    }
    s.push_str("\n    </array>");

    for (key, owned) in [
        ("UTExportedTypeDeclarations", true),
        ("UTImportedTypeDeclarations", false),
    ] {
        let declared: Vec<(usize, &DocumentType)> = docs
            .iter()
            .enumerate()
            .filter(|(_, d)| d.owned == owned && d.uti.is_some())
            .collect();
        if declared.is_empty() {
            continue;
        }
        s.push_str(&format!("\n    <key>{key}</key>\n    <array>"));
        for (i, doc) in declared {
            s.push_str("\n        <dict>");
            s.push_str(&format!(
                "\n            <key>UTTypeIdentifier</key><string>{}</string>",
                xml(doc.uti.as_deref().unwrap_or_default())
            ));
            s.push_str(&format!(
                "\n            <key>UTTypeDescription</key><string>{}</string>",
                xml(&doc.name)
            ));
            s.push_str(
                "\n            <key>UTTypeConformsTo</key>\n            <array><string>public.data</string><string>public.content</string></array>",
            );
            if let Some(Some(icon)) = icons.get(i) {
                s.push_str(&format!(
                    "\n            <key>UTTypeIconFile</key><string>{}</string>",
                    xml(icon)
                ));
            }
            s.push_str("\n            <key>UTTypeTagSpecification</key>\n            <dict>");
            s.push_str(
                "\n                <key>public.filename-extension</key>\n                <array>",
            );
            for ext in &doc.extensions {
                s.push_str(&format!("<string>{}</string>", xml(ext)));
            }
            s.push_str("</array>");
            s.push_str(&format!(
                "\n                <key>public.mime-type</key><string>{}</string>",
                xml(&doc.mime)
            ));
            s.push_str("\n            </dict>");
            s.push_str("\n        </dict>");
        }
        s.push_str("\n    </array>");
    }
    s
}

/// Linux: the `.desktop` entry's `MimeType` values for the document types
/// (each followed by `;`), to go after any scheme handler.
pub fn desktop_mime_types(docs: &[DocumentType]) -> String {
    docs.iter().map(|d| format!("{};", d.mime)).collect()
}

// Linux packaging: exercised by unit tests on every platform.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
/// Linux: the shared-mime-info XML for the types the app owns
/// (`/usr/share/mime/packages/<identifier>.xml`), or `None` when it owns
/// none: a type defined elsewhere is the system's to describe.
pub fn mime_xml(docs: &[DocumentType]) -> Option<String> {
    let owned: Vec<&DocumentType> = docs.iter().filter(|d| d.owned).collect();
    if owned.is_empty() {
        return None;
    }
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">\n",
    );
    for doc in owned {
        s.push_str(&format!("  <mime-type type=\"{}\">\n", xml(&doc.mime)));
        s.push_str(&format!("    <comment>{}</comment>\n", xml(&doc.name)));
        for ext in &doc.extensions {
            s.push_str(&format!("    <glob pattern=\"*.{}\"/>\n", xml(ext)));
        }
        s.push_str("  </mime-type>\n");
    }
    s.push_str("</mime-info>\n");
    Some(s)
}

// Linux packaging: exercised by unit tests on every platform.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
/// Linux: the `.deb`'s `postinst` and `postrm`, which refresh the MIME,
/// desktop and icon caches so the types take effect without a log-out. Each
/// tool may be missing on a minimal system; that's no reason to fail.
pub fn deb_scripts() -> (String, String) {
    let body = "#!/bin/sh\nset -e\n\
                if command -v update-mime-database >/dev/null 2>&1; then update-mime-database /usr/share/mime || true; fi\n\
                if command -v update-desktop-database >/dev/null 2>&1; then update-desktop-database -q /usr/share/applications || true; fi\n\
                if command -v gtk-update-icon-cache >/dev/null 2>&1; then gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true; fi\n\
                exit 0\n";
    (body.to_string(), body.to_string())
}

/// Windows: the installer's registry lines for each type, and the
/// uninstaller's that remove them. `exe` is the installed executable's name;
/// `icons[i]` is the `.ico` installed beside it for `docs[i]`, if any.
pub fn nsis(
    docs: &[DocumentType],
    identifier: &str,
    exe: &str,
    icons: &[Option<String>],
    q: impl Fn(&str) -> String,
    escaped: impl Fn(&str) -> String,
) -> (Vec<String>, Vec<String>) {
    let mut install = Vec::new();
    let mut uninstall = Vec::new();
    for (i, doc) in docs.iter().enumerate() {
        let prog_id = doc.prog_id(identifier);
        let key = format!("Software\\Classes\\{prog_id}");
        install.push(format!(
            "  WriteRegStr HKCU {} \"\" {}",
            q(&key),
            q(&doc.name)
        ));
        let icon = match icons.get(i) {
            Some(Some(ico)) => format!("'\"$INSTDIR\\{}\"'", escaped(ico)),
            _ => format!("'\"$INSTDIR\\{}\",0'", escaped(exe)),
        };
        install.push(format!(
            "  WriteRegStr HKCU {} \"\" {icon}",
            q(&format!("{key}\\DefaultIcon"))
        ));
        install.push(format!(
            "  WriteRegStr HKCU {} \"\" '\"$INSTDIR\\{}\" \"%1\"'",
            q(&format!("{key}\\shell\\open\\command")),
            escaped(exe)
        ));
        uninstall.push(format!("  DeleteRegKey HKCU {}", q(&key)));
        for ext in &doc.extensions {
            let ext_key = format!("Software\\Classes\\.{ext}");
            let with = format!("{ext_key}\\OpenWithProgids");
            install.push(format!(
                "  WriteRegStr HKCU {} {} \"\"",
                q(&with),
                q(&prog_id)
            ));
            if doc.owned {
                install.push(format!(
                    "  WriteRegStr HKCU {} \"\" {}",
                    q(&ext_key),
                    q(&prog_id)
                ));
                // Ours only while it still points at us: another app may
                // have taken the extension since.
                uninstall.push(format!("  ReadRegStr $0 HKCU {} \"\"", q(&ext_key)));
                uninstall.push(format!("  StrCmp $0 {} 0 +2", q(&prog_id)));
                uninstall.push(format!("  DeleteRegValue HKCU {} \"\"", q(&ext_key)));
            }
            uninstall.push(format!(
                "  DeleteRegValue HKCU {} {}",
                q(&with),
                q(&prog_id)
            ));
            uninstall.push(format!("  DeleteRegKey /ifempty HKCU {}", q(&with)));
            uninstall.push(format!("  DeleteRegKey /ifempty HKCU {}", q(&ext_key)));
        }
    }
    if !docs.is_empty() {
        // SHCNE_ASSOCCHANGED: Explorer re-reads the associations.
        let notify =
            "  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'".to_string();
        install.push(notify.clone());
        uninstall.push(notify);
    }
    (install, uninstall)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(toml_text: &str) -> Vec<RawDocument> {
        #[derive(Deserialize)]
        struct Wrap {
            documents: Vec<RawDocument>,
        }
        toml::from_str::<Wrap>(toml_text).unwrap().documents
    }

    fn kver() -> Vec<DocumentType> {
        resolve(
            raw(r#"
                [[documents]]
                name = "Kver document"
                extensions = ["kver"]
                mime = "application/vnd.kver.document+json"

                [[documents]]
                name = "Markdown"
                extensions = ["md", "markdown"]
                mime = "text/markdown"
                role = "viewer"
                owned = false
            "#),
            std::path::Path::new("/tmp/project"),
            "no.gets.kver",
            "kver",
        )
        .unwrap()
    }

    #[test]
    fn defaults_fill_in_and_a_type_it_doesnt_own_has_no_made_up_identifier() {
        let docs = resolve(
            raw("[[documents]]\nname = \"Note\"\nextensions = [\"elyranote\"]\n"),
            std::path::Path::new("/tmp"),
            "com.example.notes",
            "my_notes",
        )
        .unwrap();
        assert_eq!(docs[0].mime, "application/x-my-notes-elyranote");
        assert_eq!(docs[0].uti.as_deref(), Some("com.example.notes.elyranote"));
        assert_eq!(docs[0].role, Role::Editor);
        assert!(docs[0].owned);
        let docs = kver();
        assert_eq!(docs[1].uti, None);
        assert_eq!(docs[1].role, Role::Viewer);
        assert_eq!(docs[0].prog_id("no.gets.kver"), "no.gets.kver.kver");
        assert_eq!(
            docs[0].mime_icon_name(),
            "application-vnd.kver.document+json"
        );
    }

    #[test]
    fn a_declaration_that_would_register_wrong_is_refused() {
        let root = std::path::Path::new("/tmp");
        let err = |text: &str| resolve(raw(text), root, "id", "app").unwrap_err();
        assert!(
            err("[[documents]]\nname = \"N\"\nextensions = [\".kver\"]\n")
                .contains("without the dot")
        );
        assert!(
            err("[[documents]]\nname = \"N\"\nextensions = [\"KVER\"]\n").contains("lowercase")
        );
        assert!(err("[[documents]]\nname = \"N\"\nextensions = []\n").contains("is empty"));
        assert!(
            err("[[documents]]\nname = \"N\"\nextensions = [\"a\"]\nrole = \"owner\"\n")
                .contains("editor")
        );
        assert!(
            err("[[documents]]\nname = \"N\"\nextensions = [\"a\"]\nmime = \"text\"\n")
                .contains("type/subtype")
        );
        assert!(err(
            "[[documents]]\nname = \"N\"\nextensions = [\"a\"]\n[[documents]]\nname = \"M\"\nextensions = [\"a\"]\n"
        )
        .contains("declared twice"));
        // An icon that isn't there is said, not bundled past.
        assert!(err(
            "[[documents]]\nname = \"N\"\nextensions = [\"a\"]\nicon = \"assets/nope.svg\"\n"
        )
        .contains("icon \"assets/nope.svg\" isn't a file"),);
        assert!(toml::from_str::<toml::Value>("x = 1").is_ok());
        assert!(
            toml::from_str::<RawDocument>(
                "name = \"N\"\nextensions = [\"a\"]\nextension = \"b\"\n"
            )
            .is_err(),
            "a misspelt key is an error, not ignored"
        );
    }

    #[test]
    fn the_plist_declares_and_exports_what_the_app_owns_and_imports_nothing_made_up() {
        let plist = plist(&kver(), &[Some("kver-doc".into()), None]);
        assert!(plist.contains("<key>CFBundleDocumentTypes</key>"));
        assert!(plist.contains("<key>CFBundleTypeName</key><string>Kver document</string>"));
        assert!(plist.contains("<key>LSHandlerRank</key><string>Owner</string>"));
        assert!(plist.contains("<array><string>no.gets.kver.kver</string></array>"));
        assert!(plist.contains("<key>CFBundleTypeIconFile</key><string>kver-doc</string>"));
        // Markdown: by extension, an alternate handler, a viewer.
        assert!(plist.contains("<key>CFBundleTypeRole</key><string>Viewer</string>"));
        assert!(plist.contains("<key>LSHandlerRank</key><string>Alternate</string>"));
        assert!(plist.contains("<string>md</string><string>markdown</string>"));
        assert!(plist.contains("<key>UTExportedTypeDeclarations</key>"));
        assert!(plist.contains(
            "<key>public.mime-type</key><string>application/vnd.kver.document+json</string>"
        ));
        assert!(!plist.contains("UTImportedTypeDeclarations"));
        assert_eq!(plist.matches("<key>UTTypeIdentifier</key>").count(), 1);
        assert_eq!(super::plist(&[], &[]), "");
    }

    #[test]
    fn a_type_it_doesnt_own_but_names_is_imported() {
        let mut docs = kver();
        docs[1].uti = Some("net.daringfireball.markdown".into());
        let plist = plist(&docs, &[]);
        assert!(plist.contains("<key>UTImportedTypeDeclarations</key>"));
        assert!(plist.contains("<array><string>net.daringfireball.markdown</string></array>"));
    }

    #[test]
    fn linux_gets_the_mime_types_and_xml_for_what_the_app_owns() {
        let docs = kver();
        assert_eq!(
            desktop_mime_types(&docs),
            "application/vnd.kver.document+json;text/markdown;"
        );
        let xml = mime_xml(&docs).unwrap();
        assert!(xml.contains("<mime-type type=\"application/vnd.kver.document+json\">"));
        assert!(xml.contains("<glob pattern=\"*.kver\"/>"));
        assert!(xml.contains("<comment>Kver document</comment>"));
        assert!(
            !xml.contains("text/markdown"),
            "the system describes markdown"
        );
        assert_eq!(mime_xml(&docs[1..]), None);
        let (postinst, postrm) = deb_scripts();
        assert!(postinst.starts_with("#!/bin/sh\n"));
        assert!(postinst.contains("update-mime-database /usr/share/mime"));
        assert_eq!(postinst, postrm);
    }

    #[test]
    fn windows_registers_a_prog_id_and_removes_only_what_is_still_its_own() {
        let q = |t: &str| format!("\"{t}\"");
        let e = |t: &str| t.to_string();
        let (install, uninstall) = nsis(
            &kver(),
            "no.gets.kver",
            "kver.exe",
            &[Some("kver-doc.ico".into()), None],
            q,
            e,
        );
        let install = install.join("\n");
        let uninstall = uninstall.join("\n");
        assert!(install.contains(
            r#"WriteRegStr HKCU "Software\Classes\no.gets.kver.kver" "" "Kver document""#
        ));
        assert!(install.contains(r#"WriteRegStr HKCU "Software\Classes\no.gets.kver.kver\DefaultIcon" "" '"$INSTDIR\kver-doc.ico"'"#));
        assert!(install.contains(r#"WriteRegStr HKCU "Software\Classes\no.gets.kver.md\DefaultIcon" "" '"$INSTDIR\kver.exe",0'"#));
        assert!(install.contains(r#"WriteRegStr HKCU "Software\Classes\no.gets.kver.kver\shell\open\command" "" '"$INSTDIR\kver.exe" "%1"'"#));
        assert!(
            install.contains(r#"WriteRegStr HKCU "Software\Classes\.kver" "" "no.gets.kver.kver""#)
        );
        assert!(install.contains(
            r#"WriteRegStr HKCU "Software\Classes\.md\OpenWithProgids" "no.gets.kver.md" """#
        ));
        assert!(
            !install.contains(r#"WriteRegStr HKCU "Software\Classes\.md" """#),
            "markdown stays the user's"
        );
        assert!(install.contains("SHChangeNotify(i 0x08000000"));
        assert!(uninstall.contains(r#"DeleteRegKey HKCU "Software\Classes\no.gets.kver.kver""#));
        assert!(uninstall.contains(r#"StrCmp $0 "no.gets.kver.kver" 0 +2"#));
        assert!(uninstall.contains(r#"DeleteRegKey /ifempty HKCU "Software\Classes\.markdown""#));
        assert!(uninstall.contains("SHChangeNotify"));
        let (none, _) = nsis(&[], "id", "a.exe", &[], q, e);
        assert!(none.is_empty());
    }
}
