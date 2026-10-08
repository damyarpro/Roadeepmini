// Manifest schema v1 (catalog/SCHEMA.md) and its validation.
//
// A manifest is data from the repository, but it decides where the user's key
// is sent, so it is checked as if it were input: unknown keys are refused
// (a typo would otherwise silently change behaviour), every URL is https,
// every placeholder names a declared field, every request lands on a host the
// manifest declares, and secrets never appear in a URL a browser would see.

use std::collections::{BTreeMap, HashSet};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::template::{scalar_text, valid_field_name, valid_pointer, Context, HostRule, Part, Template, Values};

pub const SCHEMA: u64 = 1;

pub const CATEGORIES: &[&str] =
    &["payments", "dev", "monitoring", "work", "comms", "automation", "commerce", "support", "marketing", "content"];

/// What the island shows per item.
pub const STATUSES: &[&str] = &["ok", "info", "warn", "err", "off"];

/// Headers that belong to the HTTP client, never to a manifest.
const TRANSPORT_HEADERS: &[&str] = &["host", "content-length", "transfer-encoding", "connection", "user-agent", "cookie"];

/// Literal request headers may not carry credentials either: those go through
/// `auth`, which reads them from the Credential Manager.
const CREDENTIAL_HEADERS: &[&str] = &["authorization", "proxy-authorization"];

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Text {
    pub fa: String,
    pub en: String,
}

impl Text {
    pub fn get(&self, language: &str) -> &str {
        if language == "en" { &self.en } else { &self.fa }
    }
}

// ── Raw JSON shape ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RawManifest {
    schema: u64,
    id: String,
    name: String,
    category: String,
    color: String,
    desc: Text,
    key_url: String,
    #[serde(default)]
    docs_url: Option<String>,
    fields: Vec<RawField>,
    #[serde(default)]
    auth: Vec<RawAuth>,
    hosts: Vec<String>,
    request: RawRequest,
    list: RawList,
    #[serde(default)]
    status_map: BTreeMap<String, String>,
    #[serde(default)]
    count: Option<String>,
    #[serde(default)]
    open_url: Option<String>,
    #[serde(default)]
    web_hosts: Vec<String>,
    poll_every: u64,
    #[serde(default)]
    notify: Option<RawNotify>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawField {
    name: String,
    kind: String,
    label: Text,
    #[serde(default)]
    placeholder: String,
    #[serde(default)]
    optional: bool,
    pattern: String,
    #[serde(default)]
    help: Option<Text>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, tag = "type", rename_all = "lowercase")]
enum RawAuth {
    Bearer { field: String },
    Header { name: String, field: String, #[serde(default)] prefix: String },
    /// `pass` may be left out for an empty password (Stripe-style "key:").
    Basic { user: String, #[serde(default)] pass: Option<String> },
    Query { name: String, field: String },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRequest {
    method: String,
    url: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    query: BTreeMap<String, String>,
    #[serde(default)]
    body: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawList {
    path: String,
    max: u64,
    #[serde(default)]
    sort: Option<String>,
    #[serde(default)]
    id: Option<String>,
    title: String,
    #[serde(default)]
    subtitle: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    time: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNotify {
    on: String,
    #[serde(default)]
    statuses: Vec<String>,
}

// ── Validated service ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Secret,
    Text,
    Url,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Secret => "secret",
            Kind::Text => "text",
            Kind::Url => "url",
        }
    }
}

#[derive(Debug)]
pub struct Field {
    pub name: String,
    /// Credential Manager key: "x.<id>.<name>".
    pub key: String,
    pub kind: Kind,
    pub label: Text,
    pub placeholder: String,
    pub optional: bool,
    /// The manifest's pattern wrapped in `^(?:…)$`: anchored even if it uses `|`.
    pub pattern: Regex,
    pub help: Option<Text>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Auth {
    Bearer { field: String },
    Header { name: String, field: String, prefix: String },
    Basic { user: String, pass: Option<String> },
    Query { name: String, field: String },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Method {
    Get,
    Post,
}

#[derive(Debug)]
pub struct Request {
    pub method: Method,
    pub url: Template,
    pub headers: Vec<(String, String)>,
    pub query: Vec<(String, Template)>,
    pub body: Option<Value>,
}

#[derive(Debug)]
pub struct List {
    pub path: String,
    pub max: usize,
    pub sort_time: bool,
    pub id: Option<String>,
    pub title: String,
    pub subtitle: Option<String>,
    pub status: Option<String>,
    pub time: Option<String>,
    pub url: Option<Template>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NotifyOn {
    New,
    Status,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Notify {
    pub on: NotifyOn,
    /// Mapped statuses that raise it; empty = any.
    pub statuses: Vec<&'static str>,
}

#[derive(Debug)]
pub struct Service {
    pub id: String,
    /// "integration_<id>", the pill / task id.
    pub pill: String,
    pub name: String,
    pub category: String,
    pub color: String,
    pub desc: Text,
    pub key_url: String,
    pub docs_url: Option<String>,
    pub fields: Vec<Field>,
    pub auth: Vec<Auth>,
    /// As written, for display ("your key only goes to …").
    pub host_entries: Vec<String>,
    pub hosts: Vec<HostRule>,
    pub request: Request,
    pub list: List,
    /// Lowercased raw status → mapped status.
    pub status_map: Vec<(String, &'static str)>,
    /// The "*" entry, else "info".
    pub default_status: &'static str,
    pub count: Option<String>,
    pub open_url_raw: Option<String>,
    pub open_url: Option<Template>,
    pub web_hosts: Vec<HostRule>,
    pub poll_every: u64,
    pub notify: Option<Notify>,
}

impl Service {
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    pub fn map_status(&self, raw: Option<&str>) -> &'static str {
        raw.map(str::to_lowercase)
            .and_then(|r| self.status_map.iter().find(|(k, _)| *k == r).map(|(_, v)| *v))
            .unwrap_or(self.default_status)
    }
}

pub fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    (2..=31).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

fn is_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

/// `^…$`, where the `$` is not an escaped dollar.
fn anchored(pattern: &str) -> bool {
    let Some(body) = pattern.strip_prefix('^').and_then(|p| p.strip_suffix('$')) else { return false };
    let trailing_backslashes = body.chars().rev().take_while(|c| *c == '\\').count();
    trailing_backslashes % 2 == 0
}

/// A literal https page (key / docs links).
fn https_link(s: &str) -> bool {
    s.len() <= 500
        && reqwest::Url::parse(s).is_ok_and(|u| {
            u.scheme() == "https" && u.host_str().is_some() && u.username().is_empty() && u.password().is_none()
        })
}

/// Collects every problem of one manifest, so a contributor sees them all at once.
struct Problems(Vec<String>);

impl Problems {
    fn check(&mut self, ok: bool, message: impl FnOnce() -> String) {
        if !ok {
            self.0.push(message());
        }
    }
}

/// Parses and validates one manifest. `file` is its file name ("<id>.json");
/// `native` the reserved ids of the hand-coded services.
pub fn parse(file: &str, json: &str, native: &[&str]) -> Result<Service, Vec<String>> {
    let raw: RawManifest = serde_json::from_str(json).map_err(|e| vec![format!("not a valid manifest: {e}")])?;
    validate(file, raw, native)
}

fn validate(file: &str, raw: RawManifest, native: &[&str]) -> Result<Service, Vec<String>> {
    let mut p = Problems(Vec::new());

    p.check(raw.schema == SCHEMA, || format!("schema must be {SCHEMA}"));
    p.check(valid_id(&raw.id), || format!("id {:?} must match ^[a-z0-9][a-z0-9-]{{1,30}}$", raw.id));
    p.check(!native.contains(&raw.id.as_str()), || format!("id {:?} is reserved for a built-in service", raw.id));
    p.check(file == format!("{}.json", raw.id), || format!("file must be named {}.json", raw.id));
    p.check(!raw.name.trim().is_empty() && chars(&raw.name) <= 40, || "name must be 1–40 characters".into());
    p.check(CATEGORIES.contains(&raw.category.as_str()), || format!("category {:?} is not one of {CATEGORIES:?}", raw.category));
    p.check(is_color(&raw.color), || format!("color {:?} must be #RRGGBB", raw.color));
    for (lang, text) in [("fa", &raw.desc.fa), ("en", &raw.desc.en)] {
        p.check(!text.trim().is_empty() && chars(text) <= 90, || format!("desc.{lang} must be 1–90 characters"));
        p.check(!text.contains('\n'), || format!("desc.{lang} must be one line"));
    }
    p.check(https_link(&raw.key_url), || "keyUrl must be an https URL".into());
    if let Some(docs) = &raw.docs_url {
        p.check(https_link(docs), || "docsUrl must be an https URL".into());
    }

    // Fields.
    p.check((1..=6).contains(&raw.fields.len()), || "fields: 1–6 entries".into());
    let mut fields: Vec<Field> = Vec::new();
    for f in &raw.fields {
        let at = format!("field {:?}", f.name);
        p.check(valid_field_name(&f.name), || format!("{at}: name must match ^[A-Za-z][A-Za-z0-9_]{{0,31}}$"));
        p.check(!fields.iter().any(|g| g.name == f.name), || format!("{at}: duplicate name"));
        let kind = match f.kind.as_str() {
            "secret" => Some(Kind::Secret),
            "text" => Some(Kind::Text),
            "url" => Some(Kind::Url),
            other => {
                p.0.push(format!("{at}: kind {other:?} must be secret, text or url"));
                None
            }
        };
        for (lang, text) in [("fa", &f.label.fa), ("en", &f.label.en)] {
            p.check(!text.trim().is_empty() && chars(text) <= 60, || format!("{at}: label.{lang} must be 1–60 characters"));
        }
        p.check(chars(&f.placeholder) <= 120, || format!("{at}: placeholder is over 120 characters"));
        if let Some(help) = &f.help {
            for (lang, text) in [("fa", &help.fa), ("en", &help.en)] {
                p.check(!text.trim().is_empty() && chars(text) <= 300, || format!("{at}: help.{lang} must be 1–300 characters"));
            }
        }
        p.check(f.pattern.len() <= 300, || format!("{at}: pattern is over 300 characters"));
        p.check(anchored(&f.pattern), || format!("{at}: pattern must start with ^ and end with $"));
        let compiled = regex::RegexBuilder::new(&format!("^(?:{})$", f.pattern)).size_limit(1 << 20).build();
        if let Err(e) = &compiled {
            p.0.push(format!("{at}: pattern does not compile: {e}"));
        }
        if let (Some(kind), Ok(pattern)) = (kind, compiled) {
            fields.push(Field {
                name: f.name.clone(),
                key: format!("x.{}.{}", raw.id, f.name),
                kind,
                label: f.label.clone(),
                placeholder: f.placeholder.clone(),
                optional: f.optional,
                pattern,
                help: f.help.clone(),
            });
        }
    }
    let kind_of = |name: &str| fields.iter().find(|f| f.name == name).map(|f| f.kind);
    let mut used: HashSet<String> = HashSet::new();

    // Auth.
    p.check(raw.auth.len() <= 4, || "auth: at most 4 parts".into());
    let mut auth = Vec::new();
    for a in &raw.auth {
        let (refs, part): (Vec<&str>, Auth) = match a {
            RawAuth::Bearer { field } => (vec![field], Auth::Bearer { field: field.clone() }),
            RawAuth::Header { name, field, prefix } => {
                let lower = name.to_ascii_lowercase();
                p.check(
                    reqwest::header::HeaderName::from_bytes(name.as_bytes()).is_ok() && !TRANSPORT_HEADERS.contains(&lower.as_str()),
                    || format!("auth header name {name:?} is invalid or reserved"),
                );
                p.check(chars(prefix) <= 40 && !prefix.chars().any(char::is_control), || "auth header prefix: at most 40 plain characters".into());
                (vec![field], Auth::Header { name: name.clone(), field: field.clone(), prefix: prefix.clone() })
            }
            RawAuth::Basic { user, pass } => {
                let mut refs = vec![user.as_str()];
                refs.extend(pass.as_deref());
                (refs, Auth::Basic { user: user.clone(), pass: pass.clone() })
            }
            RawAuth::Query { name, field } => {
                p.check(
                    !name.is_empty() && name.len() <= 40 && name.chars().all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c)),
                    || format!("auth query name {name:?} is invalid"),
                );
                (vec![field], Auth::Query { name: name.clone(), field: field.clone() })
            }
        };
        for r in refs {
            p.check(kind_of(r).is_some(), || format!("auth refers to undeclared field {r:?}"));
            used.insert(r.to_string());
        }
        auth.push(part);
    }

    // Hosts.
    p.check((1..=8).contains(&raw.hosts.len()), || "hosts: 1–8 entries".into());
    p.check(raw.web_hosts.len() <= 8, || "webHosts: at most 8 entries".into());
    let host_rules = |entries: &[String], what: &str, p: &mut Problems, used: &mut HashSet<String>| -> Vec<HostRule> {
        let mut rules = Vec::new();
        for entry in entries {
            match HostRule::parse(entry) {
                Ok(rule) => {
                    if let HostRule::Field(name) = &rule {
                        p.check(kind_of(name) == Some(Kind::Url), || format!("{what} entry {entry:?} must name a url-kind field"));
                        used.insert(name.clone());
                    }
                    rules.push(rule);
                }
                Err(e) => p.0.push(format!("{what}: {e}")),
            }
        }
        rules
    };
    let hosts = host_rules(&raw.hosts, "hosts", &mut p, &mut used);
    let web_hosts = host_rules(&raw.web_hosts, "webHosts", &mut p, &mut used);

    // Placeholders: declared fields only, |base only on url kinds, never a secret.
    let mut check_template = |text: &str, context: Context, what: &str, p: &mut Problems| -> Option<Template> {
        match Template::parse(text, context) {
            Ok(t) => {
                for (name, base) in t.fields() {
                    match kind_of(name) {
                        None => p.0.push(format!("{what}: {{field.{name}}} is not a declared field")),
                        Some(Kind::Secret) => {
                            p.0.push(format!("{what}: secret field {name:?} may only be used through auth"))
                        }
                        Some(kind) => p.check(!base || kind == Kind::Url, || format!("{what}: |base needs a url-kind field")),
                    }
                    used.insert(name.to_string());
                }
                Some(t)
            }
            Err(e) => {
                p.0.push(format!("{what}: {e}"));
                None
            }
        }
    };

    // Request.
    let method = match raw.request.method.as_str() {
        "GET" => Method::Get,
        "POST" => Method::Post,
        other => {
            p.0.push(format!("request.method {other:?} must be GET or POST"));
            Method::Get
        }
    };
    p.check(raw.request.url.len() <= 500, || "request.url is over 500 characters".into());
    let url = check_template(&raw.request.url, Context::RequestUrl, "request.url", &mut p);
    p.check(raw.request.headers.len() <= 10, || "request.headers: at most 10".into());
    let mut headers = Vec::new();
    for (name, value) in &raw.request.headers {
        let lower = name.to_ascii_lowercase();
        p.check(
            reqwest::header::HeaderName::from_bytes(name.as_bytes()).is_ok()
                && !TRANSPORT_HEADERS.contains(&lower.as_str())
                && !CREDENTIAL_HEADERS.contains(&lower.as_str()),
            || format!("request header {name:?} is invalid or reserved"),
        );
        p.check(
            value.len() <= 200 && reqwest::header::HeaderValue::from_str(value).is_ok() && !value.contains("{field.") && !value.contains("{item."),
            || format!("request header {name:?}: literal value up to 200 characters (keys go through auth)"),
        );
        headers.push((name.clone(), value.clone()));
    }
    p.check(raw.request.query.len() <= 20, || "request.query: at most 20".into());
    let mut query = Vec::new();
    for (name, value) in &raw.request.query {
        p.check(!name.is_empty() && name.len() <= 60 && !name.contains('{'), || format!("request.query name {name:?} is invalid"));
        p.check(value.len() <= 500, || format!("request.query {name:?} is over 500 characters"));
        if let Some(t) = check_template(value, Context::Value, &format!("request.query {name:?}"), &mut p) {
            query.push((name.clone(), t));
        }
    }
    let body = match (&raw.request.body, method) {
        (None | Some(Value::Null), _) => None,
        (Some(_), Method::Get) => {
            p.0.push("request.body needs method POST".into());
            None
        }
        (Some(body), Method::Post) => {
            p.check(body.to_string().len() <= 4000, || "request.body is over 4000 characters".into());
            let mut strings = Vec::new();
            collect_strings(body, &mut strings);
            for s in strings {
                check_template(s, Context::Value, "request.body", &mut p);
            }
            Some(body.clone())
        }
    };

    // List.
    let l = &raw.list;
    p.check(valid_pointer(&l.path), || "list.path must be \"\" or a JSON pointer like \"/data\"".into());
    p.check((1..=20).contains(&l.max), || "list.max must be 1–20".into());
    p.check(l.sort.as_deref().is_none_or(|s| s == "time"), || "list.sort can only be \"time\"".into());
    p.check(l.sort.is_none() || l.time.is_some(), || "list.sort \"time\" needs list.time".into());
    for (what, pointer) in [
        ("id", l.id.as_deref()),
        ("title", Some(l.title.as_str())),
        ("subtitle", l.subtitle.as_deref()),
        ("status", l.status.as_deref()),
        ("time", l.time.as_deref()),
    ] {
        if let Some(pointer) = pointer {
            p.check(valid_pointer(pointer), || format!("list.{what} must be a JSON pointer like \"/name\""));
        }
    }
    let item_url = l.url.as_ref().and_then(|u| {
        p.check(u.len() <= 500, || "list.url is over 500 characters".into());
        check_template(u, Context::ItemUrl, "list.url", &mut p)
    });
    if l.url.is_some() {
        p.check(!web_hosts.is_empty(), || "list.url needs webHosts".into());
    }

    // Status map.
    p.check(raw.status_map.len() <= 40, || "statusMap: at most 40 entries".into());
    let mut status_map: Vec<(String, &'static str)> = Vec::new();
    let mut default_status = "info";
    for (key, value) in &raw.status_map {
        let mapped = STATUSES.iter().find(|s| **s == value.as_str()).copied();
        p.check(mapped.is_some(), || format!("statusMap {key:?}: {value:?} must be one of {STATUSES:?}"));
        p.check(!key.is_empty() && chars(key) <= 60, || format!("statusMap key {key:?} must be 1–60 characters"));
        let lower = key.to_lowercase();
        p.check(!status_map.iter().any(|(k, _)| *k == lower), || format!("statusMap key {key:?} appears twice (keys ignore case)"));
        if let Some(mapped) = mapped {
            if key == "*" {
                default_status = mapped;
            } else {
                status_map.push((lower, mapped));
            }
        }
    }

    if let Some(count) = &raw.count {
        p.check(valid_pointer(count) && !count.is_empty(), || "count must be a JSON pointer like \"/total\"".into());
    }

    // Open URL.
    let open_url = raw.open_url.as_ref().and_then(|u| {
        p.check(u.len() <= 500, || "openUrl is over 500 characters".into());
        check_template(u, Context::OpenUrl, "openUrl", &mut p)
    });
    if raw.open_url.is_some() {
        p.check(!web_hosts.is_empty(), || "openUrl needs webHosts".into());
    }

    p.check((60..=3600).contains(&raw.poll_every), || "pollEvery must be 60–3600 seconds".into());

    let notify = raw.notify.as_ref().map(|n| {
        let on = match n.on.as_str() {
            "new" => NotifyOn::New,
            "status" => NotifyOn::Status,
            other => {
                p.0.push(format!("notify.on {other:?} must be \"new\" or \"status\""));
                NotifyOn::New
            }
        };
        p.check(l.id.is_some(), || "notify needs list.id to tell items apart".into());
        p.check(on != NotifyOn::Status || l.status.is_some(), || "notify on \"status\" needs list.status".into());
        let mut statuses = Vec::new();
        for s in &n.statuses {
            match STATUSES.iter().find(|x| **x == s.as_str()) {
                Some(x) => statuses.push(*x),
                None => p.0.push(format!("notify.statuses: {s:?} must be one of {STATUSES:?}")),
            }
        }
        Notify { on, statuses }
    });

    for f in &fields {
        p.check(used.contains(&f.name), || format!("field {:?} is never used", f.name));
    }

    if !p.0.is_empty() {
        return Err(p.0);
    }
    // Every template that failed to parse has already added a problem above.
    let Some(url) = url else { return Err(vec!["request.url is invalid".into()]) };

    let service = Service {
        pill: format!("integration_{}", raw.id),
        id: raw.id,
        name: raw.name,
        category: raw.category,
        color: raw.color.to_ascii_uppercase(),
        desc: raw.desc,
        key_url: raw.key_url,
        docs_url: raw.docs_url,
        fields,
        auth,
        host_entries: raw.hosts,
        hosts,
        request: Request { method, url, headers, query, body },
        list: List {
            path: l.path.clone(),
            max: l.max as usize,
            sort_time: l.sort.is_some(),
            id: l.id.clone(),
            title: l.title.clone(),
            subtitle: l.subtitle.clone(),
            status: l.status.clone(),
            time: l.time.clone(),
            url: item_url,
        },
        status_map,
        default_status,
        count: raw.count,
        open_url_raw: raw.open_url,
        open_url,
        web_hosts,
        poll_every: raw.poll_every,
        notify,
    };
    dry_run(&service).map_err(|e| vec![e])?;
    Ok(service)
}

fn collect_strings<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
    match v {
        Value::String(s) => out.push(s),
        Value::Array(a) => a.iter().for_each(|x| collect_strings(x, out)),
        Value::Object(o) => o.values().for_each(|x| collect_strings(x, out)),
        _ => {}
    }
}

/// Expands the URLs with stand-in values and checks where they would go: the
/// request must land on `hosts`, links on `webHosts`. The engine checks again
/// with the real values on every poll; this catches a wrong manifest early.
fn dry_run(s: &Service) -> Result<(), String> {
    let samples_with = |text: &str| -> Values {
        s.fields
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let v = if f.kind == Kind::Url { format!("https://field{i}.example.com") } else { text.to_string() };
                (f.name.clone(), v)
            })
            .collect()
    };
    let samples = samples_with("x1");
    let other = samples_with("y2");
    let check = |t: &Template, rules: &[HostRule], what: &str, item: Option<&Value>| -> Result<(), String> {
        let text = t.expand(&samples, item, true).ok_or_else(|| format!("{what}: could not expand"))?;
        let url = reqwest::Url::parse(&text).map_err(|e| format!("{what}: {text:?} is not a URL ({e})"))?;
        if url.fragment().is_some() && what == "request.url" {
            return Err(format!("{what}: no #fragment"));
        }
        if super::template::url_allowed(&url, rules, &samples) {
            return Ok(());
        }
        // A host built from a text field ("api.{field.site}") can't be judged
        // with a stand-in value; the engine checks the real one on every poll.
        let host_of = |values: &Values| {
            t.expand(values, item, true).and_then(|u| reqwest::Url::parse(&u).ok()).and_then(|u| u.host_str().map(str::to_string))
        };
        if url.scheme() == "https" && host_of(&samples) != host_of(&other) {
            return Ok(());
        }
        Err(format!("{what}: {text:?} must be https and on one of the declared {}", if what == "request.url" { "hosts" } else { "webHosts" }))
    };
    check(&s.request.url, &s.hosts, "request.url", None)?;
    if let Some(t) = &s.open_url {
        check(t, &s.web_hosts, "openUrl", None)?;
    }
    if let Some(t) = s.list.url.as_ref().filter(|t| !t.is_raw_item_url()) {
        // Every item placeholder gets "1".
        let mut item = serde_json::Map::new();
        for part in &t.parts {
            if let Part::Item { pointer, .. } = part {
                let mut leaf = Value::String("1".into());
                for seg in pointer.split('/').skip(1).collect::<Vec<_>>().into_iter().rev() {
                    let mut level = serde_json::Map::new();
                    level.insert(seg.replace("~1", "/").replace("~0", "~"), leaf);
                    leaf = Value::Object(level);
                }
                if let Value::Object(o) = leaf {
                    merge(&mut item, o);
                }
            }
        }
        check(t, &s.web_hosts, "list.url", Some(&Value::Object(item)))?;
    }
    Ok(())
}

fn merge(into: &mut serde_json::Map<String, Value>, from: serde_json::Map<String, Value>) {
    for (k, v) in from {
        match (into.get_mut(&k), v) {
            (Some(Value::Object(a)), Value::Object(b)) => merge(a, b),
            (_, v) => {
                into.insert(k, v);
            }
        }
    }
}

/// A JSON scalar at `pointer` inside `v`, as text.
pub fn text_at(v: &Value, pointer: &str) -> Option<String> {
    scalar_text(v.pointer(pointer)?)
}

#[cfg(test)]
mod tests {
    use super::super::fixtures;
    use super::*;
    use serde_json::json;

    const NATIVE: &[&str] = &["stripe", "github"];

    /// The Supa fixture with one change applied; the problems it causes.
    fn problems_after(file: &str, change: impl FnOnce(&mut Value)) -> Vec<String> {
        let mut v: Value = serde_json::from_str(fixtures::SUPA).unwrap();
        change(&mut v);
        match parse(file, &v.to_string(), NATIVE) {
            Ok(_) => Vec::new(),
            Err(e) => e,
        }
    }

    fn rejected(change: impl FnOnce(&mut Value), needle: &str) {
        let problems = problems_after("supa.json", change);
        assert!(problems.iter().any(|p| p.contains(needle)), "expected {needle:?} in {problems:?}");
    }

    #[test]
    fn catalog_fixture_is_valid_as_is() {
        assert_eq!(problems_after("supa.json", |_| {}), Vec::<String>::new());
    }

    #[test]
    fn catalog_manifest_identity_rules() {
        rejected(|v| v["schema"] = json!(2), "schema");
        rejected(|v| v["id"] = json!("Supa"), "id");
        rejected(|v| v["id"] = json!("github"), "reserved");
        assert!(problems_after("other.json", |_| {}).iter().any(|p| p.contains("supa.json")));
        rejected(|v| v["category"] = json!("games"), "category");
        rejected(|v| v["color"] = json!("green"), "color");
        rejected(|v| v["desc"]["en"] = json!("x".repeat(91)), "desc.en");
        rejected(|v| v["keyUrl"] = json!("http://supa.example.com/tokens"), "keyUrl");
        rejected(|v| v["pollEvery"] = json!(30), "pollEvery");
        rejected(|v| v["list"]["max"] = json!(21), "list.max");
        rejected(|v| v["subtitel"] = json!("typo"), "unknown field");
    }

    #[test]
    fn catalog_patterns_must_compile_and_be_anchored() {
        rejected(|v| v["fields"][0]["pattern"] = json!("[a-z]+"), "start with ^");
        rejected(|v| v["fields"][0]["pattern"] = json!(r"^[a-z]+\$"), "start with ^");
        rejected(|v| v["fields"][0]["pattern"] = json!("^[a-z+$"), "does not compile");
        // `^a|b$` is anchored as written; the engine still wraps it whole.
        let mut v: Value = serde_json::from_str(fixtures::SUPA).unwrap();
        v["fields"][0]["pattern"] = json!("^abc|xyz$");
        let s = parse("supa.json", &v.to_string(), NATIVE).unwrap();
        assert!(s.fields[0].pattern.is_match("abc"));
        assert!(!s.fields[0].pattern.is_match("abc-and-more"));
    }

    #[test]
    fn catalog_templates_must_name_declared_fields_and_keep_secrets_out() {
        rejected(|v| v["request"]["url"] = json!("https://api.supa.example.com/v1/{field.nope}"), "not a declared field");
        rejected(|v| v["request"]["query"]["k"] = json!("{field.token}"), "only be used through auth");
        rejected(|v| v["openUrl"] = json!("https://supa.example.com/?t={field.token}"), "only be used through auth");
        rejected(|v| v["request"]["url"] = json!("{field.org|base}/v1"), "|base needs a url-kind field");
        rejected(|v| v["request"]["headers"]["X-Org"] = json!("{field.org}"), "literal value");
        rejected(|v| v["request"]["headers"]["Authorization"] = json!("Bearer abc"), "reserved");
        rejected(|v| v["auth"] = json!([{ "type": "bearer", "field": "nope" }]), "undeclared field");
        rejected(|v| v["auth"] = json!([{ "type": "header", "name": "Host", "field": "token" }]), "reserved");
        rejected(|v| v["auth"] = json!([]), "never used");
        rejected(|v| v["request"]["body"] = json!({ "a": 1 }), "needs method POST");
    }

    #[test]
    fn catalog_requests_and_links_must_land_on_declared_hosts() {
        rejected(|v| v["request"]["url"] = json!("https://elsewhere.example.com/v1"), "declared hosts");
        rejected(|v| v["request"]["url"] = json!("http://api.supa.example.com/v1"), "declared hosts");
        rejected(|v| v["openUrl"] = json!("https://phish.example.com/"), "webHosts");
        rejected(|v| v["list"]["url"] = json!("https://phish.example.com/{item./id}"), "webHosts");
        rejected(|v| v["hosts"] = json!(["*.com"]), "wildcard");
        rejected(|v| v["hosts"] = json!(["{field.org}"]), "url-kind field");
        rejected(|v| v["webHosts"] = json!([]), "needs webHosts");
    }

    #[test]
    fn catalog_status_and_notify_rules() {
        rejected(|v| v["statusMap"]["X"] = json!("bad"), "statusMap");
        rejected(|v| v["statusMap"]["active_healthy"] = json!("ok"), "twice");
        rejected(|v| v["notify"] = json!({ "on": "changed" }), "notify.on");
        rejected(|v| v["notify"]["statuses"] = json!(["red"]), "notify.statuses");
        rejected(
            |v| {
                v["list"].as_object_mut().unwrap().remove("id");
            },
            "list.id",
        );
        rejected(|v| v["list"]["sort"] = json!("name"), "list.sort");
    }

    #[test]
    fn catalog_basic_auth_password_may_be_left_out() {
        let mut v: Value = serde_json::from_str(fixtures::SUPA).unwrap();
        v["auth"] = json!([{ "type": "basic", "user": "token" }]);
        let s = parse("supa.json", &v.to_string(), NATIVE).unwrap();
        assert_eq!(s.auth, [Auth::Basic { user: "token".into(), pass: None }]);
    }
}
