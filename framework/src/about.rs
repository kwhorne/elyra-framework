//! Application "About" metadata — the data behind the framework's built-in
//! About dialog.
//!
//! Set it once on the [`App`] builder; the shell serves it at the private
//! `/__about` endpoint and `@elyra/runtime` renders a themed dialog from it.
//! On macOS the standard **About <App>** menu item opens the same dialog (the
//! shell emits an `elyra:about` event that the runtime listens for).
//!
//! ```ignore
//! App::new()
//!     .title("BlogWriter")
//!     .about(
//!         AboutInfo::new("BlogWriter", env!("CARGO_PKG_VERSION"))
//!             .description("Generate and publish blog articles on a schedule.")
//!             .website("elyracode.com")
//!             .repository("github.com/kwhorne/blogwriter")
//!             .author("Knut W. Horne", "kwhorne.com")
//!             .icon("/icon.svg"),
//!     )
//!     .run()
//! ```
//!
//! [`App`]: crate::App

use serde::Serialize;

/// Metadata shown in the built-in About dialog.
///
/// Every field is optional except `name` and `version`; unset rows are simply
/// omitted from the dialog. Build it fluently with the setters.
#[derive(Clone, Debug, Default, Serialize)]
pub struct AboutInfo {
    /// Application display name (e.g. `"BlogWriter"`).
    pub name: String,
    /// Version string (typically `env!("CARGO_PKG_VERSION")`).
    pub version: String,
    /// One or two sentences describing the app.
    pub description: String,
    /// Marketing / docs URL, shown in the "Website" row.
    pub website: Option<String>,
    /// Source URL, shown in the "GitHub" row.
    pub repository: Option<String>,
    /// Author name, shown in the "Developed by" row.
    pub author: Option<String>,
    /// Author URL, shown next to the author name.
    pub author_url: Option<String>,
    /// Path/URL to an icon asset (e.g. `"/icon.svg"`). Falls back to a built-in
    /// Elyra mark when unset.
    pub icon: Option<String>,
}

impl AboutInfo {
    /// Start building About metadata with the required name + version.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            ..Default::default()
        }
    }

    /// Set the description shown under the title.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set the "Website" row URL.
    pub fn website(mut self, url: impl Into<String>) -> Self {
        self.website = Some(url.into());
        self
    }

    /// Set the "GitHub" row URL.
    pub fn repository(mut self, url: impl Into<String>) -> Self {
        self.repository = Some(url.into());
        self
    }

    /// Set the "Developed by" row (name + URL).
    pub fn author(mut self, name: impl Into<String>, url: impl Into<String>) -> Self {
        self.author = Some(name.into());
        self.author_url = Some(url.into());
        self
    }

    /// Set the icon asset path/URL (e.g. `"/icon.svg"`).
    pub fn icon(mut self, path: impl Into<String>) -> Self {
        self.icon = Some(path.into());
        self
    }

    /// MessagePack-encode as a named map (object on the JS side).
    pub(crate) fn to_msgpack(&self) -> Vec<u8> {
        rmp_serde::to_vec_named(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_sets_fields() {
        let about = AboutInfo::new("App", "1.2.3")
            .description("Desc")
            .website("example.com")
            .repository("github.com/x/y")
            .author("Me", "me.com")
            .icon("/icon.svg");
        assert_eq!(about.name, "App");
        assert_eq!(about.version, "1.2.3");
        assert_eq!(about.author.as_deref(), Some("Me"));
        assert_eq!(about.author_url.as_deref(), Some("me.com"));
        assert_eq!(about.icon.as_deref(), Some("/icon.svg"));
    }

    #[test]
    fn encodes_as_a_msgpack_map() {
        let bytes = AboutInfo::new("App", "1.0").to_msgpack();
        assert!(!bytes.is_empty());
        // A small map (<16 entries) is encoded with a 0x8n fixmap marker, so the
        // JS side decodes it to an object rather than an array.
        assert_eq!(bytes[0] & 0xf0, 0x80, "expected a fixmap marker");
    }
}
