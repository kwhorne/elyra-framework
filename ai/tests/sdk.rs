use elyra_ai::{Ai, Message, Provider, Role};

#[test]
fn provider_metadata_matches_defaults() {
    assert_eq!(Provider::Anthropic.default_text_model(), "claude-sonnet-5");
    assert_eq!(Provider::OpenAI.default_text_model(), "gpt-4o");
    assert_eq!(Provider::Anthropic.env_key(), "ANTHROPIC_API_KEY");
    assert_eq!(Provider::OpenAI.env_key(), "OPENAI_API_KEY");
}

#[test]
fn default_provider_is_anthropic() {
    let ai = Ai::builder().build();
    assert_eq!(ai.default_provider(), Provider::Anthropic);
}

#[test]
fn builder_overrides_default_provider_and_model() {
    let ai = Ai::builder()
        .default_provider(Provider::OpenAI)
        .text_model("gpt-4o-mini")
        .build();
    assert_eq!(ai.default_provider(), Provider::OpenAI);
}

#[test]
fn message_constructors() {
    assert_eq!(Message::system("s").role, Role::System);
    assert_eq!(Message::user("u").role, Role::User);
    assert_eq!(Message::assistant("a").role, Role::Assistant);
}
