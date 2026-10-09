#![cfg_attr(windows, windows_subsystem = "windows")]

mod autostart;
mod elevation;
mod handle;
mod identity;
mod logutil;
mod net;
mod pairing;
mod protocol;
mod qr;
mod ui;
mod updater;
mod ws;

use std::net::SocketAddr;

fn main() -> eframe::Result {
    elevation::exit_if_admin_maintenance();
    if elevation::invalid_elevation_attempt() {
        std::process::exit(1);
    }
    // Windows：开机启动项由未提权进程在提权重启前同步；提权进程里 apply 什么也不做。
    // macOS 没有提权分支，仍在下面原来的位置调用，相对更新检查的顺序不变。
    if cfg!(windows) {
        autostart::apply();
    }
    // 设置里打开管理员模式时，新的普通进程要等旧进程退出并放开端口，再交接。
    if let Err(reason) = elevation::wait_for_handoff_parent() {
        return ui::show_startup_failure(reason);
    }
    if elevation::relaunch_if_needed() {
        return Ok(());
    }
    let post_update = updater::is_post_update_launch();
    if !cfg!(windows) {
        autostart::apply();
    }
    updater::cleanup_stale_updater_script();
    let identity = match identity::load() {
        Ok(identity) => identity,
        Err(err) => {
            // 管理员实例拿不到受保护的密钥时不接收输入，也不回退普通密钥。
            // 失败说明只写 Program Files 的 state\；普通进程这里是空操作。
            logutil::write("identity store failed");
            elevation::note_startup_failure("identity store failed", &err.to_string());
            std::process::exit(1);
        }
    };
    logutil::clear();

    let rt = tokio::runtime::Runtime::new().expect("tokio");
    let state = ws::AppState::new(identity);
    let bind = SocketAddr::from(([0, 0, 0, 0], protocol::PORT));
    match rt.block_on(ws::serve_with_retry(
        state.clone(),
        bind,
        post_update || elevation::relaunched(),
    )) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            logutil::write("connection listen already_running");
            elevation::note_startup_failure("listen already in use", "");
            #[cfg(windows)]
            if elevation::handoff_failure().is_some() {
                return ui::show_startup_failure(
                    "管理员模式未就绪，9618 端口已被占用。请关闭占用程序后重试。",
                );
            }
            std::process::exit(if elevation::relaunched() { 1 } else { 0 });
        }
        Err(_) => {
            logutil::write("connection listen failed");
            elevation::note_startup_failure("listen failed", "");
            #[cfg(windows)]
            if elevation::handoff_failure().is_some() {
                return ui::show_startup_failure("管理员模式未就绪，当前也无法监听 9618 端口。");
            }
            std::process::exit(1);
        }
    }

    let viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([ui::WINDOW_W, 860.0])
        .with_title("AgentsPads")
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
        "AgentsPads",
        native_options,
        Box::new(move |cc| Ok(Box::new(ui::PairingApp::new(cc, state)))),
    );
    #[cfg(windows)]
    handle::stop_input();
    if result.is_err() {
        elevation::note_startup_failure("GUI initialization failed", "");
    }
    drop(rt);
    result
}
