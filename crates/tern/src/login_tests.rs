#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use tern_ssh::secrecy::ExposeSecret;
use tern_ssh::{ChallengePrompt, Prompt, oneshot};

use super::*;

fn answer<T>(mut rx: oneshot::Receiver<T>) -> T {
    rx.try_recv().unwrap().expect("no reply was sent")
}

fn password() -> (Login, oneshot::Receiver<Option<SecretString>>, Vec<u8>) {
    let (tx, rx) = oneshot::channel();
    let (login, shown) = Login::start(Prompt::Password {
        user: "deploy".into(),
        host: "web".into(),
        reply: tx,
    });
    (login, rx, shown)
}

#[test]
fn password_is_asked_like_openssh_and_never_echoed() {
    let (mut login, rx, shown) = password();
    assert_eq!(shown, b"deploy@web's password: ");
    let step = login.input(b"hunter2");
    assert_eq!(
        step,
        Step {
            display: Vec::new(),
            done: false
        }
    );
    let step = login.input(b"\r");
    assert!(step.done);
    assert_eq!(answer(rx).unwrap().expose_secret(), "hunter2");
}

#[test]
fn backspace_edits_the_answer() {
    let (mut login, rx, _) = password();
    login.input(b"hunterX\x7f2\r");
    assert_eq!(answer(rx).unwrap().expose_secret(), "hunter2");
}

#[test]
fn ctrl_c_cancels() {
    let (mut login, rx, _) = password();
    let step = login.input(b"abc\x03");
    assert!(step.done);
    assert_eq!(step.display, b"^C\r\n");
    assert!(answer(rx).is_none());
}

#[test]
fn arrow_keys_do_not_leak_into_the_answer() {
    let (mut login, rx, _) = password();
    login.input(b"pw");
    login.input(b"\x1b[A");
    login.input(b"\r");
    assert_eq!(answer(rx).unwrap().expose_secret(), "pw");
}

#[test]
fn host_key_question_echoes_and_insists_on_yes_or_no() {
    let (tx, rx) = oneshot::channel();
    let (mut login, shown) = Login::start(Prompt::UnknownHostKey {
        host: "web".into(),
        port: 22,
        algorithm: "ssh-ed25519".into(),
        fingerprint_sha256: "SHA256:abc".into(),
        reply: tx,
    });
    let shown = String::from_utf8(shown).unwrap();
    assert!(shown.starts_with("The authenticity of host 'web (web:22)' can't be established."));
    assert!(shown.ends_with("Are you sure you want to continue connecting (yes/no)? "));

    let step = login.input(b"y\r");
    assert!(!step.done);
    assert_eq!(step.display, b"y\r\nPlease type 'yes' or 'no': ");
    assert!(login.input(b"yes\r").done);
    assert!(answer(rx));
}

#[test]
fn passphrase_names_the_key() {
    let (tx, _rx) = oneshot::channel();
    let (_, shown) = Login::start(Prompt::KeyPassphrase {
        path: PathBuf::from("/k/id_ed25519"),
        reply: tx,
    });
    assert_eq!(shown, b"Enter passphrase for key '/k/id_ed25519': ");
}

#[test]
fn challenge_asks_each_prompt_in_turn() {
    let (tx, rx) = oneshot::channel();
    let prompt = |text: &str, echo| ChallengePrompt {
        text: text.into(),
        echo,
    };
    let (mut login, shown) = Login::start(Prompt::Challenge {
        name: String::new(),
        instructions: "Two steps".into(),
        prompts: vec![prompt("User: ", true), prompt("Code: ", false)],
        reply: tx,
    });
    assert_eq!(shown, b"Two steps\r\nUser: ");
    let step = login.input(b"me\r");
    assert_eq!(step.display, b"me\r\nCode: ");
    let step = login.input(b"123\r");
    assert_eq!(step.display, b"\r\n");
    assert!(step.done);
    let answers = answer(rx).unwrap();
    let answers: Vec<&str> = answers.iter().map(|a| a.expose_secret()).collect();
    assert_eq!(answers, ["me", "123"]);
}

#[test]
fn a_typed_password_is_kept_for_the_offer_to_save() {
    let (mut login, rx, _) = password();
    assert!(login.input(b"hunter2\r").done);
    assert_eq!(login.take_answer().unwrap().expose_secret(), "hunter2");
    assert_eq!(answer(rx).unwrap().expose_secret(), "hunter2");
}

#[test]
fn a_local_question_returns_its_line_and_ctrl_c_returns_nothing() {
    let (mut login, shown) = Login::ask("Vault passphrase: ", false);
    assert_eq!(shown, b"Vault passphrase: ");
    let step = login.input(b"s3cret\r");
    assert!(step.done);
    assert_eq!(step.display, b"\r\n");
    assert_eq!(login.take_answer().unwrap().expose_secret(), "s3cret");

    let (mut login, _) = Login::ask("Vault passphrase: ", false);
    assert!(login.input(b"abc\x03").done);
    assert!(login.take_answer().is_none());
}
