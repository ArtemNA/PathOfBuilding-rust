//! `Modules/BuildSiteTools.lua`: the sites builds are shared on. The labels and the patterns of the list are [`crate::tabs::import::WEBSITES`], this table holds the rest by the same index.

use pob_gfx::Gfx;

use crate::launch::{PageParams, launch};

/// `(regexURL, downloadURL, codeOut, postUrl, postFields)` (BuildSiteTools.lua:19-42).
pub const SITE_NET: [(&str, &str, &str, &str, &str); 8] = [
    (
        "maxroll%.gg/poe/pob/(.+)%s*$",
        "maxroll%.gg/poe/api/pob/%1",
        "https://maxroll.gg/poe/pob/",
        "https://maxroll.gg/poe/api/pob",
        "pobCode=",
    ),
    (
        "pob%.codes/b/([^?%s]+)[^%s]*%s*$",
        "api.pob.codes/%1/raw",
        "https://pob.codes/b/",
        "https://api.pob.codes/pob/plain",
        "",
    ),
    (
        "pobb%.in/(.+)%s*$",
        "pobb.in/pob/%1",
        "https://pobb.in/",
        "https://pobb.in/pob/",
        "",
    ),
    (
        "poe%.ninja/?p?o?e?1?/pob/(%w+)%s*$",
        "poe.ninja/poe1/pob/raw/%1",
        "",
        "https://poe.ninja/poe1/pob/api/upload",
        "code=",
    ),
    ("pastebin%.com/(%w+)%s*$", "pastebin.com/raw/%1", "", "", ""),
    ("pastebinp%.com/(%w+)%s*$", "pastebinp.com/raw/%1", "", "", ""),
    ("rentry%.co/(%w+)%s*$", "rentry.co/paste/%1/raw", "", "", ""),
    (
        "poedb%.tw/pob/(.+)%s*$",
        "poedb.tw/pob/%1/raw",
        "",
        "https://poedb.tw/pob/api/gen",
        "",
    ),
];

/// `linkURL` of the sites (BuildSiteTools.lua:19-42).
const SITE_LINK: [&str; 8] = [
    "maxroll%.gg/poe/pob/%1",
    "pob.codes/b/%1",
    "pobb.in/%1",
    "poe.ninja/poe1/pob/%1",
    "pastebin.com/%1",
    "pastebin.com/%1",
    "rentry.co/%1",
    "poedb.tw/pob/%1",
];

/// `link:gsub(websiteInfo.regexURL, websiteInfo.downloadURL)` (BuildSiteTools.lua:100), `site` 1-based.
pub fn site_code_url(link: &str, site: usize) -> String {
    let (regex, download, ..) = SITE_NET[site - 1];
    let out = pob_lua::gsub(link.as_bytes(), regex.as_bytes(), pob_lua::Repl::Str(download.as_bytes()), None)
        .expect("pattern")
        .0;
    String::from_utf8_lossy(&out).into_owned()
}

/// `buildSites.DownloadBuild(link, websiteInfo, callback)` of ImportTab and PartyTab (:88-113): `callback(isSuccess, data)`.
pub fn download_build(link: &str, site: usize, callback: impl FnOnce(&mut Gfx, bool, String) + 'static) {
    launch().download_page(
        &site_code_url(link, site),
        PageParams::default(),
        move |g, response, err| match err {
            Some(e) => callback(g, false, e),
            None => callback(g, true, response.body),
        },
    );
}

/// The pattern `^pob:[/\\]*<id>[/\\]+(.+)` of a site (BuildSiteTools.lua:93, :122).
fn uri_pattern(site: usize) -> String {
    format!("^pob:[/\\]*{}[/\\]+(.+)", crate::tabs::import::WEBSITES[site].1.to_ascii_lowercase())
}

/// `uri:gsub(pattern, "https://" .. url)` of the first site that matches, in the list's order.
fn uri_replace(uri: &str, url: impl Fn(usize) -> &'static str) -> Option<String> {
    (0..crate::tabs::import::WEBSITES.len()).find_map(|site| {
        let pat = uri_pattern(site);
        let repl = format!("https://{}", url(site));
        let (out, n) = pob_lua::gsub(uri.as_bytes(), pat.as_bytes(), pob_lua::Repl::Str(repl.as_bytes()), None).expect("pattern");
        (n > 0).then(|| String::from_utf8_lossy(&out).into_owned())
    })
}

/// `buildSites.ParseImportLinkFromURI(uri)` (:117-129). Main.lua:72 keeps the result in a local nothing reads: the import link of the build is the third argument of the callback.
pub fn parse_import_link_from_uri(uri: &str) -> Option<String> {
    uri_replace(uri, |site| SITE_LINK[site])
}

/// `buildSites.DownloadBuild(link, nil, callback)`, the branch of a program start (:91-99, :101-112): `callback(isSuccess, data, siteCodeURL)`.
pub fn download_build_from_uri(uri: &str, g: &mut Gfx, callback: impl FnOnce(&mut Gfx, bool, String, Option<String>) + 'static) {
    let Some(code_url) = uri_replace(uri, |site| SITE_NET[site].1) else {
        callback(g, false, "Download information not found".to_string(), None);
        return;
    };
    let url = code_url.clone();
    launch().download_page(&url, PageParams::default(), move |g, response, err| match err {
        Some(e) => callback(g, false, e, Some(code_url)),
        None => callback(g, true, response.body, Some(code_url)),
    });
}

/// `buildSites.UploadBuild(buildCode, websiteInfo)` + `RegisterSubScript` (:47-82): `callback(pasteLink, errMsg)`. `false`: the sub-script did not start.
pub fn upload_build(code: &str, site: usize, callback: impl FnOnce(&mut Gfx, Option<String>, Option<String>) + 'static) -> bool {
    let (.., post_url, post_fields) = SITE_NET[site - 1];
    let body = format!("{post_fields}{code}").into_bytes();
    let req = launch().net_request(post_url, None, Some(body), true);
    launch()
        .launch_sub(pob_net::Job::Http(req), move |g, outcome| {
            // :71-78 `easy:perform()` is not checked: a failure is `res ~= 200` with an empty page
            let (page, ok) = match outcome {
                pob_net::Outcome::Http(Ok(r)) => (r.text(), r.code == 200),
                _ => (String::new(), false),
            };
            if ok {
                callback(g, Some(page), None);
            } else {
                callback(g, None, Some(page));
            }
        })
        .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_download_url_of_every_site() {
        for (site, link, want) in [
            (1, "https://maxroll.gg/poe/pob/abc", "https://maxroll.gg/poe/api/pob/abc"),
            (2, "https://pob.codes/b/abc?x=1", "https://api.pob.codes/abc/raw"),
            (3, "https://pobb.in/abc", "https://pobb.in/pob/abc"),
            (4, "https://poe.ninja/poe1/pob/abc", "https://poe.ninja/poe1/pob/raw/abc"),
            (4, "https://poe.ninja/pob/abc", "https://poe.ninja/poe1/pob/raw/abc"),
            (5, "https://pastebin.com/abc", "https://pastebin.com/raw/abc"),
            (6, "https://pastebinp.com/abc", "https://pastebinp.com/raw/abc"),
            (7, "https://rentry.co/abc", "https://rentry.co/paste/abc/raw"),
            (8, "https://poedb.tw/pob/abc", "https://poedb.tw/pob/abc/raw"),
        ] {
            assert_eq!(site_code_url(link, site), want, "{link}");
        }
    }
}
