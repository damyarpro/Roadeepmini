// Roadeep Generation Hub (`/generation/v1` on the API host): subtypes, cost
// quotes, paid submits, status and cancel. Mirrors the web client
// (src/network/generation-hub/client.ts) and the mobile one
// (src/features/generation/{hub,request}.ts).
//
// The flow is always quote → submit. A submit carries the quote id and an
// `Idempotency-Key` header, and its body must match the quoted body byte for
// byte apart from `quote_id` and `context` — so both are built by one function.
// Only the MCP tools use this for now; there is no in-island generation UI.

use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime};

use reqwest::header;
use serde_json::{json, Map, Value};

use super::http::{self, codes, Call, Raw, RoadeepError};
use super::{Refresh, Roadeep};
use crate::log;

const BASE: &str = "/generation/v1";
/// A submit is a write that spends credits; give it room but not forever.
const SUBMIT_TIMEOUT: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Inputs are prompts and URLs. Anything bigger is a mistake, not a request.
const MAX_INPUT_BYTES: usize = 64 * 1024;
const MAX_CAPABILITY_LEN: usize = 64;
const MAX_MODEL_LEN: usize = 200;

/// A quote that no longer matches its body: nothing was charged, estimate again.
pub const QUOTE_RETRY_CODES: &[&str] = &["QUOTE_EXPIRED", "QUOTE_NOT_FOUND", "QUOTE_MISMATCH", "QUOTE_STALE"];

// ── Validation and body building (pure) ───────────────────────────────────────

/// Generation, quote and thread ids: what the server issues, never a path.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Subtype slugs go into a URL path, so no slash, dot-dot or percent.
pub fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 100
        && slug.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && slug.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !slug.contains("..")
}

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Model(String),
    Subtype(String),
}

/// The canonical quote/submit body, validated.
#[derive(Debug, Clone, PartialEq)]
pub struct GenerationRequest {
    pub target: Target,
    pub capability: String,
    pub input: Map<String, Value>,
    pub controls: Option<Map<String, Value>>,
}

fn text_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn no_control_chars(s: &str) -> bool {
    !s.chars().any(char::is_control)
}

impl GenerationRequest {
    /// From tool arguments: exactly one of `model`/`subtype`, a capability, an
    /// `input` object and optional known `controls`.
    pub fn from_args(args: &Value) -> Result<Self, RoadeepError> {
        let target = match (text_arg(args, "model"), text_arg(args, "subtype")) {
            (Some(_), Some(_)) | (None, None) => {
                return Err(RoadeepError::validation("model", "Give exactly one of `subtype` or `model`."))
            }
            (Some(m), None) => {
                if m.len() > MAX_MODEL_LEN || !no_control_chars(m) {
                    return Err(RoadeepError::validation("model", "That model id is not valid."));
                }
                Target::Model(m.to_string())
            }
            (None, Some(s)) => {
                if !valid_slug(s) {
                    return Err(RoadeepError::validation("subtype", "That subtype slug is not valid."));
                }
                Target::Subtype(s.to_string())
            }
        };

        let capability = text_arg(args, "capability")
            .ok_or_else(|| RoadeepError::validation("capability", "`capability` is required."))?;
        let capability_ok = capability.len() <= MAX_CAPABILITY_LEN
            && capability.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/'));
        if !capability_ok {
            return Err(RoadeepError::validation("capability", "That capability is not valid."));
        }

        let input = match args.get("input") {
            Some(Value::Object(map)) => map.clone(),
            _ => return Err(RoadeepError::validation("input", "`input` must be an object of model inputs.")),
        };
        if Value::Object(input.clone()).to_string().len() > MAX_INPUT_BYTES {
            return Err(RoadeepError::validation("input", "`input` is too large."));
        }

        let controls = match args.get("controls") {
            None | Some(Value::Null) => None,
            Some(Value::Object(map)) => Some(validate_controls(map)?),
            Some(_) => return Err(RoadeepError::validation("controls", "`controls` must be an object.")),
        };

        Ok(Self { target, capability: capability.to_string(), input, controls: controls.filter(|c| !c.is_empty()) })
    }

    /// The body for `POST /cost-estimates/`. The live server rejects `context` here.
    pub fn quote_body(&self) -> Value {
        let mut body = Map::new();
        match &self.target {
            Target::Model(m) => body.insert("model".into(), json!(m)),
            Target::Subtype(s) => body.insert("subtype".into(), json!(s)),
        };
        body.insert("capability".into(), json!(self.capability));
        body.insert("input".into(), Value::Object(self.input.clone()));
        if let Some(controls) = &self.controls {
            body.insert("controls".into(), Value::Object(controls.clone()));
        }
        body.insert("routing".into(), json!({ "provider": "auto" }));
        Value::Object(body)
    }

    /// The quoted body plus `quote_id` and the standalone-generation context
    /// (without it the server opens a new chat thread per generation).
    pub fn submit_body(&self, quote_id: &str) -> Value {
        let mut body = self.quote_body();
        body["quote_id"] = json!(quote_id);
        body["context"] = json!({ "message_type": "generation" });
        body
    }
}

fn validate_controls(map: &Map<String, Value>) -> Result<Map<String, Value>, RoadeepError> {
    let mut out = Map::new();
    for (key, value) in map {
        match (key.as_str(), value) {
            ("refine_prompt" | "iranize_prompts", Value::Bool(_)) => {
                out.insert(key.clone(), value.clone());
            }
            ("preset_id", Value::String(s)) if valid_id(s.trim()) => {
                out.insert(key.clone(), json!(s.trim()));
            }
            ("refine_prompt" | "iranize_prompts" | "preset_id", _) => {
                return Err(RoadeepError::validation("controls", &format!("`controls.{key}` has the wrong type.")))
            }
            _ => return Err(RoadeepError::validation("controls", &format!("Unknown control `{key}`."))),
        }
    }
    Ok(out)
}

/// RFC 4122 v4, as the web and mobile clients send.
pub fn idempotency_key() -> String {
    uuid::Uuid::new_v4().to_string()
}

// ── Response shaping (pure) ───────────────────────────────────────────────────

fn pick(v: &Value, keys: &[&str]) -> Value {
    let mut out = Map::new();
    for key in keys {
        if let Some(x) = v.get(*key).filter(|x| !x.is_null()) {
            out.insert((*key).to_string(), x.clone());
        }
    }
    Value::Object(out)
}

/// `items` (live server) or `results` (documented shape) or a bare array.
fn page_items(data: &Value) -> Vec<Value> {
    data.get("items")
        .or_else(|| data.get("results"))
        .and_then(Value::as_array)
        .or_else(|| data.as_array())
        .cloned()
        .unwrap_or_default()
}

pub fn shape_subtypes(data: &Value) -> Value {
    let has_more = data.pointer("/pagination/has_next").and_then(Value::as_bool).unwrap_or(false)
        || data.get("next").and_then(Value::as_str).is_some_and(|s| !s.is_empty());
    let subtypes: Vec<Value> = page_items(data)
        .iter()
        .filter(|s| s.get("slug").and_then(Value::as_str).is_some())
        .map(|s| {
            let mut out = s.as_object().cloned().unwrap_or_default();
            // Members, schemas and artwork belong to roadeep_get_subtype.
            for heavy in ["members", "thumbnail_url", "icon_url", "image_url", "ui_config"] {
                out.remove(heavy);
            }
            Value::Object(out)
        })
        .collect();
    json!({ "subtypes": subtypes, "has_more": has_more })
}

pub fn shape_subtype(data: &Value) -> Value {
    let members: Vec<Value> = data
        .get("members")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|m| m.get("model").and_then(Value::as_str).is_some())
        .map(|m| {
            let name = m
                .get("custom_name")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .or_else(|| m.get("display_name").and_then(Value::as_str))
                .unwrap_or_else(|| m["model"].as_str().unwrap_or_default());
            let mut out = pick(m, &["model", "capability", "provider"]);
            out["name"] = json!(name);
            out["available"] = json!(m.get("available").and_then(Value::as_bool).unwrap_or(true));
            out["is_default"] = json!(m.get("is_default").and_then(Value::as_bool).unwrap_or(false));
            for (from, to) in [
                ("input_schema", "input_schema"),
                ("custom_ui_field_properties", "field_properties"),
                ("pricing", "pricing"),
            ] {
                if let Some(v) = m.get(from).filter(|v| v.as_object().is_some_and(|o| !o.is_empty())) {
                    out[to] = v.clone();
                }
            }
            out
        })
        .collect();
    let mut out = pick(data, &["slug", "name", "description", "capability", "category"]);
    out["members"] = Value::Array(members);
    out
}

/// The provider's own cost is internal margin; the user only sees theirs.
pub fn shape_quote(data: &Value) -> Value {
    let mut out = pick(data, &["quote_id", "expires_at", "model", "subtype", "capability", "provider", "confidence"]);
    if let Some(cost) = data.get("estimated_customer_cost") {
        out["estimated_cost"] = cost.clone();
    }
    if let Some(credits) = data.get("required_credits_estimate") {
        out["required_credits"] = credits.clone();
    }
    out
}

pub fn shape_submit(data: &Value) -> Value {
    pick(data, &["id", "status", "model", "subtype", "capability", "quote_id"])
}

pub fn is_terminal(status: &str) -> bool {
    matches!(status, "succeeded" | "failed" | "cancelled")
}

pub fn shape_generation(data: &Value) -> Value {
    let mut out = pick(data, &["id", "status", "model", "subtype", "capability"]);
    let status = data.get("status").and_then(Value::as_str).unwrap_or("");
    out["terminal"] = json!(is_terminal(status));
    let assets: Vec<Value> = data
        .pointer("/outputs/assets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|a| a.get("url").and_then(Value::as_str).is_some())
        .map(|a| pick(a, &["kind", "url"]))
        .collect();
    out["assets"] = Value::Array(assets);
    if let Some(cost) = data.get("cost").filter(|c| c.is_object()) {
        out["cost"] = pick(cost, &["status", "customer", "charged_credits"]);
    }
    if let Some(err) = data.get("error").filter(|e| !e.is_null()) {
        out["error"] = match err {
            Value::String(s) => json!({ "message": s }),
            other => pick(other, &["code", "message"]),
        };
    }
    out
}

// ── Calls ─────────────────────────────────────────────────────────────────────

pub async fn list_subtypes(rd: &Roadeep) -> Result<Value, RoadeepError> {
    let data = rd.request(Call::get(format!("{BASE}/subtypes/"))).await?;
    Ok(shape_subtypes(&data))
}

pub async fn get_subtype(rd: &Roadeep, slug: &str) -> Result<Value, RoadeepError> {
    if !valid_slug(slug) {
        return Err(RoadeepError::validation("slug", "That subtype slug is not valid."));
    }
    let data = rd.request(Call::get(format!("{BASE}/subtypes/{slug}/"))).await?;
    Ok(shape_subtype(&data))
}

pub async fn estimate(rd: &Roadeep, req: &GenerationRequest) -> Result<Value, RoadeepError> {
    let data = rd.request(Call::post(format!("{BASE}/cost-estimates/"), req.quote_body())).await?;
    Ok(shape_quote(&data))
}

pub async fn status(rd: &Roadeep, id: &str) -> Result<Value, RoadeepError> {
    if !valid_id(id) {
        return Err(RoadeepError::validation("id", "That generation id is not valid."));
    }
    let data = rd.request(Call::get(format!("{BASE}/generations/{id}/"))).await?;
    Ok(shape_generation(&data))
}

/// Best effort: the generation may already be terminal.
pub async fn cancel(rd: &Roadeep, id: &str) -> Result<Value, RoadeepError> {
    if !valid_id(id) {
        return Err(RoadeepError::validation("id", "That generation id is not valid."));
    }
    rd.request(Call::post(format!("{BASE}/generations/{id}/cancel/"), json!({}))).await?;
    Ok(json!({ "id": id, "cancel_requested": true }))
}

/// The paid step. One recovery only: a network error or 5xx means the server
/// may have accepted it, so it is resent once with the *same* key (a safe
/// replay). A stale quote is never re-quoted here — the user would be charged
/// a price nobody showed them — it comes back as an error to estimate again.
///
/// This spends money and does not ask anyone: callers must already hold a
/// human's approval of this quote (the MCP tool gets it from mcp::confirm).
pub async fn start(rd: &Roadeep, req: &GenerationRequest, quote_id: &str) -> Result<Value, RoadeepError> {
    if !valid_id(quote_id) {
        return Err(RoadeepError::validation("quote_id", "That quote id is not valid."));
    }
    let body = req.submit_body(quote_id);
    let key = idempotency_key();
    let result = match submit(rd, &body, &key).await {
        Err(err) if is_uncertain(&err) => {
            log::line(format!("roadeep: generation submit uncertain ({}), replaying with the same key", err.code));
            submit(rd, &body, &key).await
        }
        other => other,
    };
    match result {
        Ok(data) => Ok(shape_submit(&data)),
        Err(mut err) if QUOTE_RETRY_CODES.contains(&err.code.as_str()) => {
            err.message = format!(
                "{} The quote is no longer valid; nothing was charged. Call roadeep_estimate_generation again and confirm the new cost.",
                err.message.trim_end()
            );
            Err(err)
        }
        Err(err) => Err(err),
    }
}

fn is_uncertain(err: &RoadeepError) -> bool {
    err.code == codes::NETWORK || err.code == codes::TIMEOUT || err.status.is_some_and(|s| s >= 500)
}

/// `Call` has no custom headers, so the submit goes out on its own client with
/// the same rules as `Roadeep::request`: Bearer only to the API host, 401 →
/// one shared refresh → one retry, the envelope unwrapped, errors normalized.
async fn submit(rd: &Roadeep, body: &Value, key: &str) -> Result<Value, RoadeepError> {
    let path = format!("{BASE}/generations/");
    if !rd.session.has_session() {
        return Err(RoadeepError::new(codes::NOT_SIGNED_IN, "Sign in to Roadeep first."));
    }
    let generation = rd.session.generation();
    let token = match rd.session.access() {
        Some(token) => token,
        // Refresh token only (e.g. the access token was never persisted): refresh first.
        None => match rd.refresh(None).await {
            Refresh::Ok(access) => access,
            _ => return Err(RoadeepError::new(codes::SESSION_EXPIRED, "Your Roadeep session expired. Sign in again.")),
        },
    };
    let mut raw = send_submit(rd, &path, body, key, &token).await?;
    if raw.status == 401 {
        match rd.refresh(Some(&token)).await {
            Refresh::Ok(access) => raw = send_submit(rd, &path, body, key, &access).await?,
            Refresh::Invalid => {
                if rd.session.generation() == generation && rd.session.has_session() {
                    rd.end_session("expired", false);
                }
                let mut err = RoadeepError::new(codes::SESSION_EXPIRED, "Your Roadeep session expired. Sign in again.");
                err.status = Some(401);
                err.request_id = raw.request_id;
                return Err(err);
            }
            Refresh::Retriable => {}
        }
    }
    raw.into_result().inspect_err(|err| {
        log::line(format!(
            "roadeep POST {path} error {} status={} req={}",
            err.code,
            err.status.map(|s| s.to_string()).unwrap_or_else(|| "-".into()),
            err.request_id.as_deref().unwrap_or("-")
        ));
    })
}

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        http::proxy_policy(reqwest::Client::builder())
            .connect_timeout(CONNECT_TIMEOUT)
            .user_agent(concat!("Roadeep-Windows/", env!("CARGO_PKG_VERSION")))
            .https_only(http::base_url().starts_with("https://"))
            .build()
            .unwrap_or_else(|err| {
                log::line(format!("roadeep: generation client builder failed ({err}), using defaults"));
                reqwest::Client::new()
            })
    })
}

async fn send_submit(rd: &Roadeep, path: &str, body: &Value, key: &str, token: &str) -> Result<Raw, RoadeepError> {
    let base = rd.transport.base();
    let url = format!("{base}{path}");
    if !http::is_trusted(&url, base) {
        return Err(RoadeepError::new(codes::NETWORK, "Refused to send credentials to a foreign URL."));
    }
    let started = Instant::now();
    let response = client()
        .post(&url)
        .timeout(SUBMIT_TIMEOUT)
        .header(header::ACCEPT, "application/json")
        .header("Idempotency-Key", key)
        .bearer_auth(token)
        .json(body)
        .send()
        .await
        .map_err(|err| {
            let code = if err.is_timeout() { codes::TIMEOUT } else { codes::NETWORK };
            // reqwest's Display never includes headers, so no token can leak here.
            log::line(format!("roadeep POST {path} failed {code} {}ms: {err}", started.elapsed().as_millis()));
            RoadeepError::new(code, http::fallback_message(None))
        })?;

    let status = response.status().as_u16();
    let request_id = response.headers().get("x-request-id").and_then(|v| v.to_str().ok()).map(str::to_string);
    let retry_after = http::parse_retry_after(
        response.headers().get(header::RETRY_AFTER).and_then(|v| v.to_str().ok()),
        SystemTime::now(),
    );
    let text = response.text().await.map_err(|err| {
        log::line(format!("roadeep POST {path} body read failed: {err}"));
        RoadeepError::new(codes::NETWORK, http::fallback_message(None))
    })?;
    log::line(format!(
        "roadeep POST {path} {status} {}ms req={}",
        started.elapsed().as_millis(),
        request_id.as_deref().unwrap_or("-")
    ));
    Ok(Raw { status, request_id, retry_after, has_more: None, body: http::parse_body(&text) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: Value) -> Result<GenerationRequest, RoadeepError> {
        GenerationRequest::from_args(&v)
    }

    #[test]
    fn exactly_one_of_model_or_subtype() {
        let base = json!({ "capability": "text-to-image", "input": { "prompt": "a cat" } });
        let mut both = base.clone();
        both["model"] = json!("fal-ai/flux");
        both["subtype"] = json!("image");
        assert_eq!(args(both).unwrap_err().code, codes::VALIDATION);
        assert_eq!(args(base.clone()).unwrap_err().code, codes::VALIDATION);

        let mut by_subtype = base.clone();
        by_subtype["subtype"] = json!("image-gen");
        assert_eq!(args(by_subtype).unwrap().target, Target::Subtype("image-gen".into()));
        let mut by_model = base;
        by_model["model"] = json!("fal-ai/flux/dev");
        assert_eq!(args(by_model).unwrap().target, Target::Model("fal-ai/flux/dev".into()));
    }

    #[test]
    fn bad_arguments_are_refused() {
        let ok = json!({ "subtype": "image", "capability": "text-to-image", "input": { "prompt": "x" } });
        for (key, bad) in [
            ("subtype", json!("../admin")),
            ("subtype", json!("a/b")),
            ("capability", json!("")),
            ("capability", json!("text to image")),
            ("input", json!("a cat")),
            ("input", json!(["a cat"])),
            ("controls", json!({ "refine_prompt": "yes" })),
            ("controls", json!({ "unknown": true })),
            ("controls", json!([1])),
        ] {
            let mut v = ok.clone();
            v[key] = bad.clone();
            assert!(args(v).is_err(), "{key} = {bad}");
        }
        let mut huge = ok.clone();
        huge["input"]["prompt"] = json!("x".repeat(MAX_INPUT_BYTES));
        assert!(args(huge).is_err());
    }

    #[test]
    fn quote_and_submit_bodies_match_apart_from_quote_id_and_context() {
        let req = args(json!({
            "subtype": "image",
            "capability": "text-to-image",
            "input": { "prompt": "a cat", "num_images": 1 },
            "controls": { "refine_prompt": true }
        }))
        .unwrap();
        let quote = req.quote_body();
        assert_eq!(
            quote,
            json!({
                "subtype": "image",
                "capability": "text-to-image",
                "input": { "prompt": "a cat", "num_images": 1 },
                "controls": { "refine_prompt": true },
                "routing": { "provider": "auto" }
            })
        );
        assert!(quote.get("context").is_none(), "the live server rejects context on quotes");

        let mut submit = req.submit_body("q-123");
        assert_eq!(submit["quote_id"], "q-123");
        assert_eq!(submit["context"]["message_type"], "generation");
        let obj = submit.as_object_mut().unwrap();
        obj.remove("quote_id");
        obj.remove("context");
        assert_eq!(submit, quote);

        // No controls → no controls key at all.
        let bare = args(json!({ "model": "m", "capability": "c", "input": {}, "controls": {} })).unwrap();
        assert!(bare.quote_body().get("controls").is_none());
    }

    #[test]
    fn ids_and_slugs() {
        assert!(valid_id("3f1c2b9e-0000-4000-8000-123456789abc"));
        for bad in ["", "../x", "a/b", "a b", "a%2f", &"x".repeat(129)] {
            assert!(!valid_id(bad), "{bad}");
        }
        assert!(valid_slug("text-to-image"));
        assert!(valid_slug("video_v2.1"));
        for bad in ["", ".hidden", "a/b", "a..b", "a%20", "a?b"] {
            assert!(!valid_slug(bad), "{bad}");
        }
        let a = idempotency_key();
        assert_eq!(a.len(), 36);
        assert_ne!(a, idempotency_key());
    }

    #[test]
    fn quotes_hide_the_provider_cost() {
        let q = shape_quote(&json!({
            "quote_id": "q1", "expires_at": "2026-10-01T10:00:00Z", "model": null, "subtype": "image",
            "capability": "text-to-image", "provider": "fal", "provider_endpoint_id": "e",
            "estimated_provider_cost": { "amount": "0.01", "currency": "USD" },
            "estimated_customer_cost": { "amount": "1200", "currency": "IRT" },
            "required_credits_estimate": 3, "confidence": "high"
        }));
        assert_eq!(q["quote_id"], "q1");
        assert_eq!(q["estimated_cost"]["amount"], "1200");
        assert_eq!(q["required_credits"], 3);
        assert!(q.get("estimated_provider_cost").is_none());
        assert!(q.get("model").is_none(), "nulls are dropped");
    }

    #[test]
    fn generation_status_shape() {
        let g = shape_generation(&json!({
            "id": "g1", "status": "succeeded", "capability": "text-to-image",
            "outputs": { "assets": [ { "id": "a", "kind": "image", "url": "https://cdn/x.png", "meta": {} }, { "kind": "image" } ] },
            "cost": { "status": "settled", "customer": { "amount": "1", "currency": "IRT" }, "provider": { "amount": "0.1" }, "charged_credits": 2 },
            "error": null
        }));
        assert_eq!(g["terminal"], true);
        assert_eq!(g["assets"], json!([{ "kind": "image", "url": "https://cdn/x.png" }]));
        assert!(g["cost"].get("provider").is_none());
        assert!(g.get("error").is_none());

        let f = shape_generation(&json!({ "id": "g2", "status": "running" }));
        assert_eq!(f["terminal"], false);
        let e = shape_generation(&json!({ "id": "g3", "status": "failed", "error": { "code": "PROVIDER_UNAVAILABLE", "message": "down", "details": {} } }));
        assert_eq!(e["error"], json!({ "code": "PROVIDER_UNAVAILABLE", "message": "down" }));
    }

    #[test]
    fn subtype_shapes() {
        let list = shape_subtypes(&json!({ "items": [ { "slug": "image", "name": "Image", "members": [1] }, { "name": "no slug" } ], "pagination": { "has_next": true } }));
        assert_eq!(list["subtypes"].as_array().unwrap().len(), 1);
        assert!(list["subtypes"][0].get("members").is_none());
        assert_eq!(list["has_more"], true);
        assert_eq!(shape_subtypes(&json!({ "results": [ { "slug": "a" } ] }))["has_more"], false);

        let detail = shape_subtype(&json!({
            "slug": "image", "name": "Image",
            "members": [
                { "model": "m1", "display_name": "M1", "custom_name": "Fancy", "capability": "text-to-image",
                  "input_schema": { "type": "object" }, "custom_ui_field_properties": {}, "is_default": true, "thumbnail_url": "/x" },
                { "model": "m2", "available": false },
                { "display_name": "no model" }
            ]
        }));
        let members = detail["members"].as_array().unwrap();
        assert_eq!(members.len(), 2);
        assert_eq!(members[0]["name"], "Fancy");
        assert_eq!(members[0]["is_default"], true);
        assert_eq!(members[0]["input_schema"], json!({ "type": "object" }));
        assert!(members[0].get("field_properties").is_none(), "empty objects are dropped");
        assert!(members[0].get("thumbnail_url").is_none());
        assert_eq!(members[1]["available"], false);
        assert_eq!(members[1]["name"], "m2");
    }
}
