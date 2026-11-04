//! The gate of `data.cache` (R3-cache): the binary form of every table = its JSON, bit for bit, and a cache that does not belong to the lock is not used.

use std::fmt::Debug;

use pob_data::bincache::{CacheFile, Table, encode_table, table_files, write_cache};
use pob_data::{
    cluster_jewels, config_options, constants, data_dir, gems, globals, item_bases, item_mods, lock_path, mod_cache, mod_scalability,
    pantheons, rules, tree, verify_lock, wire,
};
use serde::de::DeserializeOwned;
use serde_json::Value as Json;

fn text(rel: &str) -> String {
    std::fs::read_to_string(data_dir().join(rel)).unwrap()
}

fn table_of(text: &str) -> Table {
    Table::new(encode_table(text).unwrap()).unwrap()
}

/// Key order, kinds and number bits; `==` on `Json` would pass `1` for `1.0` and ignore the order of the keys.
fn same(a: &Json, b: &Json, at: &str) {
    match (a, b) {
        (Json::Null, Json::Null) => {}
        (Json::Bool(x), Json::Bool(y)) => assert_eq!(x, y, "{at}"),
        (Json::String(x), Json::String(y)) => assert_eq!(x, y, "{at}"),
        (Json::Number(x), Json::Number(y)) => {
            assert_eq!(x.as_u64(), y.as_u64(), "{at}");
            assert_eq!(x.as_i64(), y.as_i64(), "{at}");
            assert_eq!(x.as_f64().map(f64::to_bits), y.as_f64().map(f64::to_bits), "{at}");
        }
        (Json::Array(x), Json::Array(y)) => {
            assert_eq!(x.len(), y.len(), "{at}");
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                same(p, q, &format!("{at}[{i}]"));
            }
        }
        (Json::Object(x), Json::Object(y)) => {
            assert_eq!(x.len(), y.len(), "{at}");
            for ((k, p), (l, q)) in x.iter().zip(y) {
                assert_eq!(k, l, "{at}");
                same(p, q, &format!("{at}.{k}"));
            }
        }
        _ => panic!("{at}: {a} / {b}"),
    }
}

#[test]
fn every_table_equals_its_json() {
    let files = table_files(&data_dir()).unwrap();
    assert!(files.len() > 40, "{files:?}");
    for rel in &files {
        let t = text(rel);
        let json: Json = serde_json::from_str(&t).unwrap();
        let cached: Json = table_of(&t).read().unwrap();
        same(&json, &cached, rel);
    }
}

fn typed<T: DeserializeOwned + Debug>(rel: &str) {
    let t = text(rel);
    let json: T = serde_json::from_str(&t).unwrap();
    let cached: T = table_of(&t).read().unwrap();
    assert!(format!("{json:?}") == format!("{cached:?}"), "{rel}");
}

/// Through the models the loaders use (options, enums, `$num`, key parsing): `Debug` prints every float in full.
#[test]
fn typed_tables_equal_their_json() {
    typed::<item_bases::ItemBases>("item-bases.json");
    typed::<constants::Constants>("constants.json");
    typed::<rules::Rules>("rules.json");
    typed::<globals::Globals>("globals.json");
    typed::<gems::Gems>("gems.json");
    typed::<config_options::ConfigOptions>("config-options.json");
    typed::<cluster_jewels::ClusterJewels>("cluster-jewels.json");
    // the model is a `HashMap`, whose `Debug` order differs per instance
    typed::<std::collections::BTreeMap<String, Vec<mod_scalability::ModScalabilityEntry>>>("mod-scalability.json");
    typed::<wire::OrderedMap<pantheons::Pantheon>>("pantheons.json");
    typed::<wire::OrderedMap<item_mods::ItemModLines>>("item-mods/ItemExclusive.json");
    typed::<wire::OrderedMap<item_mods::ItemModLines>>("item-mods/Foulborn.json");
    for v in tree::COMMITTED_TREE_VERSIONS {
        typed::<tree::TreeJson>(&format!("tree/{v}.json"));
    }
}

/// Patched and built: the lazy entries = the eager ones from the JSON, line by line and in order.
#[test]
fn mod_cache_entries_equal_their_json() {
    let json = mod_cache::from_json();
    let lazy = mod_cache::from_table(table_of(&text("mod-cache.json"))).unwrap();
    assert_eq!(json.len(), lazy.len());
    for (a, b) in json.entries.iter().zip(lazy.entries.iter()) {
        assert!(format!("{a:?}") == format!("{b:?}"), "{}", a.line);
        let bits = |e: &mod_cache::ModCacheEntry| -> Vec<u64> {
            e.mods
                .iter()
                .flatten()
                .filter_map(|m| {
                    if let pob_mods::Value::Num(n) = m.value {
                        Some(n.to_bits())
                    } else {
                        None
                    }
                })
                .collect()
        };
        assert_eq!(bits(a), bits(b), "{}", a.line);
        assert!(std::ptr::eq(lazy.get(&a.line).unwrap(), b), "{}", a.line);
    }
    assert_eq!(json.malformed_dropped(), lazy.malformed_dropped());
    assert!(lazy.get("not a line of the cache").is_none());
}

/// A cache of another lock, of another format or cut short is as good as none.
#[test]
fn a_foreign_cache_is_not_used() {
    let data = data_dir();
    let sha = verify_lock(&data, &lock_path(&data)).unwrap().manifest_sha256;
    let tmp = std::env::temp_dir().join(format!("pob-data-cache-{}", std::process::id()));
    std::fs::create_dir_all(tmp.join("sub")).unwrap();
    std::fs::write(tmp.join("a.json"), r#"{"b":[1,-2,0.5,-0.0,"x",null,true],"a":{"1":1e300}}"#).unwrap();
    std::fs::write(tmp.join("manifest.json"), "{}").unwrap();
    let path = tmp.join("data.cache");
    assert_eq!(write_cache(&tmp, &sha, &path).unwrap(), 1);
    let good = CacheFile::open(&path, &sha).expect("its own lock");
    let got: Json = good.table("a.json").unwrap().read().unwrap();
    same(
        &got,
        &serde_json::from_str(&std::fs::read_to_string(tmp.join("a.json")).unwrap()).unwrap(),
        "a.json",
    );
    assert!(good.table("manifest.json").is_none());
    assert!(CacheFile::open(&path, &"0".repeat(sha.len())).is_none(), "another lock");
    assert!(CacheFile::open(&tmp.join("none"), &sha).is_none(), "missing");
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &bytes[..bytes.len() - 1]).unwrap();
    assert!(CacheFile::open(&path, &sha).is_none(), "cut short");
    let mut other = bytes.clone();
    other[8] ^= 1;
    std::fs::write(&path, &other).unwrap();
    assert!(CacheFile::open(&path, &sha).is_none(), "another format");
    let mut damaged = bytes.clone();
    *damaged.last_mut().unwrap() ^= 1;
    std::fs::write(&path, &damaged).unwrap();
    let opened = CacheFile::open(&path, &sha).expect("the head is whole");
    assert!(opened.table("a.json").is_none(), "a damaged table is the JSON");
    std::fs::remove_dir_all(&tmp).unwrap();
}
