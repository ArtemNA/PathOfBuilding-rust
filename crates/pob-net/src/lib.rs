//! The port's `LaunchSubScript` + `lcurl` (UI-ARCH §2.6, §4.4): a [`Job`] runs on its own `std::thread`, the result waits in a channel until the frame polls it
//! ([`Net::poll`]), as `OnSubFinished` / `OnSubError` come on the main thread in the original.
//!
//! The [`Transport`] is the only way out of the process:
//! - [`NoNetwork`], the default of every `Net`: refuses and counts. Nothing but the application's `main` turns the network on, so no test can reach it by accident.
//! - [`UreqTransport`], the real one (`ureq` + `rustls`).
//! - [`ScriptedTransport`], recorded replies for tests and gates.
//!
//! Tokens: a [`Request`] holds its `Authorization` header in `header`; `Debug` of a request writes the URL only, and nothing here logs a header, a body or a reply.

use std::collections::{HashSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod hash;
mod oauth;
mod ureq_transport;

pub use hash::{base64_decode, base64_encode, sha1_hex, sha256};
pub use oauth::{OauthServerResult, parse_redirect_request, run_oauth_server};
pub use ureq_transport::UreqTransport;

/// Connecting. The original has no timeout at all (`lcurl`'s default, a request can hang forever); a frozen sub-script of the port would be only a label that never changes,
/// but `Fetching...` must end some day.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// The whole request: see [`CONNECT_TIMEOUT`].
pub const TOTAL_TIMEOUT: Duration = Duration::from_secs(300);
/// A reply larger than this is an error, not an allocation of whatever a server sends.
pub const MAX_REPLY_BYTES: u64 = 256 * 1024 * 1024;
/// A file downloaded with [`Job::Download`] larger than this is an error.
pub const MAX_DOWNLOAD_BYTES: u64 = 1024 * 1024 * 1024;

/// One `easy` handle of `lcurl` (Launch.lua:261-314, BuildSiteTools.lua:50-79, PoBArchivesProvider.lua:51-76, TreeTab.lua:768-783).
#[derive(Clone, Default)]
pub struct Request {
    pub url: String,
    /// `OPT_HTTPHEADER`: the lines of `requestHeader` (`[^\r\n]+`).
    pub header: Option<String>,
    /// `OPT_POST` + `OPT_POSTFIELDS`.
    pub body: Option<Vec<u8>>,
    /// `OPT_IPRESOLVE`: `launch.connectionProtocol` (0 whatever, 1 IPv4, 2 IPv6).
    pub connection_protocol: Option<f64>,
    /// `OPT_PROXY`.
    pub proxy_url: Option<String>,
    /// `OPT_SSL_VERIFYPEER = 0`, `OPT_SSL_VERIFYHOST = 0`.
    pub no_ssl: bool,
    /// `OPT_USERAGENT` (`Path of Building/<version>`).
    pub user_agent: String,
    /// `OPT_FOLLOWLOCATION = 1`. Off: the reply of a redirect is the reply (`INFO_REDIRECT_URL`, TreeTab.lua:778).
    pub follow_redirects: bool,
}

impl std::fmt::Debug for Request {
    /// The URL alone: a header holds the bearer token, a body the refresh token.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Request").field("url", &self.url).finish_non_exhaustive()
    }
}

/// What the `lcurl` calls of a sub-script leave: the code, the header text, the body.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reply {
    /// `INFO_RESPONSE_CODE`.
    pub code: u32,
    /// `responseHeader`: the status line and the header lines, as `OPT_HEADERFUNCTION` collects them.
    pub header: String,
    pub body: Vec<u8>,
}

impl Reply {
    /// `getinfo(INFO_REDIRECT_URL)`: the `Location` of a redirect.
    pub fn redirect_url(&self) -> Option<String> {
        if !(300..400).contains(&self.code) {
            return None;
        }
        self.header
            .lines()
            .find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("location")))
            .map(|(_, v)| v.trim().to_string())
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// `easy:perform()`: `Err` is `error:msg()`.
pub type HttpResult = Result<Reply, String>;

/// How far a [`Job::Download`] is: bytes written and the size the server announced (0: not known yet).
#[derive(Debug, Default)]
pub struct DownloadProgress {
    pub done: std::sync::atomic::AtomicU64,
    pub total: std::sync::atomic::AtomicU64,
}

impl DownloadProgress {
    /// Percent done, once the size is known.
    pub fn percent(&self) -> Option<u64> {
        let total = self.total.load(Ordering::Relaxed);
        (total > 0).then(|| (self.done.load(Ordering::Relaxed).min(total) * 100) / total)
    }
}

/// The way out of the process.
pub trait Transport: Send + Sync {
    fn perform(&self, req: &Request) -> HttpResult;
    /// `LaunchServer.lua`: `open` gets the URL to show (`Copy` + `OpenURL`, run on the main thread).
    fn oauth_server(&self, auth_url: &str, open: &dyn Fn(String)) -> OauthServerResult;
    /// The reply's body into a file. Here through memory; the real network streams it.
    fn download(&self, req: &Request, path: &std::path::Path, progress: &DownloadProgress) -> Result<(), String> {
        let r = self.perform(req)?;
        if r.code != 200 {
            return Err(format!("the server answered {}", r.code));
        }
        progress.total.store(r.body.len() as u64, Ordering::Relaxed);
        std::fs::write(path, &r.body).map_err(|e| e.to_string())?;
        progress.done.store(r.body.len() as u64, Ordering::Relaxed);
        Ok(())
    }
}

/// The default: no network. Every attempt is an error and is counted.
#[derive(Default)]
pub struct NoNetwork {
    pub attempts: AtomicUsize,
}

impl Transport for NoNetwork {
    fn perform(&self, _req: &Request) -> HttpResult {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        Err("the network is off".to_string())
    }

    fn oauth_server(&self, _auth_url: &str, _open: &dyn Fn(String)) -> OauthServerResult {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        OauthServerResult {
            err: Some("the network is off".to_string()),
            ..OauthServerResult::default()
        }
    }
}

/// One recorded answer: the requests whose URL equals `url`.
#[derive(Clone, Debug)]
pub struct Recorded {
    pub url: String,
    pub reply: HttpResult,
}

/// The replies of a test, written by hand from the schema upstream reads (no real site was asked). A URL with several replies gives them in turn, the last one stays. A `*` at the end of a recorded URL is any rest.
#[derive(Default)]
pub struct ScriptedTransport {
    replies: Mutex<Vec<(String, VecDeque<HttpResult>)>>,
    oauth: Mutex<VecDeque<OauthServerResult>>,
    /// Every request that came, in order.
    pub log: Mutex<Vec<Request>>,
    pub oauth_log: Mutex<Vec<String>>,
}

impl ScriptedTransport {
    pub fn add(&self, url: &str, reply: HttpResult) {
        let mut r = self.replies.lock().expect("replies");
        match r.iter_mut().find(|(u, _)| u == url) {
            Some((_, q)) => q.push_back(reply),
            None => r.push((url.to_string(), VecDeque::from([reply]))),
        }
    }

    /// What the local OAuth server "receives", in turn; the last stays. `state` = `None`: the `state` of the URL it was given comes back.
    pub fn set_oauth(&self, result: OauthServerResult) {
        self.oauth.lock().expect("oauth").push_back(result);
    }

    pub fn urls(&self) -> Vec<String> {
        self.log.lock().expect("log").iter().map(|r| r.url.clone()).collect()
    }
}

impl Transport for ScriptedTransport {
    fn perform(&self, req: &Request) -> HttpResult {
        self.log.lock().expect("log").push(req.clone());
        let mut r = self.replies.lock().expect("replies");
        let matches = |u: &str| match u.strip_suffix('*') {
            Some(prefix) => req.url.starts_with(prefix),
            None => u == req.url,
        };
        match r.iter_mut().find(|(u, _)| matches(u)) {
            Some((_, q)) if q.len() > 1 => q.pop_front().expect("len"),
            Some((_, q)) => q.front().cloned().expect("a recorded reply"),
            None => Err(format!("no recorded reply for {}", req.url)),
        }
    }

    fn oauth_server(&self, auth_url: &str, open: &dyn Fn(String)) -> OauthServerResult {
        self.oauth_log.lock().expect("log").push(auth_url.to_string());
        open(auth_url.to_string());
        let mut out = {
            let mut q = self.oauth.lock().expect("oauth");
            if q.len() > 1 {
                q.pop_front().expect("len")
            } else {
                q.front().cloned().unwrap_or_else(|| OauthServerResult {
                    err: Some("no recorded OAuth reply".to_string()),
                    ..OauthServerResult::default()
                })
            }
        };
        if out.code.is_some() && out.state.is_none() {
            out.state = auth_url
                .split(['?', '&'])
                .find_map(|p| p.strip_prefix("state="))
                .map(str::to_string);
        }
        out
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn checksum_matches(path: &std::path::Path, sha256_hex: &str) -> bool {
    std::fs::read(path).is_ok_and(|b| hex(&hash::sha256(&b)) == sha256_hex)
}

fn fetch_file(transport: &dyn Transport, req: &Request, path: &std::path::Path, sha256_hex: &str, progress: &DownloadProgress) -> Result<(), String> {
    if checksum_matches(path, sha256_hex) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    transport.download(req, path, progress)?;
    if checksum_matches(path, sha256_hex) {
        return Ok(());
    }
    let _ = std::fs::remove_file(path);
    Err("the downloaded file failed its checksum".to_string())
}

/// `LaunchSubScript`'s text, in the few shapes the original has.
#[derive(Clone, Debug)]
pub enum Job {
    Http(Request),
    OauthServer {
        auth_url: String,
    },
    /// A file, kept only if its SHA-256 is `sha256` (lower-case hex). A file already there with that checksum is not fetched again.
    Download {
        req: Request,
        path: std::path::PathBuf,
        sha256: String,
        /// Read by the main thread while the file comes.
        progress: Arc<DownloadProgress>,
    },
}

#[derive(Clone, Debug)]
pub enum Outcome {
    Http(HttpResult),
    Oauth(OauthServerResult),
    Download(Result<(), String>),
    /// The thread panicked (`OnSubError`).
    Crashed(String),
}

/// What a poll returns.
#[derive(Debug)]
pub enum Event {
    /// `OnSubFinished` / `OnSubError`.
    Finished(u64, Outcome),
    /// `OnSubCall("OpenURL"/"Copy", url)` of the OAuth server.
    OpenAuthUrl(u64, String),
}

/// The running sub-scripts.
pub struct Net {
    transport: Mutex<Arc<dyn Transport>>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    next: AtomicUsize,
    running: Arc<AtomicUsize>,
    cancelled: Mutex<HashSet<u64>>,
    spawn_fails: AtomicBool,
    inline: AtomicBool,
}

impl Default for Net {
    fn default() -> Self {
        Self::new(Arc::new(NoNetwork::default()))
    }
}

impl Net {
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        let (tx, rx) = channel();
        Self {
            transport: Mutex::new(transport),
            tx,
            rx,
            next: AtomicUsize::new(1),
            running: Arc::new(AtomicUsize::new(0)),
            cancelled: Mutex::new(HashSet::new()),
            spawn_fails: AtomicBool::new(false),
            inline: AtomicBool::new(false),
        }
    }

    pub fn set_transport(&self, transport: Arc<dyn Transport>) {
        *self.transport.lock().expect("transport") = transport;
    }

    /// `LaunchSubScript(...)`: the id, `None` when the thread could not start (the original returns nil and the caller goes on: no callback ever comes).
    pub fn spawn(&self, job: Job) -> Option<u64> {
        if self.spawn_fails.load(Ordering::SeqCst) {
            return None;
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst) as u64;
        let transport = self.transport.lock().expect("transport").clone();
        if self.inline.load(Ordering::SeqCst)
            && let Job::Http(req) = &job
        {
            let outcome = Outcome::Http(transport.perform(req));
            let _ = self.tx.send(Event::Finished(id, outcome));
            return Some(id);
        }
        let tx = self.tx.clone();
        let running = self.running.clone();
        running.fetch_add(1, Ordering::SeqCst);
        let on_failure = running.clone();
        let spawned = std::thread::Builder::new().name(format!("pob-net-{id}")).spawn(move || {
            let outcome = catch_unwind(AssertUnwindSafe(|| match &job {
                Job::Http(req) => Outcome::Http(transport.perform(req)),
                Job::Download { req, path, sha256, progress } => Outcome::Download(fetch_file(transport.as_ref(), req, path, sha256, progress)),
                Job::OauthServer { auth_url } => {
                    let tx2 = tx.clone();
                    let open = move |url: String| {
                        let _ = tx2.send(Event::OpenAuthUrl(id, url));
                    };
                    Outcome::Oauth(transport.oauth_server(auth_url, &open))
                }
            }))
            .unwrap_or_else(|p| {
                let msg = p
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_string())
                    .or_else(|| p.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "the network thread failed".to_string());
                Outcome::Crashed(msg)
            });
            let _ = tx.send(Event::Finished(id, outcome));
            running.fetch_sub(1, Ordering::SeqCst);
        });
        if spawned.is_err() {
            on_failure.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(id)
    }

    /// The thread cannot be stopped inside a blocking read: its result is dropped, its socket dies by [`TOTAL_TIMEOUT`].
    pub fn cancel(&self, id: u64) {
        self.cancelled.lock().expect("cancelled").insert(id);
    }

    /// Everything that has arrived. Never blocks.
    pub fn poll(&self) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(e) = self.rx.try_recv() {
            let id = match &e {
                Event::Finished(id, _) | Event::OpenAuthUrl(id, _) => *id,
            };
            let mut c = self.cancelled.lock().expect("cancelled");
            if c.contains(&id) {
                if matches!(e, Event::Finished(..)) {
                    c.remove(&id);
                }
                continue;
            }
            drop(c);
            out.push(e);
        }
        out
    }

    /// The threads still at work (tests wait for 0).
    pub fn running(&self) -> usize {
        self.running.load(Ordering::SeqCst)
    }

    /// Blocks until no thread is at work: the deterministic delivery of a test or a gate.
    pub fn wait_idle(&self) {
        while self.running() > 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// A gate's deterministic order: requests are answered in the order they were made, on this thread (a real sub-script races with the others).
    pub fn set_inline(&self, on: bool) {
        self.inline.store(on, Ordering::SeqCst);
    }

    /// A test of `LaunchSubScript` returning nil.
    pub fn set_spawn_fails(&self, on: bool) {
        self.spawn_fails.store(on, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(url: &str) -> Request {
        Request {
            url: url.to_string(),
            ..Request::default()
        }
    }

    fn drain(n: &Net) -> Vec<Event> {
        n.wait_idle();
        n.poll()
    }

    #[test]
    fn no_network_refuses_and_counts() {
        let t = Arc::new(NoNetwork::default());
        let n = Net::new(t.clone());
        let id = n.spawn(Job::Http(req("https://example.invalid/"))).expect("id");
        match drain(&n).as_slice() {
            [Event::Finished(i, Outcome::Http(Err(e)))] => {
                assert_eq!(*i, id);
                assert!(e.contains("off"));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(t.attempts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn scripted_replies_come_in_turn_and_the_last_stays() {
        let t = Arc::new(ScriptedTransport::default());
        t.add(
            "u",
            Ok(Reply {
                code: 200,
                ..Reply::default()
            }),
        );
        t.add(
            "u",
            Ok(Reply {
                code: 429,
                ..Reply::default()
            }),
        );
        for want in [200, 429, 429] {
            assert_eq!(t.perform(&req("u")).expect("reply").code, want);
        }
        assert!(t.perform(&req("v")).is_err());
        assert_eq!(t.urls(), ["u", "u", "u", "v"]);
    }

    struct Panics;
    impl Transport for Panics {
        fn perform(&self, _req: &Request) -> HttpResult {
            panic!("boom")
        }
        fn oauth_server(&self, _auth_url: &str, _open: &dyn Fn(String)) -> OauthServerResult {
            panic!("boom")
        }
    }

    #[test]
    fn a_panic_of_the_transport_is_an_event_not_a_crash() {
        let n = Net::new(Arc::new(Panics));
        n.spawn(Job::Http(req("u"))).expect("id");
        match drain(&n).as_slice() {
            [Event::Finished(_, Outcome::Crashed(m))] => assert_eq!(m, "boom"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_cancelled_result_is_dropped() {
        let t = Arc::new(ScriptedTransport::default());
        t.add("u", Ok(Reply::default()));
        let n = Net::new(t);
        let a = n.spawn(Job::Http(req("u"))).expect("id");
        let b = n.spawn(Job::Http(req("u"))).expect("id");
        n.cancel(a);
        let ev = drain(&n);
        assert_eq!(ev.len(), 1);
        assert!(matches!(ev[0], Event::Finished(i, _) if i == b));
    }

    #[test]
    fn spawn_failure_is_none() {
        let n = Net::default();
        n.set_spawn_fails(true);
        assert!(n.spawn(Job::Http(req("u"))).is_none());
        assert_eq!(n.running(), 0);
    }

    #[test]
    fn debug_of_a_request_hides_the_token() {
        let r = Request {
            url: "u".into(),
            header: Some("Authorization: Bearer SECRET".into()),
            body: Some(b"refresh_token=SECRET".to_vec()),
            ..Request::default()
        };
        assert!(!format!("{r:?}").contains("SECRET"));
    }

    #[test]
    fn redirect_url_reads_location() {
        let r = Reply {
            code: 302,
            header: "HTTP/1.1 302 Found\r\nLOCATION: https://x/y\r\n\r\n".into(),
            body: Vec::new(),
        };
        assert_eq!(r.redirect_url().as_deref(), Some("https://x/y"));
        assert_eq!(Reply { code: 200, ..r }.redirect_url(), None);
    }

    #[test]
    fn scripted_oauth_returns_the_state_of_the_url() {
        let t = ScriptedTransport::default();
        t.set_oauth(OauthServerResult {
            code: Some("c".into()),
            port: 49082,
            ..OauthServerResult::default()
        });
        let opened = Mutex::new(Vec::new());
        let r = t.oauth_server("https://h/a?x=1&state=abc&y=2", &|u| opened.lock().expect("o").push(u));
        assert_eq!(r.state.as_deref(), Some("abc"));
        assert_eq!(opened.lock().expect("o").len(), 1);
    }

    fn download_of(transport: Arc<dyn Transport>, path: &std::path::Path, sha: &str) -> Result<(), String> {
        let net = Net::new(transport);
        net.spawn(Job::Download {
            req: req("https://x/file"),
            path: path.to_path_buf(),
            sha256: sha.to_string(),
            progress: Arc::default(),
        });
        net.wait_idle();
        match net.poll().pop() {
            Some(Event::Finished(_, Outcome::Download(r))) => r,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_download_is_kept_only_with_the_right_checksum() {
        let dir = std::env::temp_dir().join(format!("pob-net-dl-{}", std::process::id()));
        let path = dir.join("sub").join("file");
        let t = Arc::new(ScriptedTransport::default());
        t.add(
            "https://x/file",
            Ok(Reply {
                code: 200,
                header: String::new(),
                body: b"payload".to_vec(),
            }),
        );
        let good = hex(&hash::sha256(b"payload"));
        download_of(t.clone(), &path, &good).expect("good file");
        assert_eq!(std::fs::read(&path).expect("kept"), b"payload");

        std::fs::remove_file(&path).expect("clean");
        let bad = hex(&hash::sha256(b"something else"));
        assert!(download_of(t.clone(), &path, &bad).is_err(), "a wrong checksum is an error");
        assert!(!path.exists(), "a file that failed its checksum is not left behind");

        // already there with the right checksum: no request at all
        std::fs::write(&path, b"payload").expect("seed");
        let quiet = Arc::new(NoNetwork::default());
        download_of(quiet.clone(), &path, &good).expect("from disk");
        assert_eq!(quiet.attempts.load(Ordering::SeqCst), 0);
        std::fs::remove_dir_all(&dir).expect("clean");
    }
}
