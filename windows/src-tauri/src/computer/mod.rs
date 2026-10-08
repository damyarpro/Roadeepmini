//! Explicitly started isolated computers. Host resources and identity are never model arguments.
mod encoding;
mod network;
mod process;
pub mod tools;
use std::{collections::HashMap, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::{Duration,Instant}};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager, State};

pub const IMAGE: &str = "roadeep-computer:1";
const MAX_COMPUTERS: usize = 4;
const WORKSPACE_TMPFS: &str = "rw,nosuid,nodev,noexec,size=128m,uid=1000,gid=1000,mode=0700";
const TEMP_TMPFS: &str = "rw,nosuid,nodev,size=128m,uid=1000,gid=1000,mode=0700";
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerStatus {
    pub available: bool,
    pub state: String,
    pub agent_id: String,
    pub takeover: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerResult {
    pub ok: bool,
    pub data: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerSetup { pub runtime_path:String, pub build_command:String }
#[tauri::command]
pub fn computer_setup(app:AppHandle) -> Result<ComputerSetup,String> {
    let packaged = app.path().resource_dir().map_err(|_| error("computer-setup-missing"))?.join("computer-runtime");
    let development = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../computer-runtime");
    let directory = if packaged.join("Dockerfile").is_file() { packaged } else if cfg!(debug_assertions) && development.join("Dockerfile").is_file() { development } else { return Err(error("computer-setup-missing")); };
    let directory = std::fs::canonicalize(directory).map_err(|_| error("computer-setup-missing"))?;
    let path = directory.to_string_lossy().strip_prefix(r"\\?\").unwrap_or(&directory.to_string_lossy()).to_string();
    if path.chars().any(char::is_control) { return Err(error("computer-setup-missing")); }
    let command = format!("docker build --tag {IMAGE} '{}'",path.replace('\'',"''"));
    Ok(ComputerSetup {runtime_path:path,build_command:command})
}
#[derive(Default)]
struct Slot { name: Option<String>, state: &'static str, takeover: bool }
pub struct ComputerState {
    instance: String,
    slots: Mutex<HashMap<String, Arc<tokio::sync::Mutex<Slot>>>>,
    containers: Mutex<HashMap<String,String>>,
    closing: AtomicBool,
}
impl Default for ComputerState {
    fn default() -> Self { Self { instance: uuid::Uuid::new_v4().to_string(), slots: Mutex::new(HashMap::new()),containers:Mutex::new(HashMap::new()),closing:AtomicBool::new(false) } }
}
pub(super) fn error(code: &'static str) -> String { crate::log::line(format!("computer: {code}")); code.into() }
pub fn identity(raw: &str) -> Result<String, String> {
    identity_with(raw, |id| crate::agents::find(id).is_some())
}
fn identity_with(raw:&str, local_exists:impl Fn(&str)->bool) -> Result<String,String> {
    if raw == "default" { return Ok(raw.into()); }
    let value = raw.strip_prefix("local:").unwrap_or(raw);
    if !crate::agents::valid_id(value) { return Err(error("computer-invalid-agent")); }
    if raw.starts_with("local:") && !local_exists(value) { return Err(error("computer-unknown-agent")); }
    Ok(raw.into())
}
fn container_name(instance: &str, agent: &str) -> String {
    let digest = Sha256::digest(agent.as_bytes());
    format!("roadeep-computer-{}-{:x}", instance.replace('-', ""), digest).chars().take(90).collect()
}
fn run_args(name: &str, instance: &str, agent: &str) -> Vec<String> {
    let label = format!("dev.roadeep.agent={:x}", Sha256::digest(agent.as_bytes()));
    ["run", "--detach", "--pull", "never", "--init", "--name", name,
        "--label", "dev.roadeep.computer=1", "--label", &format!("dev.roadeep.instance={instance}"), "--label", &label,
        "--network", "none", "--read-only", "--user", "1000:1000", "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
        "--memory", "768m", "--memory-swap", "768m", "--cpus", "1", "--pids-limit", "128", "--shm-size", "64m",
        "--tmpfs", &format!("/workspace:{WORKSPACE_TMPFS}"),
        "--tmpfs", &format!("/tmp:{TEMP_TMPFS}"),
        "--env", "HOME=/tmp/home", "--workdir", "/workspace", IMAGE].iter().map(|s| (*s).to_string()).collect()
}
fn owned(inspect: &Value, instance: &str, agent: &str) -> bool {
    let v = &inspect[0]; let labels = &v["Config"]["Labels"]; let host = &v["HostConfig"];
    let empty_array = |v:&Value| v.is_null() || v.as_array().is_some_and(Vec::is_empty);
    let tmpfs = &host["Tmpfs"];
    let digest = format!("{:x}", Sha256::digest(agent.as_bytes()));
    labels["dev.roadeep.computer"] == "1" && labels["dev.roadeep.instance"] == instance && labels["dev.roadeep.agent"] == digest
        && v["Config"]["User"] == "1000:1000" && v["Config"]["Image"] == IMAGE
        && v["HostConfig"]["NetworkMode"] == "none" && v["HostConfig"]["ReadonlyRootfs"] == true
        && v["HostConfig"]["Privileged"] == false
        && empty_array(&host["Binds"]) && empty_array(&host["CapAdd"])
        && host["CapDrop"] == json!(["ALL"])
        && host["SecurityOpt"].as_array().is_some_and(|a| a.len() == 1 && matches!(a[0].as_str(),Some("no-new-privileges" | "no-new-privileges:true")))
        && (host["PortBindings"].is_null() || host["PortBindings"].as_object().is_some_and(serde_json::Map::is_empty))
        && empty_array(&host["Devices"]) && empty_array(&host["DeviceRequests"])
        && host["Memory"] == 768 * 1024 * 1024 && host["MemorySwap"] == 768 * 1024 * 1024
        && host["NanoCpus"] == 1_000_000_000 && host["PidsLimit"] == 128 && host["ShmSize"] == 64 * 1024 * 1024
        && tmpfs.as_object().is_some_and(|m| m.len() == 2) && tmpfs["/workspace"] == WORKSPACE_TMPFS && tmpfs["/tmp"] == TEMP_TMPFS
        && v["Mounts"].as_array().is_some_and(|a| a.iter().all(|m| m["Type"] == "tmpfs" && matches!(m["Destination"].as_str(),Some("/workspace" | "/tmp"))))
}
async fn docker(args: Vec<String>, operation: Option<&Value>) -> Result<Vec<u8>, String> {
    let executable = process::executable().ok_or_else(|| error("computer-docker-missing"))?;
    process::run(&executable, &args, operation, Duration::from_secs(if operation.is_some() { 45 } else { 12 })).await
}
async fn ready() -> Result<(), String> {
    if process::executable().is_none() { return Err(error("computer-docker-missing")); }
    docker(vec!["info".into(), "--format".into(), "{{.ServerVersion}}".into()], None).await.map_err(|_| error("computer-docker-unavailable"))?;
    docker(vec!["image".into(), "inspect".into(), IMAGE.into(), "--format".into(), "{{.Id}}".into()], None).await.map_err(|_| error("computer-image-missing"))?;
    Ok(())
}
fn result(agent: &str, slot: &Slot, failure: Option<String>) -> ComputerStatus {
    ComputerStatus { available: failure.is_none(), state: if failure.is_some() { "unavailable" } else if slot.name.is_none() { "stopped" } else { slot.state }.into(), agent_id: agent.into(), takeover: slot.takeover, error: failure }
}
impl ComputerState {
    fn slot(&self, agent: &str) -> Result<Arc<tokio::sync::Mutex<Slot>>, String> {
        if self.closing.load(Ordering::SeqCst) { return Err(error("computer-shutting-down")); }
        let mut slots = self.slots.lock().map_err(|_| error("computer-state-error"))?;
        if let Some(slot) = slots.get(agent) { return Ok(slot.clone()); }
        // Idle identity records do not consume a live-computer resource slot.
        if slots.len() >= 64 { return Err(error("computer-agent-limit")); }
        let slot = Arc::new(tokio::sync::Mutex::new(Slot::default())); slots.insert(agent.into(), slot.clone()); Ok(slot)
    }
    async fn inspect(&self, slot: &Slot, agent: &str) -> Result<Value, String> {
        let name = slot.name.as_ref().ok_or_else(|| error("computer-not-started"))?;
        let data = docker(vec!["inspect".into(), name.clone()], None).await?;
        let value: Value = serde_json::from_slice(&data).map_err(|_| error("computer-runtime-protocol"))?;
        if !owned(&value, &self.instance, agent) { return Err(error("computer-ownership")); }
        Ok(value)
    }
    async fn remove_if_present(&self, slot:&Slot, agent:&str) -> Result<(),String> {
        let Some(name) = slot.name.as_ref() else { return Ok(()); };
        // Absence is confirmed by a successful daemon query, never inferred from an inspect failure.
        let listed = docker(vec!["ps".into(),"--all".into(),"--filter".into(),format!("name=^/{name}$"),"--format".into(),"{{.Names}}".into()],None).await?;
        let names = std::str::from_utf8(&listed).map_err(|_| error("computer-runtime-protocol"))?;
        if names.lines().all(|line| line.trim() != name) { return Ok(()); }
        self.inspect(slot,agent).await?;
        docker(vec!["rm".into(),"--force".into(),name.clone()],None).await?;
        Ok(())
    }
    pub async fn status(&self, raw: &str) -> Result<ComputerStatus, String> {
        let agent = identity(raw)?; let slot = self.slot(&agent)?;
        let mut slot = slot.try_lock().map_err(|_| error("computer-busy"))?;
        if let Err(code) = ready().await { return Ok(result(&agent, &slot, Some(code))); }
        if slot.name.is_some() {
            let value = self.inspect(&slot, &agent).await?;
            slot.state = if value[0]["State"]["Paused"] == true { "paused" } else if value[0]["State"]["Running"] == true { "running" } else { "stopped" };
        }
        Ok(result(&agent, &slot, None))
    }
    pub async fn lifecycle(&self, raw: &str, action: &str, on: bool) -> Result<ComputerStatus, String> {
        let started = Instant::now(); let operation_id = uuid::Uuid::new_v4();
        let agent = identity(raw)?; let arc = self.slot(&agent)?;
        let mut slot = arc.try_lock().map_err(|_| error("computer-busy"))?;
        ready().await?;
        if matches!(action, "stop" | "reset") {
            self.remove_if_present(&slot,&agent).await?;
            slot.name = None; slot.state = "stopped"; slot.takeover = false;
            self.containers.lock().map_err(|_| error("computer-state-error"))?.remove(&agent);
            if action == "stop" { return Ok(result(&agent, &slot, None)); }
        }
        if action == "start" && slot.name.is_some() {
            let value = self.inspect(&slot,&agent).await?;
            if value[0]["State"]["Running"] != true {
                self.remove_if_present(&slot,&agent).await?;
                slot.name=None;slot.state="stopped";slot.takeover=false;
                self.containers.lock().map_err(|_| error("computer-state-error"))?.remove(&agent);
            }
        }
        if matches!(action, "start" | "reset") && slot.name.is_none() {
            let name = container_name(&self.instance, &agent);
            {
                let mut names = self.containers.lock().map_err(|_| error("computer-state-error"))?;
                if self.closing.load(Ordering::SeqCst) { return Err(error("computer-shutting-down")); }
                if names.len() >= MAX_COMPUTERS { return Err(error("computer-agent-limit")); }
                names.insert(agent.clone(),name.clone());
            }
            // Record the name before spawn so cancellation/shutdown can recover a partial start.
            slot.name = Some(name.clone()); slot.state = "stopped";
            if let Err(code) = docker(run_args(&name, &self.instance, &agent), None).await {
                // The daemon may have created the container before its CLI timed out. Keep ownership recovery state.
                return Err(code);
            }
            self.inspect(&slot, &agent).await?;
            slot.state = "running";
            // Runtime initialization is real: a failed browser/runtime does not count as started.
            self.operation_locked(&agent, &slot, json!({"action":"status"})).await?;
        } else if matches!(action, "pause" | "resume" | "takeover") {
            let value = self.inspect(&slot, &agent).await?;
            let paused = value[0]["State"]["Paused"] == true;
            if action == "pause" && !paused {
                docker(vec!["pause".into(), slot.name.clone().unwrap()], None).await?; slot.state = "paused";
            } else if (action == "resume" || (action == "takeover" && on)) && paused {
                docker(vec!["unpause".into(), slot.name.clone().unwrap()], None).await?; slot.state = "running";
            }
            if action == "takeover" { slot.takeover = on; }
            if action == "resume" { slot.takeover = false; }
        }
        crate::log::line(format!("computer: op={operation_id} agent={:x} action={action} result=ok ms={}",Sha256::digest(agent.as_bytes()),started.elapsed().as_millis()));
        Ok(result(&agent, &slot, None))
    }
    async fn operation_locked(&self, agent: &str, slot: &Slot, operation: Value) -> Result<ComputerResult, String> {
        let value = self.inspect(slot, agent).await?;
        if value[0]["State"]["Paused"] == true { return Err(error("computer-paused")); }
        if value[0]["State"]["Running"] != true { return Err(error("computer-not-running")); }
        validate_operation(&operation)?;
        let output = docker(vec!["exec".into(), "--interactive".into(), "--user".into(), "1000:1000".into(), slot.name.clone().unwrap(), "node".into(), "/opt/roadeep/client.mjs".into()], Some(&operation)).await?;
        let value: Value = serde_json::from_slice(&output).map_err(|_| error("computer-runtime-protocol"))?;
        if value["ok"] != true { return Err(error("computer-operation-failed")); }
        let image = value["image"].as_str().map(str::to_owned);
        if image.as_ref().is_some_and(|s| s.len() > 2 * 1024 * 1024 || !s.starts_with("data:image/png;base64,")) { return Err(error("computer-output-limit")); }
        Ok(ComputerResult { ok: true, data: value["data"].clone(), image })
    }
    pub async fn operate(&self, raw: &str, operation: Value, human: bool) -> Result<ComputerResult, String> {
        let started = Instant::now(); let operation_id = uuid::Uuid::new_v4();
        let agent = identity(raw)?; let arc = self.slot(&agent)?;
        let slot = arc.try_lock().map_err(|_| error("computer-busy"))?;
        if human && !slot.takeover { return Err(error("computer-takeover-required")); }
        if !human && slot.takeover { return Err(error("computer-human-control")); }
        let result = self.operation_locked(&agent, &slot, operation).await;
        crate::log::line(format!("computer: op={operation_id} agent={:x} result={} ms={}",Sha256::digest(agent.as_bytes()),result.as_ref().err().map(String::as_str).unwrap_or("ok"),started.elapsed().as_millis()));
        result
    }
    pub fn agent_running(&self, raw: &str) -> bool {
        if self.closing.load(Ordering::SeqCst) { return false; }
        let Ok(slots) = self.slots.lock() else { return false; };
        slots.get(raw).and_then(|s| s.try_lock().ok()).is_some_and(|s| s.name.is_some() && s.state == "running" && !s.takeover)
    }
    pub async fn shutdown(&self) {
        self.closing.store(true,Ordering::SeqCst);
        let names: Vec<_> = self.containers.lock().map(|s| s.iter().map(|(id,name)| (id.clone(),name.clone())).collect()).unwrap_or_default();
        // Separate ownership registry permits cleanup while an operation holds its slot.
        // Parallel cleanup is bounded by the four-computer limit and app's exit deadline.
        futures_util::future::join_all(names.into_iter().map(|(id,name)| async move {
            let slot = Slot {name:Some(name.clone()),state:"stopped",takeover:false};
            if self.inspect(&slot,&id).await.is_err() { crate::log::line("computer: shutdown ownership/availability uncertain"); return; }
            if docker(vec!["rm".into(),"--force".into(),name],None).await.is_err() { crate::log::line("computer: shutdown cleanup failed"); }
        })).await;
    }
}
pub fn validate_operation(value: &Value) -> Result<(), String> {
    let fail = || error("computer-invalid-operation");
    let obj = value.as_object().ok_or_else(fail)?;
    let action = obj.get("action").and_then(Value::as_str).ok_or_else(fail)?;
    let fields: &[&str] = match action {
        "status" | "screenshot" => &["action"], "navigate" => &["action","url"], "click" => &["action","x","y"],
        "type" => &["action","text"], "key" => &["action","key"], "scroll" => &["action","deltaY"],
        "list" | "read" => &["action","path"], "write" => &["action","path","text"], "terminal" => &["action","command"], _ => return Err(fail()),
    };
    if obj.keys().any(|k| !fields.contains(&k.as_str())) { return Err(fail()); }
    let string = |key: &str, max: usize| -> Result<&str,String> { let s = value[key].as_str().ok_or_else(fail)?; if s.len() > max || s.contains('\0') { return Err(fail()); } Ok(s) };
    match action {
        "navigate" => { network::url(string("url", 4096)?).map_err(|_| fail())?; }
        "click" => { for (key,max) in [("x",1023), ("y",639)] { if !value[key].as_u64().is_some_and(|n| n <= max) { return Err(fail()); } } }
        "type" => { string("text", 16_000)?; }
        "key" => { if !["Enter","Tab","Escape","Backspace","ArrowUp","ArrowDown","ArrowLeft","ArrowRight","Control+A","Control+C","Control+V","Delete","Home","End","PageUp","PageDown"].contains(&string("key",40)?) { return Err(fail()); } }
        "scroll" => { if !value["deltaY"].as_i64().is_some_and(|n| (-4000..=4000).contains(&n)) { return Err(fail()); } }
        "list" if value.get("path").is_none() => {}
        "list" | "read" | "write" => {
            let p = string("path",512)?;
            if p.starts_with('/') || p.contains('\\') || p.contains(':') || p.split('/').any(|s| s == ".." || s.is_empty()) || p.chars().any(char::is_control) { return Err(fail()); }
            if action == "write" { string("text",64 * 1024)?; }
        }
        "terminal" => { if string("command",4000)?.trim().is_empty() { return Err(fail()); } }
        _ => {}
    }
    Ok(())
}
#[tauri::command] pub async fn computer_status(agent_id:String,state:State<'_,ComputerState>) -> Result<ComputerStatus,String> { state.status(&agent_id).await }
#[tauri::command] pub async fn computer_start(agent_id:String,state:State<'_,ComputerState>) -> Result<ComputerStatus,String> { state.lifecycle(&agent_id,"start",false).await }
#[tauri::command] pub async fn computer_pause(agent_id:String,state:State<'_,ComputerState>) -> Result<ComputerStatus,String> { state.lifecycle(&agent_id,"pause",false).await }
#[tauri::command] pub async fn computer_resume(agent_id:String,state:State<'_,ComputerState>) -> Result<ComputerStatus,String> { state.lifecycle(&agent_id,"resume",false).await }
#[tauri::command] pub async fn computer_stop(agent_id:String,state:State<'_,ComputerState>) -> Result<ComputerStatus,String> { state.lifecycle(&agent_id,"stop",false).await }
#[tauri::command] pub async fn computer_reset(agent_id:String,state:State<'_,ComputerState>) -> Result<ComputerStatus,String> { state.lifecycle(&agent_id,"reset",false).await }
#[tauri::command] pub async fn computer_takeover(agent_id:String,on:bool,state:State<'_,ComputerState>) -> Result<ComputerStatus,String> { state.lifecycle(&agent_id,"takeover",on).await }
#[tauri::command] pub async fn computer_operate(agent_id:String,operation:Value,state:State<'_,ComputerState>) -> Result<ComputerResult,String> { state.operate(&agent_id,operation,true).await }

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn identity_and_container_names_are_separate() {
        assert_eq!(identity("default").unwrap(),"default");
        assert_eq!(identity_with("research-agent_1", |_| false).unwrap(), "research-agent_1");
        assert_eq!(identity_with("local:custom_1", |_| true).unwrap(), "local:custom_1");
        assert!(identity_with("local:custom_1", |_| false).is_err());
        for bad in ["../../", "a b", "", "local:"] { assert!(identity_with(bad, |_| false).is_err()); }
        assert_ne!(container_name("one","default"),container_name("one","11111111-1111-4111-8111-111111111111"));
        assert_ne!(container_name("one","default"),container_name("two","default"));
    }
    #[test] fn argument_plan_isolated_no_host_mounts_or_network() {
        let args = run_args("safe","instance","default"); let joined = args.join(" ");
        for expected in ["--pull never","--network none","--read-only","--user 1000:1000","--cap-drop ALL","--pids-limit 128","--memory 768m","--cpus 1","no-new-privileges"] { assert!(joined.contains(expected)); }
        for forbidden in ["--volume", "--mount", "--publish", "--privileged", "docker.sock", "host-gateway", "--ipc host"] { assert!(!joined.contains(forbidden)); }
    }
    #[test] fn strict_operation_path_and_identity_contract() {
        for v in [json!({"action":"read","path":"../secret"}),json!({"action":"write","path":"/etc/passwd","text":"x"}),json!({"action":"read","path":"a\\b"}),json!({"action":"screenshot","agentId":"other"}),json!({"action":"click","x":1024,"y":0}),json!({"action":"terminal","command":""})] { assert!(validate_operation(&v).is_err()); }
        assert!(validate_operation(&json!({"action":"write","path":"notes/readme.txt","text":"hello"})).is_ok());
    }
    #[test] fn startup_empty_and_takeover_prevents_offer() { let state = ComputerState::default(); assert!(!state.agent_running("default")); assert!(state.slots.lock().unwrap().is_empty()); }
    #[tokio::test] async fn takeover_paused_and_stopped_refuse_agent_without_launch() {
        let state = ComputerState::default(); let slot = state.slot("default").unwrap();
        { let mut slot = slot.lock().await; slot.name=Some("owned".into());slot.state="running";slot.takeover=true; }
        assert!(!state.agent_running("default"));
        assert_eq!(state.operate("default",json!({"action":"terminal","command":"x"}),false).await.unwrap_err(),"computer-human-control");
        state.closing.store(true,Ordering::SeqCst);
        assert_eq!(state.operate("default",json!({"action":"status"}),false).await.unwrap_err(),"computer-shutting-down");
    }
    #[test] fn ownership_requires_full_binding_and_isolation() {
        let inspect = json!([{ "Config":{"Image":IMAGE,"User":"1000:1000","Labels":{"dev.roadeep.computer":"1","dev.roadeep.instance":"i","dev.roadeep.agent":format!("{:x}",Sha256::digest(b"default"))}},"HostConfig":{"NetworkMode":"none","ReadonlyRootfs":true,"Privileged":false,"Binds":null,"CapAdd":null,"CapDrop":["ALL"],"SecurityOpt":["no-new-privileges"],"PortBindings":{},"Devices":[],"DeviceRequests":null,"Memory":768*1024*1024,"MemorySwap":768*1024*1024,"NanoCpus":1_000_000_000,"PidsLimit":128,"ShmSize":64*1024*1024,"Tmpfs":{"/workspace":WORKSPACE_TMPFS,"/tmp":TEMP_TMPFS}},"Mounts":[]}]);
        assert!(owned(&inspect,"i","default")); assert!(!owned(&inspect,"other","default")); assert!(!owned(&inspect,"i","11111111-1111-4111-8111-111111111111"));
        for (field,value) in [("CapDrop",json!([])),("CapAdd",json!(["SYS_ADMIN"])),("SecurityOpt",json!(["seccomp=unconfined"])),("PortBindings",json!({"80/tcp":[{"HostPort":"8080"}]})),("Memory",json!(0)),("MemorySwap",json!(-1)),("NanoCpus",json!(0)),("PidsLimit",json!(0)),("ShmSize",json!(0)),("Tmpfs",json!({"/workspace":"size=5g","/tmp":TEMP_TMPFS})),("Binds",json!(["C:/host:/host"]))] {
            let mut tampered = inspect.clone(); tampered[0]["HostConfig"][field] = value;
            assert!(!owned(&tampered,"i","default"),"tampered {field}");
        }
        let mut extra_mount = inspect.clone(); extra_mount[0]["Mounts"] = json!([{"Type":"tmpfs","Destination":"/host"}]);
        assert!(!owned(&extra_mount,"i","default"));
    }
}
