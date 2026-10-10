//! The MCP tool catalog (RFC 0003 step 1): which commands are tools, and the
//! JSON Schemas built from their specta types — checked by validating real
//! serde output against them, so a schema can't drift from the wire.

use std::collections::HashMap;

use elyra::mcp::{catalog, Mcp, Tool};
use elyra::{command, commands, CommandRegistry, Ctx};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct Address {
    /// The street and number.
    street_line: String,
    city: Option<String>,
}

#[derive(Serialize, Deserialize, specta::Type)]
enum Tier {
    Free,
    Pro,
}

#[derive(Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Contact {
    Email {
        address: String,
    },
    Phone {
        number: String,
        extension: Option<u32>,
    },
}

#[derive(Serialize, Deserialize, specta::Type)]
struct NewCustomer {
    /// The customer's full name.
    name: String,
    age: u32,
    score: f64,
    vip: bool,
    tier: Tier,
    contact: Contact,
    address: Address,
    tags: Vec<String>,
    notes: HashMap<String, String>,
    extra: serde_json::Value,
}

#[derive(Serialize, Deserialize, specta::Type)]
struct Customer {
    id: i64,
    name: String,
}

#[derive(Serialize, Deserialize, specta::Type)]
struct Page<T> {
    data: Vec<T>,
    total: i64,
}

/// Create a customer.
///
/// Fails when the name is taken.
#[command(can = "customers.create")]
async fn customers_store(_ctx: Ctx, input: NewCustomer, note: Option<String>) -> Customer {
    let _ = (input, note);
    Customer {
        id: 1,
        name: "x".into(),
    }
}

/// A page of customers.
#[command(live, can = "customers.view")]
async fn customers_index(_ctx: Ctx, page: i64) -> Page<Customer> {
    let _ = page;
    Page {
        data: vec![],
        total: 0,
    }
}

/// Delete a customer.
#[command(can = "customers.delete")]
async fn customers_destroy(_ctx: Ctx, id: i64) {
    let _ = id;
}

#[command(can = "admin.wipe")]
async fn wipe(_ctx: Ctx) {}

#[derive(Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
enum Status {
    Open,
    PaidInFull,
}

#[derive(Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct OrderFilter {
    min_total: Option<f64>,
    status: Option<Status>,
}

/// Orders between two dates.
#[command(live, can = "orders.view")]
async fn orders_between(
    _ctx: Ctx,
    from: String,
    to: String,
    filter: OrderFilter,
    flagged: Option<bool>,
) -> i64 {
    let _ = (from, to, filter, flagged);
    0
}

#[derive(Serialize, Deserialize, specta::Type)]
struct Report {
    range: OrderFilter,
}

/// A nested argument: a tool, but no template.
#[command(live, can = "orders.view")]
async fn orders_report(_ctx: Ctx, report: Report) -> i64 {
    let _ = report;
    0
}

/// Needs confirmation: never a resource, so no template.
#[command(live, can = "orders.audit")]
async fn orders_audit(_ctx: Ctx, id: i64) -> i64 {
    id
}

/// A list argument: no template either.
#[command(live, can = "orders.view")]
async fn orders_tagged(_ctx: Ctx, tags: Vec<String>) -> i64 {
    let _ = tags;
    0
}

/// No ability: never a tool.
#[command]
async fn open_command(_ctx: Ctx) {}

fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::new();
    r.extend(commands![
        customers_store,
        customers_index,
        customers_destroy,
        wipe,
        open_command,
        orders_between,
        orders_report,
        orders_tagged,
        orders_audit
    ]);
    r
}

fn tools(mcp: Mcp) -> Vec<Tool> {
    catalog(&registry(), &mcp).expect("a catalog")
}

fn tool<'a>(tools: &'a [Tool], name: &str) -> &'a Tool {
    tools
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("no tool {name}"))
}

fn assert_valid(schema: &Value, instance: &Value) {
    let validator = jsonschema::validator_for(schema).expect("a valid JSON Schema");
    let errors: Vec<String> = validator
        .iter_errors(instance)
        .map(|e| e.to_string())
        .collect();
    assert!(
        errors.is_empty(),
        "{instance} should match {schema:#}: {errors:?}"
    );
}

fn assert_invalid(schema: &Value, instance: &Value) {
    let validator = jsonschema::validator_for(schema).expect("a valid JSON Schema");
    assert!(
        !validator.is_valid(instance),
        "{instance} should NOT match {schema:#}"
    );
}

#[test]
fn only_granted_commands_with_an_ability_are_tools() {
    let none = tools(Mcp::new());
    assert!(none.is_empty(), "nothing by default");

    let some = tools(Mcp::new().allow_abilities(["customers.*"]));
    let names: Vec<&str> = some.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        ["customers_destroy", "customers_index", "customers_store"],
        "name order, and neither `wipe` nor the command without an ability"
    );

    let exact = tools(Mcp::new().allow_ability("customers.view"));
    assert_eq!(exact.len(), 1);
    assert_eq!(exact[0].ability, "customers.view");
}

#[test]
fn descriptions_and_hints() {
    let all = tools(
        Mcp::new()
            .allow_abilities(["customers.*"])
            .confirm("customers.delete"),
    );
    let store = tool(&all, "customers_store");
    assert_eq!(
        store.description,
        "Create a customer.\n\nFails when the name is taken."
    );
    assert!(!store.annotations.read_only_hint && !store.annotations.destructive_hint);
    assert!(
        tool(&all, "customers_index").annotations.read_only_hint,
        "live = read-only"
    );
    assert!(
        tool(&all, "customers_destroy").annotations.destructive_hint,
        "confirmed"
    );
}

#[test]
fn the_input_schema_matches_what_serde_reads() {
    let all = tools(Mcp::new().allow_ability("customers.create"));
    let schema = &tool(&all, "customers_store").input_schema;

    // Real serde output for the argument, wrapped as MCP's named arguments.
    let input = NewCustomer {
        name: "Ada".into(),
        age: 36,
        score: 9.5,
        vip: true,
        tier: Tier::Pro,
        contact: Contact::Phone {
            number: "123".into(),
            extension: None,
        },
        address: Address {
            street_line: "1 Main St".into(),
            city: None,
        },
        tags: vec!["x".into()],
        notes: HashMap::from([("a".into(), "b".into())]),
        extra: json!({ "anything": [1, "two"] }),
    };
    let valid = json!({ "input": serde_json::to_value(&input).unwrap(), "note": "hi" });
    assert_valid(schema, &valid);
    // `note` is an Option: it may be left out.
    assert_valid(
        schema,
        &json!({ "input": serde_json::to_value(&input).unwrap() }),
    );
    // So may an Option field — serde reads a missing one as None.
    let mut sparse = valid.clone();
    sparse["input"]["address"] = json!({ "streetLine": "1 Main St" });
    assert_valid(schema, &sparse);
    let parsed: NewCustomer = serde_json::from_value(sparse["input"].clone()).unwrap();
    assert!(parsed.address.city.is_none(), "and serde agrees");

    let mut bad = valid.clone();
    bad["input"]["age"] = json!(-1);
    assert_invalid(schema, &bad);
    let mut bad = valid.clone();
    bad["input"]["tier"] = json!("Gold");
    assert_invalid(schema, &bad);
    let mut bad = valid.clone();
    bad["input"]["contact"] = json!({ "kind": "fax", "number": "1" });
    assert_invalid(schema, &bad);
    let mut bad = valid.clone();
    bad["input"]["address"] = json!({ "street_line": "renamed?" });
    assert_invalid(schema, &bad);
    assert_invalid(schema, &json!({ "note": "the input is required" }));
    let mut bad = valid.clone();
    bad["surprise"] = json!(1);
    assert_invalid(schema, &bad);

    // Field docs travel along.
    let text = schema.to_string();
    assert!(
        text.contains("\"The customer's full name.\"")
            && text.contains("\"The street and number.\"")
    );
}

#[test]
fn the_output_schema_matches_what_serde_writes() {
    let all = tools(Mcp::new().allow_abilities(["customers.*"]));
    let page = &tool(&all, "customers_index").output_schema;
    let value = serde_json::to_value(Page {
        data: vec![Customer {
            id: 7,
            name: "Ada".into(),
        }],
        total: 1,
    })
    .unwrap();
    assert_valid(page, &value);
    assert_invalid(
        page,
        &json!({ "data": [{ "id": "seven", "name": "Ada" }], "total": 1 }),
    );

    // A unit result is null.
    assert_valid(&tool(&all, "customers_destroy").output_schema, &Value::Null);
    // No arguments: an empty object, and nothing else.
    let index_input = &tool(&all, "customers_index").input_schema;
    assert_valid(index_input, &json!({ "page": 2 }));
    assert_invalid(index_input, &json!({}));
}

#[test]
fn live_commands_with_arguments_get_templates_from_their_schema() {
    let all = tools(
        Mcp::new()
            .allow_abilities(["customers.*", "orders.*"])
            .confirm("customers.delete")
            .confirm("orders.audit"),
    );
    let between = tool(&all, "orders_between").template.as_ref().unwrap();
    // Required in the path, in argument order; the rest the query — a struct's
    // fields by their serde names, in name order.
    assert_eq!(
        between.uri_template,
        "app://orders_between/{from}/{to}{?minTotal,status,flagged}"
    );
    assert_eq!(
        tool(&all, "customers_index")
            .template
            .as_ref()
            .unwrap()
            .uri_template,
        "app://customers_index/{page}"
    );

    let report = tool(&all, "orders_report");
    assert!(report.template.is_none());
    assert_eq!(
        report.no_template.as_deref(),
        Some("`report.range` is a nested object")
    );
    assert_eq!(
        tool(&all, "orders_tagged").no_template.as_deref(),
        Some("`tags` is a list")
    );
    // Not live, or confirmed: no template, and nothing to explain.
    for name in ["customers_store", "customers_destroy", "orders_audit"] {
        let t = tool(&all, name);
        assert!(t.template.is_none() && t.no_template.is_none(), "{name}");
    }
}
