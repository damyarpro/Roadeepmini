//! The Roadeep tools, as advertised by `tools/list`. The app dispatches on the
//! same names (src-tauri/src/mcp/tools.rs) and validates the arguments itself:
//! a schema is a hint to the model, never a security boundary.

use serde_json::{json, Value};

pub const WHOAMI: &str = "roadeep_whoami";
pub const LIST_MODELS: &str = "roadeep_list_models";
pub const LIST_AGENTS: &str = "roadeep_list_agents";
pub const CHAT: &str = "roadeep_chat";
pub const LIST_SUBTYPES: &str = "roadeep_list_subtypes";
pub const GET_SUBTYPE: &str = "roadeep_get_subtype";
pub const ESTIMATE_GENERATION: &str = "roadeep_estimate_generation";
pub const START_GENERATION: &str = "roadeep_start_generation";
pub const GENERATION_STATUS: &str = "roadeep_generation_status";
pub const CANCEL_GENERATION: &str = "roadeep_cancel_generation";

pub const NAMES: &[&str] = &[
    WHOAMI,
    LIST_MODELS,
    LIST_AGENTS,
    CHAT,
    LIST_SUBTYPES,
    GET_SUBTYPE,
    ESTIMATE_GENERATION,
    START_GENERATION,
    GENERATION_STATUS,
    CANCEL_GENERATION,
];

pub fn is_known(name: &str) -> bool {
    NAMES.contains(&name)
}

/// Shared limits, so the schemas and the app's validation agree.
pub const MAX_MESSAGE_CHARS: usize = 32_000;
pub const MAX_ID_LEN: usize = 128;

fn no_arguments() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

fn id_schema(description: &str) -> Value {
    json!({
        "type": "string",
        "description": description,
        "minLength": 1,
        "maxLength": MAX_ID_LEN,
        "pattern": "^[A-Za-z0-9_-]+$"
    })
}

/// The quote/submit body. `start` adds the quote id.
fn generation_schema(start: bool) -> Value {
    let mut properties = json!({
        "subtype": {
            "type": "string",
            "description": "Subtype slug from roadeep_list_subtypes (e.g. an image or video product). Give exactly one of `subtype` or `model`."
        },
        "model": {
            "type": "string",
            "description": "A member model id from roadeep_get_subtype. Give exactly one of `subtype` or `model`."
        },
        "capability": {
            "type": "string",
            "description": "The capability of the chosen member, exactly as roadeep_get_subtype reports it (e.g. text-to-image)."
        },
        "input": {
            "type": "object",
            "description": "Inputs matching the member's input_schema from roadeep_get_subtype (e.g. {\"prompt\": \"...\"}). Only send fields the schema declares.",
            "additionalProperties": true
        },
        "controls": {
            "type": "object",
            "description": "Optional product controls.",
            "properties": {
                "refine_prompt": { "type": "boolean", "description": "Let Roadeep improve the prompt first." },
                "iranize_prompts": { "type": "boolean", "description": "Adapt the prompt for Iranian cultural context." },
                "preset_id": { "type": "string", "description": "A Roadeep style preset id." }
            },
            "additionalProperties": false
        }
    });
    let mut required = vec!["capability", "input"];
    if start {
        properties["quote_id"] = json!({
            "type": "string",
            "description": "quote_id returned by roadeep_estimate_generation for this exact body. Required so the cost is always seen before paying."
        });
        required.insert(0, "quote_id");
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

/// What a tool does to the world, for the MCP annotations.
#[derive(Clone, Copy, PartialEq)]
enum Effect {
    /// Only reads.
    ReadOnly,
    /// Writes, but nothing that cannot be undone or that costs money.
    Writes,
    /// Spends credits or ends something for good.
    Destructive,
}

fn tool(name: &str, title: &str, description: &str, schema: Value, effect: Effect, idempotent: bool) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": schema,
        "annotations": {
            "title": title,
            "readOnlyHint": effect == Effect::ReadOnly,
            "destructiveHint": effect == Effect::Destructive,
            "idempotentHint": idempotent,
            "openWorldHint": true
        }
    })
}

/// Everything `tools/list` returns, in a stable order.
pub fn definitions() -> Vec<Value> {
    vec![
        tool(
            WHOAMI,
            "Roadeep account",
            "Shows the Roadeep account signed in to the Roadeep desktop app (id, name, email, phone). Use it to check that the app is running and signed in.",
            no_arguments(),
            Effect::ReadOnly,
            true,
        ),
        tool(
            LIST_MODELS,
            "List Roadeep chat models",
            "Lists the chat models available to this Roadeep account: id, display name, provider, whether it is the server default, and vision / file-input support. Pass an id as `model` to roadeep_chat.",
            no_arguments(),
            Effect::ReadOnly,
            true,
        ),
        tool(
            LIST_AGENTS,
            "List Roadeep agents",
            "Lists the official Roadeep agents (id, title, short description, starter prompts). Pass an id as `agent_id` to roadeep_chat.",
            no_arguments(),
            Effect::ReadOnly,
            true,
        ),
        tool(
            CHAT,
            "Chat with Roadeep",
            "Sends one message to Roadeep and waits for the full reply (up to ~3 minutes). Returns {reply, thread_id}. To continue the same conversation, pass the returned thread_id on the next call; without it a new thread starts. `model` only applies when starting a new thread.",
            json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The message to send.",
                        "minLength": 1,
                        "maxLength": MAX_MESSAGE_CHARS
                    },
                    "thread_id": id_schema("Continue this Roadeep thread (from a previous roadeep_chat result)."),
                    "model": {
                        "type": "string",
                        "description": "Model id from roadeep_list_models. Omit for the server default. Ignored when thread_id is given.",
                        "maxLength": MAX_ID_LEN
                    },
                    "agent_id": id_schema("Agent id from roadeep_list_agents."),
                    "web_search": { "type": "boolean", "description": "Let the model search the web." }
                },
                "required": ["message"],
                "additionalProperties": false
            }),
            Effect::Writes,
            false,
        ),
        tool(
            LIST_SUBTYPES,
            "List Roadeep generation products",
            "Lists Roadeep Generation Hub subtypes (image, video, music, … products) by slug and name. Next: roadeep_get_subtype for the members and their input schemas.",
            no_arguments(),
            Effect::ReadOnly,
            true,
        ),
        tool(
            GET_SUBTYPE,
            "Roadeep generation product details",
            "Shows one Generation Hub subtype: its member models with capability, availability, the default member, and each member's input_schema (the fields roadeep_estimate_generation expects in `input`).",
            json!({
                "type": "object",
                "properties": {
                    "slug": {
                        "type": "string",
                        "description": "Subtype slug from roadeep_list_subtypes.",
                        "minLength": 1,
                        "maxLength": 100,
                        "pattern": "^[A-Za-z0-9][A-Za-z0-9._-]*$"
                    }
                },
                "required": ["slug"],
                "additionalProperties": false
            }),
            Effect::ReadOnly,
            true,
        ),
        tool(
            ESTIMATE_GENERATION,
            "Estimate a Roadeep generation",
            "Prices a generation without running it: returns a quote with quote_id, expires_at, estimated cost and required credits. Free. Always show the cost to the user before calling roadeep_start_generation with the same body and this quote_id.",
            generation_schema(false),
            Effect::ReadOnly,
            false,
        ),
        tool(
            START_GENERATION,
            "Start a Roadeep generation (costs credits)",
            "Starts a generation and SPENDS ROADEEP CREDITS from the signed-in account. Only call it after roadeep_estimate_generation, after the user has seen and accepted the cost, with the same subtype/model, capability, input and controls plus that quote_id. The Roadeep desktop app then asks the user to confirm the quoted cost in its own dialog on their screen; if they decline or do not answer, the tool returns USER_DECLINED and nothing is charged — do not retry unless the user asks. Returns the generation id and status; then poll roadeep_generation_status. If the quote expired, is unknown (QUOTE_UNKNOWN) or no longer matches, estimate again.",
            generation_schema(true),
            Effect::Destructive,
            false,
        ),
        tool(
            GENERATION_STATUS,
            "Roadeep generation status",
            "Shows a generation's status (queued, running, succeeded, failed, cancelled), its output assets as {kind, url} once succeeded, the cost, and the error if it failed. Poll every few seconds until the status is terminal.",
            json!({
                "type": "object",
                "properties": { "id": id_schema("Generation id from roadeep_start_generation.") },
                "required": ["id"],
                "additionalProperties": false
            }),
            Effect::ReadOnly,
            true,
        ),
        tool(
            CANCEL_GENERATION,
            "Cancel a Roadeep generation",
            "Asks Roadeep to cancel a running generation. Best effort: it may already have finished; check roadeep_generation_status afterwards.",
            json!({
                "type": "object",
                "properties": { "id": id_schema("Generation id from roadeep_start_generation.") },
                "required": ["id"],
                "additionalProperties": false
            }),
            Effect::Writes,
            true,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_has_exactly_one_definition() {
        let defs = definitions();
        let names: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();
        assert_eq!(names, NAMES);
        for d in &defs {
            assert_eq!(d["inputSchema"]["type"], "object", "{}", d["name"]);
            assert!(!d["description"].as_str().unwrap().is_empty());
        }
    }

    #[test]
    fn starting_a_generation_requires_a_quote_and_says_it_costs() {
        let start = definitions().into_iter().find(|d| d["name"] == START_GENERATION).unwrap();
        let required = start["inputSchema"]["required"].as_array().unwrap();
        assert!(required.contains(&json!("quote_id")));
        assert!(start["description"].as_str().unwrap().contains("CREDITS"));
        let estimate = definitions().into_iter().find(|d| d["name"] == ESTIMATE_GENERATION).unwrap();
        assert!(!estimate["inputSchema"]["required"].as_array().unwrap().contains(&json!("quote_id")));
    }

    #[test]
    fn annotations_tell_clients_what_each_tool_does() {
        let defs = definitions();
        let hints = |name: &str| defs.iter().find(|d| d["name"] == name).unwrap()["annotations"].clone();

        let start = hints(START_GENERATION);
        assert_eq!(start["readOnlyHint"], false);
        assert_eq!(start["destructiveHint"], true, "spends credits");
        assert_eq!(start["openWorldHint"], true);

        for name in [WHOAMI, LIST_MODELS, LIST_AGENTS, LIST_SUBTYPES, GET_SUBTYPE, ESTIMATE_GENERATION, GENERATION_STATUS] {
            let h = hints(name);
            assert_eq!(h["readOnlyHint"], true, "{name}");
            assert_eq!(h["destructiveHint"], false, "{name}");
        }
        for name in [CHAT, CANCEL_GENERATION] {
            assert_eq!(hints(name)["readOnlyHint"], false, "{name}");
        }
    }
}
