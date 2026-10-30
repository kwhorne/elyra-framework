//! `Ai::fake`: a scripted model that drives the real tool loop, with no
//! network and no key.

use elyra_ai::{Ai, AiFake, Error, JsonSchema, Message, Provider, StreamChunk, Tool};
use serde::Deserialize;
use serde_json::{json, Value};

/// Answers with the city it was asked about, or fails for "Atlantis".
struct Weather;

#[elyra_ai::async_trait]
impl Tool for Weather {
    fn name(&self) -> String {
        "weather".into()
    }
    fn description(&self) -> String {
        "Today's weather in a city.".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {"city": {"type": "string"}}})
    }
    async fn call(&self, args: Value) -> elyra_ai::Result<String> {
        let city = args["city"].as_str().unwrap_or_default();
        if city == "Atlantis" {
            return Err(Error::Tool("weather".into(), "no such city".into()));
        }
        Ok(format!("Sunny in {city}"))
    }
}

#[tokio::test]
async fn the_script_drives_the_real_tools() {
    let fake = AiFake::new()
        .tool_call("weather", json!({ "city": "Oslo" }))
        .text("It's sunny in Oslo.");
    let reply = Ai::fake(fake.clone())
        .chat()
        .instructions("Be brief.")
        .message(Message::system("Answer in English."))
        .tool(Weather)
        .prompt("Weather in Oslo?")
        .await
        .unwrap();
    assert_eq!(reply.text(), "It's sunny in Oslo.");
    assert_eq!(reply.steps(), 1);
    assert!(fake.is_spent());

    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "weather");
    assert_eq!(calls[0].input, json!({ "city": "Oslo" }));
    assert_eq!(calls[0].output, "Sunny in Oslo");

    let requests = fake.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].system, "Be brief.\nAnswer in English.");
    assert_eq!(requests[0].tools, ["weather"]);
    assert_eq!(requests[0].model, "claude-sonnet-5-5");
    assert_eq!(
        requests[0].messages,
        [json!({ "role": "user", "content": "Weather in Oslo?" })]
    );
    // The second step sees the call and its result, as a provider would.
    let messages = &requests[1].messages;
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[1]["content"][0]["type"], "tool_use");
    assert_eq!(messages[1]["content"][0]["name"], "weather");
    let result = &messages[2]["content"][0];
    assert_eq!(result["type"], "tool_result");
    assert_eq!(result["tool_use_id"], messages[1]["content"][0]["id"]);
    assert_eq!(result["content"], "Sunny in Oslo");
}

#[tokio::test]
async fn several_calls_in_a_turn_and_the_failures_a_model_reads() {
    let fake = AiFake::new()
        .tool_calls([
            ("weather", json!({ "city": "Bergen" })),
            ("weather", json!({ "city": "Atlantis" })),
            ("tides", json!({})),
        ])
        .text("Done.");
    Ai::fake(fake.clone())
        .chat()
        .tool(Weather)
        .prompt("…")
        .await
        .unwrap();
    let outputs: Vec<String> = fake.calls().into_iter().map(|c| c.output).collect();
    assert_eq!(
        outputs,
        [
            "Sunny in Bergen",
            "error: tool `weather` failed: no such city",
            "error: unknown tool `tides`",
        ]
    );
    let results = fake.requests()[1].messages[2]["content"].clone();
    assert_eq!(results.as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn the_step_limit_holds() {
    let fake = AiFake::new()
        .tool_call("weather", json!({ "city": "Oslo" }))
        .tool_call("weather", json!({ "city": "Oslo" }))
        .text("never");
    let err = Ai::fake(fake.clone())
        .chat()
        .tool(Weather)
        .max_steps(1)
        .prompt("…")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::MaxSteps(1)), "{err}");
    assert_eq!(fake.calls().len(), 1);
    assert!(!fake.is_spent(), "the last reply was never asked for");
}

#[tokio::test]
async fn a_refusal_and_an_api_error() {
    let fake = AiFake::new()
        .refusal("Can't help with that.")
        .error(529, "Overloaded");
    let ai = Ai::fake(fake);
    let err = ai.chat().prompt("…").await.unwrap_err();
    assert!(
        matches!(&err, Error::Refused(r) if r == "Can't help with that."),
        "{err}"
    );
    let err = ai.chat().prompt("…").await.unwrap_err();
    assert!(
        matches!(&err, Error::Api { status: 529, message } if message == "Overloaded"),
        "{err}"
    );
}

#[derive(Deserialize, JsonSchema)]
struct Sentiment {
    label: String,
}

#[tokio::test]
async fn structured_output_is_the_scripts_json() {
    let fake = AiFake::new().text(r#"{"label":"happy"}"#);
    let sentiment: Sentiment = Ai::fake(fake.clone())
        .chat()
        .prompt_as("I love it")
        .await
        .unwrap();
    assert_eq!(sentiment.label, "happy");
    assert!(fake.requests()[0].structured);
}

#[tokio::test]
async fn a_stream_arrives_a_word_at_a_time() {
    let fake = AiFake::new().text("Hello there, Ada").usage(4, 3);
    let mut stream = Ai::fake(fake.clone()).chat().stream("Hi");
    let mut deltas = Vec::new();
    let mut done = None;
    while let Some(chunk) = stream.next().await {
        match chunk.unwrap() {
            StreamChunk::Delta(text) => deltas.push(text),
            StreamChunk::Done(usage) => done = Some(usage),
        }
    }
    assert_eq!(deltas, ["Hello ", "there, ", "Ada"]);
    let usage = done.unwrap();
    assert_eq!((usage.input_tokens, usage.output_tokens), (4, 3));
    assert!(fake.requests()[0].tools.is_empty());

    let refused = AiFake::new().refusal("No.");
    let err = Ai::fake(refused).chat().stream("…").collect_text().await;
    assert!(matches!(err, Err(Error::Refused(_))));
    let calls = AiFake::new().tool_call("weather", json!({}));
    let err = Ai::fake(calls).chat().stream("…").collect_text().await;
    assert!(matches!(err, Err(Error::Unsupported(_))));
}

#[tokio::test]
async fn usage_counts_against_the_budget_and_no_key_is_needed() {
    let fake = AiFake::new()
        .tool_call("weather", json!({ "city": "Oslo" }))
        .usage(4, 2)
        .text("Sunny.")
        .usage(4, 2)
        .text("never");
    let ai = Ai::builder().token_budget(10).fake(fake).build();
    // OpenAI, without its key: the fake answers either way.
    let reply = ai
        .chat()
        .provider(Provider::OpenAI)
        .tool(Weather)
        .prompt("…")
        .await
        .unwrap();
    let usage = reply.usage();
    assert_eq!((usage.input_tokens, usage.output_tokens), (8, 4));
    assert_eq!(ai.tokens_used(), 12);
    let err = ai.chat().prompt("…").await.unwrap_err();
    assert!(matches!(err, Error::Budget), "{err}");
}

#[tokio::test]
#[should_panic(expected = "AiFake: step 2 asked for a reply, and the script has no more")]
async fn running_out_of_script_is_loud() {
    let fake = AiFake::new().tool_call("weather", json!({ "city": "Oslo" }));
    let _ = Ai::fake(fake).chat().tool(Weather).prompt("…").await;
}
