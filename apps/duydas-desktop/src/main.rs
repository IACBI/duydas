//! Duydaş desktop client.
//!
//! Phase 0 shell: live language switching, locale-aware formatting preview and an IME-aware
//! composer with local echo. Networking arrives with `duydas-client` in Phase 1.

mod app;
mod perf;

fn main() -> iced::Result {
    let perf = perf::Probe::from_env();
    iced::application(
        move || app::App::new(perf),
        app::App::update,
        app::App::view,
    )
    .title(app::App::title)
    .theme(app::App::theme)
    .subscription(app::App::subscription)
    .window_size((1100.0, 720.0))
    .run()
}
