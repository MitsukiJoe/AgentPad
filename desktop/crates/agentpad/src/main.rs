#![cfg_attr(windows, windows_subsystem = "windows")]

mod autostart;
mod handle;
mod identity;
mod logutil;
mod net;
mod protocol;
mod qr;
mod ui;
mod updater;
mod ws;

use std::net::SocketAddr;

fn main() -> eframe::Result {
    let post_update = updater::is_post_update_launch();
    autostart::apply();
    updater::cleanup_stale_updater_script();
    let identity = identity::load();
    logutil::clear();

    let rt = tokio::runtime::Runtime::new().expect("tokio");
    let state = ws::AppState::new(identity);
    let bind = SocketAddr::from(([0, 0, 0, 0], protocol::PORT));
    match rt.block_on(ws::serve_with_retry(state.clone(), bind, post_update)) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            logutil::write("connection listen already_running");
            std::process::exit(0);
        }
        Err(_) => {
            logutil::write("connection listen failed");
            std::process::exit(1);
        }
    }

    let viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([440.0, 804.0])
        .with_title("AgentPad")
        .with_resizable(false);
    #[cfg(target_os = "windows")]
    let viewport = {
        let app_icon =
            eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon_white.png"))
                .expect("valid app icon");
        viewport.with_icon(app_icon)
    };
    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let result = eframe::run_native(
        "AgentPad",
        native_options,
        Box::new(move |cc| Ok(Box::new(ui::PairingApp::new(cc, state)))),
    );
    drop(rt);
    result
}
