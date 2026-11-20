//! A `for` header's temporaries live for the whole loop: `for v in ok(db.borrow().list(..)) { add(db, v) }` keeps the `Ref` while the
//! body adds to the same modDB, and `RefCell already borrowed` follows (user report 2026-11-20: the unique list sorted by a stat
//! panicked on every build whose slot could take a `GroupProperty` item). Two gates:
//! - builds that reach the loops that had it: `GroupProperty` (CalcSetup.lua:1874) and the resistance conversion (CalcDefence.lua:566-573);
//! - a scan of the source for the pattern itself, so a new loop of that shape fails here, not in a user's frame.

use pob_calc::{CalcMode, CalcSession, CalculateOptions};
use pob_parser::ParseMode;
use std::path::Path;
use std::rc::Rc;

fn run(xml: &str) -> pob_calc::RunRef {
    let model = pob_build::load_build_xml(xml).expect("xml parses");
    let opts = CalculateOptions {
        parse_mode: ParseMode::Full,
        skills: true,
        ..CalculateOptions::default()
    };
    let sess = CalcSession::new(Rc::new(model), opts);
    sess.build_output(CalcMode::Main).expect("buildOutput").run
}

/// A level 90 Marauder with one body armour (the `lines` as its mods, red sockets) and red gems in it.
fn build_with_body(sockets: &str, lines: &[&str]) -> String {
    let mods = lines.join("\n");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<PathOfBuilding>
	<Build level="90" targetVersion="3_0" className="Marauder" ascendClassName="None" mainSocketGroup="1" viewMode="CALCS" bandit="None"/>
	<Tree activeSpec="1">
		<Spec treeVersion="3_29" classId="1" ascendClassId="0" nodes=""/>
	</Tree>
	<Skills activeSkillSet="1">
		<SkillSet id="1">
			<Skill enabled="true" slot="Body Armour" mainActiveSkill="1" includeInFullDPS="true">
				<Gem skillId="Cleave" level="20" quality="0" enabled="true"/>
				<Gem skillId="SupportMeleePhysicalDamage" level="20" quality="0" enabled="true"/>
			</Skill>
		</SkillSet>
	</Skills>
	<Items activeItemSet="1">
		<Item id="1">
Rarity: RARE
Test Shell
Plate Vest
Sockets: {sockets}
Implicits: 0
{mods}
		</Item>
		<Item id="2">
Rarity: NORMAL
Rusted Sword
Rusted Sword
Implicits: 0
		</Item>
		<ItemSet id="1">
			<Slot name="Body Armour" itemId="1"/>
			<Slot name="Weapon 1" itemId="2"/>
		</ItemSet>
	</Items>
	<Config/>
</PathOfBuilding>
"#
    )
}

#[test]
fn group_property_of_an_equipped_item_calculates() {
    // CalcSetup.lua:1874: `GroupProperty` values go into the modDB the loop lists them from
    let r = run(&build_with_body("R-R", &["You have Vaal Pact while all socketed gems are red"]));
    let r = r.borrow();
    let keystones = r.env.mod_db.borrow().list(None, &["Keystone"]).expect("list");
    assert!(
        keystones.iter().any(|k| k.is_str("Vaal Pact")),
        "Vaal Pact reached the modDB: {keystones:?}"
    );
}

#[test]
fn resistance_conversion_with_increased_resistance_calculates() {
    // CalcDefence.lua:566-573: INC / MORE of the converted resistance go into the modDB the loop tabulates them from
    let r = run(&build_with_body(
        "R-R",
        &[
            "Modifiers to Fire Resistance also apply to Cold and Lightning Resistances at 50% of their value",
            "+40% to Fire Resistance",
            "20% increased Fire Resistance",
        ],
    ));
    let r = r.borrow();
    assert!(r.state.player.output.opt_num("ColdResist").is_some());
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The cells `X` of each `X.borrow()` / `X.borrow_mut()` in `s` (`X` = a path like `self.list` or `db`).
fn borrowed_cells(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    for pat in [".borrow()", ".borrow_mut()"] {
        let mut from = 0;
        while let Some(k) = s[from..].find(pat) {
            let end = from + k;
            let start = s[..end].rfind(|c: char| !(is_ident(c) || c == '.')).map_or(0, |i| i + 1);
            let cell = s[start..end].trim_matches('.');
            if !cell.is_empty() {
                out.push(cell.to_string());
            }
            from = end + pat.len();
        }
    }
    out
}

/// `name` as a whole argument of a call in `body`: `(name,` `(name)` `, name,` `, &name)` ...
fn passed_as_argument(body: &str, name: &str) -> bool {
    let b: Vec<char> = body.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let mut i = 0;
    while i + n.len() <= b.len() {
        if b[i..i + n.len()] == n[..]
            && (i == 0 || !is_ident(b[i - 1]) && b[i - 1] != '.')
            && b.get(i + n.len()).is_none_or(|c| !is_ident(*c))
        {
            let before = b[..i].iter().rev().find(|c| !c.is_whitespace() && **c != '&');
            let after = b[i + n.len()..].iter().find(|c| !c.is_whitespace());
            if matches!(before, Some('(' | ',')) && matches!(after, Some(',' | ')')) {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// `for <pat> in <header with X.borrow()> { <body> }` where the body hands `X` to a call or borrows it mutably.
fn loops_holding_a_borrow(src: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        let Some(rest) = t.strip_prefix("for ").and_then(|r| r.split_once(" in ").map(|x| x.1)) else {
            continue;
        };
        // the header up to the body's `{` (a few lines at most)
        let mut head = rest.to_string();
        let mut j = i;
        while !head.trim_end().ends_with('{') && j + 1 < lines.len() && j < i + 8 {
            j += 1;
            head.push(' ');
            head.push_str(lines[j]);
        }
        let cells = borrowed_cells(&head);
        if cells.is_empty() {
            continue;
        }
        // the body by brace depth
        let mut depth = 0i32;
        let mut body = String::new();
        'scan: for l in &lines[j..] {
            for ch in l.chars() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            break 'scan;
                        }
                    }
                    _ => {}
                }
            }
            body.push_str(l);
            body.push('\n');
        }
        for cell in cells {
            let last = cell.rsplit('.').next().unwrap_or(&cell).to_string();
            let mutably = body.contains(&format!("{cell}.borrow_mut()"));
            if mutably || passed_as_argument(&body, &last) {
                out.push((
                    i + 1,
                    format!("{} ({cell} {})", t, if mutably { "borrowed mutably" } else { "passed on" }),
                ));
            }
        }
    }
    out
}

#[test]
fn no_loop_holds_a_borrow_its_body_needs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut found = Vec::new();
    let mut files = 0;
    let mut stack = vec![root.clone()];
    while let Some(p) = stack.pop() {
        for e in std::fs::read_dir(&p).expect("read_dir").flatten() {
            let path = e.path();
            if path.is_dir() {
                let name = e.file_name();
                if name != "target" && name != "tests" && !name.to_string_lossy().starts_with('.') {
                    stack.push(path);
                }
            } else if path.extension().is_some_and(|x| x == "rs") && path.components().any(|c| c.as_os_str() == "src") {
                files += 1;
                let src = std::fs::read_to_string(&path).expect("read");
                for (line, what) in loops_holding_a_borrow(&src) {
                    found.push(format!("{}:{line}: {what}", path.strip_prefix(&root).unwrap_or(&path).display()));
                }
            }
        }
    }
    assert!(files > 200, "scanned {files} files");
    assert!(
        found.is_empty(),
        "take the list out of the loop header first:\n{}",
        found.join("\n")
    );
}
