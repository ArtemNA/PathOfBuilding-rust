//! `src/` of a package (`src_cache.rs`, `tools/package.ps1`): the two ways of reading give the same, and a wrong file stops the reader. The ignored tests make a whole package in
//! `CARGO_TARGET_TMPDIR` from the upstream checkout.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_data::bincache::write_tables;
use pob_ui::lua_data::{LuaKey, LuaTab, LuaVal, parse_return};
use pob_ui::src_cache::{self, COPIES, FILE, MAGIC, Package, decode, encode, same};
use pob_ui::src_cache_gen::{generate, project_tree, tables, verify};
use pob_ui::tree_art::TreeArt;

#[path = "../../pob-gfx/tests/scene/mod.rs"]
mod scene;

fn upstream() -> String {
    format!("{}/src/", scene::pob_root())
}

fn lock_sha() -> String {
    let dir = pob_data::ensure_data().expect("data.lock");
    pob_data::verify_lock(dir, &pob_data::lock_path(dir)).expect("data.lock").manifest_sha256
}

/// The package of the test run, made once. Every test takes its own copy of what it spoils.
fn package() -> PathBuf {
    static MADE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    MADE.get_or_init(|| {
        let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("src-cache-package");
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&out).expect("the folder");
        generate(&upstream(), &out, &lock_sha()).expect("generate");
        out
    })
    .clone()
}

fn root_of(dir: &Path) -> String {
    format!("{}/", dir.to_string_lossy().replace('\\', "/"))
}

/// An empty folder of its own.
fn sandbox(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("src-cache-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("the folder");
    d
}

fn read_cache(dir: &Path, sha: &str) -> Vec<(String, Vec<u8>)> {
    let c = pob_data::bincache::CacheFile::open_as(&dir.join(FILE), MAGIC, sha).expect("the cache");
    let mut names: Vec<String> = c.table_names().map(str::to_string).collect();
    names.sort();
    names
        .into_iter()
        .map(|n| {
            let b = c.raw(&n).expect("a table").expect("its bytes");
            (n, b)
        })
        .collect()
}

#[test]
fn the_form_keeps_the_order_and_every_bit() {
    let src = br#"return { [3]= { 1, 2.5, -3, -0.0, 1e300, 0.1 }, [0.5]= "x\"y", b = true, c = false, [7]= {}, "pos", { z=1, y=2 }, [1]= "second", d = nil, [-1]= 4 }"#;
    let v = parse_return(src).expect("parse");
    let back = decode(&encode(&v)).expect("decode");
    assert!(same(&v, &back));
    let t = back.as_tab().expect("a table");
    let keys: Vec<String> = t.fields.iter().map(|(k, _)| format!("{k:?}")).collect();
    assert_eq!(keys[0], "Num(3.0)");
    assert_eq!(keys[1], "Num(0.5)");
    assert_eq!(keys.len(), 8);
    let minus_zero = t.get_num(3.0).unwrap().as_tab().unwrap().arr[3].as_num().unwrap();
    assert_eq!(minus_zero.to_bits(), (-0.0f64).to_bits());
    // `same` sees what it must see: an order, a bit
    let swapped = parse_return(b"return { a=1, b=2 }").unwrap();
    assert!(!same(&swapped, &parse_return(b"return { b=2, a=1 }").unwrap()));
    assert!(!same(&parse_return(b"return { 0.0 }").unwrap(), &parse_return(b"return { -0.0 }").unwrap()));
    // a damaged table is an error, whichever byte
    let bytes = encode(&v);
    assert!(decode(&bytes[..bytes.len() - 1]).is_err());
    let mut more = bytes.clone();
    more.push(0);
    assert!(decode(&more).is_err());
}

#[test]
fn a_checkout_has_no_table_and_reads_the_text() {
    let root = upstream();
    assert!(!Path::new(&root).join(FILE).exists());
    assert_eq!(src_cache::resolve(&root, "TreeData/3_29/tree.lua"), format!("{root}TreeData/3_29/tree.lua"));
    let t = src_cache::read_lua(&root, "Data/CurrencyNames.lua").expect("the text");
    assert!(t.as_tab().is_some());
}

#[test]
fn a_table_returned_by_name_is_read() {
    let root = upstream();
    let t = src_cache::read_lua(&root, "TreeData/2_6/tree.lua").expect("2_6");
    assert!(t.as_tab().unwrap().tab("nodes").is_some());
}

/// Every file through the table = upstream's, every table = the text: the check `tools/package.ps1` stops on.
#[test]
#[ignore = "makes a whole package: the gate of U12"]
fn the_package_is_upstream() {
    let (files, tabs) = verify(&upstream(), &package(), &lock_sha()).expect("verify");
    assert!(files >= 769, "{files}");
    assert!(tabs >= 96, "{tabs}");
}

/// A table that holds a different value with a right checksum: the check of the package sees it. A byte changed without the checksum: the reader stops.
#[test]
#[ignore = "makes a whole package: the gate of U12"]
fn a_spoiled_value_fails() {
    let sha = lock_sha();
    let pkg = package();
    let dir = sandbox("value");
    let root = root_of(&dir);
    std::fs::create_dir_all(dir.join("TreeData")).expect("the folder");
    let mut t = read_cache(&pkg, &sha);

    // the byte: the checksum differs
    let at = t.iter().position(|(n, _)| n == "Data/CurrencyNames.lua").expect("a table");
    let mut bytes = std::fs::read(pkg.join(FILE)).expect("the cache");
    let table = t[at].1.clone();
    let start = bytes.windows(table.len()).position(|w| w == table).expect("the table in the file");
    bytes[start + table.len() / 2] ^= 1;
    std::fs::write(dir.join(FILE), &bytes).expect("write");
    let p = Package::open(&root, &sha);
    let e = p.table("Data/CurrencyNames.lua").expect("has it").expect_err("damaged");
    assert!(e.contains("damaged"), "{e}");
    // a package has nothing else to read
    assert!(src_cache::read_lua(&root, "Data/CurrencyNames.lua").is_err());

    // the value: a right checksum and another number, in a tree's order-deciding table too
    let spoil = |key: &str, t: &mut Vec<(String, Vec<u8>)>, f: &dyn Fn(&LuaTab) -> LuaTab| {
        let at = t.iter().position(|(n, _)| n == key).expect("a table");
        let v = decode(&t[at].1).expect("decode");
        t[at].1 = encode(&LuaVal::Table(Rc::new(f(v.as_tab().expect("a table")))));
    };
    let dropped_last = |t: &LuaTab| {
        let mut x = t.clone();
        x.fields.pop();
        x
    };
    let swapped = |t: &LuaTab| {
        let mut x = t.clone();
        x.fields.swap(0, 1);
        x
    };
    for (key, f) in [
        ("Data/CurrencyNames.lua", &dropped_last as &dyn Fn(&LuaTab) -> LuaTab),
        ("TreeData/3_29/sprites.lua", &swapped),
    ] {
        let mut spoiled = t.clone();
        spoil(key, &mut spoiled, f);
        let file = sandbox("value-table");
        write_tables(MAGIC, &spoiled, &sha, &file.join(FILE)).expect("write");
        copy_tree(&pkg.join("TreeData"), &file.join("TreeData"));
        let e = verify(&upstream(), &file, &sha).expect_err("the check of the package");
        assert!(e.contains(key), "{key}: {e}");
    }
    t.clear();
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the folder");
    for e in std::fs::read_dir(from).expect("the folder").flatten() {
        let p = e.path();
        if p.is_dir() {
            copy_tree(&p, &to.join(e.file_name()));
        } else {
            std::fs::copy(&p, to.join(e.file_name())).expect("copy");
        }
    }
}

/// `path -> (canonical, size)` of a package with a replaced record.
fn with_copies(t: &[(String, Vec<u8>)], recs: &HashMap<String, (String, u64)>) -> Vec<(String, Vec<u8>)> {
    let fields = recs
        .iter()
        .map(|(k, (to, len))| {
            (
                LuaKey::Str(k.as_str().into()),
                LuaVal::Table(Rc::new(LuaTab {
                    arr: vec![LuaVal::Str(to.as_str().into()), LuaVal::Num(*len as f64)],
                    fields: Vec::new(),
                })),
            )
        })
        .collect();
    let mut out = t.to_vec();
    let at = out.iter().position(|(n, _)| n == COPIES).expect("the copies");
    out[at].1 = encode(&LuaVal::Table(Rc::new(LuaTab { arr: Vec::new(), fields })));
    out
}

/// A record that names a file of another size stops the reader; of the same size, the check of the package.
#[test]
#[ignore = "makes a whole package: the gate of U12"]
fn a_wrong_record_of_the_copies_fails() {
    let sha = lock_sha();
    let pkg = package();
    let t = read_cache(&pkg, &sha);
    let root_pkg = root_of(&pkg);
    let opened = Package::open(&root_pkg, &sha);
    let copies = opened.copies().expect("the copies").expect("a package").clone();
    assert!(copies.len() > 300, "{}", copies.len());
    let (path, (to, len)) = copies.iter().min().expect("a record");

    // the size: at the reader
    let other_size = copies.values().find(|(c, l)| l != len && c != to).expect("another file");
    let mut recs = copies.clone();
    recs.insert(path.clone(), (other_size.0.clone(), *len));
    let dir = sandbox("record");
    write_tables(MAGIC, &with_copies(&t, &recs), &sha, &dir.join(FILE)).expect("write");
    copy_tree(&pkg.join("TreeData"), &dir.join("TreeData"));
    let e = Package::open(&root_of(&dir), &sha).resolve(&root_of(&dir), path).expect_err("the reader");
    assert!(e.contains("bytes"), "{e}");
    let e = verify(&upstream(), &dir, &sha).expect_err("the check of the package");
    assert!(e.contains("bytes"), "{e}");

    // the same size, other bytes: at the check of the package
    let canonical = dir.join(to);
    let mut bytes = std::fs::read(&canonical).expect("the file");
    let n = bytes.len() / 2;
    bytes[n] ^= 1;
    std::fs::write(&canonical, bytes).expect("write");
    write_tables(MAGIC, &t, &sha, &dir.join(FILE)).expect("write");
    let root = root_of(&dir);
    assert!(Package::open(&root, &sha).resolve(&root, path).expect("the size is right").is_some());
    let e = verify(&upstream(), &dir, &sha).expect_err("the check of the package");
    assert!(e.contains("other bytes"), "{e}");

    // a canonical file that is gone
    std::fs::remove_file(&canonical).expect("remove");
    let e = Package::open(&root, &sha).resolve(&root, path).expect_err("the reader");
    assert!(e.contains(&to[..]), "{e}");
}

/// A cache of other data or of another form: not read. A package has nothing else, and says so; the start stops on it.
#[test]
#[ignore = "makes a whole package: the gate of U12"]
fn a_stale_cache_is_clear() {
    let sha = lock_sha();
    let pkg = package();
    let t = read_cache(&pkg, &sha);
    let base = sandbox("stale");
    let (data, src) = (base.join("data"), base.join("src"));
    std::fs::create_dir_all(&data).expect("the folder");
    std::fs::create_dir_all(src.join("TreeData")).expect("the folder");
    let root = root_of(&src);

    // no file at all
    let e = src_cache::check(&data).expect_err("no cache");
    assert!(e.contains("install it again"), "{e}");

    // the cache of another data
    write_tables(MAGIC, &t, "another", &src.join(FILE)).expect("write");
    assert!(matches!(Package::open(&root, &sha), Package::Stale));
    let other_root = root_of(&sandbox("stale-other"));
    std::fs::copy(src.join(FILE), Path::new(&other_root).join(FILE)).expect("copy");
    let e = src_cache::read_lua(&other_root, "Data/QueryMods.lua").expect_err("stale");
    assert!(e.contains("install it again"), "{e}");
    let e = src_cache::product(&other_root, "Data/Skills#flavour", |_| Ok(LuaVal::Nil)).expect_err("stale");
    assert!(e.contains("install it again"), "{e}");

    // another form
    write_tables(b"POBSRC0\n", &t, &sha, &src.join(FILE)).expect("write");
    assert!(matches!(Package::open(&root, &sha), Package::Stale));

    // the file of the data is not this file
    assert!(matches!(
        {
            let d = sandbox("stale-data-cache");
            std::fs::copy(pob_data::bincache::cache_path(pob_data::ensure_data().expect("lock")), d.join(FILE)).ok();
            Package::open(&root_of(&d), &sha)
        },
        Package::Stale | Package::None
    ));

    // beside the text a stale cache is ignored, as data.cache is
    let with_text = sandbox("stale-text");
    write_tables(MAGIC, &t, "another", &with_text.join(FILE)).expect("write");
    std::fs::write(with_text.join("GameVersions.lua"), "return {}").expect("write");
    std::fs::create_dir_all(with_text.join("Data")).expect("the folder");
    std::fs::write(with_text.join("Data/x.lua"), "return { a = 1 }").expect("write");
    let v = src_cache::read_lua(&root_of(&with_text), "Data/x.lua").expect("the text");
    assert_eq!(v.as_tab().unwrap().num("a"), Some(1.0));
}

fn art_digest(a: &TreeArt) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let bits = |x: f64| x.to_bits();
    let mut names: Vec<_> = a.assets.keys().collect();
    names.sort();
    for n in names {
        let x = &a.assets[n];
        let _ = writeln!(s, "asset {n} {} {} {:?}", bits(x.width.get()), bits(x.height.get()), x.tc.map(|t| t.map(bits)));
    }
    let mut names: Vec<_> = a.sprite_map.keys().collect();
    names.sort();
    for n in names {
        let mut tys: Vec<_> = a.sprite_map[n].iter().collect();
        tys.sort_by_key(|(k, _)| k.as_str());
        for (ty, x) in tys {
            let _ = writeln!(s, "sprite {n} {ty} {} {} {:?}", bits(x.width.get()), bits(x.height.get()), x.tc.map(|t| t.map(bits)));
        }
    }
    for c in &a.connectors {
        let _ = writeln!(
            s,
            "conn {:?} {} {} {} {:?} {:?}",
            c.ascendancy_name,
            c.node_id1,
            c.node_id2,
            c.ty,
            c.vert.map(|v| v.map(bits)),
            c.tc.map(bits)
        );
    }
    let mut ids: Vec<_> = a.nodes.keys().collect();
    ids.sort();
    for id in ids {
        let n = &a.nodes[id];
        let _ = writeln!(
            s,
            "node {id} {:?} {:?} {:?} {:?} {} {:?} {:?} {:?}",
            n.icon, n.active_icon, n.inactive_icon, n.active_effect_image, n.has_mastery_effects, n.recipe, n.flavour_text, n.out
        );
    }
    let _ = writeln!(s, "groups {:?}", a.group_order);
    let mut oo: Vec<_> = a.group_oo.iter().collect();
    oo.sort_by_key(|(k, _)| **k);
    let _ = writeln!(s, "oo {oo:?}");
    let _ = writeln!(s, "order {:?}", a.node_order);
    let mut bl: Vec<_> = a.bloodline_prefix.iter().collect();
    bl.sort();
    let _ = writeln!(s, "bloodline {bl:?}");
    for x in &a.ascendancies {
        let _ = writeln!(
            s,
            "asc {} {} {:?} {:?} {:?}",
            x.id,
            x.is_alternate,
            x.flavour_text,
            x.flavour_text_colour,
            x.flavour_rect.map(|r| (bits(r.0), bits(r.1)))
        );
    }
    let _ = writeln!(s, "{} {} {} {}", a.has_class_ascendancies, bits(a.size), bits(a.version_num), a.async_handles.len());
    s
}

/// What the view gets of every tree, by the text and by the table: `tree.lua` in the cache holds only the fields the art reads, so this is where a dropped field would show.
#[test]
#[ignore = "makes a whole package: the gate of U12"]
fn the_art_of_every_tree_is_the_same() {
    let (checkout, pkg) = (upstream(), root_of(&package()));
    let mut seen = 0;
    let mut failed = Vec::new();
    for v in pob_data::tree::tree_version_list().expect("versions") {
        let tree = pob_tree::tree::load_passive_tree(v).unwrap_or_else(|e| panic!("{v}: {e}"));
        match (TreeArt::load(&checkout, &tree), TreeArt::load(&pkg, &tree)) {
            (Ok(a), Ok(b)) => {
                let (x, y) = (art_digest(&a), art_digest(&b));
                if x != y {
                    let at = x.lines().zip(y.lines()).find(|(p, q)| p != q);
                    failed.push(format!("{v}: {at:?}"));
                }
                assert!(x.len() > 1000, "{v}");
                seen += 1;
            }
            (a, b) => failed.push(format!("{v}: {:?} {:?}", a.err(), b.err())),
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
    // 42 folders of TreeData: 41 versions and `legion`
    assert_eq!(seen, 41);
}

/// The art of a tree by the text and by the table (`--ignored --nocapture`): what the first opening of the Tree tab and a change of the tree version read. Min of 7.
#[test]
#[ignore = "perf"]
fn art_load_perf() {
    let (checkout, pkg) = (upstream(), root_of(&package()));
    for v in ["3_29", "3_13", "2_6"] {
        let tree = pob_tree::tree::load_passive_tree(v).expect("tree");
        let time = |root: &str| {
            (0..7)
                .map(|_| {
                    let t = std::time::Instant::now();
                    let a = TreeArt::load(root, &tree).expect("art");
                    let d = t.elapsed();
                    drop(a);
                    d.as_secs_f64() * 1e3
                })
                .fold(f64::MAX, f64::min)
        };
        println!("{v}: text {:.1} ms, table {:.1} ms", time(&checkout), time(&pkg));
    }
}

/// `tables` holds for `tree.lua` exactly its projection.
#[test]
#[ignore = "reads every Lua file of upstream"]
fn the_tree_tables_are_projections() {
    let root = upstream();
    let mut n = 0;
    for (key, v) in tables(&root).expect("tables") {
        if key.ends_with("/tree.lua") {
            let text = parse_return(&std::fs::read(format!("{root}{key}")).expect("the text")).expect("parse");
            assert!(same(&v, &project_tree(&text).expect("project")), "{key}");
            // the orders of `pairs`
            let (t, p) = (text.as_tab().unwrap(), v.as_tab().unwrap());
            for part in ["nodes", "groups"] {
                let keys = |t: &LuaTab| t.tab(part).map(|x| x.fields.iter().map(|(k, _)| format!("{k:?}")).collect::<Vec<_>>());
                assert_eq!(keys(t), keys(p), "{key} {part}");
            }
            n += 1;
        }
    }
    assert_eq!(n, 41);
}
