use std::io::{Read, Write};
use std::path::Path;
#[cfg(target_os = "macos")]
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const CURRENT_VERSION: &str = match option_env!("AGENTPAD_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};
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
    UpdateFailed { info: Box<UpdateInfo>, message: String },
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
            if let Err(e) = perform_update(&info, &status_arc) {
                crate::logutil::write(&format!("update failed: {e}"));
                *status_arc.lock().unwrap() = UpdateStatus::UpdateFailed {
                    info: Box::new(info),
                    message: e,
                };
            }
        });
    }
}

fn fetch_latest_release() -> Result<Option<UpdateInfo>, String> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let agent = https_agent()?;
    let response = agent
        .get(&url)
        .set("User-Agent", "AgentPad-Desktop")
        .set("Accept", "application/vnd.github.v3+json")
        .call()
        .map_err(|e| format!("网络请求失败: {e}"))?;
    let mut json_text = String::new();
    response
        .into_reader()
        .take(2 * 1024 * 1024)
        .read_to_string(&mut json_text)
        .map_err(|e| format!("读取 GitHub 响应失败: {e}"))?;
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

    let expected_asset = if cfg!(target_os = "windows") {
        "agentpad-windows-x64.exe"
    } else {
        "agentpad-macos-arm64.zip"
    };

    let download_url = assets
        .iter()
        .filter_map(|a| a.get("name").and_then(|n| n.as_str()).map(|n| (n, a)))
        .find(|(name, _)| *name == expected_asset)
        .and_then(|(_, a)| {
            a.get("browser_download_url")
                .and_then(|u| u.as_str())
                .map(str::to_string)
        })
        .ok_or_else(|| format!("未找到适用的安装包 ({expected_asset})"))?;

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

fn https_agent() -> Result<ureq::Agent, String> {
    let tls = ureq::native_tls::TlsConnector::new()
        .map_err(|e| format!("初始化系统 TLS 失败: {e}"))?;
    Ok(ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout(Duration::from_secs(600))
        .tls_connector(Arc::new(tls))
        .build())
}

fn download_progress_msg(done: u64, total: u64) -> String {
    if total > 0 {
        format!("正在下载更新... {}%", done * 100 / total.max(1))
    } else {
        format!("正在下载更新... {:.1} MB", done as f64 / 1048576.0)
    }
}

fn download_with_progress(url: &str, dest: &Path, on_progress: &dyn Fn(u64, u64)) -> Result<(), String> {
    let agent = https_agent()?;
    let response = agent
        .get(url)
        .set("User-Agent", "AgentPad-Desktop")
        .call()
        .map_err(|e| format!("下载失败: {e}"))?;
    let total = response
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let mut reader = response.into_reader();
    let mut file = std::fs::File::create(dest)
        .map_err(|e| format!("创建更新文件失败（目录可能无写入权限）: {e}"))?;
    let mut buf = [0u8; 65536];
    let mut done: u64 = 0;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("下载中断: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("写入更新文件失败: {e}"))?;
        done += n as u64;
        on_progress(done, total);
    }
    if total > 0 && done != total {
        return Err(format!("下载不完整（{done}/{total} 字节）"));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn perform_update(info: &UpdateInfo, status: &Arc<Mutex<UpdateStatus>>) -> Result<(), String> {
    let exe_path = std::env::current_exe().map_err(|e| format!("获取当前路径失败: {e}"))?;
    let exe_dir = exe_path.parent().ok_or_else(|| "无法获取程序目录".to_string())?;

    let tmp_file = exe_dir.join("agentpad_update.new");
    let report = |done: u64, total: u64| {
        *status.lock().unwrap() = UpdateStatus::Updating(download_progress_msg(done, total));
    };
    download_with_progress(&info.download_url, &tmp_file, &report)?;

    *status.lock().unwrap() = UpdateStatus::Updating("正在替换程序文件...".into());
    let old_path = exe_dir.join(format!(
        "{}.old",
        exe_path
            .file_name()
            .map(|f| f.to_string_lossy())
            .unwrap_or_default()
    ));
    let _ = std::fs::remove_file(&old_path);

    // 运行中的 EXE 可以改名但不能删除：先让位，再落位新文件
    std::fs::rename(&exe_path, &old_path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_file);
        format!("无法移动旧程序文件（可能被安全软件占用或目录无写入权限）: {e}")
    })?;
    if let Err(e) = std::fs::rename(&tmp_file, &exe_path) {
        let _ = std::fs::rename(&old_path, &exe_path);
        return Err(format!("替换程序文件失败，已恢复旧版本: {e}"));
    }

    *status.lock().unwrap() = UpdateStatus::Updating("正在启动新版本...".into());
    Command::new(&exe_path)
        .spawn()
        .map_err(|e| format!("启动新版本失败: {e}"))?;
    std::process::exit(0);
}

#[cfg(target_os = "macos")]
fn find_app_bundle(exe: &Path) -> Option<PathBuf> {
    let mut dir = exe.parent()?;
    loop {
        if dir.extension().and_then(|e| e.to_str()) == Some("app") {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

#[cfg(target_os = "macos")]
fn perform_update(info: &UpdateInfo, status: &Arc<Mutex<UpdateStatus>>) -> Result<(), String> {
    let tmp_dir = PathBuf::from("/tmp/agentpad_update");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("创建临时目录失败: {e}"))?;

    let report = |done: u64, total: u64| {
        *status.lock().unwrap() = UpdateStatus::Updating(download_progress_msg(done, total));
    };
    let zip_path = tmp_dir.join("agentpad.zip");
    download_with_progress(&info.download_url, &zip_path, &report)?;

    *status.lock().unwrap() = UpdateStatus::Updating("正在解压更新...".into());
    let unzip_ok = Command::new("unzip")
        .args(["-q", "-o"])
        .arg(&zip_path)
        .arg("-d")
        .arg(&tmp_dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !unzip_ok {
        return Err("解压更新包失败".into());
    }
    let new_app = tmp_dir.join("AgentPad.app");
    if !new_app.exists() {
        return Err("更新包内未找到 AgentPad.app".into());
    }

    let current_exe = std::env::current_exe().map_err(|e| format!("获取当前路径失败: {e}"))?;
    let Some(bundle) = find_app_bundle(&current_exe) else {
        // 非 .app 运行（如 cargo 开发构建）：打开解压目录手动处理
        let _ = Command::new("open").arg(&tmp_dir).spawn();
        std::process::exit(0);
    };

    *status.lock().unwrap() = UpdateStatus::Updating("正在替换应用...".into());
    let parent = bundle
        .parent()
        .ok_or_else(|| "无法获取应用所在目录".to_string())?;
    let old_bundle = parent.join(format!(
        "{}.old",
        bundle
            .file_name()
            .map(|f| f.to_string_lossy())
            .unwrap_or_default()
    ));
    let _ = std::fs::remove_dir_all(&old_bundle);

    if std::fs::rename(&bundle, &old_bundle).is_ok() {
        if let Err(e) = std::fs::rename(&new_app, &bundle) {
            let _ = std::fs::rename(&old_bundle, &bundle);
            return Err(format!("替换应用失败，已保留原位置: {e}"));
        }
    } else {
        // rename 失败（跨卷/权限）：原地覆盖，运行中进程继续使用旧 inode
        let ditto_ok = Command::new("ditto")
            .args([&new_app, &bundle])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ditto_ok {
            return Err("替换应用失败：应用所在目录不可写".into());
        }
    }

    *status.lock().unwrap() = UpdateStatus::Updating("正在启动新版本...".into());
    Command::new("open")
        .arg("-n")
        .arg(&bundle)
        .spawn()
        .map_err(|e| format!("启动新版本失败: {e}"))?;
    std::process::exit(0);
}

/// 启动时自检：清理历史版本遗留的更新产物（旧脚本式更新残留、换名替换留下的 .old）
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
            let _ = std::fs::remove_file(exe_dir.join("agentpad_update.tmp"));
            let _ = std::fs::remove_file(exe_dir.join("agentpad_update.new"));
            if let Some(name) = exe_path.file_name() {
                let _ = std::fs::remove_file(exe_dir.join(format!("{}.old", name.to_string_lossy())));
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(bundle) = find_app_bundle(&exe_path) {
                if let (Some(parent), Some(name)) = (bundle.parent(), bundle.file_name()) {
                    let _ = std::fs::remove_dir_all(
                        parent.join(format!("{}.old", name.to_string_lossy())),
                    );
                }
            }
        }
        let _ = std::fs::remove_dir_all("/tmp/agentpad_update");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updater_starts_idle() {
        let updater = Updater::new();
        assert_eq!(*updater.status.lock().unwrap(), UpdateStatus::Idle);
    }

    #[test]
    fn compares_release_versions() {
        assert!(is_newer_version("0.1.1", "0.1.0"));
        assert!(!is_newer_version("0.1.0", "0.1.0"));
        assert!(!is_newer_version("0.0.9", "0.1.0"));
    }

    #[test]
    fn progress_message_formats() {
        assert_eq!(download_progress_msg(50, 200), "正在下载更新... 25%");
        assert!(download_progress_msg(1, 0).starts_with("正在下载更新..."));
    }
}
