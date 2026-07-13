//! The built-in About metadata is carried through `prepare()` and defaults to
//! the primary window title when left unset.

use elyra::{AboutInfo, App};

#[test]
fn about_defaults_to_window_title() {
    let prepared = App::new().title("Fallback Title").prepare();
    assert_eq!(prepared.about.name, "Fallback Title");
}

#[test]
fn about_builder_is_carried_through_prepare() {
    let prepared = App::new()
        .title("Ignored")
        .about(
            AboutInfo::new("MyApp", "9.9.9")
                .description("A test app.")
                .website("example.com")
                .repository("github.com/x/y")
                .author("Knut", "kwhorne.com"),
        )
        .prepare();

    let about = &prepared.about;
    assert_eq!(about.name, "MyApp");
    assert_eq!(about.version, "9.9.9");
    assert_eq!(about.description, "A test app.");
    assert_eq!(about.website.as_deref(), Some("example.com"));
    assert_eq!(about.repository.as_deref(), Some("github.com/x/y"));
    assert_eq!(about.author.as_deref(), Some("Knut"));
    assert_eq!(about.author_url.as_deref(), Some("kwhorne.com"));
}
