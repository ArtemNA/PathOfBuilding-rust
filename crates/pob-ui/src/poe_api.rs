//! `Classes/PoEAPI.lua` over `launch` (the tokens: `main.lastToken`, `main.lastRefreshToken`, `main.tokenExpiry`, in memory and in Settings.xml as the original keeps them; `self.authToken` and
//! `main.lastToken` are one value here, `UpdateMain` writes them together). No token goes to a log: the Lua's `ConPrintf` lines here say what happened, not what came.
//!
//! Not ported: `poe_api_response.json`, what `DownloadWithRefresh` writes in dev mode (PoEAPI.lua:177-184): the port's dev mode has no working directory of its own, and it would be a character's data on disk.

use std::cell::{Cell, RefCell};
use std::hash::{BuildHasher, Hasher};

use pob_gfx::Gfx;
use serde_json::Value as Json;

use crate::launch::{PageParams, PageResponse, launch};
use crate::modules::main::main;
use crate::rate_limiter::RateLimiter;

const BASE_URL: &str = "https://api.pathofexile.com";
const TOKEN_URL: &str = "https://www.pathofexile.com/oauth/token";
/// `scopesOAuth`.
const SCOPES: [&str; 4] = ["account:profile", "account:leagues", "account:characters", "account:trade"];
/// `self.ERROR_NO_AUTH`.
const ERROR_NO_AUTH: &str = "No auth token";

thread_local! {
    static CLOCK: Cell<Option<f64>> = const { Cell::new(None) };
}

/// A fixed `os.time()` for a gate (the rate limit's countdown is on the screen); the original's runner replaces `os.time` the same way.
pub fn set_os_time(t: Option<f64>) {
    CLOCK.with(|c| c.set(t));
}

/// `os.time()`.
pub fn os_time() -> f64 {
    CLOCK.with(Cell::get).unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs() as f64)
    })
}

/// `main.api`.
#[derive(Debug, Default)]
pub struct PoeApi {
    pub retries: Cell<u32>,
    pub rate_limiter: RefCell<RateLimiter>,
}

/// The `base64.encode(...)` of PKCE (PoEAPI.lua:72-74): URL alphabet, no padding.
fn base64_url(data: &[u8]) -> String {
    pob_net::base64_encode(data)
        .replace('+', "-")
        .replace('/', "_")
        .trim_end_matches('=')
        .to_owned()
}

/// The OS's randomness through std (`RandomState` is seeded by it), no crate.
fn random_u64() -> u64 {
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(os_time().to_bits());
    h.finish()
}

/// `client_id=pob&grant_type=refresh_token&refresh_token=...` etc.: the reply of the token endpoint (PoEAPI.lua:49-60, :142-146). `None`: `dkjson.decode` found nothing.
fn store_tokens(body: &str) -> Option<()> {
    let v: Json = serde_json::from_str(body).ok()?;
    let m = main();
    {
        let mut a = m.accounts.borrow_mut();
        a.last_token = v["access_token"].as_str().map(str::to_owned);
        a.last_refresh_token = v["refresh_token"].as_str().map(str::to_owned);
        a.token_expiry = Some(os_time() + v["expires_in"].as_f64().unwrap_or(0.0));
    }
    m.save_settings();
    m.api.retries.set(0);
    Some(())
}

/// `ResetDetails` (:77-82) + `UpdateMain` (:85-90).
pub fn reset_details() {
    let m = main();
    {
        let mut a = m.accounts.borrow_mut();
        a.last_token = None;
        a.last_refresh_token = None;
        a.token_expiry = None;
    }
    m.save_settings();
}

/// `self.authToken ~= nil`.
pub fn is_authorized() -> bool {
    main().accounts.borrow().last_token.is_some()
}

/// `ValidateAuth(callback)` (:34-69).
pub fn validate_auth(g: &mut Gfx, callback: impl FnOnce(&mut Gfx, bool, Option<String>) + 'static) {
    let (auth, refresh, expiry) = {
        let m = main();
        let a = m.accounts.borrow();
        (a.last_token.is_some(), a.last_refresh_token.clone(), a.token_expiry.unwrap_or(0.0))
    };
    let Some(refresh) = refresh.filter(|_| auth) else {
        callback(g, false, None);
        return;
    };
    if expiry >= os_time() {
        callback(g, true, None);
        return;
    }
    g.con_printf("Auth token expired");
    let body = format!("client_id=pob&grant_type=refresh_token&refresh_token={refresh}");
    launch().download_page(
        TOKEN_URL,
        PageParams {
            header: None,
            body: Some(body),
        },
        move |g, response, err| {
            if let Some(err) = err {
                g.con_printf("Failed to recreate auth token");
                reset_details();
                callback(g, false, Some(err));
                return;
            }
            if store_tokens(&response.body).is_some() {
                callback(g, true, None);
            } else {
                reset_details();
                callback(g, false, Some("Malformed response".to_owned()));
            }
        },
    );
}

/// `FetchAuthToken(callback)` (:93-154). PKCE's verifier is 32 random bytes where the Lua has `tostring(math.random(2^32-1))` seeded by `os.time()`: the shape of the exchange is the same, the secret is not guessable.
pub fn fetch_auth_token(callback: impl FnOnce(&mut Gfx, Option<String>) + 'static) {
    let mut secret = Vec::with_capacity(32);
    for _ in 0..4 {
        secret.extend_from_slice(&random_u64().to_le_bytes());
    }
    let code_verifier = base64_url(&secret);
    let code_challenge = base64_url(&pob_net::sha256(code_verifier.as_bytes()));
    // 16 character hex string
    let initial_state = format!("{:016x}", random_u64());
    let auth_url = format!(
        "https://www.pathofexile.com/oauth/authorize?client_id=pob&response_type=code&scope={}&state={initial_state}&code_challenge={code_challenge}&code_challenge_method=S256",
        SCOPES.join("%20")
    );
    launch().launch_sub(pob_net::Job::OauthServer { auth_url }, move |g, outcome| {
        let result = match outcome {
            pob_net::Outcome::Oauth(r) => r,
            pob_net::Outcome::Http(_) => return,
            pob_net::Outcome::Crashed(e) => pob_net::OauthServerResult {
                err: Some(e),
                ..pob_net::OauthServerResult::default()
            },
        };
        let Some(code) = result.code else {
            g.con_printf("Failed to get code from server");
            reset_details();
            callback(g, Some(result.err.unwrap_or_else(|| ERROR_NO_AUTH.to_owned())));
            return;
        };
        if result.state.as_deref() != Some(initial_state.as_str()) {
            g.con_printf("OAuth state mismatch during authentication");
            reset_details();
            callback(g, Some("OAuth state mismatch".to_owned()));
            return;
        }
        let form = format!(
            "client_id=pob&grant_type=authorization_code&code={code}&redirect_uri=http://localhost:{}&scope={}&code_verifier={code_verifier}",
            result.port,
            SCOPES.join(" ")
        );
        launch().download_page(
            TOKEN_URL,
            PageParams {
                header: None,
                body: Some(form),
            },
            move |g, response, err| {
                if let Some(err) = err {
                    g.con_printf("Failed to get token from server");
                    reset_details();
                    callback(g, Some(err));
                    return;
                }
                if store_tokens(&response.body).is_none() {
                    panic!("Classes/PoEAPI.lua:143: attempt to index local 'responseLua' (a nil value)");
                }
                // :148: the window comes back from the browser
                g.set_foreground();
                callback(g, None);
            },
        );
    });
}

/// `DownloadWithRefresh(endpoint, callback)` (:158-190).
pub fn download_with_refresh(
    g: &mut Gfx,
    endpoint: String,
    callback: impl FnOnce(&mut Gfx, Option<PageResponse>, Option<String>) + 'static,
) {
    validate_auth(g, move |g, valid, validation_err| {
        if !valid {
            reset_details();
            callback(g, None, Some(validation_err.unwrap_or_else(|| ERROR_NO_AUTH.to_owned())));
            return;
        }
        let token = main().accounts.borrow().last_token.clone().unwrap_or_default();
        let url = format!("{BASE_URL}{endpoint}");
        launch().download_page(
            &url,
            PageParams {
                header: Some(format!("Authorization: Bearer {token}")),
                body: None,
            },
            move |g, response, err| {
                let api = main().api.clone();
                if err.as_deref().is_some_and(|e| e.contains("401")) && api.retries.get() < 1 {
                    // try once again with refresh token
                    api.retries.set(1);
                    main().accounts.borrow_mut().token_expiry = Some(0.0);
                    download_with_refresh(g, endpoint, callback);
                } else {
                    api.retries.set(0);
                    if err.is_some() {
                        g.con_printf(&format!("Failed to download {endpoint}"));
                    }
                    callback(g, Some(response), err);
                }
            },
        );
    });
}

/// `DownloadWithRateLimit(policy, url, callback)` (:196-221): `callback(body, err, timeNext)`.
pub fn download_with_rate_limit(
    g: &mut Gfx,
    policy: &'static str,
    url: String,
    callback: impl FnOnce(&mut Gfx, Option<Json>, Option<String>, Option<f64>) + 'static,
) {
    let now = os_time();
    let api = main().api.clone();
    let time_next = api.rate_limiter.borrow_mut().next_request_time(policy, now);
    if now < time_next {
        callback(g, None, Some("Response code: 429".to_owned()), Some(time_next));
        return;
    }
    let request_id = api.rate_limiter.borrow_mut().insert_request(policy, now);
    download_with_refresh(g, url, move |g, response, err| {
        let api = main().api.clone();
        let mut limiter = api.rate_limiter.borrow_mut();
        limiter.finish_request(policy, request_id);
        let header = response.as_ref().map_or("", |r| r.header.as_str());
        limiter.update_from_header(header, policy, os_time());
        if let Some(err) = err {
            drop(limiter);
            // `callback(response, errMsg, nil)`: the callers read only `errMsg`
            callback(g, None, Some(err), None);
            return;
        }
        let status_429 = header
            .lines()
            .next()
            .is_some_and(|l| l.starts_with("HTTP/") && l.split(' ').nth(1) == Some("429"));
        if status_429 {
            let next = limiter.next_request_time(policy, now);
            drop(limiter);
            callback(g, None, Some("Response code: 429".to_owned()), Some(next));
            return;
        }
        drop(limiter);
        let body = response.and_then(|r| serde_json::from_str::<Json>(&r.body).ok());
        callback(g, body, None, None);
    });
}

fn realm_path(realm: &str) -> String {
    if realm == "pc" { String::new() } else { format!("/{realm}") }
}

/// `DownloadCharacterList(realm, callback)` (:226-229).
pub fn download_character_list(
    g: &mut Gfx,
    realm: &str,
    callback: impl FnOnce(&mut Gfx, Option<Json>, Option<String>, Option<f64>) + 'static,
) {
    download_with_rate_limit(
        g,
        "character-list-request-limit",
        format!("/character{}", realm_path(realm)),
        callback,
    );
}

/// `DownloadCharacter(realm, name, callback)` (:235-238).
pub fn download_character(
    g: &mut Gfx,
    realm: &str,
    name: &str,
    callback: impl FnOnce(&mut Gfx, Option<Json>, Option<String>, Option<f64>) + 'static,
) {
    download_with_rate_limit(
        g,
        "character-request-limit",
        format!("/character{}/{name}", realm_path(realm)),
        callback,
    );
}
