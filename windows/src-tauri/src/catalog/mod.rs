// The integrations catalog: services described by a JSON manifest bundled in
// the app (src-tauri/catalog/*.json, embedded by build.rs) and run by one
// generic engine (engine.rs) — no code per service.
//
// Manifests are parsed and validated once, on first use. A manifest that
// fails is logged and left out; the app never stops over one. The `catalog`
// tests validate every bundled manifest strictly, so a bad one fails CI.

pub mod engine;
pub mod manifest;
pub mod template;

use std::sync::LazyLock;

use serde::Serialize;

pub use manifest::{Field, Kind, Service, Text};

mod bundled {
    include!(concat!(env!("OUT_DIR"), "/catalog_files.rs"));
}

pub struct Catalog {
    pub services: Vec<Service>,
}

impl Catalog {
    /// Every valid manifest of `files` ((file name, JSON) pairs). Problems
    /// come back per file for the caller to log or fail on.
    pub fn from_files(files: &[(&str, &str)]) -> (Catalog, Vec<(String, Vec<String>)>) {
        let native = crate::integrations::native_ids();
        let mut services: Vec<Service> = Vec::new();
        let mut problems = Vec::new();
        for (file, json) in files {
            match manifest::parse(file, json, &native) {
                Ok(service) if services.iter().any(|s| s.id == service.id) => {
                    problems.push((file.to_string(), vec![format!("duplicate id {:?}", service.id)]));
                }
                Ok(service) => services.push(service),
                Err(errors) => problems.push((file.to_string(), errors)),
            }
        }
        services.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.id.cmp(&b.id)));
        (Catalog { services }, problems)
    }

    pub fn by_pill(&self, pill: &str) -> Option<&Service> {
        self.services.iter().find(|s| s.pill == pill)
    }

    /// The service and field behind a Credential Manager key "x.<id>.<name>".
    pub fn field_for_key(&self, key: &str) -> Option<(&Service, &Field)> {
        let (id, name) = key.strip_prefix("x.")?.split_once('.')?;
        let service = self.services.iter().find(|s| s.id == id)?;
        Some((service, service.field(name)?))
    }

    pub fn entries(&self) -> Vec<CatalogEntry> {
        self.services.iter().map(CatalogEntry::from).collect()
    }
}

static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    let (catalog, problems) = Catalog::from_files(bundled::FILES);
    // Tests reach this through settings and secrets; they must not write the
    // user's log (the bundled-manifest test reports problems itself).
    if cfg!(not(test)) {
        for (file, errors) in &problems {
            crate::log::line(format!("catalog: skipped {file}: {}", errors.join("; ")));
        }
        crate::log::line(format!("catalog: {} services loaded", catalog.services.len()));
    }
    catalog
});

/// The bundled catalog, loaded on first use.
pub fn get() -> &'static Catalog {
    &CATALOG
}

// ── What the settings window gets (`catalog_list`) ────────────────────────────

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub category: String,
    pub color: String,
    pub desc: Text,
    pub key_url: String,
    pub docs_url: Option<String>,
    /// Only when it needs no field value; a templated one is resolved per poll
    /// and reaches the island in the payload.
    pub open_url: Option<String>,
    /// Where the key may be sent, as the manifest lists them. A "{field.x}"
    /// entry stands for the host of the URL the user enters in field x.
    pub hosts: Vec<String>,
    pub poll_every: u64,
    pub fields: Vec<FieldEntry>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FieldEntry {
    pub key: String,
    pub name: String,
    pub kind: &'static str,
    pub label: Text,
    pub placeholder: String,
    pub optional: bool,
    pub help: Option<Text>,
}

impl From<&Service> for CatalogEntry {
    fn from(s: &Service) -> Self {
        CatalogEntry {
            id: s.id.clone(),
            name: s.name.clone(),
            category: s.category.clone(),
            color: s.color.clone(),
            desc: s.desc.clone(),
            key_url: s.key_url.clone(),
            docs_url: s.docs_url.clone(),
            open_url: s.open_url.as_ref().filter(|t| !t.has_placeholders()).and(s.open_url_raw.clone()),
            hosts: s.host_entries.clone(),
            poll_every: s.poll_every,
            fields: s
                .fields
                .iter()
                .map(|f| FieldEntry {
                    key: f.key.clone(),
                    name: f.name.clone(),
                    kind: f.kind.as_str(),
                    label: f.label.clone(),
                    placeholder: f.placeholder.clone(),
                    optional: f.optional,
                    help: f.help.clone(),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    //! Manifests for the engine tests — not the real catalog.

    pub const SUPA: &str = r##"{
      "schema": 1, "id": "supa", "name": "Supa", "category": "dev", "color": "#3ecf8e",
      "desc": { "fa": "پروژه‌ها", "en": "Projects" },
      "keyUrl": "https://supa.example.com/tokens", "docsUrl": "https://supa.example.com/docs",
      "fields": [
        { "name": "token", "kind": "secret", "label": { "fa": "توکن", "en": "Token" },
          "placeholder": "sbp_…", "pattern": "^[A-Za-z0-9_.\\-]{10,300}$" },
        { "name": "org", "kind": "text", "label": { "fa": "سازمان", "en": "Org" },
          "optional": true, "pattern": "^[a-z0-9-]{1,40}$" }
      ],
      "auth": [ { "type": "bearer", "field": "token" } ],
      "hosts": ["api.supa.example.com"],
      "request": { "method": "GET", "url": "https://api.supa.example.com/v1/projects",
                   "headers": { "Accept": "application/json" }, "query": { "per_page": "3", "org": "{field.org}" } },
      "list": { "path": "/data", "max": 2, "sort": "time", "id": "/id", "title": "/name",
                "subtitle": "/region", "status": "/status", "time": "/created_at",
                "url": "https://supa.example.com/dashboard/project/{item./id}" },
      "statusMap": { "ACTIVE_HEALTHY": "ok", "COMING_UP": "info", "*": "warn" },
      "count": "/total",
      "openUrl": "https://supa.example.com/dashboard/projects",
      "webHosts": ["supa.example.com"],
      "pollEvery": 120,
      "notify": { "on": "new", "statuses": ["err", "warn"] }
    }"##;

    pub const JIRA: &str = r##"{
      "schema": 1, "id": "jiro", "name": "Jiro", "category": "work", "color": "#0052CC",
      "desc": { "fa": "کارهای من", "en": "My issues" },
      "keyUrl": "https://id.example.com/manage/api-tokens",
      "fields": [
        { "name": "site", "kind": "text", "label": { "fa": "سایت", "en": "Site" }, "pattern": "^[a-z0-9][a-z0-9-]{0,62}$" },
        { "name": "email", "kind": "text", "label": { "fa": "ایمیل", "en": "Email" }, "pattern": "^[^@\\s]+@[^@\\s]+$" },
        { "name": "token", "kind": "secret", "label": { "fa": "توکن", "en": "Token" }, "pattern": "^\\S{8,300}$" }
      ],
      "auth": [ { "type": "basic", "user": "email", "pass": "token" } ],
      "hosts": ["*.atlassian.net"],
      "request": { "method": "POST", "url": "https://{field.site}.atlassian.net/rest/api/3/search",
                   "body": { "jql": "assignee = currentUser() AND project = \"{field.site}\"", "maxResults": 5 } },
      "list": { "path": "/issues", "max": 5, "id": "/id", "title": "/fields/summary", "status": "/fields/status/name",
                "url": "https://{field.site}.atlassian.net/browse/{item./key}" },
      "statusMap": { "Done": "ok", "In Progress": "info", "Blocked": "err" },
      "count": "/total",
      "openUrl": "https://{field.site}.atlassian.net/jira",
      "webHosts": ["*.atlassian.net"],
      "pollEvery": 300,
      "notify": { "on": "status", "statuses": ["err"] }
    }"##;

    pub const SHOP: &str = r##"{
      "schema": 1, "id": "shop-woo", "name": "Woo", "category": "commerce", "color": "#7F54B3",
      "desc": { "fa": "سفارش‌ها", "en": "Orders" },
      "keyUrl": "https://woo.example.com/keys",
      "fields": [
        { "name": "siteUrl", "kind": "url", "label": { "fa": "آدرس", "en": "Site URL" }, "pattern": "^https://\\S{4,200}$" },
        { "name": "key", "kind": "secret", "label": { "fa": "کلید", "en": "Key" }, "pattern": "^ck_[a-f0-9]{40}$" },
        { "name": "secret", "kind": "secret", "label": { "fa": "رمز", "en": "Secret" }, "pattern": "^cs_[a-f0-9]{40}$" }
      ],
      "auth": [ { "type": "basic", "user": "key", "pass": "secret" }, { "type": "query", "name": "x", "field": "key" } ],
      "hosts": ["{field.siteUrl}"],
      "request": { "method": "GET", "url": "{field.siteUrl|base}/wp-json/wc/v3/orders" },
      "list": { "path": "", "max": 3, "id": "/id", "title": "/number", "status": "/status", "time": "/date_created_gmt",
                "url": "{item./links/self|url}" },
      "openUrl": "{field.siteUrl|base}/wp-admin/edit.php?post_type=shop_order",
      "webHosts": ["{field.siteUrl}"],
      "pollEvery": 60
    }"##;

    pub fn catalog() -> super::Catalog {
        let (catalog, problems) =
            super::Catalog::from_files(&[("supa.json", SUPA), ("jiro.json", JIRA), ("shop-woo.json", SHOP)]);
        assert!(problems.is_empty(), "{problems:?}");
        catalog
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// W3's manifests are checked here: every bundled file must validate.
    #[test]
    fn catalog_bundled_manifests_are_all_valid() {
        let (catalog, problems) = Catalog::from_files(bundled::FILES);
        let report: Vec<String> = problems
            .iter()
            .map(|(file, errors)| format!("catalog/{file}:\n  - {}", errors.join("\n  - ")))
            .collect();
        assert!(report.is_empty(), "{} invalid manifest(s):\n{}", report.len(), report.join("\n"));
        assert_eq!(catalog.services.len(), bundled::FILES.len());
    }

    #[test]
    fn catalog_fixtures_load_and_list_sorted_by_name() {
        let catalog = fixtures::catalog();
        let names: Vec<&str> = catalog.services.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Jiro", "Supa", "Woo"]);
        let entries = catalog.entries();
        let supa = entries.iter().find(|e| e.id == "supa").unwrap();
        assert_eq!(supa.color, "#3ECF8E");
        assert_eq!(supa.open_url.as_deref(), Some("https://supa.example.com/dashboard/projects"));
        assert_eq!(supa.fields[0].key, "x.supa.token");
        assert_eq!(supa.fields[0].kind, "secret");
        assert_eq!(supa.fields[1].kind, "text");
        // A templated open URL needs the user's values: not listed.
        assert_eq!(entries.iter().find(|e| e.id == "jiro").unwrap().open_url, None);
        let json = serde_json::to_value(supa).unwrap();
        for key in ["keyUrl", "docsUrl", "openUrl", "hosts", "pollEvery", "fields"] {
            assert!(json.get(key).is_some(), "{key}");
        }
        assert_eq!(json["fields"][1]["optional"], true);
        assert!(json["fields"][0]["help"].is_null());
    }

    #[test]
    fn catalog_keys_resolve_to_their_field() {
        let catalog = fixtures::catalog();
        let (s, f) = catalog.field_for_key("x.shop-woo.siteUrl").unwrap();
        assert_eq!((s.id.as_str(), f.kind), ("shop-woo", Kind::Url));
        for bad in ["x.supa", "x.supa.nope", "x.nope.token", "supa.token", "x..token", "stripe-api-key"] {
            assert!(catalog.field_for_key(bad).is_none(), "{bad}");
        }
        assert_eq!(catalog.by_pill("integration_shop-woo").map(|s| s.id.as_str()), Some("shop-woo"));
        assert!(catalog.by_pill("shop-woo").is_none());
    }

    #[test]
    fn catalog_duplicate_ids_are_refused() {
        let (catalog, problems) = Catalog::from_files(&[("supa.json", fixtures::SUPA), ("supa.json", fixtures::SUPA)]);
        assert_eq!(catalog.services.len(), 1);
        assert_eq!(problems.len(), 1);
    }
}
