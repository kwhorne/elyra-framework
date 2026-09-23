//! Translations — Laravel's `__()` / `trans_choice()`, for both halves of the app.
//!
//! Messages live in one JSON file per locale (`lang/en.json`, `lang/nb.json`),
//! nested or flat:
//!
//! ```json
//! { "welcome": "Hello, :name!",
//!   "apples": "{0} No apples|{1} One apple|[2,*] :count apples",
//!   "nav": { "home": "Home" } }
//! ```
//!
//! ```ignore
//! #[derive(rust_embed::RustEmbed)]
//! #[folder = "lang/"]
//! struct Lang;
//!
//! App::new().provider(I18nProvider::embedded::<Lang>().fallback("en"));
//!
//! let t = ctx.get::<Translator>();
//! t.get("welcome", &[("name", "Ada")]);   // "Hei, Ada!" in nb
//! t.choice("apples", 3, &[]);             // "3 epler"
//! t.get("nav.home", &[]);                 // nested keys use dots
//! t.set_locale("nb");                     // remembered across launches; emits `elyra:locale`
//! ```
//!
//! The frontend gets the same catalog through `$t(..)` in `@elyra/runtime`.
//!
//! **Lookup:** the current locale (`nb-NO`), then its language (`nb`), then the
//! fallback locale. A key found nowhere returns the key itself, as in Laravel, so
//! a missing translation is visible rather than blank.
//!
//! **Placeholders:** `:name`, and `:Name` / `:NAME` for a capitalised /
//! upper-cased value.
//!
//! **Plurals:** segments separated by `|`. A segment may start with an exact
//! count `{0}` or a range `[2,*]` / `[1,19]`; otherwise the locale's plural rule
//! picks the segment (`"apple|apples"` in English; one form in Japanese; three
//! in Russian or Polish) — the same rules Laravel uses.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use parking_lot::RwLock;
use serde_json::Value;

type OnChange = Arc<dyn Fn(&str) + Send + Sync>;

/// Translation catalogs plus the current locale.
pub struct Translator {
    /// locale -> flattened key -> message.
    catalogs: HashMap<String, BTreeMap<String, String>>,
    fallback: String,
    current: RwLock<String>,
    on_change: RwLock<Option<OnChange>>,
}

impl Default for Translator {
    fn default() -> Self {
        Self::new("en")
    }
}

impl Translator {
    /// No catalogs yet; `fallback` is the locale used when a key is missing.
    pub fn new(fallback: &str) -> Self {
        let fallback = normalize(fallback);
        Self {
            catalogs: HashMap::new(),
            current: RwLock::new(fallback.clone()),
            fallback,
            on_change: RwLock::new(None),
        }
    }

    /// Add (or merge into) a locale's catalog from a JSON object.
    ///
    /// # Panics
    /// If `json` isn't a JSON object — a broken translation file is wiring.
    pub fn add_json(mut self, locale: &str, json: &str) -> Self {
        let value: Value = serde_json::from_str(json)
            .unwrap_or_else(|e| panic!("translations for `{locale}` are not valid JSON: {e}"));
        let Value::Object(_) = value else {
            panic!("translations for `{locale}` must be a JSON object");
        };
        let catalog = self.catalogs.entry(normalize(locale)).or_default();
        flatten("", &value, catalog);
        self
    }

    /// Load every `<locale>.json` in a directory.
    pub fn from_dir(fallback: &str, dir: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        let mut t = Self::new(fallback);
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Some(locale) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let json = std::fs::read_to_string(&path)?;
            t = t.add_json(locale, &json);
        }
        Ok(t)
    }

    /// Load every `<locale>.json` from a `#[derive(RustEmbed)]` folder, so the
    /// translations ship inside the binary.
    pub fn embedded<E: rust_embed::RustEmbed>(fallback: &str) -> Self {
        let mut t = Self::new(fallback);
        for name in E::iter() {
            let Some(locale) = name.strip_suffix(".json") else {
                continue;
            };
            if let Some(file) = E::get(&name) {
                let json = String::from_utf8_lossy(&file.data).into_owned();
                t = t.add_json(locale, &json);
            }
        }
        t
    }

    /// The current locale, e.g. `nb-no`.
    pub fn locale(&self) -> String {
        self.current.read().clone()
    }

    /// The fallback locale.
    pub fn fallback(&self) -> &str {
        &self.fallback
    }

    /// Locales that have a catalog, sorted.
    pub fn available(&self) -> Vec<String> {
        let mut locales: Vec<String> = self.catalogs.keys().cloned().collect();
        locales.sort();
        locales
    }

    /// Switch locale. With [`I18nProvider`] this is remembered across launches
    /// and announced on `elyra:locale`, so the frontend re-renders.
    pub fn set_locale(&self, locale: &str) {
        let locale = normalize(locale);
        if !self
            .candidates(&locale)
            .iter()
            .any(|c| self.catalogs.contains_key(c))
        {
            crate::warn!(
                target: "elyra::i18n",
                "no translations for `{locale}`; falling back to `{}`",
                self.fallback
            );
        }
        *self.current.write() = locale.clone();
        if let Some(hook) = self.on_change.read().clone() {
            hook(&locale);
        }
    }

    pub(crate) fn on_change(&self, hook: impl Fn(&str) + Send + Sync + 'static) {
        *self.on_change.write() = Some(Arc::new(hook));
    }

    /// `nb-no` -> [`nb-no`, `nb`]; the fallback is tried after these.
    fn candidates(&self, locale: &str) -> Vec<String> {
        let mut out = vec![locale.to_owned()];
        if let Some((language, _)) = locale.split_once('-') {
            out.push(language.to_owned());
        }
        out
    }

    /// The lookup chain for the current locale, most specific first.
    fn chain(&self) -> Vec<String> {
        let mut chain = self.candidates(&self.locale());
        for c in self.candidates(&self.fallback) {
            if !chain.contains(&c) {
                chain.push(c);
            }
        }
        chain
    }

    fn raw(&self, key: &str) -> Option<&str> {
        self.chain()
            .iter()
            .find_map(|locale| self.catalogs.get(locale)?.get(key))
            .map(String::as_str)
    }

    /// The message for `key` with `:placeholders` filled in, or the key itself.
    pub fn get(&self, key: &str, params: &[(&str, &str)]) -> String {
        match self.raw(key) {
            Some(message) => replace(message, params),
            None => key.to_owned(),
        }
    }

    /// The plural form of `key` for `count` (also available as `:count`).
    pub fn choice(&self, key: &str, count: i64, params: &[(&str, &str)]) -> String {
        let Some(message) = self.raw(key) else {
            return key.to_owned();
        };
        let segment = select_plural(message, count, &self.locale());
        let count_text = count.to_string();
        let mut all: Vec<(&str, &str)> = vec![("count", &count_text)];
        all.extend_from_slice(params);
        replace(&segment, &all)
    }

    /// Every message the current locale resolves to — fallback first, then the
    /// language, then the exact locale on top — for the frontend.
    pub fn resolved(&self) -> BTreeMap<String, String> {
        let mut merged = BTreeMap::new();
        for locale in self.chain().iter().rev() {
            if let Some(catalog) = self.catalogs.get(locale) {
                merged.extend(catalog.iter().map(|(k, v)| (k.clone(), v.clone())));
            }
        }
        merged
    }
}

/// `nb_NO.UTF-8` / `NB-no` -> `nb-no`.
fn normalize(tag: &str) -> String {
    let tag = tag.split(['.', '@']).next().unwrap_or(tag);
    tag.trim().replace('_', "-").to_ascii_lowercase()
}

fn flatten(prefix: &str, value: &Value, out: &mut BTreeMap<String, String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(&path, child, out);
            }
        }
        Value::String(s) => {
            out.insert(prefix.to_owned(), s.clone());
        }
        Value::Number(n) => {
            out.insert(prefix.to_owned(), n.to_string());
        }
        Value::Bool(b) => {
            out.insert(prefix.to_owned(), b.to_string());
        }
        Value::Null | Value::Array(_) => {}
    }
}

/// Fill `:name`, `:Name` and `:NAME`. Longer names first, so `:name` can't eat
/// the start of `:namespace`.
fn replace(message: &str, params: &[(&str, &str)]) -> String {
    let mut params: Vec<&(&str, &str)> = params.iter().collect();
    params.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
    let mut out = message.to_owned();
    for (name, value) in params {
        let mut upper_first = value.chars();
        let capitalised = match upper_first.next() {
            Some(c) => c.to_uppercase().chain(upper_first).collect(),
            None => String::new(),
        };
        let mut cap_name = name.chars();
        let cap_key = match cap_name.next() {
            Some(c) => c.to_uppercase().chain(cap_name).collect::<String>(),
            None => continue,
        };
        out = out
            .replace(&format!(":{}", name.to_uppercase()), &value.to_uppercase())
            .replace(&format!(":{cap_key}"), &capitalised)
            .replace(&format!(":{name}"), value);
    }
    out
}

/// Pick the segment of a plural message for `count` in `locale`.
fn select_plural(message: &str, count: i64, locale: &str) -> String {
    let segments: Vec<&str> = message.split('|').collect();
    // Explicit `{n}` / `[a,b]` conditions win.
    for segment in &segments {
        if let Some((cond, text)) = condition(segment) {
            if matches_condition(cond, count) {
                return text.trim().to_owned();
            }
        }
    }
    let plain: Vec<&str> = segments
        .iter()
        .map(|s| condition(s).map_or(*s, |(_, text)| text).trim())
        .collect();
    let index = plural_index(locale, count).min(plain.len().saturating_sub(1));
    plain.get(index).copied().unwrap_or_default().to_owned()
}

/// `{0} none` -> (`0`, ` none`); `[2,*] many` -> (`2,*`, ` many`).
fn condition(segment: &str) -> Option<(&str, &str)> {
    let s = segment.trim_start();
    let close = match s.chars().next()? {
        '{' => '}',
        '[' => ']',
        _ => return None,
    };
    let end = s.find(close)?;
    Some((&s[1..end], &s[end + 1..]))
}

fn matches_condition(cond: &str, count: i64) -> bool {
    let bound = |s: &str| -> Option<Option<i64>> {
        match s.trim() {
            "*" => Some(None),
            n => n.parse().ok().map(Some),
        }
    };
    match cond.split_once(',') {
        Some((from, to)) => {
            let (Some(from), Some(to)) = (bound(from), bound(to)) else {
                return false;
            };
            from.is_none_or(|f| count >= f) && to.is_none_or(|t| count <= t)
        }
        None => cond.trim().parse::<i64>().is_ok_and(|n| n == count),
    }
}

/// Which plural form a count takes — Laravel's `MessageSelector` rules, keyed
/// by language (`pt-br` is its own case). Languages not listed use the common
/// one/other split.
fn plural_index(locale: &str, n: i64) -> usize {
    let n = n.unsigned_abs();
    let language = locale.split('-').next().unwrap_or(locale);
    let (m10, m100) = (n % 10, n % 100);
    match (locale, language) {
        ("pt-br", _) => usize::from(n > 1),
        (
            _,
            "az" | "bo" | "dz" | "id" | "ja" | "jv" | "ka" | "km" | "kn" | "ko" | "ms" | "th"
            | "tr" | "vi" | "zh",
        ) => 0,
        (
            _,
            "am" | "bh" | "fil" | "fr" | "gun" | "hi" | "hy" | "ln" | "mg" | "nso" | "ti" | "wa",
        ) => usize::from(n > 1),
        (_, "be" | "bs" | "hr" | "ru" | "sh" | "sr" | "uk") => {
            if m10 == 1 && m100 != 11 {
                0
            } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
                1
            } else {
                2
            }
        }
        (_, "cs" | "sk") => match n {
            1 => 0,
            2..=4 => 1,
            _ => 2,
        },
        (_, "ga") => match n {
            1 => 0,
            2 => 1,
            _ => 2,
        },
        (_, "lt") => {
            if m10 == 1 && m100 != 11 {
                0
            } else if m10 >= 2 && !(10..20).contains(&m100) {
                1
            } else {
                2
            }
        }
        (_, "sl") => match m100 {
            1 => 0,
            2 => 1,
            3 | 4 => 2,
            _ => 3,
        },
        (_, "mk") => usize::from(m10 != 1),
        (_, "lv") => {
            if n == 0 {
                0
            } else if m10 == 1 && m100 != 11 {
                1
            } else {
                2
            }
        }
        (_, "pl") => {
            if n == 1 {
                0
            } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
                1
            } else {
                2
            }
        }
        (_, "ro") => {
            if n == 1 {
                0
            } else if n == 0 || (1..20).contains(&m100) {
                1
            } else {
                2
            }
        }
        (_, "ar") => match n {
            0 => 0,
            1 => 1,
            2 => 2,
            _ if (3..=10).contains(&m100) => 3,
            _ if (11..=99).contains(&m100) => 4,
            _ => 5,
        },
        _ => usize::from(n != 1),
    }
}

/// Where the catalogs come from.
enum Source {
    Embedded(Box<dyn Fn(&str) -> Translator + Send + Sync>),
    Dir(std::path::PathBuf),
    Ready(std::sync::Mutex<Option<Translator>>),
}

/// A [`Provider`](crate::Provider) that binds a [`Translator`], picks the
/// locale (a saved choice, else the OS language, else the fallback), remembers
/// `set_locale` in the [`Store`](crate::Store), and announces changes on
/// `elyra:locale`.
pub struct I18nProvider {
    source: Source,
    fallback: String,
    locale: Option<String>,
}

impl I18nProvider {
    /// Translations embedded with `#[derive(RustEmbed)]`.
    pub fn embedded<E: rust_embed::RustEmbed + 'static>() -> Self {
        Self::with(Source::Embedded(Box::new(|fallback| {
            Translator::embedded::<E>(fallback)
        })))
    }

    /// Translations read from a directory at startup (handy in development).
    pub fn from_dir(dir: impl Into<std::path::PathBuf>) -> Self {
        Self::with(Source::Dir(dir.into()))
    }

    /// A translator built by hand (tests, generated catalogs).
    pub fn with_translator(translator: Translator) -> Self {
        let fallback = translator.fallback().to_owned();
        let mut provider = Self::with(Source::Ready(std::sync::Mutex::new(Some(translator))));
        provider.fallback = fallback;
        provider
    }

    fn with(source: Source) -> Self {
        Self {
            source,
            fallback: "en".into(),
            locale: None,
        }
    }

    /// The locale to use when a key is missing (default `en`).
    pub fn fallback(mut self, locale: &str) -> Self {
        self.fallback = locale.to_owned();
        self
    }

    /// Start in this locale instead of the saved or system one.
    pub fn locale(mut self, locale: &str) -> Self {
        self.locale = Some(locale.to_owned());
        self
    }
}

/// The `Store` key the chosen locale is saved under.
const STORE_KEY: &str = "elyra.locale";

impl crate::Provider for I18nProvider {
    fn register(&self, container: &mut crate::Container) {
        let translator = match &self.source {
            Source::Embedded(build) => build(&self.fallback),
            Source::Dir(dir) => Translator::from_dir(&self.fallback, dir).unwrap_or_else(|e| {
                panic!("could not read translations from {}: {e}", dir.display())
            }),
            Source::Ready(cell) => cell
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
                .expect("I18nProvider registered twice"),
        };
        container.bind(translator);
    }

    fn boot(&self, ctx: &crate::Ctx) {
        let translator = ctx.get::<Translator>();
        let store = ctx.try_get::<crate::store::Store>();
        let saved = store
            .as_ref()
            .and_then(|s| s.get(STORE_KEY))
            .and_then(|v| v.as_str().map(str::to_owned));
        let initial = self
            .locale
            .clone()
            .or(saved)
            .or_else(sys_locale::get_locale)
            .unwrap_or_else(|| self.fallback.clone());
        *translator.current.write() = normalize(&initial);

        let bus = ctx.try_get::<crate::EventBus>();
        translator.on_change(move |locale| {
            if let Some(store) = &store {
                store.set(STORE_KEY, Value::from(locale));
            }
            if let Some(bus) = &bus {
                let _ = bus.emit("elyra:locale", &locale);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t() -> Translator {
        Translator::new("en")
            .add_json(
                "en",
                r#"{ "welcome": "Hello, :name!", "only_en": "English only",
                     "apples": "{0} No apples|{1} One apple|[2,*] :count apples",
                     "files": "file|files", "nav": { "home": "Home" } }"#,
            )
            .add_json(
                "nb",
                r#"{ "welcome": "Hei, :name!", "apples": "{0} Ingen epler|{1} Ett eple|[2,*] :count epler",
                     "nav": { "home": "Hjem" } }"#,
            )
            .add_json("nb-NO", r#"{ "nav": { "home": "Heim" } }"#)
    }

    #[test]
    fn lookup_falls_back_from_region_to_language_to_fallback() {
        let t = t();
        assert_eq!(t.get("welcome", &[("name", "Ada")]), "Hello, Ada!");
        t.set_locale("nb_NO.UTF-8");
        assert_eq!(t.locale(), "nb-no");
        assert_eq!(t.get("nav.home", &[]), "Heim", "the region wins");
        assert_eq!(
            t.get("welcome", &[("name", "Ada")]),
            "Hei, Ada!",
            "then the language"
        );
        assert_eq!(t.get("only_en", &[]), "English only", "then the fallback");
        assert_eq!(
            t.get("no.such.key", &[]),
            "no.such.key",
            "then the key itself"
        );
    }

    #[test]
    fn placeholders_follow_the_case_they_are_written_in() {
        let t = Translator::new("en").add_json(
            "en",
            r#"{ "m": ":name / :Name / :NAME", "ns": ":namespace then :name" }"#,
        );
        assert_eq!(t.get("m", &[("name", "ada")]), "ada / Ada / ADA");
        assert_eq!(
            t.get("ns", &[("name", "x"), ("namespace", "app")]),
            "app then x",
            ":name must not eat the start of :namespace"
        );
    }

    #[test]
    fn explicit_plural_conditions() {
        let t = t();
        assert_eq!(t.choice("apples", 0, &[]), "No apples");
        assert_eq!(t.choice("apples", 1, &[]), "One apple");
        assert_eq!(t.choice("apples", 7, &[]), "7 apples");
        t.set_locale("nb");
        assert_eq!(t.choice("apples", 7, &[]), "7 epler");
    }

    #[test]
    fn locale_plural_rules_pick_the_form() {
        assert_eq!(select_plural("file|files", 1, "en"), "file");
        assert_eq!(select_plural("file|files", 0, "en"), "files");
        assert_eq!(
            select_plural("fichier|fichiers", 0, "fr"),
            "fichier",
            "French: 0 is singular"
        );
        assert_eq!(
            select_plural("ファイル|x", 5, "ja"),
            "ファイル",
            "Japanese: one form"
        );
        // Russian: 1 файл, 2 файла, 5 файлов, 11 файлов, 21 файл, 22 файла.
        let ru = "файл|файла|файлов";
        let forms: Vec<String> = [1, 2, 5, 11, 21, 22]
            .iter()
            .map(|&n| select_plural(ru, n, "ru"))
            .collect();
        assert_eq!(
            forms,
            ["файл", "файла", "файлов", "файлов", "файл", "файла"]
        );
        // Polish: 1 plik, 2 pliki, 5 plików, 12 plików, 22 pliki.
        let pl = "plik|pliki|plików";
        let forms: Vec<String> = [1, 2, 5, 12, 22]
            .iter()
            .map(|&n| select_plural(pl, n, "pl"))
            .collect();
        assert_eq!(forms, ["plik", "pliki", "plików", "plików", "pliki"]);
        // Fewer segments than forms: the last one covers the rest.
        assert_eq!(select_plural("only", 5, "ru"), "only");
    }

    #[test]
    fn ranges_can_be_bounded_or_open() {
        let msg = "[0,0] none|[1,9] a few|[10,*] lots";
        assert_eq!(select_plural(msg, 0, "en"), "none");
        assert_eq!(select_plural(msg, 9, "en"), "a few");
        assert_eq!(select_plural(msg, 1000, "en"), "lots");
    }

    #[test]
    fn resolved_merges_the_chain_for_the_frontend() {
        let t = t();
        t.set_locale("nb-NO");
        let all = t.resolved();
        assert_eq!(all["nav.home"], "Heim");
        assert_eq!(all["welcome"], "Hei, :name!");
        assert_eq!(all["only_en"], "English only");
    }

    #[test]
    fn set_locale_calls_the_change_hook() {
        let t = t();
        let seen = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let s = seen.clone();
        t.on_change(move |l| s.lock().push(l.to_owned()));
        t.set_locale("NB");
        assert_eq!(*seen.lock(), ["nb"]);
    }

    #[test]
    #[should_panic(expected = "translations for `nb` are not valid JSON")]
    fn a_broken_catalog_fails_loudly() {
        let _ = Translator::new("en").add_json("nb", "{ nope");
    }
}
