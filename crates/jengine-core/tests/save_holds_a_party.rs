//! **Can the save system hold more than a number?**
//!
//! It is documented to take a nested table, it is wired in all four runtimes,
//! it has an Engine Test Lab scene - and it has no Rust test at all, and every
//! save in the arcade is one scalar: a high score, a best lap, a depth. That
//! is exactly the standing renderer materials had before a game drove them and
//! found the whole subsystem could not depend on anything a game knows.
//!
//! A party RPG is the first thing here that would keep real structure across a
//! scene swap - six characters with stats, inventory and position - so this
//! asks the question before a game is built on the answer.

use jengine_core::save::SaveSystem;
use serde_json::json;

fn party() -> serde_json::Value {
    json!({
        "gold": 412,
        "day": 7,
        "members": [
            { "name": "Ilse",  "class": "fighter", "hp": [18, 22],
              "inv": ["long sword", "chain", "3 rations"], "at": [4, 11] },
            { "name": "Bram",  "class": "cleric",  "hp": [11, 14],
              "inv": ["mace", "shield"], "spells": { "cure": 2, "bless": 1 }, "at": [4, 12] },
            { "name": "Odile", "class": "mage",    "hp": [6, 6],
              "inv": ["dagger"], "spells": { "sleep": 1, "magic missile": 2 }, "at": [5, 11] }
        ],
        "flags": { "gate_opened": true, "bribed_guard": false }
    })
}

#[test]
fn a_nested_structure_survives_a_commit_and_a_reload() {
    let dir = std::env::temp_dir().join(format!("jengine_save_party_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let mut save = SaveSystem::new();
    save.set_base_dir(dir.clone());
    save.set("adventure", "party", party());
    save.commit("adventure").expect("commit writes the slot");

    // A *fresh* system, as a relaunched game would have.
    let mut reloaded = SaveSystem::new();
    reloaded.set_base_dir(dir.clone());
    let got = reloaded.get("adventure", "party").expect("the party is still there");

    assert_eq!(got, party(), "the party did not survive the round trip");

    // And the parts a game would actually read back, named so a failure says
    // which shape was lost rather than dumping the whole tree.
    assert_eq!(got["members"].as_array().map(|a| a.len()), Some(3), "member list");
    assert_eq!(got["members"][1]["spells"]["cure"], json!(2), "a nested map inside an array");
    assert_eq!(got["members"][0]["inv"][2], json!("3 rations"), "an array inside an array");
    assert_eq!(got["flags"]["bribed_guard"], json!(false), "a false boolean, not a missing key");
    assert_eq!(got["members"][2]["hp"][0], json!(6), "a number inside two levels");

    let _ = std::fs::remove_dir_all(&dir);
}

/// `false` and `0` must come back as themselves. A save that turns a `false`
/// into a missing key is the shape that makes a flag look unset forever.
#[test]
fn falsey_values_are_not_lost() {
    let dir = std::env::temp_dir().join(format!("jengine_save_falsey_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let mut save = SaveSystem::new();
    save.set_base_dir(dir.clone());
    save.set("s", "zero", json!(0));
    save.set("s", "no", json!(false));
    save.set("s", "empty_list", json!([]));
    save.set("s", "empty_map", json!({}));
    save.set("s", "empty_str", json!(""));
    save.commit("s").unwrap();

    let mut r = SaveSystem::new();
    r.set_base_dir(dir.clone());
    assert_eq!(r.get("s", "zero"), Some(json!(0)));
    assert_eq!(r.get("s", "no"), Some(json!(false)));
    assert_eq!(r.get("s", "empty_list"), Some(json!([])));
    assert_eq!(r.get("s", "empty_map"), Some(json!({})));
    assert_eq!(r.get("s", "empty_str"), Some(json!("")));

    let _ = std::fs::remove_dir_all(&dir);
}

/// Nothing persists without a commit - the docs say so, and a game that
/// assumed otherwise would lose a party on every quit.
#[test]
fn nothing_persists_without_a_commit() {
    let dir = std::env::temp_dir().join(format!("jengine_save_nocommit_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let mut save = SaveSystem::new();
    save.set_base_dir(dir.clone());
    save.set("s", "party", party());
    assert!(save.is_dirty("s"), "an uncommitted slot is dirty");

    let mut r = SaveSystem::new();
    r.set_base_dir(dir.clone());
    assert_eq!(r.get("s", "party"), None, "it should not be on disk yet");

    let _ = std::fs::remove_dir_all(&dir);
}
