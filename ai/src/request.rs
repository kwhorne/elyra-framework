use crate::{message::Message, provider::Provider};

/// A structured-output request: force the model to call a synthetic tool whose
/// arguments are the JSON result (not executed).
pub(crate) struct StructuredTool {
    pub name: String,
    pub schema: serde_json::Value,
}

/// A normalized text-generation request handed to a provider driver.
pub(crate) struct TextRequest {
    pub provider: Provider,
    pub model: String,
    pub system: Option<String>,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub max_tokens: u32,
    pub force: Option<StructuredTool>,
    pub max_steps: u32,
}
