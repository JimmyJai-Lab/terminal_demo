use std::sync::Arc;
use std::time::Duration;

use gpui::{
    App, AppContext as _, Axis, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Styled as _, Task, Window, div,
};
use gpui_component::{
    ActiveTheme as _, Root,
    dock::{DockArea, DockAreaState, DockEvent, DockItem, DockPlacement, PanelView},
};

use crate::panels::{self, Kind, LastFocusedTabPanel, PANEL_KINDS};
use crate::persistence;
use crate::top_bar::{AddPanel, ResetLayout, TopBar};

const LAYOUT_VERSION: usize = 1;
const DOCK_AREA_ID: &str = "main-dock";

pub struct TerminalWorkspace {
    top_bar: Entity<TopBar>,
    dock_area: Entity<DockArea>,
    last_saved: Option<DockAreaState>,
    _save_task: Option<Task<()>>,
}

impl TerminalWorkspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let dock_area =
            cx.new(|cx| DockArea::new(DOCK_AREA_ID, Some(LAYOUT_VERSION), window, cx));
        let weak_dock = dock_area.downgrade();

        if !try_load(&dock_area, window, cx) {
            apply_default_layout(weak_dock.clone(), window, cx);
        }

        cx.subscribe_in(
            &dock_area,
            window,
            |this, dock_area, ev: &DockEvent, window, cx| {
                if matches!(ev, DockEvent::LayoutChanged) {
                    this.schedule_save(dock_area.clone(), window, cx);
                }
            },
        )
        .detach();

        let top_bar = cx.new(|_| TopBar::new("terminal_demo"));

        Self {
            top_bar,
            dock_area,
            last_saved: None,
            _save_task: None,
        }
    }

    fn schedule_save(
        &mut self,
        dock_area: Entity<DockArea>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._save_task = Some(cx.spawn_in(window, async move |this, window| {
            window
                .background_executor()
                .timer(Duration::from_millis(500))
                .await;
            _ = this.update_in(window, |this, _, cx| {
                let state = dock_area.read(cx).dump(cx);
                if Some(&state) == this.last_saved.as_ref() {
                    return;
                }
                if let Err(err) = persistence::save(&state) {
                    log::warn!("save layout failed: {err:?}");
                } else {
                    this.last_saved = Some(state);
                }
            });
        }));
    }

    fn on_add_panel(
        &mut self,
        action: &AddPanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(kind) = Kind::from_id(action.0.as_ref()) else {
            return;
        };
        let panel = panels::build_kind(kind, window, cx);

        // Prefer the most recently focused TabPanel so the new tab lands where the user is
        // working. Falls back to DockArea::add_panel(Center) if nothing was ever focused
        // (e.g. fresh layout, or the focused panel was just closed).
        let target = cx
            .global::<LastFocusedTabPanel>()
            .0
            .borrow()
            .clone()
            .and_then(|w| w.upgrade());

        if let Some(tab_panel) = target {
            tab_panel.update(cx, |tp, cx| tp.add_panel(panel, window, cx));
        } else {
            self.dock_area.update(cx, |dock_area, cx| {
                dock_area.add_panel(panel, DockPlacement::Center, None, window, cx);
            });
        }
    }

    fn on_reset_layout(
        &mut self,
        _: &ResetLayout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = persistence::clear();
        let weak = self.dock_area.downgrade();
        apply_default_layout(weak, window, cx);
        self.last_saved = None;
    }
}

impl Render for TerminalWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (bg, fg) = {
            let theme = cx.theme();
            (theme.background, theme.foreground)
        };
        let sheet_layer = Root::render_sheet_layer(window, cx);
        let dialog_layer = Root::render_dialog_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);

        div()
            .id("terminal-workspace")
            .on_action(cx.listener(Self::on_add_panel))
            .on_action(cx.listener(Self::on_reset_layout))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(fg)
            .child(self.top_bar.clone())
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(self.dock_area.clone()),
            )
            .children(sheet_layer)
            .children(dialog_layer)
            .children(notification_layer)
    }
}

fn try_load(
    dock_area: &Entity<DockArea>,
    window: &mut Window,
    cx: &mut Context<TerminalWorkspace>,
) -> bool {
    let Ok(Some(state)) = persistence::load() else {
        return false;
    };
    if state.version != Some(LAYOUT_VERSION) {
        return false;
    }
    dock_area
        .update(cx, |dock_area, cx| dock_area.load(state, window, cx))
        .is_ok()
}

fn apply_default_layout(
    dock_area: gpui::WeakEntity<DockArea>,
    window: &mut Window,
    cx: &mut App,
) {
    let watchlist = build(PANEL_KINDS[0], window, cx);
    let chart = build(PANEL_KINDS[1], window, cx);
    let details = build(PANEL_KINDS[2], window, cx);

    // Pure recursive tiling: one horizontal split with three children.
    // No left/right/bottom dock zones — user reshapes via drag-to-edge.
    let layout = DockItem::split_with_sizes(
        Axis::Horizontal,
        vec![
            DockItem::tabs(vec![watchlist], &dock_area, window, cx),
            DockItem::tabs(vec![chart], &dock_area, window, cx),
            DockItem::tabs(vec![details], &dock_area, window, cx),
        ],
        vec![Some(gpui::px(280.)), None, Some(gpui::px(320.))],
        &dock_area,
        window,
        cx,
    );

    _ = dock_area.update(cx, |view, cx| {
        view.set_version(LAYOUT_VERSION, window, cx);
        view.set_center(layout, window, cx);
    });
}

fn build(kind: Kind, window: &mut Window, cx: &mut App) -> Arc<dyn PanelView> {
    panels::build_kind(kind, window, cx)
}
