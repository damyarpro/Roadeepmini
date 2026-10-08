// The cost quotes this app has handed out over MCP, kept in memory until they
// expire. The paid step only runs on a quote listed here, for the exact body
// that was quoted, so the confirmation dialog always shows a price Roadeep
// actually gave — never one the MCP client made up.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::roadeep::generation::{self, GenerationRequest, Target};
use crate::roadeep::http::RoadeepError;

pub const QUOTE_UNKNOWN: &str = "QUOTE_UNKNOWN";
/// Same code Roadeep uses, so the model reacts the same way: estimate again.
pub const QUOTE_MISMATCH: &str = "QUOTE_MISMATCH";

/// When Roadeep's `expires_at` is missing or unreadable.
const DEFAULT_TTL: Duration = Duration::from_secs(10 * 60);
/// Never trust a far-future expiry: a quote is a price for now, not for later.
const MAX_TTL: Duration = Duration::from_secs(60 * 60);
/// A model looping on estimates must not grow this without bound.
const MAX_QUOTES: usize = 64;

/// What the confirmation dialog shows, and what the submit must match.
#[derive(Debug, Clone, PartialEq)]
pub struct Quote {
    /// `GenerationRequest::quote_body()` of the estimated request.
    pub body: Value,
    pub target: Target,
    pub capability: String,
    /// e.g. "1200 IRT"; None when Roadeep did not say.
    pub cost: Option<String>,
    pub credits: Option<String>,
    expires: SystemTime,
}

#[derive(Default)]
struct Store(HashMap<String, Quote>);

impl Store {
    fn prune(&mut self, now: SystemTime) {
        self.0.retain(|_, q| q.expires > now);
    }

    fn insert(&mut self, id: String, quote: Quote, now: SystemTime) {
        self.prune(now);
        if self.0.len() >= MAX_QUOTES && !self.0.contains_key(&id) {
            if let Some(soonest) = self.0.iter().min_by_key(|(_, q)| q.expires).map(|(k, _)| k.clone()) {
                self.0.remove(&soonest);
            }
        }
        self.0.insert(id, quote);
    }

    fn get(&mut self, id: &str, now: SystemTime) -> Option<Quote> {
        self.prune(now);
        self.0.get(id).cloned()
    }
}

fn store() -> MutexGuard<'static, Store> {
    static STORE: OnceLock<Mutex<Store>> = OnceLock::new();
    // A poisoned lock only means a panic elsewhere mid-insert; the map is still sound.
    STORE.get_or_init(Mutex::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// "1200 IRT" from `{amount, currency}`, or the bare value.
fn display(v: Option<&Value>) -> Option<String> {
    let text = |v: &Value| match v {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    };
    match v? {
        Value::Object(o) => {
            let amount = o.get("amount").and_then(text)?;
            Some(match o.get("currency").and_then(text) {
                Some(currency) => format!("{amount} {currency}"),
                None => amount,
            })
        }
        other => text(other),
    }
}

/// Seconds since the epoch for an RFC 3339 timestamp ("2026-10-01T10:00:00.5+03:30").
fn parse_rfc3339(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let num = |from: usize, to: usize| -> Option<i64> { s.get(from..to)?.parse::<u32>().ok().map(i64::from) };
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hour, minute, second) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut rest = &s[19..];
    if let Some(frac) = rest.strip_prefix('.') {
        let digits = frac.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        rest = &frac[digits..];
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ if rest.len() == 6 && rest.as_bytes()[3] == b':' => {
            let sign = match rest.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let (h, m) = (rest[1..3].parse::<i64>().ok()?, rest[4..6].parse::<i64>().ok()?);
            sign * (h * 3600 + m * 60)
        }
        _ => return None,
    };
    // Days from civil (H. Hinnant), proleptic Gregorian.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    Some(days * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

fn expiry(expires_at: Option<&str>, now: SystemTime) -> SystemTime {
    let latest = now + MAX_TTL;
    match expires_at.and_then(parse_rfc3339) {
        Some(secs) if secs > 0 => (UNIX_EPOCH + Duration::from_secs(secs as u64)).min(latest),
        Some(_) => now,
        None => now + DEFAULT_TTL,
    }
}

fn quote_from(shaped: &Value, req: &GenerationRequest, now: SystemTime) -> Option<(String, Quote)> {
    let id = shaped.get("quote_id").and_then(Value::as_str).filter(|id| generation::valid_id(id))?;
    let quote = Quote {
        body: req.quote_body(),
        target: req.target.clone(),
        capability: req.capability.clone(),
        cost: display(shaped.get("estimated_cost")),
        credits: display(shaped.get("required_credits")),
        expires: expiry(shaped.get("expires_at").and_then(Value::as_str), now),
    };
    Some((id.to_string(), quote))
}

/// Records a quote `roadeep_estimate_generation` is about to return
/// (`shaped` is `generation::shape_quote`'s output).
pub fn remember(shaped: &Value, req: &GenerationRequest) {
    let now = SystemTime::now();
    if let Some((id, quote)) = quote_from(shaped, req, now) {
        store().insert(id, quote, now);
    }
}

/// The quote behind `quote_id`, if this app issued it, it is still valid, and
/// `req` is exactly what was quoted.
pub fn for_request(quote_id: &str, req: &GenerationRequest) -> Result<Quote, RoadeepError> {
    let found = store().get(quote_id, SystemTime::now());
    check(found, req)
}

fn check(found: Option<Quote>, req: &GenerationRequest) -> Result<Quote, RoadeepError> {
    let Some(quote) = found else {
        return Err(RoadeepError::new(
            QUOTE_UNKNOWN,
            "The Roadeep app has no valid quote with that id (it expired, or was not issued here). Call roadeep_estimate_generation first, show the user the cost, then start with the new quote_id.",
        ));
    };
    if quote.body != req.quote_body() {
        return Err(RoadeepError::new(
            QUOTE_MISMATCH,
            "These generation settings differ from what was quoted. Nothing was charged. Call roadeep_estimate_generation again with the exact settings and use that quote_id.",
        ));
    }
    Ok(quote)
}

/// A quote that was used (or that Roadeep refused) is not offered again.
pub fn forget(quote_id: &str) {
    store().0.remove(quote_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(prompt: &str) -> GenerationRequest {
        GenerationRequest::from_args(&json!({
            "subtype": "image", "capability": "text-to-image", "input": { "prompt": prompt }
        }))
        .unwrap()
    }

    fn shaped(id: &str, expires_at: &str) -> Value {
        json!({
            "quote_id": id, "expires_at": expires_at,
            "estimated_cost": { "amount": "1200", "currency": "IRT" }, "required_credits": 3
        })
    }

    #[test]
    fn timestamps() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("2000-03-01T00:00:00Z"), Some(951_868_800));
        assert_eq!(parse_rfc3339("2026-10-01T10:00:00.123456Z"), Some(1_790_848_800));
        assert_eq!(parse_rfc3339("2026-10-01T13:30:00+03:30"), Some(1_790_848_800));
        assert_eq!(parse_rfc3339("2026-10-01T06:00:00-04:00"), Some(1_790_848_800));
        for bad in ["", "2026-10-01", "2026-13-01T00:00:00Z", "2026-10-01T00:00:00", "2026-10-01T00:00:00+0330", "x026-10-01T00:00:00Z"] {
            assert_eq!(parse_rfc3339(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_quote_keeps_its_cost_and_expires() {
        let now = UNIX_EPOCH + Duration::from_secs(1_790_848_000);
        let mut store = Store::default();
        let (id, quote) = quote_from(&shaped("q1", "2026-10-01T10:00:00Z"), &request("a cat"), now).unwrap();
        assert_eq!(quote.cost.as_deref(), Some("1200 IRT"));
        assert_eq!(quote.credits.as_deref(), Some("3"));
        assert_eq!(quote.target, Target::Subtype("image".into()));
        store.insert(id, quote, now);

        assert!(store.get("q1", now + Duration::from_secs(799)).is_some());
        assert!(store.get("q1", now + Duration::from_secs(800)).is_none(), "expired at expires_at");
        assert!(store.0.is_empty(), "and pruned");
    }

    #[test]
    fn expiry_is_bounded_and_defaulted() {
        let now = UNIX_EPOCH + Duration::from_secs(1_790_848_000);
        assert_eq!(expiry(Some("2099-01-01T00:00:00Z"), now), now + MAX_TTL);
        assert_eq!(expiry(Some("garbage"), now), now + DEFAULT_TTL);
        assert_eq!(expiry(None, now), now + DEFAULT_TTL);
        assert!(expiry(Some("2020-01-01T00:00:00Z"), now) < now, "already expired");
    }

    #[test]
    fn unknown_and_mismatched_quotes_are_refused() {
        let req = request("a cat");
        assert_eq!(check(None, &req).unwrap_err().code, QUOTE_UNKNOWN);
        let (_, quote) = quote_from(&shaped("q1", "2099-01-01T00:00:00Z"), &req, SystemTime::now()).unwrap();
        assert_eq!(check(Some(quote.clone()), &request("a dog")).unwrap_err().code, QUOTE_MISMATCH);
        assert_eq!(check(Some(quote.clone()), &req).unwrap(), quote);
    }

    #[test]
    fn quotes_without_a_valid_id_are_not_kept_and_the_store_is_capped() {
        let req = request("a cat");
        assert!(quote_from(&json!({ "estimated_cost": 5 }), &req, SystemTime::now()).is_none());
        assert!(quote_from(&json!({ "quote_id": "../x" }), &req, SystemTime::now()).is_none());

        let now = SystemTime::now();
        let mut store = Store::default();
        for i in 0..MAX_QUOTES + 10 {
            let (id, q) = quote_from(&shaped(&format!("q{i}"), "2099-01-01T00:00:00Z"), &req, now).unwrap();
            store.insert(id, q, now);
        }
        assert_eq!(store.0.len(), MAX_QUOTES);
    }

    #[test]
    fn costs_are_shown_as_given() {
        assert_eq!(display(Some(&json!({ "amount": 12.5, "currency": "USD" }))).as_deref(), Some("12.5 USD"));
        assert_eq!(display(Some(&json!({ "amount": "7" }))).as_deref(), Some("7"));
        assert_eq!(display(Some(&json!("9"))).as_deref(), Some("9"));
        assert_eq!(display(Some(&json!({ "currency": "IRT" }))), None);
        assert_eq!(display(None), None);
    }

    #[test]
    fn the_shared_store_remembers_and_forgets() {
        let req = request("shared store");
        remember(&shaped("q-shared-test", "2099-01-01T00:00:00Z"), &req);
        assert!(for_request("q-shared-test", &req).is_ok());
        forget("q-shared-test");
        assert_eq!(for_request("q-shared-test", &req).unwrap_err().code, QUOTE_UNKNOWN);
    }
}
