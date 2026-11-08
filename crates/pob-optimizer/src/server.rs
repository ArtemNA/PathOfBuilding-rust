//! The page of the optimizer, on 127.0.0.1 only. One search at a time; the page asks for its state every second.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use pob_engine::engine::Engine;
use pob_optimizer::{Control, Goal, Locks, Options, Report, optimize_with};
use pob_tree::NodeType;
use serde_json::{Value, json};

const PAGE: &str = include_str!("page.html");
const MOST_BYTES: usize = 16 * 1024 * 1024;

struct Job {
    control: Arc<Control>,
    done: Arc<Mutex<Option<Result<Report, String>>>>,
}

#[derive(Default)]
struct State {
    job: Mutex<Option<Job>>,
}

fn words(v: &Value, key: &str) -> Vec<String> {
    v[key]
        .as_str()
        .unwrap_or("")
        .split('\n')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn lower(v: &Value, key: &str) -> Vec<String> {
    words(v, key).iter().map(|s| s.to_lowercase()).collect()
}

fn number(v: &Value, key: &str) -> Option<f64> {
    v[key].as_f64().or_else(|| v[key].as_str().and_then(|s| s.trim().parse().ok()))
}

/// A build code or the XML of a build.
fn build_xml(input: &str) -> Result<String, String> {
    let t = input.trim();
    if t.starts_with('<') {
        return Ok(t.replace("\r\n", "\n"));
    }
    let bytes = pob_build::code::decode_build_code(t).ok_or("this is neither a build code nor build XML")?;
    Ok(String::from_utf8_lossy(&bytes).replace("\r\n", "\n"))
}

fn start(state: &State, cfg: &Value) -> Result<(), String> {
    let mut slot = state.job.lock().map_err(|_| "locked")?;
    if slot.as_ref().is_some_and(|j| j.done.lock().is_ok_and(|d| d.is_none())) {
        return Err("a search is already running".into());
    }
    let xml = build_xml(cfg["build"].as_str().unwrap_or(""))?;
    let goal = Goal {
        min_elemental_resist: number(cfg, "minRes").unwrap_or(f64::NEG_INFINITY),
        min_chaos_resist: number(cfg, "minChaos"),
        min_ehp: number(cfg, "minEhp"),
        dps_weight: number(cfg, "dpsWeight").unwrap_or(1.0),
        ehp_weight: number(cfg, "ehpWeight").unwrap_or(1.0),
        forbid_gems: lower(cfg, "forbidGems"),
    };
    let opt = Options {
        seconds: number(cfg, "seconds").unwrap_or(60.0).clamp(1.0, 3600.0),
        moves_per_round: 400,
        threads: std::thread::available_parallelism().map_or(1, std::num::NonZero::get),
        seed: number(cfg, "seed").unwrap_or(1.0) as u64,
    };
    let control = Arc::new(Control::default());
    let done = Arc::new(Mutex::new(None));
    let (c, d, cfg) = (control.clone(), done.clone(), cfg.clone());
    std::thread::spawn(move || {
        let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Report, String> {
            let spec = Engine::load_xml(&xml)?.spec_rc();
            let mut locks = Locks::resolve(&spec, &words(&cfg, "forbidNodes"), &words(&cfg, "requireNodes"))?;
            drop(spec);
            locks.swap_supports = cfg["swapSupports"].as_bool().unwrap_or(false);
            locks.tattoos = cfg["tattoos"].as_bool().unwrap_or(false);
            locks.clusters = cfg["clusters"].as_bool().unwrap_or(false);
            locks.item_slots = words(&cfg, "itemSlots");
            locks.forbid_items = lower(&cfg, "forbidItems");
            locks.forbid_cluster_mods = lower(&cfg, "forbidClusterMods");
            locks.forbid_cluster_combos = lower(&cfg, "forbidClusterCombos")
                .iter()
                .map(|c| c.split('+').map(|m| m.trim().to_string()).collect())
                .collect();
            optimize_with(&xml, &goal, &locks, opt, &c)
        }))
        .unwrap_or_else(|_| Err("the calculation engine stopped on this build".into()));
        if let Ok(mut slot) = d.lock() {
            *slot = Some(run);
        }
    });
    *slot = Some(Job { control, done });
    Ok(())
}

/// What the fields can suggest: the nodes of this build's tree, and the game's gems, uniques and cluster mods.
fn catalog(cfg: &Value) -> Result<Value, String> {
    let xml = build_xml(cfg["build"].as_str().unwrap_or(""))?;
    let spec = Engine::load_xml(&xml)?.spec_rc();
    let mut nodes: Vec<(String, bool)> = spec
        .nodes
        .iter()
        .filter(|n| !n.is_subgraph_node() && n.tree_node().ascendancy_name.is_none())
        .filter(|n| matches!(n.ty(), NodeType::Normal | NodeType::Notable | NodeType::Keystone))
        .map(|n| (n.dn().to_string(), n.alloc))
        .collect();
    nodes.sort();
    let mut all: Vec<String> = nodes.iter().map(|n| n.0.clone()).collect();
    all.dedup();
    let mut have: Vec<String> = nodes.iter().filter(|n| n.1).map(|n| n.0.clone()).collect();
    have.dedup();
    let mut gems: Vec<String> = pob_data::gems::gems().gems.0.iter().map(|(_, g)| g.name.clone()).collect();
    gems.sort_unstable();
    gems.dedup();
    let mut uniques: Vec<String> = pob_optimizer::uniques()?.into_iter().map(|u| u.0).collect();
    uniques.sort_unstable();
    uniques.dedup();
    Ok(json!({ "nodes": all, "have": have, "gems": gems, "uniques": uniques, "clusterMods": pob_optimizer::cluster_mod_names() }))
}

fn eval_json(e: &pob_optimizer::Eval) -> Value {
    json!({
        "dps": e.dps, "ehp": e.ehp, "life": e.life, "fire": e.fire, "cold": e.cold,
        "lightning": e.lightning, "chaos": e.chaos, "warnings": e.warnings, "dropped": e.dropped_config,
    })
}

fn steps_json(steps: &[pob_optimizer::Step]) -> Value {
    Value::Array(
        steps
            .iter()
            .map(
                |s| json!({ "change": s.change, "score": s.score, "dps": s.eval.dps, "ehp": s.eval.ehp, "dropped": s.eval.dropped_config }),
            )
            .collect(),
    )
}

fn status(state: &State) -> Value {
    let Ok(slot) = state.job.lock() else {
        return json!({ "state": "idle" });
    };
    let Some(job) = slot.as_ref() else {
        return json!({ "state": "idle" });
    };
    let done = job.done.lock().ok();
    match done.as_ref().and_then(|d| d.as_ref()) {
        None => {
            let p = job.control.progress.lock().map(|p| p.clone()).unwrap_or_default();
            json!({ "state": "running", "tried": p.tried, "round": p.round, "steps": steps_json(&p.steps) })
        }
        Some(Err(e)) => json!({ "state": "error", "error": e }),
        Some(Ok(r)) => json!({
            "state": "done",
            "tried": r.tried, "allowed": r.accepted, "seconds": r.seconds, "score": r.best_score,
            "start": eval_json(&r.start), "best": eval_json(&r.best), "steps": steps_json(&r.steps),
            "byKind": r.tried_by_kind,
            "rejected": r.rejected.iter().map(|(k, n)| json!({ "why": k.label(), "n": n })).collect::<Vec<_>>(),
            "refused": r.engine_errors.iter().map(|(k, n)| json!({ "why": k, "n": n })).collect::<Vec<_>>(),
            "code": pob_build::code::encode_build_code(r.best_xml.as_bytes()),
        }),
    }
}

fn respond(s: &mut TcpStream, code: &str, kind: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {code}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
}

fn send_json(s: &mut TcpStream, code: &str, v: &Value) {
    respond(s, code, "application/json", v.to_string().as_bytes());
}

fn handle(mut s: TcpStream, state: &State, port: u16) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break p;
        }
        match s.read(&mut chunk) {
            Ok(n) if n > 0 && buf.len() < 65536 => buf.extend_from_slice(&chunk[..n]),
            _ => return,
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (method, path) = (first.next().unwrap_or(""), first.next().unwrap_or(""));
    let header = |name: &str| {
        head.split("\r\n").skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_string())
        })
    };
    let length: usize = header("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    if length > MOST_BYTES {
        respond(&mut s, "413 Too Large", "text/plain", b"too large");
        return;
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        match s.read(&mut chunk) {
            Ok(n) if n > 0 => body.extend_from_slice(&chunk[..n]),
            _ => return,
        }
    }
    // Another site in the browser must not be able to drive this: only our own address, as the Host and, when sent, as the Origin.
    let ours = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    let host_ok = header("host").is_some_and(|h| ours.contains(&h));
    let origin_ok = header("origin").is_none_or(|o| ours.iter().any(|a| o == format!("http://{a}")));
    if !host_ok || !origin_ok {
        respond(&mut s, "403 Forbidden", "text/plain", b"forbidden");
        return;
    }
    match (method, path) {
        ("GET", "/") => respond(&mut s, "200 OK", "text/html; charset=utf-8", PAGE.as_bytes()),
        ("GET", "/api/status") => send_json(&mut s, "200 OK", &status(state)),
        ("POST", "/api/run") => {
            let cfg: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            match start(state, &cfg) {
                Ok(()) => send_json(&mut s, "200 OK", &json!({ "ok": true })),
                Err(e) => send_json(&mut s, "400 Bad Request", &json!({ "error": e })),
            }
        }
        ("POST", "/api/catalog") => {
            let cfg: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            match catalog(&cfg) {
                Ok(v) => send_json(&mut s, "200 OK", &v),
                Err(e) => send_json(&mut s, "400 Bad Request", &json!({ "error": e })),
            }
        }
        ("POST", "/api/stop") => {
            if let Ok(slot) = state.job.lock()
                && let Some(j) = slot.as_ref()
            {
                j.control.stop.store(true, Ordering::Relaxed);
            }
            send_json(&mut s, "200 OK", &json!({ "ok": true }));
        }
        _ => respond(&mut s, "404 Not Found", "text/plain", b"not found"),
    }
}

pub fn serve(port: u16) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("port {port}: {e}"))?;
    println!("Open http://127.0.0.1:{port}/ in a browser. Ctrl+C stops the program.");
    let state = Arc::new(State::default());
    for stream in listener.incoming().flatten() {
        let st = state.clone();
        std::thread::spawn(move || handle(stream, &st, port));
    }
    Ok(())
}
