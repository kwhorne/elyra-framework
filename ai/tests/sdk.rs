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

struct Specialist;
impl elyra_ai::Agent for Specialist {
    fn instructions(&self) -> String {
        "You are a refunds specialist.".into()
    }
    fn name(&self) -> String {
        "refunds_specialist".into()
    }
    fn description(&self) -> String {
        "Delegate refund eligibility questions.".into()
    }
}

#[test]
fn agent_tool_exposes_name_description_and_schema() {
    use elyra_ai::Tool;
    let ai = Ai::builder().build();
    let tool = elyra_ai::AgentTool::new(&ai, Specialist);
    assert_eq!(tool.name(), "refunds_specialist");
    assert_eq!(tool.description(), "Delegate refund eligibility questions.");
    assert_eq!(tool.parameters()["type"], "object");
    assert!(tool.parameters()["properties"]["task"].is_object());
}

#[tokio::test]
async fn budget_blocks_prompts_when_exhausted() {
    let ai = Ai::builder().token_budget(0).build();
    assert_eq!(ai.tokens_used(), 0);
    let err = ai.chat().prompt("hi").await.unwrap_err();
    assert!(matches!(err, elyra_ai::Error::Budget));
}
