#![allow(clippy::unwrap_used, clippy::disallowed_methods)]
use super::*;

#[test]
fn encode_decode_round_trips_and_drops_unknown_names() {
    let mut files = Files::new();
    files.insert("hosts.json".into(), b"{\"v\":1}".to_vec());
    files.insert("settings.json".into(), vec![0, 255, 7]);
    assert_eq!(decode(&encode(&files).unwrap()).unwrap(), files);

    let evil = br#"{"../../.ssh/authorized_keys":"aGk=","hosts.json":"e30="}"#;
    let out = decode(evil).unwrap();
    assert_eq!(out.keys().collect::<Vec<_>>(), vec!["hosts.json"]);
}

fn hosts(list: &[(&str, &str)]) -> Vec<u8> {
    let items: Vec<_> = list
        .iter()
        .map(|(n, h)| serde_json::json!({"name": n, "host": h, "port": 22, "user": "root"}))
        .collect();
    serde_json::to_vec_pretty(&serde_json::json!({"version": 1, "connections": items})).unwrap()
}

fn bundle(pairs: &[(&str, &[u8])]) -> Files {
    pairs
        .iter()
        .map(|(n, b)| ((*n).to_owned(), b.to_vec()))
        .collect()
}

#[test]
fn item_decisions_take_the_side_that_changed_and_ask_only_when_both_did() {
    let base = bundle(&[
        (HOSTS_FILE, &hosts(&[("web", "1.1.1.1")])),
        ("settings.json", b"{\"a\":1}"),
        ("keymap.json", b"[]"),
        (VAULT_FILE, b"vault-1"),
    ]);
    let state = SyncState {
        last_hash: Some("x".into()),
        items: tern_sync::item_hashes(&base, &ITEMS),
        hosts_base: Some(String::from_utf8(hosts(&[("web", "1.1.1.1")])).unwrap()),
    };
    let mut local = base.clone();
    let mut remote = base.clone();
    // Mac edits the keymap; the remote edits the vault; both edit the settings.
    local.insert("keymap.json".into(), b"[1]".to_vec());
    remote.insert(VAULT_FILE.into(), b"vault-2".to_vec());
    local.insert("settings.json".into(), b"{\"a\":2}".to_vec());
    remote.insert("settings.json".into(), b"{\"a\":3}".to_vec());

    let asks = decide_items(&state, &local, &remote, &Resolutions::new())
        .unwrap()
        .unwrap_err();
    assert_eq!(asks, ["settings.json"], "only the item changed on both");

    let resolve = Resolutions::from([("settings.json".to_owned(), Side::Remote)]);
    let picks = decide_items(&state, &local, &remote, &resolve)
        .unwrap()
        .unwrap();
    assert_eq!(picks["keymap.json"], Pick::Mac);
    assert_eq!(picks[VAULT_FILE], Pick::Remote);
    assert_eq!(picks["settings.json"], Pick::Remote);
    assert_eq!(picks[HOSTS_FILE], Pick::Mac, "untouched");

    let resolve = Resolutions::from([("settings.json".to_owned(), Side::Mac)]);
    let picks = decide_items(&state, &local, &remote, &resolve)
        .unwrap()
        .unwrap();
    assert_eq!(picks["settings.json"], Pick::Mac);
}

#[test]
fn hosts_changed_on_both_sides_merge_without_asking() {
    let base = bundle(&[(HOSTS_FILE, &hosts(&[("web", "1.1.1.1")]))]);
    let state = SyncState {
        last_hash: Some("x".into()),
        items: tern_sync::item_hashes(&base, &ITEMS),
        hosts_base: Some(String::from_utf8(hosts(&[("web", "1.1.1.1")])).unwrap()),
    };
    let local = bundle(&[(HOSTS_FILE, &hosts(&[("web", "1.1.1.1"), ("db", "2.2.2.2")]))]);
    let remote = bundle(&[(
        HOSTS_FILE,
        &hosts(&[("web", "1.1.1.1"), ("cache", "3.3.3.3")]),
    )]);
    let picks = decide_items(&state, &local, &remote, &Resolutions::new())
        .unwrap()
        .unwrap();
    let Pick::Merged(bytes) = &picks[HOSTS_FILE] else {
        unreachable!("{:?}", picks[HOSTS_FILE]);
    };
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.contains("db") && text.contains("cache") && text.contains("web"));

    // The same connection edited differently on both sides is the one real conflict.
    let local = bundle(&[(HOSTS_FILE, &hosts(&[("web", "10.0.0.1")]))]);
    let remote = bundle(&[(HOSTS_FILE, &hosts(&[("web", "11.0.0.1")]))]);
    let asks = decide_items(&state, &local, &remote, &Resolutions::new())
        .unwrap()
        .unwrap_err();
    assert_eq!(asks, [HOSTS_FILE]);
}

fn bare_repo(root: &Path) -> String {
    let bare = root.join("remote.git");
    let ok = std::process::Command::new("git")
        .args(["init", "--quiet", "--bare", "-b", "main"])
        .arg(&bare)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    bare.to_string_lossy().into_owned()
}

fn run_sync(dir: &Path, url: &str, vault: &Vault, resolve: &Resolutions) -> Outcome {
    sync(dir, Some(url), vault, None, None, resolve).unwrap().0
}

#[test]
fn two_macs_adding_different_hosts_converge_through_a_git_remote() {
    let root = tempfile::tempdir().unwrap();
    let url = bare_repo(root.path());
    let (a, b) = (root.path().join("a"), root.path().join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let pass = || SecretString::from("pw".to_owned());
    let (va, vb) = (Vault::new(pass()), Vault::new(pass()));
    let none = Resolutions::new();

    std::fs::write(a.join(HOSTS_FILE), hosts(&[("web", "1.1.1.1")])).unwrap();
    assert!(matches!(run_sync(&a, &url, &va, &none), Outcome::Pushed));
    // B has nothing of its own, so it simply takes A's copy (and A's vault).
    let vb_pulled = match sync(&b, Some(&url), &vb, Some("pw"), None, &none).unwrap() {
        (Outcome::Pulled, Some(v)) => v,
        (o, _) => unreachable!("{}", outcome_name(&o)),
    };

    std::fs::write(
        a.join(HOSTS_FILE),
        hosts(&[("web", "1.1.1.1"), ("db", "2.2.2.2")]),
    )
    .unwrap();
    std::fs::write(
        b.join(HOSTS_FILE),
        hosts(&[("web", "1.1.1.1"), ("cache", "3.3.3.3")]),
    )
    .unwrap();
    assert!(matches!(run_sync(&a, &url, &va, &none), Outcome::Pushed));
    // Both changed: B merges instead of asking, and uploads the union.
    assert!(matches!(
        run_sync(&b, &url, &vb_pulled, &none),
        Outcome::Merged
    ));
    let on_b = std::fs::read_to_string(b.join(HOSTS_FILE)).unwrap();
    assert!(on_b.contains("db") && on_b.contains("cache") && on_b.contains("web"));
    assert!(matches!(run_sync(&a, &url, &va, &none), Outcome::Pulled));
    let on_a = std::fs::read_to_string(a.join(HOSTS_FILE)).unwrap();
    assert!(on_a.contains("db") && on_a.contains("cache"));
    assert!(matches!(
        run_sync(&b, &url, &vb_pulled, &none),
        Outcome::UpToDate
    ));
}

#[test]
fn a_record_without_item_hashes_asks_the_whole_bundle_question_once() {
    let root = tempfile::tempdir().unwrap();
    let url = bare_repo(root.path());
    let (a, b) = (root.path().join("a"), root.path().join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let none = Resolutions::new();
    let (va, vb) = (
        Vault::new(SecretString::from("pw".to_owned())),
        Vault::new(SecretString::from("pw".to_owned())),
    );
    std::fs::write(a.join(HOSTS_FILE), hosts(&[("web", "1.1.1.1")])).unwrap();
    run_sync(&a, &url, &va, &none);
    // B holds something of its own and has the old single-hash record.
    std::fs::write(b.join(HOSTS_FILE), hosts(&[("other", "9.9.9.9")])).unwrap();
    std::fs::write(b.join(STATE_FILE), br#"{"last_hash":"stale"}"#).unwrap();
    let (out, _) = sync(&b, Some(&url), &vb, None, None, &none).unwrap();
    assert!(matches!(out, Outcome::Conflict { whole: true, .. }));
    // Keep this Mac: pushed, and the record now has per-item hashes.
    let (out, _) = sync(&b, Some(&url), &vb, None, Some(Plan::Push), &none).unwrap();
    assert!(matches!(out, Outcome::Pushed));
    assert!(!load_state(&b).items.is_empty());
}

#[test]
fn the_same_host_edited_on_both_sides_is_asked_and_the_answer_applies() {
    let root = tempfile::tempdir().unwrap();
    let url = bare_repo(root.path());
    let (a, b) = (root.path().join("a"), root.path().join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let none = Resolutions::new();
    let va = Vault::new(SecretString::from("pw".to_owned()));
    std::fs::write(a.join(HOSTS_FILE), hosts(&[("web", "1.1.1.1")])).unwrap();
    run_sync(&a, &url, &va, &none);
    let vb = match sync(&b, Some(&url), &va, Some("pw"), None, &none).unwrap() {
        (Outcome::Pulled, Some(v)) => v,
        (o, _) => unreachable!("{}", outcome_name(&o)),
    };
    std::fs::write(a.join(HOSTS_FILE), hosts(&[("web", "10.0.0.1")])).unwrap();
    std::fs::write(b.join(HOSTS_FILE), hosts(&[("web", "11.0.0.1")])).unwrap();
    run_sync(&a, &url, &va, &none);
    match run_sync(&b, &url, &vb, &none) {
        Outcome::Conflict {
            whole: false,
            items,
        } => assert_eq!(items, [HOSTS_FILE]),
        o => unreachable!("{}", outcome_name(&o)),
    }
    assert!(
        std::fs::read_to_string(b.join(HOSTS_FILE))
            .unwrap()
            .contains("11.0.0.1"),
        "asking changes nothing"
    );
    let choose = Resolutions::from([(HOSTS_FILE.to_owned(), Side::Remote)]);
    assert!(matches!(run_sync(&b, &url, &vb, &choose), Outcome::Merged));
    assert!(
        std::fs::read_to_string(b.join(HOSTS_FILE))
            .unwrap()
            .contains("10.0.0.1")
    );
}

fn outcome_name(o: &Outcome) -> &'static str {
    match o {
        Outcome::UpToDate => "UpToDate",
        Outcome::Pushed => "Pushed",
        Outcome::Pulled => "Pulled",
        Outcome::Merged => "Merged",
        Outcome::Conflict { .. } => "Conflict",
    }
}
