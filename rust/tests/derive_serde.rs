//! `#[derive(Input)]` follows `#[serde(...)]`: every path in a value's
//! serde_json form reads the same through the typed value.

use std::collections::BTreeMap;

use rulekit::ast::Segment;
use rulekit::value::{Val, ValueRef};
use rulekit::{Input, Opts};
use serde::Serialize;
use serde_json::{Value, json};

type JsonObject = serde_json::Map<String, Value>;

/// Shapes mirrored from qcontrol's `qevents` crate.
mod events {
    use super::*;

    #[derive(Serialize, rulekit::Input)]
    #[serde(transparent)]
    pub struct Timestamp(pub String);

    #[derive(Serialize, rulekit::Input)]
    #[serde(rename_all = "lowercase")]
    pub enum Severity {
        Debug,
        Info,
        Warn,
        Error,
    }

    #[derive(Serialize, rulekit::Input)]
    #[serde(untagged)]
    pub enum EventRecord {
        Entity(EntityRecord),
        Host(HostRecord),
    }

    #[derive(Serialize, rulekit::Input)]
    pub struct EntityRecord {
        pub timestamp: Timestamp,
        #[serde(default)]
        pub severity: Severity,
        pub entity_id: String,
        #[serde(flatten)]
        pub event: EntityEvent,
    }

    #[derive(Serialize, rulekit::Input)]
    pub struct HostRecord {
        pub timestamp: Timestamp,
        pub severity: Severity,
        #[serde(flatten)]
        pub event: HostEvent,
    }

    #[derive(Serialize, rulekit::Input)]
    #[serde(untagged)]
    pub enum EntityEvent {
        Process(ProcessEvent),
        Llm(LlmEvent),
        Agent(AgentEvent),
        Plugin(CustomPluginEvent),
    }

    #[derive(Serialize, rulekit::Input)]
    #[serde(tag = "type", content = "payload")]
    pub enum ProcessEvent {
        #[serde(rename = "process.started")]
        Started(ProcessStarted),
        #[serde(rename = "process.stopped")]
        Stopped(ProcessStopped),
    }

    #[derive(Serialize, rulekit::Input)]
    #[serde(rename_all = "camelCase")]
    pub struct ProcessStarted {
        pub pid: u32,
        pub exe_path: String,
        /// No `skip_serializing_if`: `None` is written as `null`.
        pub parent_pid: Option<u32>,
        pub argv: Vec<String>,
        pub started_at: Timestamp,
        #[serde(rename = "user-name")]
        pub user_name: String,
        #[serde(rename(serialize = "ser_only", deserialize = "de_only"))]
        pub both: bool,
        pub r#type: String,
    }

    #[derive(Serialize, rulekit::Input)]
    pub struct ProcessStopped {
        pub pid: u32,
        pub status: ExitStatus,
    }

    #[derive(Serialize, rulekit::Input)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ExitStatus {
        Exited { code: i32 },
        Signaled { signal: i32 },
    }

    #[derive(Serialize, rulekit::Input)]
    #[serde(tag = "type", content = "payload")]
    pub enum LlmEvent {
        #[serde(rename = "llm.request")]
        Request(Request),
        #[serde(rename = "llm.response")]
        Response(Response),
        #[serde(rename = "llm.heartbeat")]
        Heartbeat,
        #[serde(rename = "llm.pair")]
        Pair(u32, String),
        #[serde(rename = "llm.inline", rename_all = "PascalCase")]
        Inline { model_name: String },
        // A tuple variant, so the derive's skipped-variant `(..)` pattern is covered.
        #[serde(skip)]
        Hidden(#[allow(dead_code)] String),
        #[serde(untagged)]
        Raw(JsonObject),
    }

    #[derive(Serialize, rulekit::Input)]
    pub struct LlmEventContext {
        pub session_id: String,
        pub name: String,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        pub raw: Option<JsonObject>,
    }

    #[derive(Serialize, rulekit::Input)]
    pub struct Request {
        pub context: LlmEventContext,
        pub model: String,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        pub request_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        pub system_instructions: Option<String>,
    }

    #[derive(Serialize, rulekit::Input)]
    #[serde(rename_all = "snake_case")]
    pub enum ResponsePhase {
        Completed,
        StreamStarted,
        Errored,
    }

    #[derive(Serialize, rulekit::Input)]
    pub struct Response {
        pub context: LlmEventContext,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        pub model: Option<String>,
        pub phase: ResponsePhase,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        pub duration_ms: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        pub cost_usd: Option<f64>,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        pub tags: Vec<String>,
        #[serde(skip_serializing_if = "is_zero", default)]
        pub retries: u32,
    }

    fn is_zero(n: &u32) -> bool {
        *n == 0
    }

    #[derive(Serialize, rulekit::Input)]
    #[serde(tag = "type", content = "payload")]
    pub enum AgentEvent {
        #[serde(rename = "agent.message")]
        Message(Message),
        #[serde(rename = "agent.tool_call")]
        ToolCall(ToolCall),
    }

    #[derive(Serialize, rulekit::Input)]
    #[serde(rename_all = "snake_case")]
    pub enum Role {
        User,
        Assistant,
    }

    #[derive(Serialize, rulekit::Input)]
    pub struct Message {
        pub context: LlmEventContext,
        pub role: Role,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        pub prompt: Option<String>,
        #[serde(skip)]
        pub internal: String,
        #[serde(skip_serializing)]
        pub cache: u32,
    }

    #[derive(Serialize, rulekit::Input)]
    pub struct ToolMeta {
        pub vendor: String,
        pub version: u8,
    }

    #[derive(Serialize, rulekit::Input)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum Tool {
        Builtin {
            name: String,
        },
        Mcp {
            server: String,
            name: String,
        },
        Skill {
            name: String,
            #[serde(skip_serializing_if = "Option::is_none", default)]
            path: Option<String>,
        },
        Unknown,
        Vendored(ToolMeta),
        #[serde(untagged)]
        Other(String),
    }

    #[derive(Serialize, rulekit::Input)]
    pub struct ToolCall {
        pub context: LlmEventContext,
        pub tool: Tool,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        pub call_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        pub arguments: Option<JsonObject>,
    }

    #[derive(Serialize, rulekit::Input)]
    pub struct CustomPluginEvent {
        #[serde(rename = "type")]
        pub event_type: String,
        pub payload: CustomPluginPayload,
    }

    #[derive(Serialize, rulekit::Input)]
    pub struct CustomPluginPayload {
        pub plugin_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub note: Option<String>,
        #[serde(flatten)]
        pub extra: JsonObject,
    }

    #[derive(Serialize, rulekit::Input)]
    #[serde(untagged)]
    pub enum HostEvent {
        Installation(InstallationEvent),
    }

    #[derive(Serialize, rulekit::Input)]
    #[serde(tag = "type", content = "payload")]
    pub enum InstallationEvent {
        #[serde(rename = "installation.discovered")]
        Discovered(Installation),
    }

    #[derive(Serialize, rulekit::Input)]
    pub struct Installation {
        pub id: String,
        pub labels: BTreeMap<String, String>,
        #[serde(flatten)]
        pub details: InstallationDetails,
        #[serde(flatten)]
        pub agent: Option<AgentInfo>,
    }

    #[derive(Serialize, rulekit::Input)]
    pub struct InstallationDetails {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub default_model: Option<String>,
        pub warnings: Vec<String>,
    }

    #[derive(Serialize, rulekit::Input)]
    pub struct AgentInfo {
        pub agent_name: String,
    }
}

/// Representations and attributes `qevents` does not use.
mod shapes {
    use super::*;

    #[derive(Serialize, rulekit::Input)]
    #[serde(rename_all = "SCREAMING_SNAKE_CASE", rename_all_fields = "camelCase")]
    pub enum External {
        Unit,
        NewType(u32),
        Tuple(i64, String),
        Struct {
            field_one: u8,
            two_words: String,
        },
        #[serde(rename_all = "kebab-case")]
        Kebab {
            some_field: bool,
        },
        #[serde(rename = "renamed")]
        Renamed(Option<String>),
        Skipping(u8, #[serde(skip)] u8, String),
        Empty(),
        EmptyStruct {},
        #[serde(untagged)]
        Loose(String),
    }

    #[derive(Serialize, rulekit::Input)]
    #[serde(untagged)]
    pub enum Untagged {
        Unit,
        Number(f64),
        Pair(bool, Option<i8>),
        Fields { a: i16, b: Vec<u8> },
    }

    #[derive(Serialize, rulekit::Input)]
    #[serde(tag = "t", content = "c", rename_all = "kebab-case")]
    pub enum Adjacent {
        UnitOne,
        Wrapped(Inner),
        Fields { x: u64 },
        Tuple(u8, u8),
    }

    #[derive(Serialize, rulekit::Input)]
    pub struct Inner {
        pub value: String,
        pub nested: Option<Box<Inner>>,
    }

    #[derive(Serialize, rulekit::Input)]
    #[serde(tag = "t", rename = "Thing")]
    pub struct TaggedStruct {
        pub a: u8,
    }

    #[derive(Serialize, rulekit::Input)]
    pub struct Pair(pub u8, pub String);

    #[derive(Serialize, rulekit::Input)]
    pub struct Marker;

    #[derive(Serialize, rulekit::Input)]
    pub struct Newtype(pub Inner);

    #[derive(Serialize, rulekit::Input)]
    #[serde(transparent)]
    pub struct Transparent {
        pub inner: Option<Inner>,
        #[serde(skip)]
        pub _ignored: u8,
    }

    #[derive(Serialize, rulekit::Input)]
    pub struct Holder {
        pub external: Vec<External>,
        pub untagged: Vec<Untagged>,
        pub adjacent: Vec<Adjacent>,
        pub tagged: TaggedStruct,
        pub pair: Pair,
        pub marker: Marker,
        pub newtype: Newtype,
        pub transparent: Transparent,
        pub cases: Cases,
        pub variants: Vec<CaseVariants>,
    }

    macro_rules! case_struct {
        ($name:ident, $rule:literal) => {
            #[derive(Serialize, rulekit::Input)]
            #[serde(rename_all = $rule)]
            pub struct $name {
                pub field_one: u8,
                pub two_word_name: u8,
            }
        };
    }
    case_struct!(Lower, "lowercase");
    case_struct!(Upper, "UPPERCASE");
    case_struct!(Pascal, "PascalCase");
    case_struct!(Camel, "camelCase");
    case_struct!(Snake, "snake_case");
    case_struct!(ScreamingSnake, "SCREAMING_SNAKE_CASE");
    case_struct!(Kebab, "kebab-case");
    case_struct!(ScreamingKebab, "SCREAMING-KEBAB-CASE");

    #[derive(Serialize, rulekit::Input)]
    pub struct Cases {
        pub lower: Lower,
        pub upper: Upper,
        pub pascal: Pascal,
        pub camel: Camel,
        pub snake: Snake,
        pub screaming_snake: ScreamingSnake,
        pub kebab: Kebab,
        pub screaming_kebab: ScreamingKebab,
    }

    macro_rules! case_enum {
        ($name:ident, $rule:literal) => {
            #[derive(Serialize, rulekit::Input)]
            #[serde(rename_all = $rule)]
            pub enum $name {
                OneVariant,
                TwoWordVariant,
            }
        };
    }
    case_enum!(VLower, "lowercase");
    case_enum!(VUpper, "UPPERCASE");
    case_enum!(VPascal, "PascalCase");
    case_enum!(VCamel, "camelCase");
    case_enum!(VSnake, "snake_case");
    case_enum!(VScreamingSnake, "SCREAMING_SNAKE_CASE");
    case_enum!(VKebab, "kebab-case");
    case_enum!(VScreamingKebab, "SCREAMING-KEBAB-CASE");

    #[derive(Serialize, rulekit::Input)]
    #[serde(untagged)]
    pub enum CaseVariants {
        Lower(VLower),
        Upper(VUpper),
        Pascal(VPascal),
        Camel(VCamel),
        Snake(VSnake),
        ScreamingSnake(VScreamingSnake),
        Kebab(VKebab),
        ScreamingKebab(VScreamingKebab),
    }
}

use events::*;
use shapes::*;

/// What a lookup found, comparable across typed and JSON inputs.
#[derive(Debug, PartialEq)]
enum Found {
    Missing,
    Null,
    Bool(bool),
    Int(i64),
    Uint(u64),
    Float(f64),
    Str(String),
    Array(Vec<Found>),
    Object,
}

fn found(value: ValueRef<'_>) -> Found {
    match value {
        ValueRef::Null => Found::Null,
        ValueRef::Bool(b) => Found::Bool(b),
        ValueRef::Int(n) => Found::Int(n),
        ValueRef::Uint(n) => Found::Uint(n),
        ValueRef::Float(n) => Found::Float(n),
        ValueRef::Str(s) => Found::Str(s.to_owned()),
        ValueRef::Array(items) => Found::Array(items.iter().map(found).collect()),
        ValueRef::Object(_) => Found::Object,
        other => panic!("unexpected value {other:?}"),
    }
}

/// Equal, except that a struct, map-like variant, or tuple read whole is
/// opaque: its paths are compared one by one.
fn same(typed: &Found, wire: &Found) -> bool {
    match (typed, wire) {
        (Found::Object, Found::Object | Found::Array(_)) => true,
        (Found::Array(a), Found::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
        }
        _ => typed == wire,
    }
}

fn lookup(input: &dyn Input, path: &[Segment]) -> Found {
    match input.get(&(), path).expect("lookup") {
        None => Found::Missing,
        Some(val) => found(Val::as_ref(&val)),
    }
}

fn key(k: &str) -> Segment {
    Segment::Key {
        key: k.to_owned(),
        bracket: false,
    }
}

/// Every path in `value`, objects and arrays included.
fn paths(value: &Value, prefix: &mut Vec<Segment>, out: &mut Vec<Vec<Segment>>) {
    out.push(prefix.clone());
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                prefix.push(key(k));
                paths(v, prefix, out);
                prefix.pop();
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                prefix.push(Segment::Index(i));
                paths(v, prefix, out);
                prefix.pop();
            }
        }
        _ => {}
    }
}

/// Typed lookup equals serde_json lookup at every path of the serialized
/// value, and one step past each of them.
fn assert_wire_parity<T: Serialize + Input>(value: &T) -> Value {
    let json = serde_json::to_value(value).expect("serialize");
    let mut all = Vec::new();
    paths(&json, &mut Vec::new(), &mut all);
    for path in &all {
        let typed = lookup(value, path);
        let wire = lookup(&json, path);
        assert!(
            same(&typed, &wire),
            "path {path:?}: {typed:?} != {wire:?} in {json}"
        );
        for past in [key("zz_absent"), Segment::Index(999)] {
            let mut path = path.clone();
            path.push(past);
            assert_eq!(
                lookup(value, &path),
                Found::Missing,
                "path {path:?} in {json}"
            );
            assert_eq!(
                lookup(&json, &path),
                Found::Missing,
                "path {path:?} in {json}"
            );
        }
    }
    for absent in [vec![key("zz_absent")], vec![Segment::Index(0)]] {
        assert_eq!(lookup(value, &absent), lookup(&json, &absent), "{absent:?}");
    }
    json
}

fn ctx(name: &str) -> LlmEventContext {
    LlmEventContext {
        session_id: format!("s-{name}"),
        name: name.to_owned(),
        raw: None,
    }
}

fn obj(value: Value) -> JsonObject {
    match value {
        Value::Object(map) => map,
        _ => unreachable!(),
    }
}

fn ts() -> Timestamp {
    Timestamp("2026-06-02T12:34:57.999999999Z".to_owned())
}

fn entity(severity: Severity, event: EntityEvent) -> EventRecord {
    EventRecord::Entity(EntityRecord {
        timestamp: ts(),
        severity,
        entity_id: "e-1".into(),
        event,
    })
}

fn event_records() -> Vec<EventRecord> {
    let started = |parent_pid| ProcessStarted {
        pid: 42,
        exe_path: "/bin/sh".into(),
        parent_pid,
        argv: vec!["sh".into(), "-c".into()],
        started_at: ts(),
        user_name: "ada".into(),
        both: true,
        r#type: "raw-ident".into(),
    };
    vec![
        entity(
            Severity::Info,
            EntityEvent::Process(ProcessEvent::Started(started(None))),
        ),
        entity(
            Severity::Debug,
            EntityEvent::Process(ProcessEvent::Started(started(Some(1)))),
        ),
        entity(
            Severity::Warn,
            EntityEvent::Process(ProcessEvent::Stopped(ProcessStopped {
                pid: 42,
                status: ExitStatus::Signaled { signal: 9 },
            })),
        ),
        entity(
            Severity::Error,
            EntityEvent::Process(ProcessEvent::Stopped(ProcessStopped {
                pid: 7,
                status: ExitStatus::Exited { code: -1 },
            })),
        ),
        entity(
            Severity::Info,
            EntityEvent::Llm(LlmEvent::Request(Request {
                context: LlmEventContext {
                    raw: Some(obj(json!({"k": [1, {"n": null}], "s": "v"}))),
                    ..ctx("claude")
                },
                model: "gpt-x".into(),
                request_id: Some("r1".into()),
                system_instructions: None,
            })),
        ),
        entity(
            Severity::Info,
            EntityEvent::Llm(LlmEvent::Response(Response {
                context: ctx("codex"),
                model: None,
                phase: ResponsePhase::StreamStarted,
                duration_ms: Some(u64::MAX),
                cost_usd: Some(0.25),
                tags: vec![],
                retries: 0,
            })),
        ),
        entity(
            Severity::Info,
            EntityEvent::Llm(LlmEvent::Response(Response {
                context: ctx("codex"),
                model: Some("m".into()),
                phase: ResponsePhase::Errored,
                duration_ms: None,
                cost_usd: Some(2.0),
                tags: vec!["a".into()],
                retries: 3,
            })),
        ),
        entity(Severity::Debug, EntityEvent::Llm(LlmEvent::Heartbeat)),
        entity(
            Severity::Debug,
            EntityEvent::Llm(LlmEvent::Pair(5, "five".into())),
        ),
        entity(
            Severity::Debug,
            EntityEvent::Llm(LlmEvent::Inline {
                model_name: "inline".into(),
            }),
        ),
        entity(
            Severity::Debug,
            EntityEvent::Llm(LlmEvent::Raw(obj(json!({"type": "raw", "x": 1})))),
        ),
        entity(
            Severity::Info,
            EntityEvent::Agent(AgentEvent::Message(Message {
                context: ctx("claude"),
                role: Role::Assistant,
                prompt: Some("hi".into()),
                internal: "secret".into(),
                cache: 9,
            })),
        ),
        entity(
            Severity::Info,
            EntityEvent::Agent(AgentEvent::Message(Message {
                context: ctx("claude"),
                role: Role::User,
                prompt: None,
                internal: String::new(),
                cache: 0,
            })),
        ),
    ]
    .into_iter()
    .chain(
        [
            Tool::Builtin {
                name: "Bash".into(),
            },
            Tool::Mcp {
                server: "gh".into(),
                name: "issues".into(),
            },
            Tool::Skill {
                name: "grill-me".into(),
                path: None,
            },
            Tool::Skill {
                name: "grill-me".into(),
                path: Some("/s/SKILL.md".into()),
            },
            Tool::Unknown,
            Tool::Vendored(ToolMeta {
                vendor: "acme".into(),
                version: 2,
            }),
            Tool::Other("free-form".into()),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, tool)| {
            entity(
                Severity::Info,
                EntityEvent::Agent(AgentEvent::ToolCall(ToolCall {
                    context: ctx("claude"),
                    tool,
                    call_id: (i % 2 == 0).then(|| format!("c{i}")),
                    arguments: (i % 3 == 0).then(|| obj(json!({"cmd": "ls", "n": i}))),
                })),
            )
        }),
    )
    .chain([
        entity(
            Severity::Info,
            EntityEvent::Plugin(CustomPluginEvent {
                event_type: "plugin.event".into(),
                payload: CustomPluginPayload {
                    plugin_name: "p".into(),
                    note: None,
                    extra: obj(json!({"note": "from extra", "deep": {"a": [true]}})),
                },
            }),
        ),
        entity(
            Severity::Info,
            EntityEvent::Plugin(CustomPluginEvent {
                event_type: "plugin.event".into(),
                payload: CustomPluginPayload {
                    plugin_name: "p".into(),
                    note: Some("own".into()),
                    extra: obj(json!({"other": 1.5})),
                },
            }),
        ),
        EventRecord::Host(HostRecord {
            timestamp: ts(),
            severity: Severity::Info,
            event: HostEvent::Installation(InstallationEvent::Discovered(Installation {
                id: "i-1".into(),
                labels: BTreeMap::from([("team".into(), "core".into())]),
                details: InstallationDetails {
                    default_model: Some("sonnet".into()),
                    warnings: vec!["old".into()],
                },
                agent: Some(AgentInfo {
                    agent_name: "claude".into(),
                }),
            })),
        }),
        EventRecord::Host(HostRecord {
            timestamp: ts(),
            severity: Severity::Warn,
            event: HostEvent::Installation(InstallationEvent::Discovered(Installation {
                id: "i-2".into(),
                labels: BTreeMap::new(),
                details: InstallationDetails {
                    default_model: None,
                    warnings: vec![],
                },
                agent: None,
            })),
        }),
    ])
    .collect()
}

fn inner(value: &str, nested: Option<Inner>) -> Inner {
    Inner {
        value: value.into(),
        nested: nested.map(Box::new),
    }
}

fn holder() -> Holder {
    Holder {
        external: vec![
            External::Unit,
            External::NewType(7),
            External::Tuple(-3, "t".into()),
            External::Struct {
                field_one: 1,
                two_words: "two".into(),
            },
            External::Kebab { some_field: true },
            External::Renamed(None),
            External::Renamed(Some("r".into())),
            External::Skipping(1, 2, "three".into()),
            External::Empty(),
            External::EmptyStruct {},
            External::Loose("loose".into()),
        ],
        untagged: vec![
            Untagged::Unit,
            Untagged::Number(1.5),
            Untagged::Pair(true, None),
            Untagged::Pair(false, Some(-8)),
            Untagged::Fields {
                a: 3,
                b: vec![1, 2],
            },
        ],
        adjacent: vec![
            Adjacent::UnitOne,
            Adjacent::Wrapped(inner("w", Some(inner("deeper", None)))),
            Adjacent::Fields { x: 9 },
            Adjacent::Tuple(1, 2),
        ],
        tagged: TaggedStruct { a: 4 },
        pair: Pair(8, "eight".into()),
        marker: Marker,
        newtype: Newtype(inner("n", None)),
        transparent: Transparent {
            inner: Some(inner("t", None)),
            _ignored: 0,
        },
        cases: Cases {
            lower: Lower {
                field_one: 1,
                two_word_name: 2,
            },
            upper: Upper {
                field_one: 1,
                two_word_name: 2,
            },
            pascal: Pascal {
                field_one: 1,
                two_word_name: 2,
            },
            camel: Camel {
                field_one: 1,
                two_word_name: 2,
            },
            snake: Snake {
                field_one: 1,
                two_word_name: 2,
            },
            screaming_snake: ScreamingSnake {
                field_one: 1,
                two_word_name: 2,
            },
            kebab: Kebab {
                field_one: 1,
                two_word_name: 2,
            },
            screaming_kebab: ScreamingKebab {
                field_one: 1,
                two_word_name: 2,
            },
        },
        variants: vec![
            CaseVariants::Lower(VLower::TwoWordVariant),
            CaseVariants::Upper(VUpper::TwoWordVariant),
            CaseVariants::Pascal(VPascal::TwoWordVariant),
            CaseVariants::Camel(VCamel::TwoWordVariant),
            CaseVariants::Snake(VSnake::TwoWordVariant),
            CaseVariants::ScreamingSnake(VScreamingSnake::TwoWordVariant),
            CaseVariants::Kebab(VKebab::TwoWordVariant),
            CaseVariants::ScreamingKebab(VScreamingKebab::OneVariant),
            CaseVariants::Lower(VLower::OneVariant),
            CaseVariants::Upper(VUpper::OneVariant),
            CaseVariants::Pascal(VPascal::OneVariant),
            CaseVariants::Camel(VCamel::OneVariant),
            CaseVariants::Snake(VSnake::OneVariant),
            CaseVariants::ScreamingSnake(VScreamingSnake::OneVariant),
            CaseVariants::Kebab(VKebab::OneVariant),
            CaseVariants::ScreamingKebab(VScreamingKebab::TwoWordVariant),
        ],
    }
}

#[test]
fn event_records_match_their_json() {
    for record in event_records() {
        assert_wire_parity(&record);
    }
}

#[test]
fn other_representations_match_their_json() {
    let json = assert_wire_parity(&holder());
    // Spot-check that the fixture covers what it claims.
    assert_eq!(json["external"][0], "UNIT");
    assert_eq!(json["external"][3]["STRUCT"]["fieldOne"], 1);
    assert_eq!(json["external"][4]["KEBAB"]["some-field"], true);
    assert_eq!(json["external"][7]["SKIPPING"], json!([1, "three"]));
    assert_eq!(json["untagged"][0], Value::Null);
    assert_eq!(json["adjacent"][0], json!({"t": "unit-one"}));
    assert_eq!(json["tagged"], json!({"t": "Thing", "a": 4}));
    assert_eq!(json["cases"]["screaming_kebab"]["TWO-WORD-NAME"], 2);
    assert_eq!(json["variants"][4], "two_word_variant");
    assert_eq!(assert_wire_parity(&Marker), Value::Null);
}

fn eval(rule: &str, input: &impl Input) -> Option<bool> {
    let rule = rulekit::parse(rule).expect("parse");
    let result = rule.eval(&(), input, Opts::default());
    if result.pass() {
        Some(true)
    } else if result.fail() {
        Some(false)
    } else {
        None
    }
}

#[test]
fn rules_read_wire_names() {
    let records = event_records();
    let llm = &records[4];
    assert_eq!(
        eval(
            r#"starts_with(type, "process.") or payload.model == "gpt-x""#,
            llm
        ),
        Some(true)
    );
    assert_eq!(eval(r#"type == "llm.request""#, llm), Some(true));
    assert_eq!(eval(r#"severity == "info""#, llm), Some(true));
    assert_eq!(
        eval(r#"timestamp == "2026-06-02T12:34:57.999999999Z""#, llm),
        Some(true)
    );
    assert_eq!(eval(r#"payload.context.raw.s == "v""#, llm), Some(true));
    let started = &records[0];
    assert_eq!(eval(r#"payload.exePath == "/bin/sh""#, started), Some(true));
    assert_eq!(
        eval(r#"payload["user-name"] == "ada""#, started),
        Some(true)
    );
    assert_eq!(eval(r#"payload.ser_only == true"#, started), Some(true));
    assert_eq!(eval(r#"payload.type == "raw-ident""#, started), Some(true));
    // Serialized `None` is null, as on the wire.
    for rule in [
        "payload.parentPid != 1",
        "payload.parentPid == 1",
        "payload.parentPid.x == 1",
        "payload.context.raw.k[1].n != 1",
    ] {
        for record in &records {
            eval_wire(rule, record);
        }
    }
}

/// Evaluate on the typed value, asserting its serde_json form agrees.
fn eval_wire<T: Serialize + Input>(rule: &str, input: &T) -> Option<bool> {
    let typed = eval(rule, input);
    let json = serde_json::to_value(input).expect("serialize");
    assert_eq!(typed, eval(rule, &json), "{rule} on {json}");
    typed
}

#[test]
fn skipped_and_absent_fields_are_missing() {
    let records = event_records();
    let message = &records[11];
    let EventRecord::Entity(EntityRecord {
        event: EntityEvent::Agent(AgentEvent::Message(fields)),
        ..
    }) = message
    else {
        unreachable!()
    };
    assert_eq!((fields.internal.as_str(), fields.cache), ("secret", 9));
    for field in ["payload.internal", "payload.cache"] {
        let rule = rulekit::parse(&format!("{field} == 1")).unwrap();
        let result = rule.eval(&(), message, Opts::default());
        assert_eq!(result.missing_fields().collect::<Vec<_>>(), [field]);
    }

    // A skipped variant is never serialized; every path is missing.
    let hidden = LlmEvent::Hidden("h".into());
    for path in [vec![], vec![key("type")], vec![key("payload")]] {
        assert_eq!(lookup(&hidden, &path), Found::Missing);
    }
    assert_eq!(assert_wire_parity(&ResponsePhase::Completed), "completed");

    // `skip_serializing_if` held: missing, not null.
    let no_prompt = &records[12];
    let rule = rulekit::parse(r#"payload.prompt == "x""#).unwrap();
    let result = rule.eval(&(), no_prompt, Opts::default());
    assert_eq!(
        result.missing_fields().collect::<Vec<_>>(),
        ["payload.prompt"]
    );
}

#[test]
fn path_through_null_is_missing() {
    #[derive(Serialize, rulekit::Input)]
    struct User {
        id: u32,
    }
    #[derive(Serialize, rulekit::Input)]
    struct Row {
        user: Option<User>,
    }
    let row = Row { user: None };
    let json = assert_wire_parity(&row);
    assert_eq!(json, json!({"user": null}));
    // `user` is present (null), so a comparison completes.
    assert_eq!(eval_wire("user != 1", &row), eval("user != 1", &json));
    assert!(eval("user != 1", &row).is_some());
    for input in [&row as &dyn Input, &json] {
        let rule = rulekit::parse("user.id == 1").unwrap();
        let result = rule.eval(&(), input, Opts::default());
        assert_eq!(result.missing_fields().collect::<Vec<_>>(), ["user.id"]);
    }
}

#[test]
fn rulekit_attributes_win_over_serde() {
    #[derive(Serialize, rulekit::Input)]
    #[serde(rename_all = "camelCase")]
    struct Row {
        #[serde(rename = "serdeName")]
        #[rulekit(rename = "rk_name")]
        field: u8,
        #[rulekit(bytes)]
        some_bytes: Vec<u8>,
    }
    #[derive(Serialize, rulekit::Input)]
    #[serde(tag = "type", content = "payload")]
    enum Event {
        Body {
            #[rulekit(bytes)]
            body: Vec<u8>,
        },
    }
    let row = Row {
        field: 1,
        some_bytes: b"abc".to_vec(),
    };
    assert_eq!(eval("rk_name == 1", &row), Some(true));
    assert_eq!(eval("serdeName == 1", &row), None);
    assert_eq!(eval(r#"someBytes == "abc""#, &row), Some(true));
    let event = Event::Body {
        body: b"POST".to_vec(),
    };
    assert_eq!(eval(r#"payload.body == "POST""#, &event), Some(true));
    assert_eq!(eval(r#"type == "Body""#, &event), Some(true));
}

#[test]
fn derived_enums_nest_in_plain_structs() {
    #[derive(rulekit::Input)]
    struct Wrapper<'a> {
        record: &'a EventRecord,
        severities: Vec<Severity>,
    }
    let records = event_records();
    let wrapper = Wrapper {
        record: &records[4],
        severities: vec![Severity::Warn, Severity::Error],
    };
    assert_eq!(
        eval(r#"record.payload.model == "gpt-x""#, &wrapper),
        Some(true)
    );
    assert_eq!(eval(r#"severities contains "error""#, &wrapper), Some(true));
    assert_eq!(eval(r#"severities[0] == "warn""#, &wrapper), Some(true));
}
