use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, FontData, FontDefinitions, FontFamily, TextureHandle, TextureOptions, ThemePreference,
    Vec2,
};
use egui_material_icons::icons::{ICON_COMPUTER, ICON_DARK_MODE, ICON_LIGHT_MODE};
use egui_material_icons::MaterialIcon;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::net::{self, Nic, NicKind};
use crate::protocol::{QrPayload, PORT};
use crate::qr;
use crate::ws::AppState;

const CONTENT_W: f32 = 344.0;
const QR_FRAME: f32 = 284.0;
const QR_INSET: f32 = 12.0;
const CONTROL_H: f32 = 40.0;
const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

pub struct PairingApp {
    state: Arc<AppState>,
    nics: Vec<Nic>,
    selected_ip: String,
    last_nic_scan: Instant,
    qr_tex: Option<(String, u32, bool, TextureHandle)>,
    tray: Option<TrayIcon>,
    tray_dark: bool,
    #[cfg(target_os = "windows")]
    window_icon_dark: Option<bool>,
    mi_show: MenuItem,
    mi_pause: CheckMenuItem,
    mi_logs: MenuItem,
    mi_quit: MenuItem,
    theme: ThemePreference,
    quit: bool,
    hidden: bool,
    ax_ok: bool,
    copied_at: Option<Instant>,
    sharing_enabled: bool,
    permission_expanded: bool,
    autostart_enabled: bool,
    updater: Arc<crate::updater::Updater>,
    last_update_check: Instant,
}

impl PairingApp {
    pub fn new(cc: &eframe::CreationContext<'_>, state: Arc<AppState>) -> Self {
        install_cjk_font(&cc.egui_ctx);
        egui_material_icons::initialize(&cc.egui_ctx);
        let theme = theme_from_str(&crate::identity::load_theme());
        cc.egui_ctx.set_theme(theme);
        apply_app_style(&cc.egui_ctx);
        let nics = net::list_nics();
        let selected_ip = net::default_ip(&nics).unwrap_or_default();
        let mi_show = MenuItem::new("显示配对码", true, None);
        let mi_pause = CheckMenuItem::new("暂停接收", true, false, None);
        let mi_logs = MenuItem::new("打开日志", true, None);
        let mi_quit = MenuItem::new("退出", true, None);
        let menu = Menu::new();
        let _ = menu.append_items(&[
            &mi_show,
            &mi_pause,
            &mi_logs,
            &PredefinedMenuItem::separator(),
            &mi_quit,
        ]);
        let tray_dark = match theme {
            ThemePreference::Dark => true,
            ThemePreference::Light => false,
            ThemePreference::System => cc.egui_ctx.theme() == egui::Theme::Dark,
        };
        let builder = TrayIconBuilder::new()
            .with_tooltip("AgentPad")
            .with_menu(Box::new(menu))
            .with_icon(tray_icon(tray_dark));
        let tray = match builder.build() {
            Ok(t) => Some(t),
            Err(e) => {
                crate::logutil::write(&format!("tray failed: {e}"));
                None
            }
        };
        let updater = Arc::new(crate::updater::Updater::new());
        updater.check_for_updates();

        Self {
            state,
            nics,
            selected_ip,
            last_nic_scan: Instant::now(),
            qr_tex: None,
            tray,
            tray_dark,
            #[cfg(target_os = "windows")]
            window_icon_dark: None,
            mi_show,
            mi_pause,
            mi_logs,
            mi_quit,
            theme,
            quit: false,
            hidden: false,
            ax_ok: agentpad_input::accessibility_trusted(),
            copied_at: None,
            sharing_enabled: false,
            permission_expanded: false,
            autostart_enabled: crate::autostart::enabled(),
            updater,
            last_update_check: Instant::now(),
        }
    }

    fn sync_desktop_icons(&mut self, _ctx: &egui::Context, _frame: &eframe::Frame, dark: bool) {
        if dark != self.tray_dark {
            if let Some(tray) = &self.tray {
                if let Err(e) = tray.set_icon(Some(tray_icon(dark))) {
                    crate::logutil::write(&format!("tray icon update failed: {e}"));
                } else {
                    self.tray_dark = dark;
                }
            } else {
                self.tray_dark = dark;
            }
        }

        #[cfg(target_os = "windows")]
        if self.window_icon_dark != Some(dark) {
            let icon = tray_icon_data(dark);
            _ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(Arc::new(icon.clone()))));

            if let Some(window) = _frame.winit_window() {
                use winit::platform::windows::WindowExtWindows;
                let taskbar_icon =
                    winit::window::Icon::from_rgba(icon.rgba, icon.width, icon.height)
                        .expect("valid taskbar icon");
                window.set_taskbar_icon(Some(taskbar_icon));
            }
            self.window_icon_dark = Some(dark);
        }
    }

    fn refresh_nics(&mut self) {
        let nics = net::list_nics();
        if !nics.iter().any(|n| n.ip == self.selected_ip) {
            self.selected_ip = net::default_ip(&nics).unwrap_or_default();
        }
        self.nics = nics;
        self.last_nic_scan = Instant::now();
    }

    fn payload(&self) -> QrPayload {
        // Selected NIC first; phone stores only `ip` on scan. Full list is for
        // optional post-connect collect-all via `connected.ips`, not QR.
        let mut ips: Vec<String> = Vec::with_capacity(self.nics.len().max(1));
        if !self.selected_ip.is_empty() {
            ips.push(self.selected_ip.clone());
        }
        for n in &self.nics {
            if n.ip != self.selected_ip {
                ips.push(n.ip.clone());
            }
        }
        QrPayload::new(
            self.state.identity.device_id.clone(),
            self.state.identity.name.clone(),
            agentpad_input::os().to_string(),
            self.selected_ip.clone(),
            ips,
        )
    }

    fn cycle_nic(&mut self, dir: i32) {
        if self.nics.is_empty() {
            return;
        }
        let i = self
            .nics
            .iter()
            .position(|n| n.ip == self.selected_ip)
            .unwrap_or(0) as i32;
        let n = self.nics.len() as i32;
        let j = (i + dir).rem_euclid(n) as usize;
        self.selected_ip = self.nics[j].ip.clone();
    }

    fn show_window(&mut self, ctx: &egui::Context) {
        self.hidden = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    fn hide_window(&mut self, ctx: &egui::Context) {
        self.hidden = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
    }

    fn poll_tray(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if ev.id == self.mi_show.id() {
                self.show_window(ctx);
            } else if ev.id == self.mi_pause.id() {
                let next = !self.state.paused.load(std::sync::atomic::Ordering::SeqCst);
                self.state.set_paused(next);
                self.mi_pause.set_checked(next);
            } else if ev.id == self.mi_logs.id() {
                crate::logutil::open_dir();
            } else if ev.id == self.mi_quit.id() {
                self.quit = true;
                crate::logutil::write("quit from tray");
                ctx.request_repaint();
                // Hidden pairing window: Viewport Close is often ignored once on
                // Windows (event loop keeps running until a second quit). Exit
                // directly when already in tray-only mode.
                if self.hidden {
                    std::process::exit(0);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn apply_window_cmds(&mut self, ctx: &egui::Context) {
        // read viewport without holding the lock across send_viewport_cmd
        let (close, minimized) = ctx.input(|i| {
            let v = i.viewport();
            (v.close_requested(), v.minimized == Some(true))
        });
        if close && !self.quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide_window(ctx);
        }
        if minimized && !self.quit && !self.hidden {
            self.hide_window(ctx);
        }
    }

    fn tick(&mut self, ctx: &egui::Context) {
        self.poll_tray(ctx);
        self.apply_window_cmds(ctx);
        if self.last_nic_scan.elapsed() > Duration::from_secs(2) {
            self.refresh_nics();
        }
        if self.last_update_check.elapsed() >= UPDATE_CHECK_INTERVAL {
            self.updater.check_for_updates();
            self.last_update_check = Instant::now();
        }
        ctx.set_theme(self.theme);
        apply_app_style(ctx);
        self.poll_ax(ctx);
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    fn poll_ax(&mut self, _ctx: &egui::Context) {
        let ax = agentpad_input::accessibility_trusted();
        if ax != self.ax_ok {
            crate::logutil::write(&format!(
                "accessibility {ax} {}",
                agentpad_input::accessibility_debug()
            ));
        }
        self.ax_ok = ax;
    }
}

impl eframe::App for PairingApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if !self.sharing_enabled {
            if let Some(window) = frame.winit_window() {
                window.set_content_protected(false);
                self.sharing_enabled = true;
            }
        }
        let ctx = ui.ctx().clone();
        let notice = permission_notice(crate::autostart::running_from_app_bundle(), self.ax_ok);
        if notice.is_some() != self.permission_expanded {
            self.permission_expanded = notice.is_some();
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(
                440.0,
                if self.permission_expanded {
                    892.0
                } else {
                    772.0
                },
            )));
        }
        let dark = ui.visuals().dark_mode;
        self.sync_desktop_icons(&ctx, frame, dark);
        let page = if dark {
            egui::Color32::from_rgb(20, 20, 20)
        } else {
            egui::Color32::from_rgb(240, 240, 240)
        };
        let surface = if dark {
            egui::Color32::from_rgb(44, 43, 40)
        } else {
            egui::Color32::from_rgb(209, 208, 204)
        };
        let subtle = if dark {
            egui::Color32::from_rgb(44, 43, 40)
        } else {
            egui::Color32::from_rgb(230, 229, 226)
        };
        let border = if dark {
            egui::Color32::from_rgb(61, 60, 57)
        } else {
            egui::Color32::from_rgb(188, 186, 183)
        };
        let muted = if dark {
            egui::Color32::from_rgb(164, 164, 164)
        } else {
            egui::Color32::from_rgb(96, 96, 96)
        };
        let text = if dark {
            egui::Color32::from_rgb(240, 240, 240)
        } else {
            egui::Color32::from_rgb(20, 20, 20)
        };
        let accent = egui::Color32::from_rgb(94, 175, 249);

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(page).inner_margin(24))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(CONTENT_W, 58.0),
                        egui::Layout::left_to_right(egui::Align::Min),
                        |ui| {
                            ui.vertical(|ui| {
                                ui.label(
                                    egui::RichText::new("AgentPad")
                                        .size(22.0)
                                        .strong()
                                        .color(text),
                                );
                                ui.add_space(2.0);
                                ui.horizontal(|ui| {
                                    let (dot, _) = ui.allocate_exact_size(
                                        Vec2::splat(10.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().circle_filled(
                                        dot.center(),
                                        5.0,
                                        egui::Color32::from_rgb(116, 181, 128),
                                    );
                                    ui.add_space(2.0);
                                    ui.label(
                                        egui::RichText::new(format!("监听 {PORT}"))
                                            .size(12.0)
                                            .color(muted),
                                    );
                                    ui.add_space(6.0);
                                    ui.label(
                                        egui::RichText::new(format!("v{}", crate::updater::CURRENT_VERSION))
                                            .size(12.0)
                                            .color(muted),
                                    );

                                    let update_status = self.updater.status.lock().unwrap().clone();
                                    match update_status {
                                        crate::updater::UpdateStatus::Available(info) => {
                                            ui.add_space(4.0);
                                            let update_btn = ui.add(
                                                egui::Button::new(
                                                    egui::RichText::new(format!("更新到 v{}", info.version))
                                                        .size(12.0)
                                                        .color(egui::Color32::from_rgb(116, 181, 128))
                                                        .underline(),
                                                )
                                                .frame(false),
                                            );
                                            if update_btn
                                                .on_hover_text("确认下载并安装此版本")
                                                .clicked()
                                            {
                                                self.updater.start_update(&info);
                                            }
                                        }
                                        crate::updater::UpdateStatus::Checking => {
                                            ui.add_space(4.0);
                                            ui.label(
                                                egui::RichText::new("检查中...")
                                                    .size(12.0)
                                                    .color(muted),
                                            );
                                        }
                                        crate::updater::UpdateStatus::Updating(msg) => {
                                            ui.add_space(4.0);
                                            ui.label(
                                                egui::RichText::new(msg)
                                                    .size(12.0)
                                                    .color(accent),
                                            );
                                        }
                                        crate::updater::UpdateStatus::UpdateFailed { info, message } => {
                                            ui.add_space(4.0);
                                            let short: String = if message.chars().count() > 60 {
                                                format!("{}...", message.chars().take(60).collect::<String>())
                                            } else {
                                                message.clone()
                                            };
                                            ui.label(
                                                egui::RichText::new(short)
                                                    .size(12.0)
                                                    .color(muted),
                                            )
                                            .on_hover_text(&message);
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        egui::RichText::new("重试更新")
                                                            .size(12.0)
                                                            .color(muted),
                                                    )
                                                    .frame(false),
                                                )
                                                .clicked()
                                            {
                                                self.updater.start_update(&info);
                                            }
                                        }
                                        status @ (crate::updater::UpdateStatus::Idle
                                        | crate::updater::UpdateStatus::UpToDate
                                        | crate::updater::UpdateStatus::Failed(_)) => {
                                            ui.add_space(4.0);
                                            let label = match status {
                                                crate::updater::UpdateStatus::UpToDate => "已是最新 · 重新检查",
                                                crate::updater::UpdateStatus::Failed(_) => "检查失败 · 重试",
                                                _ => "检查更新",
                                            };
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        egui::RichText::new(label)
                                                            .size(12.0)
                                                            .color(muted),
                                                    )
                                                    .frame(false),
                                                )
                                                .clicked()
                                            {
                                                self.updater.check_for_updates();
                                            }
                                        }
                                    }
                                });
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Min),
                                |ui| {
                                    let changed = theme_chip(
                                        ui,
                                        &mut self.theme,
                                        ThemePreference::Dark,
                                        accent,
                                        surface,
                                        border,
                                        muted,
                                    ) | theme_chip(
                                        ui,
                                        &mut self.theme,
                                        ThemePreference::Light,
                                        accent,
                                        surface,
                                        border,
                                        muted,
                                    ) | theme_chip(
                                        ui,
                                        &mut self.theme,
                                        ThemePreference::System,
                                        accent,
                                        surface,
                                        border,
                                        muted,
                                    );
                                    if changed {
                                        crate::identity::save_theme(theme_to_str(self.theme));
                                        ctx.set_theme(self.theme);
                                        apply_app_style(&ctx);
                                    }
                                },
                            );
                        },
                    );

                    ui.add_space(20.0);
                    if let Some(notice) = notice {
                    let notice_fill = if dark {
                        egui::Color32::from_rgb(48, 39, 26)
                    } else {
                        egui::Color32::from_rgb(255, 246, 230)
                    };
                    egui::Frame::new()
                        .fill(notice_fill)
                        .stroke(egui::Stroke::new(
                            1.0,
                            egui::Color32::from_rgb(185, 126, 45),
                        ))
                        .corner_radius(12)
                        .inner_margin(14)
                        .show(ui, |ui| {
                            ui.set_width(CONTENT_W - 28.0);
                            let (title, body) = match notice {
                                PermissionNotice::DevelopmentLaunch => (
                                    "请从 AgentPad.app 启动",
                                    "当前是开发启动方式。请关闭此实例后打开打包应用，不要给启动器授权。",
                                ),
                                PermissionNotice::NeedsAccess => (
                                    "需要辅助功能权限",
                                    "用于从手机发送按键并控制指针。若更新后列表里已有 AgentPad 但授权无效，请先删除旧项，再点击下方按钮为当前版本重新授权。",
                                ),
                            };
                            ui.label(egui::RichText::new(title).strong());
                            ui.label(egui::RichText::new(body).color(muted));
                            if notice == PermissionNotice::NeedsAccess {
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    let request = egui::Button::new(
                                        egui::RichText::new("请求授权")
                                            .color(egui::Color32::from_rgb(20, 20, 20)),
                                    )
                                    .fill(accent)
                                    .stroke(egui::Stroke::NONE)
                                    .corner_radius(8);
                                    if ui.add(request).clicked() {
                                        agentpad_input::prompt_accessibility();
                                        self.ax_ok = agentpad_input::accessibility_trusted();
                                    }
                                    if ui.button("打开系统设置").clicked() {
                                        agentpad_input::open_accessibility_settings();
                                    }
                                });
                            }
                        });
                        ui.add_space(16.0);
                    }

                    ui.label(
                        egui::RichText::new("连接手机")
                            .size(18.0)
                            .strong()
                            .color(text),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new("扫描二维码或复制地址")
                            .size(13.0)
                            .color(muted),
                    );
                    ui.add_space(20.0);

                    let scale = ctx.pixels_per_point();
                    let px = qr::raster_px(scale);
                    let payload = serde_json::to_string(&self.payload()).unwrap_or_default();
                    let stale = self
                        .qr_tex
                        .as_ref()
                        .map(|(p, s, d, _)| p != &payload || *s != px || *d != dark)
                        .unwrap_or(true);
                    if stale {
                        if let Ok(img) = qr::color_image(&payload, px, dark) {
                            let tex = ctx.load_texture("qr", img, TextureOptions::NEAREST);
                            self.qr_tex = Some((payload, px, dark, tex));
                        }
                    }
                    if let Some((_, _, _, tex)) = &self.qr_tex {
                        let (rect, _) = ui.allocate_exact_size(
                            Vec2::splat(QR_FRAME),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(rect, 18, surface);
                        ui.painter().rect_stroke(
                            rect,
                            18,
                            egui::Stroke::new(1.0, border),
                            egui::StrokeKind::Inside,
                        );
                        ui.put(
                            rect.shrink(QR_INSET),
                            egui::Image::new((tex.id(), Vec2::splat(qr::QR_PT))),
                        );
                    }

                    ui.add_space(20.0);
                    ui.label(
                        egui::RichText::new("当前网卡")
                            .size(12.0)
                            .color(muted),
                    );
                    ui.add_space(6.0);
                    let (nic_rect, _) = ui.allocate_exact_size(
                        Vec2::new(CONTENT_W, CONTROL_H),
                        egui::Sense::hover(),
                    );
                    let left_rect = egui::Rect::from_min_size(
                        nic_rect.left_top(),
                        Vec2::splat(CONTROL_H),
                    );
                    let right_rect = egui::Rect::from_min_size(
                        egui::pos2(nic_rect.right() - CONTROL_H, nic_rect.top()),
                        Vec2::splat(CONTROL_H),
                    );
                    let label_rect = egui::Rect::from_min_max(
                        egui::pos2(left_rect.right() + 8.0, nic_rect.top()),
                        egui::pos2(right_rect.left() - 8.0, nic_rect.bottom()),
                    );
                    if ui
                        .put(left_rect, neutral_button("‹", surface, border, muted))
                        .clicked()
                    {
                        self.cycle_nic(-1);
                    }
                    let label = current_nic_label(&self.nics, &self.selected_ip);
                    ui.put(
                        label_rect,
                        egui::Label::new(
                            egui::RichText::new(label)
                                .size(14.0)
                                .strong()
                                .color(text),
                        )
                        .halign(egui::Align::Center),
                    );
                    if ui
                        .put(
                            right_rect,
                            neutral_button("›", surface, border, muted),
                        )
                        .clicked()
                    {
                        self.cycle_nic(1);
                    }

                    ui.add_space(12.0);
                    let addr = format!("{}:{PORT}", self.selected_ip);
                    let (address_rect, _) = ui.allocate_exact_size(
                        Vec2::new(CONTENT_W, 48.0),
                        egui::Sense::hover(),
                    );
                    ui.painter().rect_filled(address_rect, 10, subtle);
                    ui.painter().rect_stroke(
                        address_rect,
                        10,
                        egui::Stroke::new(1.0, border),
                        egui::StrokeKind::Inside,
                    );
                    let inner = address_rect.shrink2(Vec2::new(12.0, 7.0));
                    let copy_rect = egui::Rect::from_min_size(
                        egui::pos2(inner.right() - 88.0, inner.top()),
                        Vec2::new(88.0, 34.0),
                    );
                    let address_label_rect = egui::Rect::from_min_max(
                        inner.left_top(),
                        egui::pos2(copy_rect.left() - 8.0, inner.bottom()),
                    );
                    ui.put(
                        address_label_rect,
                        egui::Label::new(
                            egui::RichText::new(&addr).monospace().strong().color(text),
                        )
                        .halign(egui::Align::Min),
                    );
                    let label = copy_label(self.copied_at, Instant::now());
                    if ui
                        .put(
                            copy_rect,
                            neutral_button(label, surface, border, muted),
                        )
                        .clicked()
                    {
                        ctx.copy_text(addr);
                        self.copied_at = Some(Instant::now());
                    }
                    if crate::autostart::available() {
                        ui.add_space(14.0);
                        ui.allocate_ui_with_layout(
                            Vec2::new(CONTENT_W, 28.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                let mut enabled = self.autostart_enabled;
                                if ui.checkbox(&mut enabled, "开机启动").changed() {
                                    match crate::autostart::set_enabled(enabled) {
                                        Ok(()) => self.autostart_enabled = enabled,
                                        Err(e) => crate::logutil::write(&format!(
                                            "autostart setting: {e}"
                                        )),
                                    }
                                }
                            },
                        );
                    }
                    ui.add_space(24.0);
                    ui.add_sized(Vec2::new(CONTENT_W, 1.0), egui::Separator::default());
                    ui.add_space(14.0);
                    ui.label(
                        egui::RichText::new("关闭窗口后仍在菜单栏运行 · 从托盘菜单退出")
                            .size(12.0)
                            .color(muted),
                    );
                });
            });
    }
}

fn theme_icon_spec(theme: ThemePreference) -> (MaterialIcon, &'static str) {
    match theme {
        ThemePreference::System => (ICON_COMPUTER, "跟随系统"),
        ThemePreference::Light => (ICON_LIGHT_MODE, "浅色模式"),
        ThemePreference::Dark => (ICON_DARK_MODE, "深色模式"),
    }
}

fn theme_chip(
    ui: &mut egui::Ui,
    cur: &mut ThemePreference,
    this: ThemePreference,
    accent: egui::Color32,
    surface: egui::Color32,
    border: egui::Color32,
    muted: egui::Color32,
) -> bool {
    let selected = *cur == this;
    let (icon, tooltip) = theme_icon_spec(this);
    let button = egui::Button::new(icon.rich_text().size(18.0).color(if selected {
        egui::Color32::from_rgb(20, 20, 20)
    } else {
        muted
    }))
    .fill(if selected { accent } else { surface })
    .stroke(egui::Stroke::new(
        1.0,
        if selected { accent } else { border },
    ))
    .corner_radius(9);
    let response = ui
        .add_sized(Vec2::new(40.0, 34.0), button)
        .on_hover_text(tooltip);
    if response.clicked() {
        *cur = this;
        true
    } else {
        false
    }
}

fn neutral_button<'a>(
    label: &'a str,
    fill: egui::Color32,
    border: egui::Color32,
    text: egui::Color32,
) -> egui::Button<'a> {
    egui::Button::new(egui::RichText::new(label).size(13.0).color(text))
        .fill(fill)
        .stroke(egui::Stroke::new(1.0, border))
        .corner_radius(9)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PermissionNotice {
    DevelopmentLaunch,
    NeedsAccess,
}

const ICON_WHITE_PNG: &[u8] = include_bytes!("../assets/icon_white.png");
const ICON_BLACK_PNG: &[u8] = include_bytes!("../assets/icon_black.png");

fn permission_notice(packaged: bool, ax_ok: bool) -> Option<PermissionNotice> {
    if !packaged {
        Some(PermissionNotice::DevelopmentLaunch)
    } else if !ax_ok {
        Some(PermissionNotice::NeedsAccess)
    } else {
        None
    }
}

fn copy_label(copied_at: Option<Instant>, now: Instant) -> &'static str {
    if copied_at.is_some_and(|at| now.duration_since(at) < Duration::from_secs(2)) {
        "已复制"
    } else {
        "复制地址"
    }
}

fn apply_app_style(ctx: &egui::Context) {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        ctx.style_mut_of(theme, |style| {
            style.spacing.item_spacing = Vec2::new(8.0, 8.0);
            style.spacing.button_padding = Vec2::new(12.0, 7.0);
            style.spacing.interact_size.y = 34.0;
            for widget in [
                &mut style.visuals.widgets.inactive,
                &mut style.visuals.widgets.hovered,
                &mut style.visuals.widgets.active,
                &mut style.visuals.widgets.open,
            ] {
                widget.corner_radius = 8.into();
                widget.expansion = 0.0;
            }
        });
    }
}

fn theme_from_str(s: &str) -> ThemePreference {
    match s {
        "light" => ThemePreference::Light,
        "dark" => ThemePreference::Dark,
        _ => ThemePreference::System,
    }
}

fn theme_to_str(p: ThemePreference) -> &'static str {
    match p {
        ThemePreference::Light => "light",
        ThemePreference::Dark => "dark",
        ThemePreference::System => "system",
    }
}

fn current_nic_label(nics: &[Nic], ip: &str) -> String {
    nics.iter()
        .find(|n| n.ip == ip)
        .map(|n| match n.kind {
            NicKind::Wifi => format!("Wi-Fi  {}", n.ip),
            NicKind::Tunnel => format!("隧道  {}", n.ip),
            NicKind::Virtual => format!("虚拟/共享 · {}  {}", n.name, n.ip),
            NicKind::Ethernet | NicKind::Other => format!("{}  {}", n.name, n.ip),
        })
        .unwrap_or_else(|| {
            if ip.is_empty() {
                "未找到局域网 IP".into()
            } else {
                ip.to_string()
            }
        })
}

fn install_cjk_font(ctx: &egui::Context) {
    let Some(bytes) = cjk_font_bytes() else {
        crate::logutil::write("no CJK font found");
        return;
    };
    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("cjk".to_owned(), Arc::new(FontData::from_owned(bytes)));
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "cjk".to_owned());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .push("cjk".to_owned());
    ctx.set_fonts(fonts);
}

fn cjk_font_bytes() -> Option<Vec<u8>> {
    const CANDIDATES: &[&str] = &[
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/PrivateFrameworks/FontServices.framework/Versions/A/Resources/Reserved/PingFangUI.ttc",
        "/System/Library/Fonts/Supplemental/Songti.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyh.ttf",
        r"C:\Windows\Fonts\simhei.ttf",
    ];
    for path in CANDIDATES {
        if let Ok(bytes) = std::fs::read(path) {
            crate::logutil::write(&format!("cjk font {path}"));
            return Some(bytes);
        }
    }
    None
}

fn tray_icon_data(dark: bool) -> egui::IconData {
    let bytes = if dark { ICON_BLACK_PNG } else { ICON_WHITE_PNG };
    eframe::icon_data::from_png_bytes(bytes).expect("valid tray icon")
}

fn tray_icon(dark: bool) -> Icon {
    let icon = tray_icon_data(dark);
    Icon::from_rgba(icon.rgba, icon.width, icon.height).expect("valid tray icon")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_buttons_use_material_icons_and_tooltips() {
        let (system, system_tip) = theme_icon_spec(ThemePreference::System);
        let (light, light_tip) = theme_icon_spec(ThemePreference::Light);
        let (dark, dark_tip) = theme_icon_spec(ThemePreference::Dark);
        assert_eq!(system.codepoint, ICON_COMPUTER.codepoint);
        assert_eq!(light.codepoint, ICON_LIGHT_MODE.codepoint);
        assert_eq!(dark.codepoint, ICON_DARK_MODE.codepoint);
        assert_eq!(system_tip, "跟随系统");
        assert_eq!(light_tip, "浅色模式");
        assert_eq!(dark_tip, "深色模式");
        assert_ne!(system.codepoint, light.codepoint);
        assert_ne!(light.codepoint, dark.codepoint);
    }

    #[test]
    fn tray_theme_icons_are_distinct_and_high_resolution() {
        let light = tray_icon_data(false);
        let dark = tray_icon_data(true);
        assert_eq!((light.width, light.height), (1024, 1024));
        assert_eq!((dark.width, dark.height), (1024, 1024));
        assert_ne!(light.rgba, dark.rgba);
    }

    #[test]
    fn permission_notice_never_requests_access_for_bare_binary() {
        assert_eq!(
            permission_notice(false, false),
            Some(PermissionNotice::DevelopmentLaunch),
        );
        assert_eq!(
            permission_notice(false, true),
            Some(PermissionNotice::DevelopmentLaunch),
        );
        assert_eq!(
            permission_notice(true, false),
            Some(PermissionNotice::NeedsAccess),
        );
        assert_eq!(permission_notice(true, true), None);
    }

    #[test]
    fn copy_feedback_expires() {
        let now = Instant::now();
        assert_eq!(copy_label(None, now), "复制地址");
        assert_eq!(copy_label(Some(now), now), "已复制");
        assert_eq!(
            copy_label(Some(now - Duration::from_secs(3)), now),
            "复制地址",
        );
    }

    #[test]
    fn update_check_interval_is_24_hours() {
        assert_eq!(UPDATE_CHECK_INTERVAL, Duration::from_secs(24 * 60 * 60));
    }

    #[test]
    fn layout_uses_one_exact_grid() {
        assert_eq!(QR_FRAME - QR_INSET * 2.0, qr::QR_PT);
        assert_eq!(CONTROL_H * 2.0 + 248.0 + 16.0, CONTENT_W);
        assert_eq!(224.0 + 88.0 + 8.0 + 24.0, CONTENT_W);
    }
}
