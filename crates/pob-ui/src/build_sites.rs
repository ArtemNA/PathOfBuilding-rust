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
