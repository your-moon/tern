//! Login questions asked inside the terminal, worded as OpenSSH asks them, so answering a
//! password or host-key prompt in tern feels the same as in `ssh`. Pure: bytes in, bytes out.

use std::collections::VecDeque;

use tern_ssh::SecretString;
use tern_ssh::oneshot::Sender;
use tern_ssh::{ChallengePrompt, Prompt};

/// What the terminal should show after a keystroke, and whether the question is answered.
#[derive(Debug, PartialEq, Eq)]
pub struct Step {
    pub display: Vec<u8>,
    pub done: bool,
}

/// One login question in progress. Keystrokes go here instead of to the remote.
#[derive(Debug)]
pub struct Login {
    kind: Kind,
    line: String,
    echo: bool,
    /// The submitted secret or local answer, kept for the caller (offer to save, vault unlock).
    answer: Option<SecretString>,
}

#[derive(Debug)]
enum Kind {
    Secret(Option<Sender<Option<SecretString>>>),
    YesNo(Option<Sender<bool>>),
    /// A question tern asks for itself; the answer stays here for [`Login::take_answer`].
    Local,
    Challenge {
        pending: VecDeque<ChallengePrompt>,
        answers: Vec<SecretString>,
        reply: Option<Sender<Option<Vec<SecretString>>>>,
    },
}

impl Login {
    /// Starts a question; returns it with the text to show.
    pub fn start(prompt: Prompt) -> (Self, Vec<u8>) {
        match prompt {
            Prompt::Password { user, host, reply } => (
                Self::new(Kind::Secret(Some(reply)), false),
                format!("{user}@{host}'s password: ").into_bytes(),
            ),
            Prompt::KeyPassphrase { path, reply } => (
                Self::new(Kind::Secret(Some(reply)), false),
                format!("Enter passphrase for key '{}': ", path.display()).into_bytes(),
            ),
            Prompt::UnknownHostKey {
                host,
                port,
                algorithm,
                fingerprint_sha256,
                reply,
            } => (
                Self::new(Kind::YesNo(Some(reply)), true),
                format!(
                    "The authenticity of host '{host} ({host}:{port})' can't be established.\r\n\
                     {algorithm} key fingerprint is {fingerprint_sha256}.\r\n\
                     Are you sure you want to continue connecting (yes/no)? "
                )
                .into_bytes(),
            ),
            Prompt::Challenge {
                name,
                instructions,
                prompts,
                reply,
            } => {
                let pending: VecDeque<_> = prompts.into();
                let mut display = Vec::new();
                for line in [name, instructions].iter().filter(|l| !l.is_empty()) {
                    display.extend_from_slice(line.replace('\n', "\r\n").as_bytes());
                    display.extend_from_slice(b"\r\n");
                }
                let echo = pending.front().is_some_and(|p| p.echo);
                if let Some(first) = pending.front() {
                    display.extend_from_slice(first.text.as_bytes());
                }
                let kind = Kind::Challenge {
                    pending,
                    answers: Vec::new(),
                    reply: Some(reply),
                };
                (Self::new(kind, echo), display)
            }
        }
    }

    /// A question tern asks for itself, such as the vault passphrase.
    pub fn ask(text: &str, echo: bool) -> (Self, Vec<u8>) {
        (Self::new(Kind::Local, echo), text.as_bytes().to_vec())
    }

    /// The answer once the question is done: the line typed for a password, passphrase or
    /// local question; `None` after Ctrl-C or for other kinds.
    pub fn take_answer(&mut self) -> Option<SecretString> {
        self.answer.take()
    }

    fn new(kind: Kind, echo: bool) -> Self {
        Self {
            kind,
            line: String::new(),
            echo,
            answer: None,
        }
    }

    /// Feeds keystrokes. Enter submits, Backspace edits, Ctrl-C cancels; escape sequences
    /// such as arrow keys are ignored.
    pub fn input(&mut self, bytes: &[u8]) -> Step {
        let mut display = Vec::new();
        let text = String::from_utf8_lossy(bytes);
        for ch in text.chars() {
            match ch {
                '\r' | '\n' => {
                    display.extend_from_slice(b"\r\n");
                    if let Some(next) = self.submit() {
                        display.extend_from_slice(&next);
                    } else {
                        return Step {
                            display,
                            done: true,
                        };
                    }
                }
                '\u{3}' => {
                    display.extend_from_slice(b"^C\r\n");
                    self.cancel();
                    return Step {
                        display,
                        done: true,
                    };
                }
                '\u{7f}' | '\u{8}' => {
                    if self.line.pop().is_some() && self.echo {
                        display.extend_from_slice(b"\x08 \x08");
                    }
                }
                '\u{1b}' => break,
                c if !c.is_control() => {
                    self.line.push(c);
                    if self.echo {
                        let mut buf = [0; 4];
                        display.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                    }
                }
                _ => {}
            }
        }
        Step {
            display,
            done: false,
        }
    }

    /// Ends the line. Returns the next question's text, or `None` when this login is answered.
    fn submit(&mut self) -> Option<Vec<u8>> {
        let line = std::mem::take(&mut self.line);
        match &mut self.kind {
            Kind::Secret(reply) => {
                self.answer = Some(SecretString::from(line.clone()));
                if let Some(r) = reply.take() {
                    let _ = r.send(Some(SecretString::from(line)));
                }
                None
            }
            Kind::Local => {
                self.answer = Some(SecretString::from(line));
                None
            }
            Kind::YesNo(reply) => match line.trim().to_ascii_lowercase().as_str() {
                answer @ ("yes" | "no") => {
                    if let Some(r) = reply.take() {
                        let _ = r.send(answer == "yes");
                    }
                    None
                }
                _ => Some(b"Please type 'yes' or 'no': ".to_vec()),
            },
            Kind::Challenge {
                pending,
                answers,
                reply,
            } => {
                pending.pop_front();
                answers.push(SecretString::from(line));
                if let Some(next) = pending.front() {
                    self.echo = next.echo;
                    return Some(next.text.as_bytes().to_vec());
                }
                if let Some(r) = reply.take() {
                    let _ = r.send(Some(std::mem::take(answers)));
                }
                None
            }
        }
    }

    fn cancel(&mut self) {
        match &mut self.kind {
            Kind::Secret(reply) => {
                if let Some(r) = reply.take() {
                    let _ = r.send(None);
                }
            }
            Kind::YesNo(reply) => {
                if let Some(r) = reply.take() {
                    let _ = r.send(false);
                }
            }
            Kind::Local => {}
            Kind::Challenge { reply, .. } => {
                if let Some(r) = reply.take() {
                    let _ = r.send(None);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "login_tests.rs"]
mod tests;
