//! `Classes/TradeQueryRateLimiter.lua`: the local account of the limits a server reports in its headers. No clock of its own: every call gets `now` (`os.time()` in the Lua).

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Bucket {
    request: f64,
    timeout: f64,
    decremented: bool,
}

#[derive(Clone, Debug, Default)]
struct Rule {
    /// By window (seconds).
    limits: BTreeMap<i64, Bucket>,
    state: BTreeMap<i64, Bucket>,
}

#[derive(Clone, Debug, Default)]
struct Policy {
    rules: BTreeMap<String, Rule>,
    retry_after: Option<f64>,
}

#[derive(Clone, Debug, Default)]
struct History {
    timestamps: Vec<f64>,
    max_window: f64,
    last_check: Option<f64>,
}

#[derive(Debug, Default)]
pub struct RateLimiter {
    policies: BTreeMap<String, BTreeMap<String, Rule>>,
    retry_after: BTreeMap<String, f64>,
    last_update: BTreeMap<String, f64>,
    history: BTreeMap<String, History>,
    pending: BTreeMap<String, Vec<u64>>,
    request_id: u64,
}

/// :9-62 `limitMargin`.
const LIMIT_MARGIN: f64 = 1.0;
/// :184 "PoE2 release date": blocks until the limits are known.
const BLOCKED: f64 = 1_956_528_000.0;

/// `headerString:gmatch("([%a%d%-]+): ([%g ]+)")`, the names lowercased (:67-74). The last of a name wins.
fn parse_headers(header: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in header.lines() {
        let Some((k, v)) = line.split_once(": ") else { continue };
        let k = k.rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).next().unwrap_or("");
        if k.is_empty() {
            continue;
        }
        let v: String = v.chars().take_while(|c| *c == ' ' || c.is_ascii_graphic()).collect();
        if !v.is_empty() {
            out.insert(k.to_ascii_lowercase(), v);
        }
    }
    out
}

/// `8:10:60,15:60:120`: request:window:timeout.
fn parse_buckets(value: &str) -> BTreeMap<i64, Bucket> {
    let mut out = BTreeMap::new();
    for bucket in value.split(',').filter(|b| !b.is_empty()) {
        let mut it = bucket.split(':').filter(|p| !p.is_empty());
        let (r, w, t) = (it.next(), it.next(), it.next());
        let num = |x: Option<&str>| x.and_then(pob_lua::tonumber);
        if let Some(w) = num(w) {
            out.insert(
                w as i64,
                Bucket {
                    request: num(r).unwrap_or(0.0),
                    timeout: num(t).unwrap_or(0.0),
                    decremented: false,
                },
            );
        }
    }
    out
}

fn parse_policy(header: &str, policy: &str, now: f64) -> (String, Policy) {
    let headers = parse_headers(header);
    let name = headers.get("x-rate-limit-policy").cloned().unwrap_or_else(|| policy.to_owned());
    let mut out = Policy::default();
    if let Some(r) = headers.get("retry-after").and_then(|r| pob_lua::tonumber(r)) {
        out.retry_after = Some(now + r);
    }
    if let Some(rules) = headers.get("x-rate-limit-rules") {
        for rule in rules.split(',').filter(|r| !r.is_empty()) {
            let rule = rule.to_ascii_lowercase();
            let limits = headers
                .get(&format!("x-rate-limit-{rule}"))
                .map(|v| parse_buckets(v))
                .unwrap_or_default();
            let state = headers
                .get(&format!("x-rate-limit-{rule}-state"))
                .map(|v| parse_buckets(v))
                .unwrap_or_default();
            out.rules.insert(rule, Rule { limits, state });
        }
    }
    (name, out)
}

impl RateLimiter {
    /// :120-160 `UpdateFromHeader`.
    pub fn update_from_header(&mut self, header: &str, policy: &str, now: f64) {
        let (key, mut value) = parse_policy(header, policy, now);
        self.history.entry(key.clone()).or_default();
        if let Some(r) = value.retry_after.take() {
            self.retry_after.insert(key.clone(), r);
        }
        // :80-90 `ReduceLimits`
        for rule in value.rules.values_mut() {
            for w in rule.limits.values_mut() {
                w.request = (w.request - LIMIT_MARGIN).max(1.0);
            }
        }
        let no_pending = self.pending.get(&key).is_none_or(Vec::is_empty);
        if !self.policies.contains_key(&key) || no_pending {
            self.policies.insert(key.clone(), value.rules.clone());
        } else if let Some(old) = self.policies.get_mut(&key) {
            for (rule, rv) in &value.rules {
                for (window, st) in &rv.state {
                    if let Some(o) = old.get_mut(rule).and_then(|r| r.state.get_mut(window))
                        && st.request > o.request
                    {
                        o.request = st.request;
                    }
                }
            }
        }
        self.last_update.insert(key.clone(), now);
        let max_window = value.rules.values().flat_map(|r| r.limits.keys()).copied().max().unwrap_or(0);
        self.history.get_mut(&key).expect("just made").max_window = max_window as f64;
    }

    /// :163-210 `NextRequestTime`.
    pub fn next_request_time(&mut self, policy: &str, now: f64) -> f64 {
        if !self.policies.contains_key(policy) {
            return if self.history.get(policy).is_some_and(|h| !h.timestamps.is_empty()) {
                BLOCKED
            } else {
                now
            };
        }
        if let Some(r) = self.retry_after.get(policy)
            && *r >= now
        {
            return now.max(*r);
        }
        self.age_out_requests(policy, now);
        let mut next = now;
        let last_update = self.last_update.get(policy).copied().unwrap_or(now);
        let stamps = self.history.get(policy).map_or(&[][..], |h| h.timestamps.as_slice());
        for rule in self.policies[policy].values() {
            for (window, limit) in &rule.limits {
                let Some(st) = rule.state.get(window) else { continue };
                if st.timeout > 0.0 {
                    next = next.max(last_update + st.timeout + 1.0);
                }
                if st.request >= limit.request {
                    let w = *window as f64;
                    let oldest = stamps.iter().take_while(|t| **t >= now - w).count();
                    next = if oldest == 0 {
                        next.max(last_update + limit.timeout + 1.0)
                    } else {
                        next.max(stamps[oldest - 1] + w + 1.0)
                    };
                }
            }
        }
        next
    }

    /// :212-237 `InsertRequest`.
    pub fn insert_request(&mut self, policy: &str, now: f64) -> u64 {
        let h = self.history.entry(policy.to_owned()).or_default();
        let at = h.timestamps.iter().position(|v| now >= *v).unwrap_or(0);
        h.timestamps.insert(at, now);
        if let Some(rules) = self.policies.get_mut(policy) {
            for rule in rules.values_mut() {
                for w in rule.state.values_mut() {
                    w.request += 1.0;
                }
            }
            self.last_update.insert(policy.to_owned(), now);
        }
        let id = self.request_id;
        self.request_id += 1;
        self.pending.entry(policy.to_owned()).or_default().push(id);
        id
    }

    /// :239-247 `FinishRequest`.
    pub fn finish_request(&mut self, policy: &str, id: u64) {
        if let Some(p) = self.pending.get_mut(policy) {
            p.retain(|x| *x != id);
        }
    }

    /// :249-279 `AgeOutRequests`.
    fn age_out_requests(&mut self, policy: &str, now: f64) {
        let Some(h) = self.history.get_mut(policy) else { return };
        let last = *h.last_check.get_or_insert(now);
        if last == now {
            return;
        }
        let rules = self.policies.get_mut(policy).expect("checked by the caller");
        let max_window = h.max_window;
        let mut i = h.timestamps.len();
        while i > 0 {
            i -= 1;
            let t = h.timestamps[i];
            for rule in rules.values_mut() {
                for (window, st) in &mut rule.state {
                    let w = *window as f64;
                    if t >= last - w && t < now - w && !st.decremented {
                        st.request = (st.request - 1.0).max(0.0);
                        st.decremented = true;
                    }
                }
            }
            if t < now - max_window {
                h.timestamps.remove(i);
            }
        }
        for rule in rules.values_mut() {
            for st in rule.state.values_mut() {
                st.decremented = false;
            }
        }
        h.last_check = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "HTTP/1.1 200 OK\r\nX-Rate-Limit-Policy: character-list-request-limit\r\nX-Rate-Limit-Rules: Account\r\nX-Rate-Limit-Account: 3:5:60\r\nX-Rate-Limit-Account-State: 1:5:0\r\n\r\n";

    #[test]
    fn the_first_request_is_free_the_second_waits_for_the_limits() {
        let mut r = RateLimiter::default();
        let p = "character-list-request-limit";
        assert_eq!(r.next_request_time(p, 100.0), 100.0);
        let id = r.insert_request(p, 100.0);
        assert_eq!(r.next_request_time(p, 100.0), BLOCKED);
        r.finish_request(p, id);
        r.update_from_header(HEADER, p, 100.0);
        assert_eq!(r.next_request_time(p, 100.0), 100.0);
    }

    #[test]
    fn at_the_limit_the_next_time_is_after_the_window() {
        let mut r = RateLimiter::default();
        let p = "character-list-request-limit";
        let id = r.insert_request(p, 100.0);
        r.finish_request(p, id);
        r.update_from_header(HEADER, p, 100.0);
        // the limit 3 less the margin is 2; the state says 1, one more makes it
        r.insert_request(p, 100.0);
        assert_eq!(r.next_request_time(p, 100.0), 100.0 + 5.0 + 1.0);
    }

    #[test]
    fn retry_after_blocks_until_then() {
        let mut r = RateLimiter::default();
        r.update_from_header("HTTP/1.1 429 Too Many\r\nRetry-After: 30\r\n\r\n", "character-request-limit", 100.0);
        assert_eq!(r.next_request_time("character-request-limit", 100.0), 130.0);
        assert_eq!(r.next_request_time("character-request-limit", 131.0), 131.0);
    }

    #[test]
    fn a_state_timeout_blocks() {
        let mut r = RateLimiter::default();
        let p = "character-request-limit";
        let h = "X-Rate-Limit-Rules: Ip\r\nX-Rate-Limit-Ip: 5:10:60\r\nX-Rate-Limit-Ip-State: 6:10:40\r\n";
        r.update_from_header(h, p, 100.0);
        assert_eq!(r.next_request_time(p, 100.0), 141.0_f64.max(100.0 + 60.0 + 1.0));
    }

    #[test]
    fn requests_age_out_of_the_window() {
        let mut r = RateLimiter::default();
        let p = "character-list-request-limit";
        let id = r.insert_request(p, 100.0);
        r.finish_request(p, id);
        r.update_from_header(HEADER, p, 100.0);
        r.insert_request(p, 100.0);
        assert!(r.next_request_time(p, 100.0) > 100.0);
        assert_eq!(r.next_request_time(p, 107.0), 107.0);
    }
}
