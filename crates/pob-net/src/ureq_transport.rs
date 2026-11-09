//! The real network. Never run by a test or a gate: nothing but the application's `main` constructs it into a `Net`.
//!
//! What `lcurl` does for the original and what this is, option by option:
//! `OPT_USERAGENT`, `OPT_HTTPHEADER`, `OPT_POST`/`POSTFIELDS` (curl's default content type kept), `OPT_FOLLOWLOCATION`, `OPT_ACCEPT_ENCODING ""` (gzip here, curl offers more),
//! `OPT_IPRESOLVE`, `OPT_PROXY` (http, https, socks4, socks4a, socks5, socks5h), `OPT_SSL_VERIFYPEER/HOST = 0`. No cookies: no script of the original asks for them.

use ureq::config::{Config, IpFamily};
use ureq::tls::TlsConfig;
use ureq::{Agent, Proxy};

use crate::oauth::{OauthServerResult, run_oauth_server};
use crate::{CONNECT_TIMEOUT, HttpResult, MAX_DOWNLOAD_BYTES, MAX_REPLY_BYTES, Reply, Request, TOTAL_TIMEOUT, Transport};

#[derive(Debug, Default)]
pub struct UreqTransport;

fn agent_of(req: &Request) -> Result<Agent, String> {
    agent_with(req, Some(TOTAL_TIMEOUT))
}

fn agent_with(req: &Request, total: Option<std::time::Duration>) -> Result<Agent, String> {
    let mut b = Config::builder()
        .http_status_as_error(false)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(total)
        .user_agent(req.user_agent.as_str())
        .max_redirects(if req.follow_redirects { 10 } else { 0 });
    // `CURL_IPRESOLVE_V4` = 1, `CURL_IPRESOLVE_V6` = 2
    b = match req.connection_protocol {
        Some(1.0) => b.ip_family(IpFamily::Ipv4Only),
        Some(2.0) => b.ip_family(IpFamily::Ipv6Only),
        _ => b,
    };
    if let Some(p) = req.proxy_url.as_deref() {
        let proxy = Proxy::new(p).map_err(|e| format!("proxy: {e}"))?;
        b = b.proxy(Some(proxy));
    }
    if req.no_ssl {
        b = b.tls_config(TlsConfig::builder().disable_verification(true).build());
    }
    Ok(b.build().new_agent())
}

fn header_lines(req: &Request) -> Vec<(String, String)> {
    let Some(h) = req.header.as_deref() else {
        return Vec::new();
    };
    h.split(['\r', '\n'])
        .filter(|l| !l.is_empty())
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

impl Transport for UreqTransport {
    fn perform(&self, req: &Request) -> HttpResult {
        let agent = agent_of(req)?;
        let headers = header_lines(req);
        let result = match &req.body {
            Some(body) => {
                let mut r = agent.post(&req.url);
                if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
                    r = r.header("Content-Type", "application/x-www-form-urlencoded");
                }
                for (k, v) in &headers {
                    r = r.header(k, v);
                }
                r.send(body.as_slice())
            }
            None => {
                let mut r = agent.get(&req.url);
                for (k, v) in &headers {
                    r = r.header(k, v);
                }
                r.call()
            }
        };
        let mut response = result.map_err(|e| e.to_string())?;
        let mut header = format!(
            "{:?} {} {}\r\n",
            response.version(),
            response.status().as_u16(),
            response.status().canonical_reason().unwrap_or("")
        );
        for (k, v) in response.headers() {
            header.push_str(k.as_str());
            header.push_str(": ");
            header.push_str(&String::from_utf8_lossy(v.as_bytes()));
            header.push_str("\r\n");
        }
        header.push_str("\r\n");
        let code = u32::from(response.status().as_u16());
        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_REPLY_BYTES)
            .read_to_vec()
            .map_err(|e| e.to_string())?;
        Ok(Reply { code, header, body })
    }

    fn oauth_server(&self, auth_url: &str, open: &dyn Fn(String)) -> OauthServerResult {
        run_oauth_server(auth_url, open)
    }

    /// Without the overall time limit: a large file on a slow line takes as long as it takes.
    fn download(&self, req: &Request, path: &std::path::Path) -> Result<(), String> {
        let agent = agent_with(req, None)?;
        let mut r = agent.get(&req.url);
        for (k, v) in &header_lines(req) {
            r = r.header(k, v);
        }
        let mut response = r.call().map_err(|e| e.to_string())?;
        if response.status().as_u16() != 200 {
            return Err(format!("the server answered {}", response.status().as_u16()));
        }
        let mut reader = response.body_mut().with_config().limit(MAX_DOWNLOAD_BYTES).reader();
        let mut file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        std::io::copy(&mut reader, &mut file).map(drop).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(proxy: &str) -> Request {
        Request {
            url: "https://example.invalid/".into(),
            proxy_url: Some(proxy.into()),
            user_agent: "t".into(),
            ..Request::default()
        }
    }

    /// Building an agent asks no one.
    #[test]
    fn agents_of_every_option_build() {
        for p in ["http://127.0.0.1:1", "socks5://127.0.0.1:1", "socks4://127.0.0.1:1"] {
            assert!(agent_of(&with(p)).is_ok(), "{p}");
        }
        assert!(agent_of(&with("nonsense://x:y:z")).is_err());
        for proto in [0.0, 1.0, 2.0] {
            let r = Request {
                connection_protocol: Some(proto),
                no_ssl: true,
                ..Request::default()
            };
            assert!(agent_of(&r).is_ok());
        }
    }

    #[test]
    fn header_lines_as_curl_splits_them() {
        let r = Request {
            header: Some("A: 1\r\nB:2\n\nnot a header".into()),
            ..Request::default()
        };
        assert_eq!(
            header_lines(&r),
            [("A".to_string(), "1".to_string()), ("B".to_string(), "2".to_string())]
        );
    }
}
