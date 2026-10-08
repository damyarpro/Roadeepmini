// The built-in local agents (default_agents.json, Persian, everyday Iranian
// use) and the one-time step that adds them to agents.json and puts four of
// them on the island. Their ids are "default-<role>": the windows recognise
// them by it.
//
// The step runs while `settings.default_agents_version` is below VERSION and
// then records VERSION, so an agent the user deletes afterwards stays deleted.
// It is all or nothing: when the agent store cannot be read cleanly, nothing
// changes and the step is tried again at the next launch.

use std::path::Path;

use serde::Deserialize;

use crate::agents::{self, LocalAgent, SeedReport};
use crate::settings::{parse_pill, Pill, Settings, MAX_ACTIVE_PILLS};

/// The set applied by this build. A later version must seed only the agents it
/// introduces, or agents the user deleted would come back.
pub const VERSION: u32 = 1;

/// The agents on the island by default, in pill order.
pub const ISLAND: [&str; 4] =
    ["default-letter-writer", "default-translator", "default-day-planner", "default-iranian-chef"];

const DATA: &str = include_str!("default_agents.json");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    id: String,
    name: String,
    description: String,
    color: String,
    web_search: bool,
    instructions: String,
    starter_prompts: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DataFile {
    agents: Vec<Entry>,
}

/// The built-in agents, timestamps unset (seeding sets them). The data ships
/// in the binary and the tests check it, so failing to parse it is a build bug.
pub fn all() -> Vec<LocalAgent> {
    match serde_json::from_str::<DataFile>(DATA) {
        Ok(file) => file
            .agents
            .into_iter()
            .map(|e| LocalAgent {
                id: e.id,
                name: e.name,
                instructions: e.instructions,
                model: String::new(),
                web_search: e.web_search,
                base_agent_id: None,
                description: e.description,
                color: e.color,
                starter_prompts: e.starter_prompts,
                created_at: 0,
                updated_at: 0,
            })
            .collect(),
        Err(err) => {
            crate::log::line(format!("default agents: bundled data does not parse: {err}"));
            Vec::new()
        }
    }
}

/// `active_integrations` of a new install.
pub fn island_pills() -> Vec<String> {
    ISLAND.iter().map(|id| format!("agent:local:{id}")).collect()
}

/// What `apply` did, for the log (counts only).
#[derive(Debug, PartialEq)]
pub struct Applied {
    pub seeded: SeedReport,
    /// Keyless services taken off the island and out of "my services".
    pub services_removed: usize,
    pub pills_added: usize,
}

/// The Credential Manager keys of a native or catalog service; None for an id
/// that is neither (left alone).
fn service_keys(pill: &str) -> Option<Vec<String>> {
    if let Some((_, keys)) = crate::integrations::SERVICES.iter().find(|(id, _)| *id == pill) {
        return Some(keys.iter().map(|k| k.to_string()).collect());
    }
    crate::catalog::get().by_pill(pill).map(|s| s.fields.iter().map(|f| f.key.clone()).collect())
}

/// A native or catalog service with no key stored at all.
fn keyless_service(id: &str, has_key: &impl Fn(&str) -> bool) -> bool {
    matches!(parse_pill(id), Some(Pill::Integration(_)))
        && service_keys(id).is_some_and(|keys| !keys.iter().any(|k| has_key(k)))
}

/// The one-time step (see the top of the file). `Ok(None)`: already applied.
/// `Err`: the agent store could not be read cleanly; nothing changed.
/// `has_key` asks the Credential Manager (a stub in tests).
pub fn apply(
    settings: &mut Settings,
    agents_path: &Path,
    has_key: impl Fn(&str) -> bool,
) -> Result<Option<Applied>, String> {
    if settings.default_agents_version >= VERSION {
        return Ok(None);
    }
    let seeded = agents::seed_in(agents_path, &all())?;

    // Services the user never set up make way for the agents; one with any key
    // stays exactly where it is.
    let before = settings.active_integrations.len() + settings.added_integrations.len();
    settings.active_integrations.retain(|id| !keyless_service(id, &has_key));
    settings.added_integrations.retain(|id| !keyless_service(id, &has_key));
    let services_removed = before - settings.active_integrations.len() - settings.added_integrations.len();

    let mut pills_added = 0;
    for id in ISLAND {
        let pill = format!("agent:local:{id}");
        if settings.active_integrations.len() >= MAX_ACTIVE_PILLS {
            break;
        }
        if seeded.ids.iter().any(|x| x == id) && !settings.active_integrations.contains(&pill) {
            settings.active_integrations.push(pill);
            pills_added += 1;
        }
    }
    settings.default_agents_version = VERSION;
    Ok(Some(Applied { seeded, services_removed, pills_added }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{load_from, save_to, validate, AgentDraft, MAX_AGENTS};
    use std::path::PathBuf;

    fn temp_dir(tag: &str) -> PathBuf {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("roadeep-defaults-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn no_keys(_: &str) -> bool {
        false
    }

    fn pill(id: &str) -> String {
        format!("agent:local:{id}")
    }

    fn user_agent(id: &str) -> LocalAgent {
        LocalAgent {
            id: id.into(),
            name: format!("Mine {id}"),
            instructions: "Be brief.".into(),
            model: String::new(),
            web_search: false,
            base_agent_id: None,
            description: String::new(),
            color: String::new(),
            starter_prompts: Vec::new(),
            created_at: 1,
            updated_at: 1,
        }
    }

    /// What the old defaults left on an install that never configured them.
    fn old_install() -> Settings {
        let services = ["integration_resend", "integration_n8n", "integration_vercel", "integration_github"];
        Settings {
            active_integrations: services.iter().map(|s| s.to_string()).collect(),
            added_integrations: services.iter().map(|s| s.to_string()).collect(),
            default_agents_version: 0,
            ..Default::default()
        }
    }

    #[test]
    fn the_bundled_agents_are_valid_and_distinct() {
        let all = all();
        assert_eq!(all.len(), 10);
        let mut ids = std::collections::HashSet::new();
        let mut colors = std::collections::HashSet::new();
        for a in &all {
            assert!(a.id.starts_with("default-") && agents::valid_id(&a.id), "{}", a.id);
            assert!(ids.insert(a.id.clone()), "duplicate id {}", a.id);
            assert!(colors.insert(a.color.to_uppercase()), "duplicate colour {}", a.color);
            assert_ne!(a.color.to_uppercase(), "#FF6A00", "the brand orange stays the brand's");
            assert!((3..=4).contains(&a.starter_prompts.len()), "{}", a.id);
            assert!(!a.description.is_empty(), "{}", a.id);
            let draft = AgentDraft {
                id: Some(a.id.clone()),
                name: a.name.clone(),
                instructions: a.instructions.clone(),
                model: a.model.clone(),
                web_search: a.web_search,
                base_agent_id: None,
                description: a.description.clone(),
                color: a.color.clone(),
                starter_prompts: a.starter_prompts.clone(),
            };
            let clean = validate(&draft).unwrap_or_else(|e| panic!("{}: {e:?}", a.id));
            // Stored as written: nothing for the store's cleaning to change.
            assert_eq!(clean.instructions, a.instructions, "{}", a.id);
            assert_eq!(clean.color, a.color, "{}", a.id);
            assert_eq!(clean.starter_prompts, a.starter_prompts, "{}", a.id);
            let n = a.instructions.chars().count();
            assert!((600..=1500).contains(&n), "{}: {n} characters of instructions", a.id);
        }
        for id in ISLAND {
            assert!(ids.contains(id), "{id}");
        }
    }

    /// The server answers a message holding a "save to memory" phrase with its
    /// canned memory reply; instructions ride in the first message and starter
    /// prompts are messages. Stricter than the server: these stems never appear.
    #[test]
    fn no_default_trips_the_memory_detector() {
        let stems = [
            "remember", "memoriz", "memoris", "memory", "save this", "store this", "note this", "keep this for later",
            "حفظ", "به خاطر", "بخاطر", "یادت", "یادتان", "ذخیره", "فراموش", "حافظه",
        ];
        for a in all() {
            for text in [&a.name, &a.description, &a.instructions].into_iter().chain(a.starter_prompts.iter()) {
                let norm = text
                    .to_lowercase()
                    .replace('\u{200c}', " ")
                    .replace('ي', "ی")
                    .replace('ك', "ک")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                for stem in stems {
                    assert!(!norm.contains(stem), "{}: «{stem}» in {text}", a.id);
                }
            }
        }
    }

    #[test]
    fn a_fresh_install_gets_the_agents_and_their_pills() {
        let dir = temp_dir("fresh");
        let path = dir.join("agents.json");
        let mut s = Settings::default();
        assert_eq!(s.active_integrations, island_pills());
        assert!(s.added_integrations.is_empty());
        assert_eq!(s.default_agents_version, 0);

        let done = apply(&mut s, &path, no_keys).unwrap().expect("applied");
        assert_eq!((done.seeded.added, done.seeded.present, done.seeded.over_limit), (10, 0, 0));
        assert_eq!((done.services_removed, done.pills_added), (0, 0));
        assert_eq!(s.active_integrations, island_pills());
        assert_eq!(s.default_agents_version, VERSION);

        let stored = load_from(&path).unwrap();
        assert_eq!(stored.len(), 10);
        assert!(stored.iter().all(|a| a.created_at > 0 && a.created_at == a.updated_at));
        assert_eq!(apply(&mut s, &path, no_keys).unwrap(), None, "runs once");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keyless_services_make_way_for_the_agents() {
        let dir = temp_dir("keyless");
        let path = dir.join("agents.json");
        let mut s = old_install();
        let done = apply(&mut s, &path, no_keys).unwrap().expect("applied");
        assert_eq!(done.services_removed, 8, "four pills and four added services");
        assert_eq!(done.pills_added, 4);
        assert_eq!(s.active_integrations, island_pills());
        assert!(s.added_integrations.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_configured_service_stays_and_the_user_keeps_their_agents() {
        let dir = temp_dir("configured");
        let path = dir.join("agents.json");
        let mine = user_agent("3f1c2b9e-0000-4000-8000-123456789abc");
        save_to(&path, std::slice::from_ref(&mine)).unwrap();
        let mut s = old_install();
        s.active_integrations = vec![
            "integration_github".into(),
            pill(&mine.id),
            "integration_n8n".into(),
            "integration_better-stack-not-in-catalog".into(),
        ];
        s.added_integrations.push("integration_stripe".into());
        // A partly configured service still counts as set up.
        let keys = ["github-token", "n8n-url"];
        apply(&mut s, &path, |k| keys.contains(&k)).unwrap().expect("applied");
        assert_eq!(
            s.active_integrations,
            ["integration_github", pill(&mine.id).as_str(), "integration_n8n", "integration_better-stack-not-in-catalog"],
            "no free slot: no agent pill, and an unknown id is not ours to remove"
        );
        assert_eq!(s.added_integrations, ["integration_n8n", "integration_github"]);

        let stored = load_from(&path).unwrap();
        assert_eq!(stored.len(), 11);
        assert_eq!(stored[0], mine, "the user's agent is first and untouched");

        // Two free slots → the first two island agents.
        let mut s = old_install();
        s.active_integrations = vec!["integration_github".into(), "integration_vercel".into(), pill(&mine.id)];
        let done = apply(&mut s, &path, |k| k == "github-token").unwrap().expect("applied");
        assert_eq!(done.seeded.present, 10, "already there: not added twice");
        assert_eq!(
            s.active_integrations,
            ["integration_github", pill(&mine.id).as_str(), pill(ISLAND[0]).as_str(), pill(ISLAND[1]).as_str()]
        );
        assert_eq!(load_from(&path).unwrap().len(), 11);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_default_the_user_deleted_does_not_come_back() {
        let dir = temp_dir("deleted");
        let path = dir.join("agents.json");
        let mut s = Settings::default();
        apply(&mut s, &path, no_keys).unwrap().expect("applied");
        agents::delete_in(&path, "default-translator").unwrap();
        s.active_integrations.retain(|p| p != &pill("default-translator"));

        assert_eq!(apply(&mut s, &path, no_keys).unwrap(), None);
        // Even through a settings file round trip.
        let json = serde_json::to_string(&s).unwrap();
        let mut again: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(apply(&mut again, &path, no_keys).unwrap(), None);
        assert!(!load_from(&path).unwrap().iter().any(|a| a.id == "default-translator"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_older_settings_file_has_version_zero() {
        let older = r#"{"soundEnabled":true,"soundVolume":0.1,"autoCloseInterval":15,"absenceInterval":180,
            "activeIntegrations":[],"screen":"primary","autostart":false,"hooksInstalled":false}"#;
        let s: Settings = serde_json::from_str(older).unwrap();
        assert_eq!(s.default_agents_version, 0);
    }

    #[test]
    fn a_damaged_agent_store_is_left_alone_and_nothing_changes() {
        let dir = temp_dir("corrupt");
        let path = dir.join("agents.json");
        std::fs::write(&path, b"{ not json").unwrap();
        let mut s = old_install();
        let before = s.clone();
        assert!(apply(&mut s, &path, no_keys).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"{ not json", "not seeded, not moved");
        assert_eq!(s.default_agents_version, 0, "tried again next launch");
        assert_eq!(s.active_integrations, before.active_integrations);
        assert_eq!(s.added_integrations, before.added_integrations);

        // Valid JSON with an entry today's rules reject is damaged too.
        let raw = serde_json::json!({ "version": 1, "agents": [{ "id": "bad id", "name": "x", "instructions": "y", "createdAt": 1, "updatedAt": 1 }] });
        std::fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
        assert!(apply(&mut s, &path, no_keys).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), serde_json::to_vec(&raw).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreadable_agent_store_is_not_seeded() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = temp_dir("locked");
        let path = dir.join("agents.json");
        save_to(&path, &[user_agent("mine-1")]).unwrap();
        let mut s = old_install();
        {
            let _held = std::fs::OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();
            assert!(apply(&mut s, &path, no_keys).is_err());
        }
        assert_eq!(s.default_agents_version, 0);
        assert_eq!(load_from(&path).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_agent_limit_is_respected() {
        let dir = temp_dir("limit");
        let path = dir.join("agents.json");
        let mine: Vec<LocalAgent> = (0..MAX_AGENTS - 2).map(|i| user_agent(&format!("mine-{i}"))).collect();
        save_to(&path, &mine).unwrap();
        let mut s = old_install();
        let done = apply(&mut s, &path, no_keys).unwrap().expect("applied");
        assert_eq!((done.seeded.added, done.seeded.over_limit), (2, 8));
        assert_eq!(load_from(&path).unwrap().len(), MAX_AGENTS);
        // Only agents that exist get a pill: the first two defaults are the letter
        // writer and the translator, both on the island list.
        assert_eq!(s.active_integrations, [pill(ISLAND[0]), pill(ISLAND[1])]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
