//! `LaunchServer.lua`: the local server the OAuth redirect comes back to. Here only `localhost`; the code, the state and the port go back to the caller, nothing is logged.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::time::{Duration, Instant};

/// `return code, errMsg, state, port`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OauthServerResult {
    pub code: Option<String>,
    pub err: Option<String>,
    pub state: Option<String>,
    pub port: u16,
}

const PORTS: [u16; 3] = [49082, 49083, 49084];
const WAIT: Duration = Duration::from_secs(60);

const PAGE_HEAD: &str = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"UTF-8\">\n<title>PoB - Authentication Complete</title>\n</head>\n<body>\n";
const PAGE_END: &str = "</body>\n</html>\n";

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = b
                .get(i + 1..i + 3)
                .and_then(|h| std::str::from_utf8(h).ok())
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What a request line says (`handleConnection`, LaunchServer.lua:125-173): `None` = not the redirect, wait for another connection;
/// `Some((code, state, page))` otherwise. `path:gmatch("([^&=?]+)=([^&=?]+)")`.
pub fn parse_redirect_request(line: &str) -> Option<(Option<String>, Option<String>, String)> {
    let path = line.strip_prefix("GET ")?;
    let path = path.split(' ').next()?;
    if !path.starts_with('/') || path.starts_with("/favicon") {
        return None;
    }
    let (mut code, mut state, mut error, mut description) = (None, None, None, None);
    for kv in path.split(['&', '?']) {
        let Some((k, v)) = kv.split_once('=') else { continue };
        let v = percent_decode(v);
        match k {
            "code" => code = Some(v),
            "state" => state = Some(v),
            "error" => error = Some(v),
            "error_description" => description = Some(v),
            _ => {}
        }
    }
    if code.is_some() {
        let page = format!(
            "{PAGE_HEAD}<h1>PoB - Authentication Successful</h1>\n<p>Your authentication is complete! You can now return to the app.</p>\n{PAGE_END}"
        );
        return Some((code, state, page));
    }
    // the text of a failed page: only what a browser shows, the markup of the two values escaped
    let esc = |s: Option<String>| s.unwrap_or_default().replace('&', "&amp;").replace('<', "&lt;");
    let page = format!(
        "{PAGE_HEAD}<h1>PoB - Authentication Failed</h1>\n<p>Authentication failed. Please try again.</p>\n<code>{}: {}</code>\n{PAGE_END}",
        esc(error),
        esc(description)
    );
    Some((None, state, page))
}

/// The server's loop: up to 60 s, one connection at a time, until a valid request came.
pub fn run_oauth_server(auth_url: &str, open: &dyn Fn(String)) -> OauthServerResult {
    let mut bound = None;
    let mut last_err = String::new();
    for p in PORTS {
        match TcpListener::bind((Ipv4Addr::LOCALHOST, p)) {
            Ok(l) => {
                bound = Some((l, p));
                break;
            }
            Err(e) => last_err = e.to_string(),
        }
    }
    let Some((listener, port)) = bound else {
        return OauthServerResult {
            err: Some(last_err),
            ..OauthServerResult::default()
        };
    };
    open(format!("{auth_url}&redirect_uri=http://localhost:{port}"));
    if let Err(e) = listener.set_nonblocking(true) {
        return OauthServerResult {
            err: Some(e.to_string()),
            port,
            ..OauthServerResult::default()
        };
    }
    let stop_at = Instant::now() + WAIT;
    while Instant::now() < stop_at {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut line = String::new();
                let Ok(clone) = stream.try_clone() else { continue };
                if BufReader::new(clone).read_line(&mut line).is_err() {
                    continue;
                }
                if let Some((code, state, page)) = parse_redirect_request(line.trim_end()) {
                    let mut w = stream;
                    let _ = w.write_all(page.as_bytes());
                    return OauthServerResult {
                        code,
                        err: None,
                        state,
                        port,
                    };
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    OauthServerResult {
        err: Some("Timeout reached without a response received by the local server".to_string()),
        port,
        ..OauthServerResult::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_redirect_with_a_code() {
        let (code, state, page) = parse_redirect_request("GET /?code=a%2Fb&state=0123 HTTP/1.1").expect("request");
        assert_eq!(code.as_deref(), Some("a/b"));
        assert_eq!(state.as_deref(), Some("0123"));
        assert!(page.contains("Successful"));
    }

    #[test]
    fn a_redirect_with_an_error() {
        let (code, _, page) = parse_redirect_request("GET /?error=denied&error_description=no%3Cb HTTP/1.1").expect("request");
        assert_eq!(code, None);
        assert!(page.contains("denied: no&lt;b"));
    }

    #[test]
    fn what_is_not_the_redirect_waits() {
        assert_eq!(parse_redirect_request("POST / HTTP/1.1"), None);
        assert_eq!(parse_redirect_request("garbage"), None);
        assert_eq!(parse_redirect_request("GET /favicon.ico HTTP/1.1"), None);
    }
}
