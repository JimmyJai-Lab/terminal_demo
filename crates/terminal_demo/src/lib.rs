use gpui::{App, AppContext as _, Application, Bounds, Entity, WindowBounds, WindowOptions, px, size};
use gpui_component::{Root, Theme, ThemeMode};

pub mod economic_calendar;
pub mod panels;
pub mod persistence;
pub mod top_bar;
pub mod workspace;

pub use workspace::TerminalWorkspace;

/// Run the app on a freshly-created [`Application`]. Shared by native + WASM entry points.
pub fn run(app: Application) {
    app.run(|cx: &mut App| {
        init(cx);
        open_window(cx);
        cx.activate(true);
    });
}

fn init(cx: &mut App) {
    gpui_component::init(cx);
    panels::init(cx);

    // Hardcoded dark theme.
    Theme::change(ThemeMode::Dark, None, cx);

    #[cfg(target_family = "wasm")]
    install_wasm_fonts(cx);
}

fn open_window(cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        |window, cx| {
            let workspace: Entity<TerminalWorkspace> =
                cx.new(|cx| TerminalWorkspace::new(window, cx));
            cx.new(|cx| Root::new(workspace, window, cx))
        },
    )
    .expect("failed to open window");
}

#[cfg(target_family = "wasm")]
fn install_wasm_fonts(cx: &mut App) {
    use std::borrow::Cow;
    let cjk = Cow::Borrowed(include_bytes!("../../../fonts/NotoSansSC-Regular-subset.ttf").as_slice());
    let emoji = Cow::Borrowed(include_bytes!("../../../fonts/NotoEmoji-Regular.ttf").as_slice());
    let mono = Cow::Borrowed(include_bytes!("../../../fonts/JetBrainsMono-Regular.ttf").as_slice());
    cx.text_system()
        .add_fonts(vec![cjk, emoji, mono])
        .expect("failed to load fonts");
    cx.global_mut::<Theme>().font_family = "Noto Sans SC".into();
    cx.global_mut::<Theme>().mono_font_family = "JetBrains Mono".into();
}

// =====================
// WASM entry
// =====================

#[cfg(target_family = "wasm")]
mod wasm_entry {
    use super::*;
    use gpui::AppCell;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    pub fn run() -> Result<(), JsValue> {
        console_error_panic_hook::set_once();
        let _ = console_log::init_with_level(log::Level::Info);
        tracing_wasm::set_as_global_default();

        gpui_platform::web_init();
        let app = gpui_platform::single_threaded_web();

        // Mirror story-web's leak hack so the WASM `Rc<AppCell>` outlives `run()`.
        struct WasmApplication(std::rc::Rc<AppCell>);
        let wasm_app = unsafe { std::mem::transmute::<Application, WasmApplication>(app) };
        std::mem::forget(wasm_app.0.clone());
        let app: Application = unsafe { std::mem::transmute::<WasmApplication, Application>(wasm_app) };

        let app = app.with_assets(gpui_component_assets::Assets::new(
            "https://longbridge.github.io/gpui-component/gallery/",
        ));
        super::run(app);
        Ok(())
    }
}

