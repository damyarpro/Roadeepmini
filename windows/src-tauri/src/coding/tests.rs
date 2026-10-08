use super::*;
use std::io::Write;

fn meta() -> &'static str { r#"{"type":"session_meta","payload":{"id":"test-1","cwd":"C:/work"}}"# }
fn parse(parser: &mut Parser, line: &str, offset: u64) -> Option<CodingEvent> { parser.parse(line.as_bytes(), offset, 1234).unwrap() }
fn fixture() -> PathBuf { std::env::temp_dir().join(format!("roadeep-coding-{}.jsonl", uuid::Uuid::new_v4())) }

#[test]
fn wire_and_unknown_exit_are_honest() {
    let mut parser = Parser::default(); parse(&mut parser, meta(), 1);
    let e = parse(&mut parser, r#"{"type":"response_item","timestamp":"2026-10-04T00:00:00.125Z","payload":{"type":"function_call_output","call_id":"c","output":"tool completed"}}"#, 2).unwrap();
    assert!(e.exit_code.is_none()); assert_eq!(e.phase, Some("completed"));
    let value = serde_json::to_value(e).unwrap();
    assert_eq!(value["sessionId"], "codex:test-1"); assert_eq!(value["at"], 1791072000125u64);
    assert!(value.get("exitCode").is_none()); assert!(value.get("session_id").is_none());
}

#[test]
fn tool_pair_and_patches_are_normalized() {
    let mut p = Parser::default(); parse(&mut p, meta(), 1);
    let e = parse(&mut p, r#"{"type":"response_item","payload":{"type":"custom_tool_call","name":"functions.apply_patch","call_id":"x","input":"*** Begin Patch\n*** Update File: src/main.ts\n+new\n*** End Patch"}}"#, 2).unwrap();
    assert_eq!(e.files.unwrap(), vec!["src/main.ts"]); assert_eq!(e.phase, Some("started"));
    let e = parse(&mut p, r#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"x","output":"Process exited with code 2"}}"#, 3).unwrap();
    assert_eq!(e.call_id.as_deref(), Some("x")); assert_eq!(e.exit_code, Some(2)); assert_eq!(e.phase, Some("failed"));
    let ambiguous = parse(&mut p, r#"{"type":"response_item","payload":{"type":"function_call_output","output":"Process exited with code 1\nProcess exited with code 0"}}"#, 4).unwrap();
    assert!(ambiguous.exit_code.is_none());
}

#[test]
fn secrets_are_redacted_and_text_bounded() {
    let raw = "API_KEY=supersecret Bearer abcdefgh sk-secretkey123 password: hidden";
    let clean = parser::clean(raw, 500);
    for secret in ["supersecret", "abcdefgh", "secretkey123", "hidden"] { assert!(!clean.contains(secret), "{clean}"); }
    assert!(!parser::clean("-----BEGIN PRIVATE KEY-----\nsecret", 2).contains("secret"));
    assert!(parser::clean(&"😀".repeat(10000), 20).chars().count() < 40);
    assert!(!parser::clean("PASSWORD='two secret words'", 100).contains("secret words"));
}

#[test]
fn wrappers_remain_ambiguous_and_envelopes_decode() {
    let mut p = Parser::default(); parse(&mut p, meta(), 1);
    let wrapper = serde_json::json!({"type":"response_item", "payload":{"type":"custom_tool_call", "name":"functions.exec", "input":"await tools.exec_command({cmd:\"npm test\"}); await tools.exec_command({cmd:\"cargo test\"});"}}).to_string();
    assert!(parse(&mut p, &wrapper, 2).unwrap().command.is_none());
    let single = serde_json::json!({"type":"response_item", "payload":{"type":"custom_tool_call", "name":"functions.exec", "input":"await tools.exec_command({\"cmd\":\"npm test\"});"}}).to_string();
    assert_eq!(parse(&mut p, &single, 3).unwrap().command.as_deref(), Some("npm test"));
    let output = serde_json::json!({"type":"response_item", "payload":{"type":"function_call_output", "output":"{\"output\":\"tests passed\\n2 tests\",\"exit_code\":0}"}}).to_string();
    let e = parse(&mut p, &output, 4).unwrap(); assert_eq!(e.exit_code, Some(0)); assert_eq!(e.output.as_deref(), Some("tests passed\n2 tests"));
    let failure = serde_json::json!({"type":"response_item", "payload":{"type":"function_call_output", "output":"{\"isError\":true,\"content\":[]}"}}).to_string();
    assert_eq!(parse(&mut p, &failure, 5).unwrap().phase, Some("failed"));
    let patch_result = serde_json::json!({"type":"response_item", "payload":{"type":"custom_tool_call_output", "output":"{\"content\":[{\"type\":\"text\",\"text\":\"Success. Updated the following files:\\nM src/main.ts\"}]}"}}).to_string();
    let e = parse(&mut p, &patch_result, 6).unwrap();
    assert_eq!(e.output.as_deref(), Some("Success. Updated the following files:\nM src/main.ts")); assert!(e.exit_code.is_none());
}

#[test]
fn old_logs_start_at_eof_and_partial_appends_wait() {
    let path = fixture(); fs::write(&path, format!("{}\n{{invalid}}\n", meta())).unwrap();
    let size = fs::metadata(&path).unwrap().len();
    let mut tail = Tail::open(&path, size, UNIX_EPOCH, false).unwrap();
    assert!(tail.read(&path, size, 1).unwrap().0.is_empty());
    let line = r#"{"type":"event_msg","payload":{"type":"user_message","message":"private prompt"}}"#;
    let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap(); write!(f, "{}", &line[..25]).unwrap();
    assert!(tail.read(&path, fs::metadata(&path).unwrap().len(), 1).unwrap().0.is_empty());
    writeln!(f, "{}", &line[25..]).unwrap();
    let (events, invalid) = tail.read(&path, fs::metadata(&path).unwrap().len(), 1).unwrap();
    assert_eq!(events.len(), 1); assert_eq!(invalid, 0); assert_eq!(events[0].title.as_deref(), Some("New task"));
    drop(f); fs::remove_file(path).unwrap();
}

#[test]
fn oversized_lines_and_truncation_recover() {
    let path = fixture(); fs::write(&path, format!("{}\n{}\n{{invalid}}\n", meta(), "a".repeat(RECORD + 3))).unwrap();
    let mut tail = Tail::open(&path, 0, UNIX_EPOCH, true).unwrap();
    let size = fs::metadata(&path).unwrap().len();
    let mut invalid = 0; while tail.offset < size { invalid += tail.read(&path, size, 1).unwrap().1; } assert!(invalid >= 1);
    fs::write(&path, format!("{}\n", meta())).unwrap();
    let (events,_) = tail.read(&path, fs::metadata(&path).unwrap().len(), 1).unwrap(); assert_eq!(events.len(), 1);
    fs::remove_file(path).unwrap();
}

#[test]
fn defaults_and_disable_clear_evidence() {
    assert!(!crate::settings::Settings::default().observe_codex);
    let observer = CodingObserver::new(true);
    let mut p = Parser::default(); observer.inner.lock().unwrap().events.push_back(parse(&mut p, meta(), 1).unwrap());
    observer.set_enabled(false); assert!(observer.inner.lock().unwrap().events.is_empty());
    assert!(!observer.inner.lock().unwrap().enabled);
}

#[test]
fn date_folder_roundtrip_and_malformed_json() {
    for day in [0, 20000, 20730] { let (y,m,d) = civil_from_days(day); assert_eq!(parser::days_from_civil(y,m,d), day); }
    assert!(Parser::default().parse(b"not json", 1, 1).is_err());
    assert!(Parser::default().parse(br#"{"type":"session_meta","payload":{"id":"../escape"}}"#, 1, 1).is_err());
}

#[test]
fn clear_discards_partial_and_pending_backlog() {
    let path = fixture(); fs::write(&path, format!("{}\n", meta())).unwrap();
    let size = fs::metadata(&path).unwrap().len();
    let tail = Tail::open(&path, size, UNIX_EPOCH, false).unwrap();
    let observer = CodingObserver::new(true); observer.inner.lock().unwrap().files.insert(path.clone(), tail);
    let line = r#"{"type":"event_msg","payload":{"type":"user_message"}}"#;
    let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap(); write!(f, "{line}\n{{\"unfinished\"").unwrap();
    clear(&observer);
    write!(f, ":true}}\n{line}\n").unwrap();
    let mut inner = observer.inner.lock().unwrap(); let tail = inner.files.get_mut(&path).unwrap();
    let (events, invalid) = tail.read(&path, fs::metadata(&path).unwrap().len(), 1).unwrap();
    assert_eq!(events.len(), 1); assert_eq!(invalid, 0); assert_eq!(events[0].kind, "prompt");
    drop(inner); drop(f); fs::remove_file(path).unwrap();
}

#[test]
fn truncated_rollout_uses_new_event_ids() {
    let path = fixture(); fs::write(&path, format!("{}\n{}\n", meta(), " ".repeat(100))).unwrap();
    let mut tail = Tail::open(&path, 0, UNIX_EPOCH, true).unwrap();
    let (first,_) = tail.read(&path, fs::metadata(&path).unwrap().len(), 1).unwrap();
    fs::write(&path, format!("{}\n", meta())).unwrap();
    let (second,_) = tail.read(&path, fs::metadata(&path).unwrap().len(), 1).unwrap();
    assert_ne!(first[0].id, second[0].id);
    fs::remove_file(path).unwrap();
}

#[test]
fn clear_drops_missing_tail_and_recreated_path_starts_at_eof() {
    let path = fixture(); fs::write(&path, format!("{}\npartial private evidence", meta())).unwrap();
    let mut tail = Tail::open(&path, 0, UNIX_EPOCH, true).unwrap();
    tail.read(&path, fs::metadata(&path).unwrap().len(), 1).unwrap();
    assert!(!tail.partial.is_empty());
    let observer = CodingObserver::new(true); observer.inner.lock().unwrap().files.insert(path.clone(), tail);
    fs::remove_file(&path).unwrap(); clear(&observer);
    let inner = observer.inner.lock().unwrap();
    assert!(inner.files.is_empty()); assert!(inner.events.is_empty());
    assert!(inner.cleared_paths.contains(&path)); assert_eq!(inner.error, Some("clear-tail-unreadable"));
    let watermark = inner.enabled_at;
    fs::write(&path, format!("{}\n{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"user_message\"}}}}\n", meta())).unwrap();
    let metadata = fs::metadata(&path).unwrap();
    let fresh = metadata.created().unwrap_or(UNIX_EPOCH) > watermark && !inner.cleared_paths.contains(&path);
    assert!(!fresh); drop(inner);
    let mut restored = Tail::open(&path, metadata.len(), metadata.modified().unwrap(), fresh).unwrap();
    assert!(restored.read(&path, metadata.len(), 1).unwrap().0.is_empty());
    fs::remove_file(path).unwrap();
}
#[test]
fn observed_context_uses_last_input_and_quota_windows_are_independent() {
    let mut p = Parser::default(); parse(&mut p, meta(), 1);
    parse(&mut p, r#"{"type":"turn_context","payload":{"model":"fixture-model"}}"#, 2);
    let usage = parse(&mut p, r#"{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":250,"output_tokens":5000},"total_token_usage":{"input_tokens":999999},"model_context_window":1000},"rate_limits":{"primary":{"used_percent":35,"window_minutes":300,"resets_at":1791072000},"secondary":{"used_percent":80,"window_minutes":10080}}}}"#, 3).unwrap();
    assert_eq!(usage.kind, "usage"); let context = usage.context.unwrap(); assert_eq!(context.used_tokens, 250); assert_eq!(context.used_percent, 25.0); assert_eq!(context.model.as_deref(), Some("fixture-model"));
    let rate = usage.usage.unwrap(); assert_eq!(rate.primary.unwrap().resets_at, Some(1791072000000)); assert_eq!(rate.secondary.unwrap().used_percent, 80.0);
    let quota_only = parse(&mut p, r#"{"type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"primary":{"used_percent":90}}}}"#, 4).unwrap(); assert!(quota_only.context.is_none()); assert!(quota_only.usage.is_some());
    assert!(parse(&mut p, r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":9999},"model_context_window":10000},"rate_limits":{"primary":{"used_percent":120}}}}"#, 5).is_none());
}

#[test]
fn desktop_task_started_is_a_private_content_free_turn() {
    let mut p = Parser::default(); parse(&mut p, meta(), 1);
    let e = parse(&mut p, r#"{"type":"event_msg","payload":{"type":"task_started","turn_id":"turn"}}"#, 2).unwrap();
    assert_eq!(e.kind, "prompt"); assert_eq!(e.title.as_deref(), Some("New task"));
    assert!(parse(&mut p, r#"{"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","content":"private"}}}"#, 3).is_none());
}
#[test]
fn large_desktop_outputs_recover_following_events() {
    let path = fixture(); let output = serde_json::json!({"type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"tool","output":"x".repeat(1_100_000)}});
    fs::write(&path, format!("{}\n{}\n{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_complete\"}}}}\n", meta(), output)).unwrap();
    let mut tail = Tail::open(&path, 0, UNIX_EPOCH, true).unwrap(); let size = fs::metadata(&path).unwrap().len();
    let mut events = Vec::new(); let mut invalid = 0;
    while tail.offset < size { let batch = tail.read(&path, size, 1).unwrap(); events.extend(batch.0); invalid += batch.1; }
    assert_eq!(invalid, 0); assert_eq!(events.len(), 3); assert!(events[1].output.as_ref().unwrap().len() < 6100); assert_eq!(events[2].kind, "finished");
    fs::remove_file(path).unwrap();
}
#[test]
fn recent_attachment_recovers_existing_session_in_old_folder_and_respects_clear() {
    let root = std::env::temp_dir().join(format!("roadeep-coding-root-{}", uuid::Uuid::new_v4()));
    let folder = root.join("2025/01/02"); fs::create_dir_all(&folder).unwrap(); let path = folder.join("rollout-fixture.jsonl");
    fs::write(&path, format!("{}\n{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_started\"}}}}\n", meta())).unwrap();
    let mut observer = CodingObserver::new(true); observer.root = Some(root.clone()); let mut events = Vec::new();
    observer.poll_with(|event| { events.push(event); Ok(()) }); assert!(events.iter().any(|e| e.kind == "prompt"));
    let initial = events.len(); observer.poll_with(|event| { events.push(event); Ok(()) }); assert_eq!(events.len(), initial);
    clear(&observer); observer.poll_with(|event| { events.push(event); Ok(()) }); assert_eq!(events.len(), initial);
    fs::remove_file(path).unwrap(); fs::remove_dir(folder).unwrap(); fs::remove_dir(root.join("2025/01")).unwrap(); fs::remove_dir(root.join("2025")).unwrap(); fs::remove_dir(root).unwrap();
}
#[test]
#[ignore = "read-only actual desktop sessions; explicit root required"]
fn observe_actual_desktop_sessions_without_content_output() {
    let root = PathBuf::from(std::env::var_os("ROADEEP_CODEX_TEST_ROOT").expect("explicit root required")); assert!(root.is_absolute() && root.is_dir());
    let mut observer = CodingObserver::new(true); observer.root = Some(root); let mut counts = std::collections::BTreeMap::new();
    for _ in 0..4 { observer.poll_with(|event| { *counts.entry(event.kind).or_insert(0usize) += 1; Ok(()) }); }
    let inner = observer.inner.lock().unwrap(); assert!(inner.available); assert!(inner.error.is_none()); assert!(!inner.events.is_empty()); assert!(counts.get("tool").copied().unwrap_or(0) > 0);
    eprintln!("actual desktop observer: {} files, {} retained events, counts {:?}", inner.files.len(), inner.events.len(), counts);
    if let Ok(id) = std::env::var("ROADEEP_CODEX_TEST_SESSION") { assert!(inner.files.values().any(|t| t.parser.session.as_deref() == Some(&format!("codex:{id}")))); }
}

#[test]
fn bootstrap_snapshot_keeps_newest_parent_over_older_worker_tail() {
    let root = std::env::temp_dir().join(format!("roadeep-coding-order-{}", uuid::Uuid::new_v4()));
    let folder = root.join("2025/02/03"); fs::create_dir_all(&folder).unwrap();
    let parent = folder.join("rollout-parent.jsonl"); let child = folder.join("rollout-child.jsonl");
    let line = |id: &str, stamp: &str| format!("{{\"type\":\"session_meta\",\"timestamp\":\"{stamp}\",\"payload\":{{\"id\":\"{id}\"}}}}\n");
    fs::write(&parent, format!("{}{{\"type\":\"event_msg\",\"timestamp\":\"2026-10-04T12:00:00Z\",\"payload\":{{\"type\":\"task_started\"}}}}\n", line("parent", "2026-10-04T12:00:00Z"))).unwrap();
    let mut old = line("child", "2026-10-04T10:00:00Z");
    for _ in 0..300 { old.push_str("{\"type\":\"event_msg\",\"timestamp\":\"2026-10-04T10:00:01Z\",\"payload\":{\"type\":\"task_started\"}}\n"); }
    fs::write(&child, old).unwrap();
    // The later mtime of the parent ensures it is polled before the older child.
    fs::OpenOptions::new().write(true).open(&parent).unwrap().set_modified(SystemTime::now() + Duration::from_secs(1)).unwrap();
    let mut observer = CodingObserver::new(true); observer.root = Some(root.clone()); observer.poll_with(|_| Ok(()));
    let inner = observer.inner.lock().unwrap(); assert_eq!(inner.events.len(), EVENTS); assert!(inner.events.iter().any(|e| e.session_id == "codex:parent")); drop(inner);
    fs::remove_file(parent).unwrap(); fs::remove_file(child).unwrap(); fs::remove_dir(folder).unwrap(); fs::remove_dir(root.join("2025/02")).unwrap(); fs::remove_dir(root.join("2025")).unwrap(); fs::remove_dir(root).unwrap();
}

#[test]
fn oversized_metadata_identity_recovers_for_fresh_and_existing_rollouts() {
    let id = uuid::Uuid::new_v4(); let path = std::env::temp_dir().join(format!("rollout-2025-01-01T00-00-00-{id}.jsonl"));
    fs::write(&path, format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"base_instructions\":\"{}\"}}}}\n{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_started\"}}}}\n", "x".repeat(RECORD + 1))).unwrap();
    let size = fs::metadata(&path).unwrap().len();
    for fresh in [true, false] {
        let mut tail = if fresh { Tail::open(&path, size, UNIX_EPOCH, true).unwrap() } else { Tail::recent(&path, size, UNIX_EPOCH).unwrap() };
        let mut events = Vec::new(); while tail.offset < size { events.extend(tail.read(&path, size, 1).unwrap().0); }
        assert!(events.iter().any(|e| e.kind == "prompt" && e.session_id == format!("codex:{id}")));
    }
    fs::remove_file(path).unwrap();
}

#[test]
fn current_day_has_priority_over_historical_discovery_budget() {
    let root = PathBuf::from("sessions"); let today = parser::days_from_civil(2026,10,4);
    let old: Vec<_> = (0..1000).map(|n| root.join(format!("old/{n}"))).collect();
    let ordered = prioritized_folders(&root, today, old);
    assert_eq!(ordered[0], root.join("2026/10/04")); assert_eq!(ordered[1], root.join("2026/10/03")); assert_eq!(ordered[2], root.join("2026/10/05"));
    assert_eq!(ordered.len(), 1003);
}

#[test]
fn documented_terminal_events_and_aliases_are_honest_and_content_free() {
    let mut p = Parser::default(); parse(&mut p, meta(), 1);
    for reason in ["interrupted", "replaced", "review_ended", "budget_limited"] {
        let raw = serde_json::json!({"type":"event_msg","payload":{"type":"turn_aborted","reason":reason}}).to_string();
        let event = parse(&mut p, &raw, 2).unwrap(); assert_eq!(event.kind, "cancelled");
        assert_eq!(event.title.as_deref(), Some("Task interrupted")); assert!(event.output.is_none());
    }
    assert!(parse(&mut p, r#"{"type":"event_msg","payload":{"type":"turn_aborted","reason":"unknown_future_reason"}}"#, 3).is_none());
    for typ in ["turn_started", "task_started", "turn_complete", "task_complete"] {
        let raw = serde_json::json!({"type":"event_msg","payload":{"type":typ}}).to_string();
        let event = parse(&mut p, &raw, 4).unwrap();
        assert_eq!(event.kind, if typ.ends_with("started") {"prompt"} else {"finished"});
    }
    let terminal_error = serde_json::json!({"type":"event_msg","payload":{"type":"error","message":"API_KEY=secret private failure","codex_error_info":"unauthorized"}}).to_string();
    let event = parse(&mut p, &terminal_error, 5).unwrap();assert_eq!(event.kind,"error");
    assert_eq!(event.title.as_deref(),Some("Task reported an error"));assert!(event.output.is_none());
    for classification in [serde_json::json!("thread_rollback_failed"),serde_json::json!({"active_turn_not_steerable":{"turn_kind":"review"}})] {
        let raw = serde_json::json!({"type":"event_msg","payload":{"type":"error","message":"private details","codex_error_info":classification}}).to_string();
        assert!(parse(&mut p,&raw,6).is_none());
    }
    let abort_error = serde_json::json!({"type":"event_msg","payload":{"type":"turn_aborted","reason":"interrupted","error":{"message":"private details","codex_error_info":"context_window_exceeded"}}}).to_string();
    assert_eq!(parse(&mut p,&abort_error,7).unwrap().kind,"error");
    let resumed = parse(&mut p,r#"{"type":"event_msg","payload":{"type":"turn_started"}}"#,8).unwrap();
    assert_eq!(resumed.kind,"prompt");
}
