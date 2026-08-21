use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GITHUB_REPO: &str = "MitsukiJoe/AgentPad";

#[derive(Clone, Debug, PartialEq)]
pub struct UpdateInfo {
    pub tag_name: String,
    pub version: String,
    pub body: String,
    pub download_url: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    Available(UpdateInfo),
    Updating(String),
    Failed(String),
}

pub struct Updater {
    pub status: Arc<Mutex<UpdateStatus>>,
    checking: Arc<AtomicBool>,
}

impl Updater {
    pub fn new() -> Self {
        Self {
            status: Arc::new(Mutex::new(UpdateStatus::Idle)),
            checking: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn check_for_updates(&self) {
        if self.checking.swap(true, Ordering::SeqCst) {
            return;
        }
        let status_arc = Arc::clone(&self.status);
        let checking_arc = Arc::clone(&self.checking);

        std::thread::spawn(move || {
            *status_arc.lock().unwrap() = UpdateStatus::Checking;
            match fetch_latest_release() {
                Ok(Some(info)) => {
                    *status_arc.lock().unwrap() = UpdateStatus::Available(info);
                }
                Ok(None) => {
                    *status_arc.lock().unwrap() = UpdateStatus::UpToDate;
                }
                Err(e) => {
                    crate::logutil::write(&format!("check update failed: {e}"));
                    *status_arc.lock().unwrap() = UpdateStatus::Failed(e);
                }
            }
            checking_arc.store(false, Ordering::SeqCst);
        });
    }

    pub fn start_update(&self, info: &UpdateInfo) {
        let status_arc = Arc::clone(&self.status);
        let info = info.clone();
        std::thread::spawn(move || {
            *status_arc.lock().unwrap() = UpdateStatus::Updating("正在下载更新...".into());
            if let Err(e) = perform_update(&info) {
                crate::logutil::write(&format!("update failed: {e}"));
                *status_arc.lock().unwrap() = UpdateStatus::Failed(e);
            }
        });
    }
}

fn fetch_latest_release() -> Result<Option<UpdateInfo>, String> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let output = Command::new("curl")
        .args([
            "-sL",
            "--max-time",
            "10",
            "-H",
            "User-Agent: AgentPad-Desktop",
            "-H",
            "Accept: application/vnd.github.v3+json",
            &url,
        ])
        .output()
        .map_err(|e| format!("执行 curl 失败: {e}"))?;

    if !output.status.success() {
        return Err(format!("网络请求失败: exit code {:?}", output.status.code()));
    }

    let json_text = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&json_text)
        .map_err(|e| format!("解析 GitHub 响应失败: {e}"))?;

    let tag_name = v.get("tag_name")
        .and_then(|t| t.as_str())
        .ok_or_else(|| "未找到版本标签".to_string())?
        .to_string();

    let remote_ver = tag_name.trim_start_matches('v').trim().to_string();
    if !is_newer_version(&remote_ver, CURRENT_VERSION) {
        return Ok(None);
    }

    let body = v.get("body").and_then(|b| b.as_str()).unwrap_or("").to_string();
    let assets = v.get("assets").and_then(|a| a.as_array()).ok_or_else(|| "未找到发布文件".to_string())?;

    #[cfg(target_os = "windows")]
    let expected_asset = "agentpad-windows-x64.exe";
    #[cfg(target_os = "macos")]
    let expected_asset = "agentpad-macos-arm64.zip";

    let download_url = assets.iter().find_map(|asset| {
        let name = asset.get("name")?.as_str()?;
        #[cfg(target_os = "macos")]
        if name == "agentpad-macos-arm64.zip" || name == "agentpad-macos-arm64.dmg" {
            return asset.get("browser_download_url")?.as_str().map(|s| s.to_string());
        }
        #[cfg(target_os = "windows")]
        if name == expected_asset || name.ends_with(".exe") {
            return asset.get("browser_download_url")?.as_str().map(|s| s.to_string());
        }
        None
    }).ok_or_else(|| format!("未找到适用的安装包 ({expected_asset})"))?;

    Ok(Some(UpdateInfo {
        tag_name,
        version: remote_ver,
        body,
        download_url,
    }))
}

fn is_newer_version(remote: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.split('.')
            .filter_map(|s| s.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok())
            .collect()
    };
    let r = parse(remote);
    let c = parse(current);
    r > c
}

#[cfg(target_os = "windows")]
fn perform_update(info: &UpdateInfo) -> Result<(), String> {
    let exe_path = std::env::current_exe().map_err(|e| format!("获取当前路径失败: {e}"))?;
    let exe_dir = exe_path.parent().ok_or_else(|| "无法获取程序目录".to_string())?;
    let updater_script = exe_dir.join("agentpad_updater.bat");
    let pid = std::process::id();

    // 生成 Windows 专属更新脚本
    let script_content = format!(
        r#"@echo off
setlocal enabledelayedexpansion
title AgentPad Auto Updater
echo [AgentPad] 正在等待旧版本退出 (PID: {pid})...

:wait_loop
tasklist /fi "PID eq {pid}" | findstr /i "{pid}" >nul
if not errorlevel 1 (
    timeout /t 1 /nobreak >nul
    goto wait_loop
)

echo [AgentPad] 正在下载最新版本 ({ver})...
set TARGET=%~dp0{exe_name}
if not exist "%TARGET%" set TARGET=%~dp0agentpad.exe
set TEMP_FILE=%~dp0agentpad_update.tmp

curl -L -f --max-time 120 -o "%TEMP_FILE%" "{url}"
if not exist "%TEMP_FILE%" (
    echo [错误] 下载失败，请检查网络后重试。
    pause
    exit /b 1
)

echo [AgentPad] 正在替换程序文件...
move /y "%TEMP_FILE%" "%TARGET%" >nul
if errorlevel 1 (
    echo [错误] 替换文件失败，请以管理员身份重试。
    del "%TEMP_FILE%" >nul 2>&1
    pause
    exit /b 1
)

echo [AgentPad] 更新完成，正在启动新版本...
start "" "%TARGET%"
del "%~f0" >nul 2>&1
exit /b 0
"#,
        pid = pid,
        ver = info.version,
        exe_name = exe_path.file_name().and_then(|f| f.to_str()).unwrap_or("agentpad-windows-x64.exe"),
        url = info.download_url
    );

    std::fs::write(&updater_script, script_content)
        .map_err(|e| format!("写入更新脚本失败: {e}"))?;

    // 启动外部脚本接管更新
    Command::new("cmd")
        .args(["/C", "start", "", updater_script.to_str().unwrap_or("agentpad_updater.bat")])
        .spawn()
        .map_err(|e| format!("启动更新脚本失败: {e}"))?;

    // 退出当前进程
    std::process::exit(0);
}

#[cfg(target_os = "macos")]
fn perform_update(info: &UpdateInfo) -> Result<(), String> {
    let tmp_dir = PathBuf::from("/tmp/agentpad_update");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    let _ = std::fs::create_dir_all(&tmp_dir);

    let is_zip = info.download_url.ends_with(".zip");
    if is_zip {
        let zip_path = tmp_dir.join("agentpad.zip");
        let curl_res = Command::new("curl")
            .args(["-L", "-f", "--max-time", "120", "-o", zip_path.to_str().unwrap(), &info.download_url])
            .status()
            .map_err(|e| format!("下载更新包失败: {e}"))?;

        if !curl_res.success() {
            return Err("下载更新包失败".into());
        }

        let unzip_res = Command::new("unzip")
            .args(["-q", zip_path.to_str().unwrap(), "-d", tmp_dir.to_str().unwrap()])
            .status()
            .map_err(|e| format!("解压更新包失败: {e}"))?;

        if !unzip_res.success() {
            return Err("解压更新包失败".into());
        }

        let new_app = tmp_dir.join("AgentPad.app");
        let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let is_in_apps = current_exe.to_string_lossy().starts_with("/Applications/AgentPad.app");

        if is_in_apps && new_app.exists() {
            let _ = Command::new("ditto")
                .args([new_app.to_str().unwrap(), "/Applications/AgentPad.app"])
                .status();
            let _ = Command::new("open")
                .args(["-n", "/Applications/AgentPad.app"])
                .spawn();
            std::process::exit(0);
        } else {
            // 打开解压目录让用户使用
            let _ = Command::new("open").args([tmp_dir.to_str().unwrap()]).spawn();
            std::process::exit(0);
        }
    } else {
        // 如果是 DMG
        let dmg_path = tmp_dir.join("agentpad.dmg");
        let curl_res = Command::new("curl")
            .args(["-L", "-f", "--max-time", "120", "-o", dmg_path.to_str().unwrap(), &info.download_url])
            .status()
            .map_err(|e| format!("下载 DMG 失败: {e}"))?;

        if !curl_res.success() {
            return Err("下载 DMG 失败".into());
        }

        let _ = Command::new("open").args([dmg_path.to_str().unwrap()]).spawn();
        std::process::exit(0);
    }
}

/// 启动时自检：清理上次遗留的专属更新脚本（防止误删无关文件）
pub fn cleanup_stale_updater_script() {
    #[cfg(target_os = "windows")]
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let script = exe_dir.join("agentpad_updater.bat");
            if script.exists() {
                if let Ok(content) = std::fs::read_to_string(&script) {
                    if content.contains("AgentPad Auto Updater") {
                        let _ = std::fs::remove_file(script);
                    }
                }
            }
        }
    }
}
