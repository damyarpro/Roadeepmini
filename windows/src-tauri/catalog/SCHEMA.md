# Catalog manifests (schema 1)

Every `*.json` file in this folder is one service of the integrations market. It
is bundled into the app at build time and run by one generic engine
(`src/catalog/`): there is no per-service code. `cargo test catalog` validates
every manifest strictly and names the file and the reason when one is wrong;
at runtime an invalid manifest is logged and skipped.

Rules of thumb for a new service:

- One HTTPS request, read-only (GET, or a POST that only *queries*: GraphQL,
  search). Nothing that needs OAuth, several requests or a webhook.
- A personal token / API key the user can create themselves; link to the page
  where it's made (`keyUrl`).
- Ask for the narrowest read-only scope, and say which one in `help`.
- Only list what you are sure of. A wrong endpoint is worse than a missing one.

## Shape

```jsonc
{
  "schema": 1,
  "id": "supabase",                 // ^[a-z0-9][a-z0-9-]{1,30}$, unique, not a native id
  "name": "Supabase",
  "category": "dev",                // payments|dev|monitoring|work|comms|automation|commerce|support|marketing|content
  "color": "#3ECF8E",               // #RRGGBB
  "desc": { "fa": "…", "en": "…" }, // one line, ≤ 90 characters each
  "keyUrl": "https://supabase.com/dashboard/account/tokens",
  "docsUrl": "https://supabase.com/docs/reference/api",   // optional
  "fields": [
    { "name": "token", "kind": "secret",
      "label": { "fa": "…", "en": "…" },
      "placeholder": "sbp_…", "optional": false,
      "pattern": "^sbp_[A-Za-z0-9_]{20,120}$",            // required, anchored ^…$
      "help": { "fa": "…", "en": "…" } }                  // optional
  ],
  "auth": [ { "type": "bearer", "field": "token" } ],
  "hosts": ["api.supabase.com"],
  "request": {
    "method": "GET",
    "url": "https://api.supabase.com/v1/projects",
    "headers": { "Accept": "application/json" },
    "query": { "per_page": "10" },
    "body": null
  },
  "list": {
    "path": "", "max": 10,
    "id": "/id", "title": "/name", "subtitle": "/region", "status": "/status",
    "url": "https://supabase.com/dashboard/project/{item./id}"
  },
  "statusMap": { "ACTIVE_HEALTHY": "ok", "COMING_UP": "info", "*": "warn" },
  "openUrl": "https://supabase.com/dashboard/projects",
  "webHosts": ["supabase.com"],
  "pollEvery": 300,
  "notify": { "on": "status", "statuses": ["err"] }
}
```

The real file is [`supabase.json`](supabase.json).

## Fields

| key | |
|---|---|
| `name` | Field name, used in templates and in the Credential Manager key `x.<id>.<name>`. |
| `kind` | `secret` (stored, never read back), `text` (ids, slugs, emails; read back), `url` (an https base URL; read back). |
| `label`, `placeholder`, `help` | What the settings form shows. `help` is optional. |
| `optional` | `true` if the service works without it. |
| `pattern` | Anchored regex (Rust `regex` syntax, no look-around) the value must match before any request is made. Keep it strict: it is the first line of defence against a value being smuggled into a URL. |

## Auth

Zero or more parts; all are applied.

| type | |
|---|---|
| `{ "type": "bearer", "field": "token" }` | `Authorization: Bearer <token>` |
| `{ "type": "header", "name": "X-Api-Key", "field": "token", "prefix": "" }` | `<name>: <prefix><value>` (e.g. prefix `"Token token="`) |
| `{ "type": "basic", "user": "email", "pass": "token" }` | HTTP Basic with two field values |
| `{ "type": "query", "name": "key", "field": "token" }` | `?key=<value>` |

Auth values never go in `request.headers` — those take literal values only.

## Hosts

`hosts` lists where requests may go, `webHosts` where the "open" link and item
links may point. Entries are an exact host (`api.example.com`), one wildcard
label (`*.atlassian.net` matches `team.atlassian.net`, not `a.b.atlassian.net`),
or `{field.siteUrl}` (the host of that url-kind field's value). A request whose
host doesn't match is refused; an open/item URL that doesn't match is dropped.

## Templates

- `{field.<name>}` → the field's value, percent-encoded as a path segment or
  query value (JSON-escaped inside `body` strings). Usable in `request.url`,
  `request.query`, `request.body`, `openUrl` and `list.url`.
- `{field.<name>|base}` → only for a `url`-kind field, only at the very start
  of `request.url`: the value without a trailing `/`.
- `{item.<pointer>}` → in `list.url` only: a value of the item, percent-encoded.

## List mapping

`list.path` is a JSON pointer to the array (`""` = the root). Every other
pointer is relative to one item. `title` is required; `id`, `subtitle`,
`status`, `time` and `url` are optional. `max` (1..20) items are kept;
`"sort": "time"` puts the newest first. `time` accepts RFC 3339, `YYYY-MM-DD`
or a unix number (seconds if < 1e12, else milliseconds); leave it out when the
API returns dates without a timezone.

`count` (optional) is a pointer from the response root to a total; by default
the number of items is shown.

## Status and notifications

`statusMap` maps the raw value at `list.status` to `ok | info | warn | err |
off`. Keys are case-insensitive; `"*"` is the fallback; with no match and no
`"*"` the status is `info`. The raw value is shown as a small chip.

`notify` (optional) raises a badge and a sound:

- `{ "on": "new" }` — an item id that wasn't there at the previous poll.
- `{ "on": "status" }` — an item's mapped status changed.
- `statuses` limits it to items whose mapped status is in the list (empty =
  any). Pick events the user wants to be interrupted for.

`pollEvery` is in seconds, 60..3600. Use 60 only for things that need a fast
reaction (incidents, builds); 300+ for slow-moving lists.

## Text

`desc`, `label` and `help` carry their own `{ "fa", "en" }` pair. Persian is
informal-polite like the rest of the app. In Persian help text, wrap Latin
fragments that contain punctuation or arrows in U+2068 … U+2069 (`⁨ … ⁩`)
so they don't reorder the sentence.
