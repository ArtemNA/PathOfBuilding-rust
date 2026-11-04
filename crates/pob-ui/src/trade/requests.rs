//! `Classes/TradeQueryRequests.lua`: the queues of the trade API, respecting what the server says about its limits (`rate_limiter.rs`). Everything goes through `launch:DownloadPage`.
//! Tokens: the bearer token is only in the header of a request, never in a URL or a log line.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pob_gfx::Gfx;
use pob_items::item::INFLUENCE_INFO;
use pob_items::lua::escape_ggg_string;
use serde_json::{Value as Json, json};

use super::dkjson;
use super::helpers::{b64_gzip_decode, encode_league, url_decode};
use crate::launch::{PageParams, launch};
use crate::lua::lmatch;
use crate::modules::main::main;
use crate::poe_api::{os_time, reset_details};
use crate::rate_limiter::{BLOCKED, RateLimiter, policy_name};

const HOST_NAME: &str = "https://www.pathofexile.com/";
/// `self.hostNamePattern`.
pub const HOST_NAME_PATTERN: &str = "h?t?t?p?s?:?/?/?w?w?w?%.?pathofexile%.com/";

/// `tradeInfluenceApiKeys` (:11-20).
fn influence_api_key(key: &str) -> Option<&'static str> {
    Some(match key {
        "shaper" => "shaper",
        "elder" => "elder",
        "adjudicator" => "warlord",
        "basilisk" => "hunter",
        "crusader" => "crusader",
        "eyrie" => "redeemer",
        "cleansing" => "searing",
        "tangle" => "tangled",
        _ => return None,
    })
}

/// One entry of `FetchResultBlock` (:398-407).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TradeItem {
    pub amount: Option<f64>,
    pub currency: Option<String>,
    pub price_type: Option<String>,
    pub item_string: String,
    pub whisper: Option<String>,
    pub trader: Option<String>,
    pub weight: String,
    pub id: Option<String>,
}

pub type BodyCb = Rc<dyn Fn(&mut Gfx, String, Option<String>)>;
pub type SearchCb = Rc<dyn Fn(&mut Gfx, Option<Json>, Option<String>)>;
pub type ItemsCb = Rc<dyn Fn(&mut Gfx, Option<Vec<TradeItem>>, Option<String>)>;
pub type FinalCb = Rc<dyn Fn(&mut Gfx, Option<Vec<TradeItem>>, Option<String>, Option<String>)>;

struct Queued {
    url: String,
    body: Option<String>,
    callback: BodyCb,
    attempts: i32,
    retry_time: Option<f64>,
}

#[derive(Clone, Copy, PartialEq)]
enum QueueKey {
    Search,
    Fetch,
}

/// `self.requestQueue`: `pairs` over two string keys (R13: of the process): the order of the constructor here. It decides only who goes first when both have a request ready, and a `break` on a waiting one.
const QUEUES: [(QueueKey, &str); 2] = [(QueueKey::Search, "search"), (QueueKey::Fetch, "fetch")];

pub struct TradeQueryRequests {
    pub max_fetch_per_search: Cell<usize>,
    pub rate_limiter: RefCell<RateLimiter>,
    search: RefCell<Vec<Queued>>,
    fetch: RefCell<Vec<Queued>>,
}

impl Default for TradeQueryRequests {
    fn default() -> Self {
        Self {
            max_fetch_per_search: Cell::new(10),
            rate_limiter: RefCell::new(RateLimiter::default()),
            search: RefCell::new(Vec::new()),
            fetch: RefCell::new(Vec::new()),
        }
    }
}

/// `tostring` of a JSON scalar as a Lua value would be.
fn lua_str(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        Json::Number(n) => pob_lua::format::tostring_num(n.as_f64().unwrap_or(0.0)),
        Json::Bool(b) => b.to_string(),
        Json::Null => "nil".to_string(),
        _ => "table".to_string(),
    }
}

fn truthy(v: &Json) -> bool {
    !matches!(v, Json::Null | Json::Bool(false))
}

/// A concatenation with `..` of a field that must be a string or a number: else a Lua error, a prompt.
fn concat_field(v: &Json, line: &str, name: &str) -> String {
    match v {
        Json::String(_) | Json::Number(_) => lua_str(v),
        _ => panic!("Classes/TradeQueryRequests.lua:{line}: attempt to concatenate field '{name}'"),
    }
}

/// `processLine(modLine)` (:362-370): `pairs(flags)` (R13): the map's order.
fn process_line(line: &Json) -> String {
    let mut s = String::new();
    if let Some(flags) = line.get("flags").and_then(Json::as_object) {
        for (name, flag) in flags {
            if truthy(flag) {
                s.push_str(&format!("{{{name}}}"));
            }
        }
    }
    let d = line
        .get("description")
        .and_then(Json::as_str)
        .unwrap_or_else(|| panic!("Modules/Common.lua: bad argument to 'escapeGGGString' (string expected, got nil)"));
    s.push_str(&escape_ggg_string(d));
    s
}

/// The loop of `FetchResultBlock` over `response.result` (:315-408).
pub fn items_of(result: &[Json]) -> Vec<TradeItem> {
    let mut items = Vec::new();
    for entry in result {
        let item = &entry["item"];
        let mut raw = vec![format!("Rarity: {}", concat_field(&item["rarity"], "321", "rarity"))];
        if item["name"].as_str() != Some("") {
            raw.push(concat_field(&item["name"], "324", "name"));
        }
        raw.push(lua_str(&item["typeLine"]));
        if let Some(props) = item["properties"].as_array() {
            for p in props {
                let name = escape_ggg_string(p["name"].as_str().unwrap_or(""));
                match p["values"].get(0) {
                    Some(v) => raw.push(format!("{name}: {}", lua_str(&v[0]))),
                    None => raw.push(name),
                }
            }
        }
        if truthy(&item["ilvl"]) {
            raw.push(format!("Item Level: {}", concat_field(&item["ilvl"], "342", "ilvl")));
        }
        if let Some(sockets) = item["sockets"].as_array() {
            let colours: Vec<String> = sockets.iter().map(|s| lua_str(&s["sColour"])).collect();
            raw.push(format!("Sockets: {}", colours.join("-")));
        }
        let list = |f: &str| -> Vec<Json> { item[f].as_array().cloned().unwrap_or_default() };
        let (enchant, scourge, implicit, explicit) = (list("enchantMods"), list("scourgeMods"), list("implicitMods"), list("explicitMods"));
        raw.push(format!("Implicits: {}", enchant.len() + scourge.len() + implicit.len()));
        for m in &enchant {
            raw.push(format!("{{enchant}}{}", process_line(m)));
        }
        for m in &scourge {
            raw.push(format!("{{scourge}}{}", process_line(m)));
        }
        for m in implicit.iter().chain(&explicit) {
            raw.push(process_line(m));
        }
        for (key, display) in INFLUENCE_INFO.iter() {
            if let Some(api) = influence_api_key(key)
                && (truthy(&item["influences"][api]) || truthy(&item[api]))
            {
                raw.push(format!("{display} Item"));
            }
        }
        if truthy(&item["duplicated"]) {
            raw.push("Mirrored".to_string());
        }
        if truthy(&item["corrupted"]) {
            raw.push("Corrupted".to_string());
        }
        let pseudo = item["pseudoMods"].get(0);
        let line = pseudo.map(|p| if p["description"].is_null() { p } else { &p["description"] });
        let weight = line
            .and_then(Json::as_str)
            .and_then(|l| lmatch(l, "Sum: (.+)"))
            .unwrap_or_else(|| "0".to_string());
        let listing = &entry["listing"];
        items.push(TradeItem {
            amount: listing["price"]["amount"].as_f64(),
            currency: listing["price"]["currency"].as_str().map(str::to_owned),
            price_type: listing["price"]["type"].as_str().map(str::to_owned),
            item_string: raw.join("\n"),
            whisper: listing["whisper"].as_str().map(str::to_owned),
            trader: listing["account"]["name"].as_str().map(str::to_owned),
            weight,
            id: entry["id"].as_str().map(str::to_owned),
        });
    }
    items
}

impl TradeQueryRequests {
    fn queue(&self, k: QueueKey) -> &RefCell<Vec<Queued>> {
        match k {
            QueueKey::Search => &self.search,
            QueueKey::Fetch => &self.fetch,
        }
    }

    /// How many wait (a gate's look).
    pub fn queued(&self) -> usize {
        self.search.borrow().len() + self.fetch.borrow().len()
    }

    /// `ProcessQueue(onRateLimit)` (:39-93).
    pub fn process_queue(self: &Rc<Self>, on_rate_limit: &Rc<dyn Fn(f64)>) {
        for (key, name) in QUEUES {
            if self.queue(key).borrow().is_empty() {
                continue;
            }
            let policy = policy_name(name);
            let now = os_time();
            let time_next = self.rate_limiter.borrow_mut().next_request_time(policy, now);
            let time_left = time_next - now;
            if time_left > 1.0 && time_next != BLOCKED {
                on_rate_limit(time_left);
            }
            let waiting = self.queue(key).borrow()[0].retry_time.is_some_and(|r| now < r);
            if waiting {
                continue;
            }
            if now < time_next {
                break;
            }
            let request = self.queue(key).borrow_mut().remove(0);
            let request_id = self.rate_limiter.borrow_mut().insert_request(policy, now);
            let mut header = "Content-Type: application/json".to_string();
            if let Some(t) = &main().accounts.borrow().last_token {
                header.push_str("\nAuthorization: Bearer ");
                header.push_str(t);
            }
            let me = self.clone();
            let url = request.url.clone();
            let body = request.body.clone();
            let orl = on_rate_limit.clone();
            launch().download_page(
                &url,
                PageParams {
                    header: Some(header),
                    body,
                },
                move |g, response, err| {
                    {
                        let mut limiter = me.rate_limiter.borrow_mut();
                        limiter.finish_request(policy, request_id);
                        limiter.update_from_header(&response.header, policy, os_time());
                    }
                    let is_429 = pob_lua::pattern::match_(response.header.as_bytes(), b"HTTP/[%d%.]+ (%d+)", None)
                        .ok()
                        .flatten()
                        .and_then(|c| match c.get(0) {
                            Some(pob_lua::pattern::Capture::Str { start, end }) => Some(response.header[start..end].to_string()),
                            _ => None,
                        })
                        .as_deref()
                        == Some("429");
                    if is_429 {
                        let retry_after = lmatch(&response.header, "Retry%-After:%s+(%d+)")
                            .and_then(|r| pob_lua::tonumber(&r))
                            .unwrap_or(0.0);
                        let mut request = request;
                        request.attempts += 1;
                        let backoff = 2f64.powi(request.attempts).min(60.0).max(retry_after);
                        request.retry_time = Some(os_time() + backoff);
                        me.queue(key).borrow_mut().insert(0, request);
                        orl(backoff);
                        return;
                    }
                    let mut err = err;
                    if err.as_deref() == Some("Response code: 401") && response.body.contains("invalid_token") {
                        err = Some(format!(
                            "{}\nAuthorization is invalid. Please Re-Log and reset",
                            err.unwrap_or_default()
                        ));
                        reset_details();
                    }
                    (request.callback)(g, response.body, err);
                },
            );
        }
    }

    /// `SearchWithQuery(realm, league, query, callback, params)` (:100-112).
    pub fn search_with_query(self: &Rc<Self>, realm: Option<&str>, league: &str, query: String, callback: ItemsCb) {
        let me = self.clone();
        self.perform_search(
            realm,
            league,
            query,
            Rc::new(move |g, response, err| {
                if let Some(err) = err {
                    callback(g, None, Some(err));
                    return;
                }
                let response = response.expect("response");
                me.fetch_results(
                    hash_list(&response),
                    response["id"].as_str().unwrap_or("").to_string(),
                    callback.clone(),
                );
            }),
        );
    }

    /// `SearchWithQueryWeightAdjusted(realm, league, query, callback, params)` (:120-224).
    pub fn search_with_query_weight_adjusted(
        self: &Rc<Self>,
        realm: &str,
        league: &str,
        query: String,
        callback: FinalCb,
        callback_query_id: Rc<dyn Fn(&mut Gfx, String)>,
    ) {
        let st = Rc::new(Adjusted {
            requests: self.clone(),
            realm: realm.to_string(),
            league: league.to_string(),
            query: RefCell::new(query),
            previous_search_id: RefCell::new(None),
            previous_search_item_ids: RefCell::new(None),
            previous_search_items: RefCell::new(None),
            current_recursion: Cell::new(0),
            callback,
            callback_query_id,
        });
        st.search();
    }

    /// `PerformSearch(realm, league, query, callback)` (:232-267).
    pub fn perform_search(&self, realm: Option<&str>, league: &str, query: String, callback: SearchCb) {
        let url = build_url(&format!("{HOST_NAME}api/trade/search"), realm, league, None);
        let league = league.to_string();
        self.search.borrow_mut().push(Queued {
            url,
            body: Some(query),
            attempts: 0,
            retry_time: None,
            callback: Rc::new(move |g, body, err| {
                let mut err = err;
                if let Some(e) = &err
                    && !e.contains("Response code: 400")
                {
                    callback(g, None, err);
                    return;
                }
                let Some(response) = dkjson::decode(&body) else {
                    callback(g, None, Some("Failed to Get Trade response".to_string()));
                    return;
                };
                let empty = response["result"].as_array().is_none_or(Vec::is_empty);
                if empty {
                    if truthy(&response["error"]) {
                        let error = &response["error"];
                        if !(truthy(&error["code"]) && truthy(&error["message"])) {
                            callback(
                                g,
                                Some(response.clone()),
                                Some("Encountered unknown error, check console for details.".to_string()),
                            );
                        }
                        let message = error["message"].as_str().unwrap_or_else(|| {
                            panic!("Classes/TradeQueryRequests.lua:252: attempt to index field 'message' (a nil value)")
                        });
                        err = Some(if message.contains("Logging in will increase this limit") {
                            "Authorization is invalid. Please Re-Log and reset".to_string()
                        } else {
                            format!("[ {}: {message} ]", lua_str(&error["code"]))
                        });
                    } else {
                        g.con_printf(&format!(
                            "Found 0 results for {HOST_NAME}api/trade/search/{league}/{}",
                            lua_str(&response["id"])
                        ));
                        err = Some("No Matching Results Found".to_string());
                    }
                }
                callback(g, Some(response), err);
            }),
        });
    }

    /// `FetchResults(itemHashes, queryId, callback)` (:273-294).
    pub fn fetch_results(&self, hashes: Vec<String>, query_id: String, callback: ItemsCb) {
        let quantity = hashes.len().min(self.max_fetch_per_search.get());
        let items: Rc<RefCell<Vec<TradeItem>>> = Rc::new(RefCell::new(Vec::new()));
        let mut start = 0;
        while start < quantity {
            let end = (start + 10).min(quantity);
            let url = format!("{HOST_NAME}api/trade/fetch/{}?query={query_id}", hashes[start..end].join(","));
            let items = items.clone();
            let callback = callback.clone();
            self.fetch_result_block(
                url,
                Rc::new(move |g, block, err| {
                    if err.is_some() {
                        callback(g, None, err);
                        return;
                    }
                    items.borrow_mut().extend(block.unwrap_or_default());
                    if items.borrow().len() >= quantity {
                        let all = items.borrow().clone();
                        callback(g, Some(all), None);
                    }
                }),
            );
            start = end;
        }
    }

    /// `FetchResultBlock(url, callback)` (:299-412).
    fn fetch_result_block(&self, url: String, callback: ItemsCb) {
        self.fetch.borrow_mut().push(Queued {
            url,
            body: None,
            attempts: 0,
            retry_time: None,
            callback: Rc::new(move |g, body, err| {
                if err.is_some() {
                    callback(g, None, err);
                    return;
                }
                let response = dkjson::decode(&body);
                let Some(result) = response.as_ref().and_then(|r| r["result"].as_array()) else {
                    let msg = if response.is_none() {
                        "JSON Parse Error: "
                    } else {
                        "Failed to Get Trade Items: "
                    };
                    callback(g, None, Some(msg.to_string()));
                    return;
                };
                callback(g, Some(items_of(result)), None);
            }),
        });
    }

    /// `SearchWithURL(url, callback)` (:415-465).
    pub fn search_with_url(self: &Rc<Self>, g: &mut Gfx, url: &str, callback: FinalCb) {
        let Some(subpath) = lmatch(url, &format!("{HOST_NAME_PATTERN}trade/search/(.+)$")) else {
            callback(g, None, Some("Invalid URL".to_string()), None);
            return;
        };
        let paths: Vec<&str> = subpath.split('/').filter(|p| !p.is_empty()).collect();
        if paths.len() < 2 || paths.len() > 3 {
            callback(g, None, Some("Invalid URL".to_string()), None);
            return;
        }
        let realm = (paths.len() == 3).then(|| paths[0].to_string());
        let league = url_decode(paths[paths.len() - 2]);
        let query_id = paths[paths.len() - 1];
        let Some(text) = b64_gzip_decode(query_id) else {
            callback(g, None, Some("URL is malformed".to_string()), None);
            return;
        };
        let malformed = |g: &mut Gfx| callback(g, None, Some("URL is malformed".to_string()), None);
        let Some(mut decoded) = dkjson::decode(&text) else {
            malformed(g);
            return;
        };
        if !decoded.is_object() || !decoded["stats"].is_array() || !decoded["stats"][0].is_object() {
            malformed(g);
            return;
        }
        let mut weight_group = None;
        for (i, group) in decoded["stats"].as_array().expect("array").iter().enumerate() {
            if !group.is_object() || !group["type"].is_string() {
                malformed(g);
                return;
            }
            if group["type"] == "weight" && weight_group.is_none() {
                weight_group = Some(i);
            }
        }
        let mut sort = json!({"price": "asc"});
        if let Some(i) = weight_group {
            // Purchase links expect the weighted group first.
            decoded["stats"].as_array_mut().expect("array").swap(0, i);
            sort = json!({"statgroup.0": "desc"});
        }
        let query = dkjson::encode(&json!({"query": decoded, "sort": sort, "engine": "new"}));
        let q = query.clone();
        let cb = callback.clone();
        self.search_with_query(
            realm.as_deref(),
            &league,
            query,
            Rc::new(move |g, items, err| cb(g, items, err, Some(q.clone()))),
        );
    }

    /// `FetchLeagues(realm, callback)` (:470-497).
    pub fn fetch_leagues(&self, realm: &str, callback: Rc<dyn Fn(&mut Gfx, Vec<String>, Option<String>)>) {
        let token = main().accounts.borrow().last_token.clone().unwrap_or_default();
        let realm = realm.to_string();
        launch().download_page(
            &format!("{HOST_NAME}api/trade/data/leagues"),
            PageParams {
                header: Some(format!("Authorization: Bearer {token}")),
                body: None,
            },
            move |g, response, err| {
                let fallback = || vec!["Standard".to_string(), "Hardcore".to_string()];
                if err.is_some() {
                    callback(g, fallback(), err);
                    return;
                }
                let parsed = dkjson::decode(&response.body);
                let error = parsed.as_ref().map(|j| &j["error"]).filter(|e| truthy(e));
                if parsed.is_none() || error.is_some() {
                    let fail = "Failed to parse trade leagues JSON".to_string();
                    let msg = match error {
                        Some(Json::Object(_)) => {
                            let e = error.expect("error");
                            e["message"]
                                .as_str()
                                .map(str::to_owned)
                                .or_else(|| truthy(&e["code"]).then(|| lua_str(&e["code"])))
                                .unwrap_or(fail)
                        }
                        Some(other) => lua_str(other),
                        None => fail,
                    };
                    callback(g, fallback(), Some(msg));
                    return;
                }
                let parsed = parsed.expect("parsed");
                let result = parsed["result"]
                    .as_array()
                    .unwrap_or_else(|| panic!("Classes/TradeQueryRequests.lua:488: bad argument #1 to 'pairs' (table expected, got nil)"));
                let leagues = result
                    .iter()
                    .filter(|v| v["realm"].as_str() == Some(realm.as_str()))
                    .map(|v| lua_str(&v["id"]))
                    .collect();
                callback(g, leagues, None);
            },
        );
    }
}

/// `response.result` as strings.
fn hash_list(response: &Json) -> Vec<String> {
    response["result"]
        .as_array()
        .map(|a| a.iter().map(lua_str).collect())
        .unwrap_or_default()
}

/// `buildUrl(root, realm, league, queryId)` (:504-517).
pub fn build_url(root: &str, realm: Option<&str>, league: &str, query_id: Option<&str>) -> String {
    let mut result = root.to_string();
    if let Some(r) = realm
        && r != "pc"
    {
        result.push('/');
        result.push_str(r);
    }
    result.push('/');
    result.push_str(&encode_league(league));
    if let Some(q) = query_id {
        result.push('/');
        result.push_str(q);
    }
    result
}

/// The upvalues of `SearchWithQueryWeightAdjusted`.
struct Adjusted {
    requests: Rc<TradeQueryRequests>,
    realm: String,
    league: String,
    query: RefCell<String>,
    previous_search_id: RefCell<Option<String>>,
    previous_search_item_ids: RefCell<Option<Vec<String>>>,
    previous_search_items: RefCell<Option<Vec<TradeItem>>>,
    current_recursion: Cell<i32>,
    callback: FinalCb,
    callback_query_id: Rc<dyn Fn(&mut Gfx, String)>,
}

/// Limit recursion to prevent potential loops (:127).
const MAX_RECURSION: i32 = 5;

impl Adjusted {
    fn result(&self, g: &mut Gfx, items: Option<Vec<TradeItem>>, err: Option<String>) {
        let q = self.query.borrow().clone();
        (self.callback)(g, items, err, Some(q));
    }

    fn search(self: &Rc<Self>) {
        let me = self.clone();
        let query = self.query.borrow().clone();
        self.requests.perform_search(
            Some(&self.realm),
            &self.league,
            query,
            Rc::new(move |g, r, e| me.on_search(g, r, e)),
        );
    }

    /// `queryJson.query.stats[1].value.min = ...` + `dkjson.encode` (:195-200, :212-217).
    fn adjust_min(&self, f: impl FnOnce(f64) -> f64) {
        let mut q = dkjson::decode(&self.query.borrow()).expect("the query was encoded here");
        let group = &mut q["query"]["stats"][0];
        if !truthy(&group["value"]) {
            group["value"] = json!({"min": 0});
        }
        let min = group["value"]["min"].as_f64().unwrap_or(0.0);
        group["value"]["min"] = dkjson::n(f(min));
        *self.query.borrow_mut() = dkjson::encode(&q);
    }

    /// `performSearchCallback(response, errMsg)` (:133-222).
    fn on_search(self: &Rc<Self>, g: &mut Gfx, response: Option<Json>, err: Option<String>) {
        self.current_recursion.set(self.current_recursion.get() + 1);
        let rec = self.current_recursion.get();
        if let Some(id) = response.as_ref().and_then(|r| r["id"].as_str()) {
            (self.callback_query_id)(g, id.to_string());
        }
        if let Some(e) = &err
            && (e != "No Matching Results Found" || rec >= MAX_RECURSION)
        {
            self.result(g, None, err);
            return;
        }
        let response = response.expect("response");
        let total = response["total"]
            .as_f64()
            .unwrap_or_else(|| panic!("Classes/TradeQueryRequests.lua:141: attempt to compare number with nil"));
        let max = self.requests.max_fetch_per_search.get() as f64;
        let id = lua_str(&response["id"]);
        let result = hash_list(&response);
        if (total >= max && total < 10000.0) || rec >= MAX_RECURSION {
            let previous = self.previous_search_items.borrow().clone();
            if let Some(previous) = previous.filter(|_| max > total) {
                let me = self.clone();
                self.requests.fetch_results(
                    result,
                    id,
                    Rc::new(move |g, items, err| {
                        if err.is_some() {
                            me.result(g, None, err);
                            return;
                        }
                        me.fill_from_previous(g, items.unwrap_or_default(), &previous);
                    }),
                );
            } else {
                let me = self.clone();
                self.requests
                    .fetch_results(result, id, Rc::new(move |g, items, err| me.result(g, items, err)));
            }
        } else if total < max {
            self.adjust_min(|m| m / 2.0);
            self.search();
        } else {
            *self.previous_search_item_ids.borrow_mut() = Some(result.clone());
            *self.previous_search_id.borrow_mut() = Some(id.clone());
            let first: Vec<String> = result.into_iter().take(10).collect();
            let me = self.clone();
            self.requests.fetch_results(
                first,
                id,
                Rc::new(move |_g, items, err| {
                    if err.is_some() {
                        me.result(_g, None, err);
                        return;
                    }
                    let items = items.unwrap_or_default();
                    let highest = pob_lua::tonumber(&items[0].weight).unwrap_or(f64::NAN);
                    *me.previous_search_items.borrow_mut() = Some(items);
                    me.adjust_min(|m| (highest + m) / 2.0);
                    me.search();
                }),
            );
        }
    }

    /// :149-187.
    fn fill_from_previous(self: &Rc<Self>, g: &mut Gfx, mut items: Vec<TradeItem>, previous: &[TradeItem]) {
        let max = self.requests.max_fetch_per_search.get();
        let mut fetched: Vec<Option<String>> = Vec::new();
        for v in &items {
            if !fetched.contains(&v.id) {
                fetched.push(v.id.clone());
            }
        }
        for v in previous {
            if items.len() >= max {
                break;
            }
            if !fetched.contains(&v.id) {
                items.push(v.clone());
                fetched.push(v.id.clone());
            }
        }
        let fill = max as i64 - items.len() as i64;
        if fill <= 0 {
            self.result(g, Some(items), None);
            return;
        }
        let mut unfetched = Vec::new();
        for v in self.previous_search_item_ids.borrow().clone().unwrap_or_default() {
            if unfetched.len() as i64 >= fill {
                break;
            }
            if !fetched.contains(&Some(v.clone())) {
                unfetched.push(v);
            }
        }
        let me = self.clone();
        let id = self.previous_search_id.borrow().clone().unwrap_or_default();
        self.requests.fetch_results(
            unfetched,
            id,
            Rc::new(move |g, new_items, err| {
                if err.is_some() {
                    me.result(g, None, err);
                    return;
                }
                let mut all = items.clone();
                all.extend(new_items.unwrap_or_default());
                me.result(g, Some(all), None);
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(
            build_url(
                "https://www.pathofexile.com/trade/search",
                Some("sony"),
                "Hardcore Ruthless",
                Some("q")
            ),
            "https://www.pathofexile.com/trade/search/sony/Hardcore%20Ruthless/q"
        );
        assert_eq!(build_url("r", Some("pc"), "Standard", None), "r/Standard");
        assert_eq!(build_url("r", None, "Standard", None), "r/Standard");
    }

    #[test]
    fn an_item_of_the_site_becomes_raw_text() {
        let e = json!({
            "id": "abc",
            "item": {"rarity": "Rare", "name": "Synth Loop", "typeLine": "Coral Ring", "ilvl": 80,
                     "properties": [{"name": "Quality", "values": [["+5%", 1]]}, {"name": "Corrupted", "values": []}],
                     "sockets": [{"sColour": "R"}, {"sColour": "G"}],
                     "enchantMods": [{"description": "E"}],
                     "implicitMods": [{"description": "I", "flags": {"crafted": true, "no": false}}],
                     "explicitMods": [{"description": "X"}],
                     "influences": {"warlord": true}, "searing": true, "duplicated": true, "corrupted": true,
                     "pseudoMods": [{"description": "Sum: 172.5"}]},
            "listing": {"price": {"amount": 3, "currency": "chaos", "type": "~price"}, "whisper": "@a hi", "account": {"name": "someone"}}
        });
        let items = items_of(&[e]);
        assert_eq!(items[0].weight, "172.5");
        assert_eq!(items[0].amount, Some(3.0));
        assert_eq!(
            items[0].item_string,
            "Rarity: Rare\nSynth Loop\nCoral Ring\nQuality: +5%\nCorrupted\nItem Level: 80\nSockets: R-G\nImplicits: 2\n{enchant}E\n{crafted}I\nX\nWarlord Item\nSearing Exarch Item\nMirrored\nCorrupted"
        );
    }

    #[test]
    fn a_magic_item_has_no_name_line() {
        let e = json!({"item": {"rarity": "Magic", "name": "", "typeLine": "Sapphire Ring"}, "listing": {"price": {}, "account": {}}});
        let i = items_of(&[e]);
        assert_eq!(i[0].item_string, "Rarity: Magic\nSapphire Ring\nImplicits: 0");
        assert_eq!(i[0].weight, "0");
    }
}
