fn main() {
    let _ = env_logger::try_init();
    let app = gpui_platform::application()
        .with_assets(gpui_component_assets::Assets);
    terminal_demo::run(app);
}
