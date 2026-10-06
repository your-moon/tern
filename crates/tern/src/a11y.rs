//! Names for controls that have no text of their own. gpui 0.3.8 integrates AccessKit, so an
//! element with an id, a role and an `aria_label` is announced by VoiceOver; the same label
//! is shown as a tooltip for sighted pointer users.

use gpui::{
    AnyView, App, AppContext as _, Context, IntoElement, ParentElement, Render, Role, SharedString,
    StatefulInteractiveElement, Styled, Toggled, Window, div, px,
};

use crate::theme::Theme;

struct Tip {
    text: SharedString,
    theme: Theme,
}

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        div()
            .mt(px(6.))
            .px(px(8.))
            .py(px(4.))
            .rounded(px(6.))
            .bg(t.popup)
            .border_1()
            .border_color(t.border)
            .text_size(px(11.))
            .text_color(t.text)
            .child(self.text.clone())
    }
}

/// Accessible names for clickable elements.
pub trait Accessible: StatefulInteractiveElement + Sized {
    /// A button with no visible text: announced as `label`, and shown as a tooltip on hover.
    fn icon_button(self, label: impl Into<SharedString>, t: &Theme) -> Self {
        let label = label.into();
        let theme = *t;
        let tip = label.clone();
        self.role(Role::Button)
            .aria_label(label)
            .tooltip(move |_, cx: &mut App| -> AnyView {
                let (text, theme) = (tip.clone(), theme);
                cx.new(|_| Tip { text, theme }).into()
            })
    }

    /// Hover text on something that already has visible text (a row's full address).
    fn hint(self, text: impl Into<SharedString>, t: &Theme) -> Self {
        let text = text.into();
        let theme = *t;
        self.tooltip(move |_, cx: &mut App| -> AnyView {
            let (text, theme) = (text.clone(), theme);
            cx.new(|_| Tip { text, theme }).into()
        })
    }

    /// A switch that is on or off, announced as `label`.
    fn switch(self, label: impl Into<SharedString>, on: bool) -> Self {
        self.role(Role::Switch)
            .aria_label(label)
            .aria_toggled(if on { Toggled::True } else { Toggled::False })
    }
}

impl<T: StatefulInteractiveElement + Sized> Accessible for T {}
