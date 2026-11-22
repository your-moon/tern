use super::*;

fn host(user: Option<&str>, port: u16) -> HostEntry {
    HostEntry {
        alias: "web".into(),
        host_name: "10.0.0.5".into(),
        port,
        user: user.map(Into::into),
        identity_files: Vec::new(),
        proxy_command: None,
        ..Default::default()
    }
}

#[test]
fn address_reads_like_an_ssh_target() {
    assert_eq!(address(&host(Some("deploy"), 22)), "deploy@10.0.0.5");
    assert_eq!(address(&host(None, 2222)), "10.0.0.5:2222");
    assert_eq!(short_address(&host(Some("deploy"), 2222)), "10.0.0.5:2222");
    let mut same = host(None, 22);
    same.host_name = "web".into();
    assert_eq!(short_address(&same), "");
}

fn entry(alias: &str, host_name: &str, port: u16) -> HostEntry {
    HostEntry {
        alias: alias.into(),
        host_name: host_name.into(),
        port,
        ..host(None, port)
    }
}

#[test]
fn meta_is_dropped_to_the_host_and_then_to_nothing_never_cut() {
    let ip = entry("prod", "103.50.205.106", 2022);
    // Wide row: the whole address.
    assert_eq!(
        fit_meta(&ip, 400., 12.).as_deref(),
        Some("103.50.205.106:2022")
    );
    // The address has the full text column on its own line.
    assert_eq!(
        fit_meta(&ip, 256. - 61., 12.).as_deref(),
        Some("103.50.205.106:2022")
    );
    // Too narrow for even the host.
    assert_eq!(fit_meta(&ip, 120., 12.).as_deref(), Some("103.50.205.106"));
    assert_eq!(fit_meta(&ip, 80., 12.), None);
    // Nothing to say when the host only repeats the name.
    assert_eq!(fit_meta(&entry("web", "web", 22), 400., 12.), None);
}

fn conn(name: &str, group: Option<&str>, tags: &[&str]) -> Connection {
    Connection {
        name: name.into(),
        host: "10.0.0.5".into(),
        port: 22,
        user: "deploy".into(),
        identity_file: None,
        group: group.map(Into::into),
        tags: tags.iter().map(|t| (*t).into()).collect(),
        ..Connection::default()
    }
}

#[test]
fn ungrouped_come_first_and_groups_sort_by_name_keeping_host_order() {
    let list = [
        conn("a", Some("Zeta"), &[]),
        conn("b", None, &[]),
        conn("c", Some("alpha"), &[]),
        conn("d", Some("Zeta"), &[]),
    ];
    let l = layout(&list);
    assert_eq!(l.ungrouped, vec![1]);
    assert_eq!(
        l.groups,
        vec![
            ("alpha".to_owned(), vec![2]),
            ("Zeta".to_owned(), vec![0, 3])
        ]
    );
}

#[test]
fn unsaved_groups_use_the_first_global_tab_and_remain_searchable() {
    let tabs = ["web", "temp", "web", "lab", "temp"].map(|alias| TabInfo {
        alias: alias.into(),
        title: None,
        logging: false,
        broadcast: false,
        dot: Dot::Idle,
        rename: None,
    });
    let saved = [host(None, 22)];
    assert_eq!(unsaved_matches("", &saved, &tabs), [1, 3]);
    assert_eq!(unsaved_matches("lab", &saved, &tabs), [3]);
    assert!(unsaved_matches("web", &saved, &tabs).is_empty());
}

#[test]
fn search_matches_tags_and_group_and_ranks_the_name_first() {
    let list = [
        conn("billing", Some("prod"), &["eu", "postgres"]),
        conn("postgres-main", None, &[]),
        conn("web", Some("staging"), &["nginx"]),
    ];
    // A tag finds a host whose name lacks it.
    assert_eq!(search("nginx", &list), vec![2]);
    assert_eq!(search("staging", &list), vec![2]);
    // A name match and a tag-only match are both found; the name match comes first.
    let list2 = [
        conn("db", Some("prod"), &["web"]),
        conn("web-frontend", None, &[]),
    ];
    assert_eq!(search("web", &list2), vec![1, 0]);
    let mut found = search("postgres", &list);
    found.sort_unstable();
    assert_eq!(found, vec![0, 1]);
    assert_eq!(search("zzz", &list), Vec::<usize>::new());
    assert_eq!(search("", &list), vec![0, 1, 2]);
}
