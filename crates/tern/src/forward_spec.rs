//! Port forwards as people type them: `-L 8080:db:5432`, `-R 9000:localhost:3000`,
//! `-D 1080`. tern-ssh parses the `ssh_config` spelling (`8080 db:5432`, space separated); this
//! turns the command-line spelling into that one, so both are accepted and `Forward`'s own
//! `Display` output (`-L 127.0.0.1:8080:db:5432`) reads back.

use tern_ssh::Forward;

/// Parses one forward spec.
///
/// # Errors
/// A message naming the spec, worded for the form.
pub fn parse(text: &str) -> Result<Forward, String> {
    let text = text.trim();
    let bad = |why: &str| format!("Port forward \"{text}\": {why}");
    let (flag, rest) = match text.as_bytes() {
        [b'-', f @ (b'L' | b'R' | b'D'), ..] => (*f, text[2..].trim()),
        _ => return Err(bad("start with -L, -R or -D.")),
    };
    if rest.is_empty() {
        return Err(bad("nothing after the flag."));
    }
    let parsed = if flag == b'D' {
        Forward::parse_dynamic(rest)
    } else {
        let args = if rest.contains(char::is_whitespace) {
            rest.to_owned()
        } else {
            colon_to_config(rest).ok_or_else(|| bad("expected [bind:]port:host:hostport."))?
        };
        if flag == b'L' {
            Forward::parse_local(&args)
        } else {
            Forward::parse_remote(&args)
        }
    };
    parsed.map_err(|_| bad("check the ports (1-65535) and the host."))
}

/// `[bind:]port:host:hostport` → `[bind:]port host:hostport`, keeping `[v6]` brackets whole.
fn colon_to_config(spec: &str) -> Option<String> {
    let mut fields = Vec::new();
    let (mut depth, mut start) = (0usize, 0usize);
    for (i, c) in spec.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => {
                fields.push(&spec[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    fields.push(&spec[start..]);
    match fields.as_slice() {
        [port, host, hostport] => Some(format!("{port} {host}:{hostport}")),
        [bind, port, host, hostport] => Some(format!("{bind}:{port} {host}:{hostport}")),
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn local(bind: &str, bp: u16, dest: &str, dp: u16) -> Forward {
        Forward::Local {
            bind_host: bind.into(),
            bind_port: bp,
            dest_host: dest.into(),
            dest_port: dp,
        }
    }

    #[test]
    fn command_line_spellings_read_into_the_right_kind_and_ports() {
        assert_eq!(
            parse("-L 8080:db.internal:5432").unwrap(),
            local("127.0.0.1", 8080, "db.internal", 5432)
        );
        assert_eq!(
            parse("-L 0.0.0.0:8080:db:5432").unwrap(),
            local("0.0.0.0", 8080, "db", 5432)
        );
        assert_eq!(
            parse("-R 9000:localhost:3000").unwrap(),
            Forward::Remote {
                bind_host: "localhost".into(),
                bind_port: 9000,
                dest_host: "localhost".into(),
                dest_port: 3000
            }
        );
        assert_eq!(
            parse("-D 1080").unwrap(),
            Forward::Dynamic {
                bind_host: "127.0.0.1".into(),
                bind_port: 1080
            }
        );
        // The ssh_config spelling is accepted too.
        assert_eq!(
            parse("-L 8080 db:5432").unwrap(),
            local("127.0.0.1", 8080, "db", 5432)
        );
    }

    #[test]
    fn what_display_prints_reads_back_for_all_three_kinds() {
        for text in [
            "-L 8080:db:5432",
            "-L 127.0.0.1:8080:[::1]:5432",
            "-R 9000:10.0.0.7:3000",
            "-R 0.0.0.0:9000:localhost:3000",
            "-D 1080",
            "-D 0.0.0.0:1081",
        ] {
            let f = parse(text).unwrap();
            assert_eq!(parse(&f.to_string()).unwrap(), f, "{text}");
        }
    }

    #[test]
    fn mistakes_are_refused_with_the_spec_in_the_message() {
        for text in [
            "8080:db:5432",
            "-X 8080:db:5432",
            "-L",
            "-L 8080:db",
            "-L 8080:db:99999",
            "-L x:db:5432",
            "-D",
            "-D abc",
        ] {
            let e = parse(text).unwrap_err();
            assert!(e.starts_with("Port forward"), "{text}: {e}");
        }
    }
}
