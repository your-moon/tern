//! The password-command side of logging in: when a server asks for a password, run the
//! connection's command before the vault, and ask first if this Mac never approved it.

use gpui::{AppContext, Context};
use tern_ssh::Prompt;

use super::Session;
use super::vault::Flow;
use crate::keeper;
use crate::password_command::{self as pc, Approved, Step};

impl Session {
    /// Runs the connection's password command for `prompt` when it has a turn. Returns the
    /// prompt back when the command does not apply (no command, not a password prompt, already
    /// tried); `None` when it is being run or asked about now.
    pub(super) fn try_command(&mut self, prompt: Prompt, cx: &mut Context<Self>) -> Option<Prompt> {
        if !matches!(prompt, Prompt::Password { .. }) {
            return Some(prompt);
        }
        let command = self.link.password_command.clone();
        let approved = command.as_deref().is_some_and(|c| {
            crate::settings::dir().is_some_and(|d| Approved::load(&d).contains(c))
        });
        match pc::plan(command.as_deref(), self.command_tried, approved) {
            Step::Skip => Some(prompt),
            Step::Approve => {
                let text = format!(
                    "This connection runs `{}` to get its password. Run it? (yes/no) ",
                    command.unwrap_or_default().replace(char::is_control, " ")
                );
                self.ask_local(&text, true, Flow::ApproveCommand(prompt), cx);
                None
            }
            Step::Run => {
                self.run_command(command.unwrap_or_default(), prompt, cx);
                None
            }
        }
    }

    /// The answer to the approval question.
    pub(super) fn command_approved(&mut self, prompt: Prompt, yes: bool, cx: &mut Context<Self>) {
        let Some(command) = self.link.password_command.clone().filter(|_| yes) else {
            // Declined: the command is not run, and the prompt goes the usual way.
            self.command_tried = true;
            return self.on_prompt(prompt, cx);
        };
        if let Some(dir) = crate::settings::dir() {
            let _ = Approved::approve(&dir, &command);
        }
        self.run_command(command, prompt, cx);
    }

    fn run_command(&mut self, command: String, prompt: Prompt, cx: &mut Context<Self>) {
        self.command_tried = true;
        let work = cx.background_spawn(async move { pc::run(&command, pc::TIMEOUT) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |s, cx| match result {
                Ok(secret) => {
                    s.show(b"\x1b[2m[password from the command]\x1b[0m\r\n", cx);
                    // Answered straight to the server, never through `typed`, so it is not
                    // offered for saving in the vault.
                    if let Some(other) = keeper::answer(prompt, secret) {
                        s.on_prompt(other, cx);
                    }
                }
                Err(failure) => {
                    s.show(
                        format!("\x1b[2m{}\x1b[0m\r\n", failure.line()).as_bytes(),
                        cx,
                    );
                    s.on_prompt(prompt, cx);
                }
            });
        })
        .detach();
    }
}
