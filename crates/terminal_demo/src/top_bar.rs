use gpui::{
    Action, Context, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Window,
    actions, div, px,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    menu::DropdownMenu as _,
};
use serde::Deserialize;

use crate::panels::PANEL_KINDS;

actions!(terminal_demo, [ResetLayout]);

#[derive(Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = terminal_demo, no_json)]
pub struct AddPanel(pub SharedString);

pub struct TopBar {
    title: SharedString,
}

impl TopBar {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
        }
    }
}

impl Render for TopBar {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = _cx.theme();

        let add_menu = Button::new("add-panel")
            .label("+ Panel")
            .small()
            .ghost()
            .dropdown_menu(|menu, _, _| {
                let mut menu = menu;
                for kind in PANEL_KINDS {
                    menu = menu.menu(
                        kind.display(),
                        Box::new(AddPanel(SharedString::from(kind.id()))),
                    );
                }
                menu
            });

        let overflow_menu = Button::new("overflow")
            .label("⋯")
            .small()
            .ghost()
            .dropdown_menu(|menu, _, _| menu.menu("Reset Layout", Box::new(ResetLayout)));

        div()
            .h(px(36.))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .px_3()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.tab_bar)
            .child(
                div()
                    .text_sm()
                    .text_color(theme.foreground)
                    .child(self.title.clone()),
            )
            .child(div().flex_1())
            .child(add_menu)
            .child(overflow_menu)
    }
}
