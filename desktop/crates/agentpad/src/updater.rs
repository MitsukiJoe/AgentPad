use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(any(target_os = "windows", test))]
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const CURRENT_VERSION: &str = match option_env!("AGENTPAD_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};
pub const GITHUB_REPO: &str = "MitsukiJoe/AgentPad";
#[cfg(any(target_os = "windows", test))]
const AFTER_UPDATE_FLAG: &str = "--after-update";
#[cfg(target_os = "windows")]
const POST_UPDATE_ENV: &str = "AGENTPAD_POST_UPDATE";

#[derive(Clone, Debug, PartialEq)]
pub struct UpdateInfo {
    pub tag_name: String,
    pub version: String,
    pub body: String,
    pub download_url: String,
    pub sha256: String,
}

#[derive(Debug, Deserialize)]
struct UpdateManifest {
    schema: u32,
    tag_name: String,
    version: String,
    release_url: String,
    #[serde(default)]
    body: String,
    assets: ManifestAssets,
}

#[derive(Debug, Deserialize)]
struct ManifestAssets {
    #[cfg(target_os = "windows")]
    windows: ManifestAsset,
    #[cfg(target_os = "macos")]
    macos: ManifestAsset,
}

#[derive(Clone, Debug, Deserialize)]
struct ManifestAsset {
    name: String,
    url: String,
    sha256: String,
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

fn manifest_urls() -> [String; 3] {
    let cache_hour = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 3600;
    let path = format!("gh/{GITHUB_REPO}@update-manifest/agentpad-update.json");
    [
        format!(
            "https://github.com/{GITHUB_REPO}/releases/latest/download/agentpad-update.json"
        ),
        format!("https://cdn.jsdelivr.net/{path}?hour={cache_hour}"),
        format!("https://cdn.jsdmirror.com/{path}?hour={cache_hour}"),
    ]
}

fn fetch_manifest(agent: &ureq::Agent, url: &str) -> Result<UpdateManifest, String> {
    let response = agent
        .get(url)
        .set("User-Agent", "AgentPad-Desktop")
        .call()
        .map_err(|e| format!("{url}: {e}"))?;
    let mut json_text = String::new();
    response
        .into_reader()
        .take(512 * 1024)
        .read_to_string(&mut json_text)
        .map_err(|e| format!("读取更新清单失败 ({url}): {e}"))?;
    serde_json::from_str(&json_text).map_err(|e| format!("解析更新清单失败 ({url}): {e}"))
}

fn expected_asset_url(tag_name: &str, asset_name: &str) -> String {
    format!(
        "https://github.com/{GITHUB_REPO}/releases/download/{tag_name}/{asset_name}"
    )
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn fetch_latest_release() -> Result<Option<UpdateInfo>, String> {
    let agent = https_agent(Duration::from_secs(15))?;
    let mut errors = Vec::new();
    let mut found_current = false;
    for (index, url) in manifest_urls().iter().enumerate() {
        let manifest = match fetch_manifest(&agent, url) {
            Ok(manifest) => manifest,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        if manifest.schema != 1
            || manifest.tag_name != format!("v{}", manifest.version)
            || manifest.version.is_empty()
        {
            errors.push(format!("更新清单字段无效 ({url})"));
            continue;
        }
        if !is_newer_version(&manifest.version, CURRENT_VERSION) {
            found_current = true;
            if index == 0 {
                return Ok(None);
            }
            continue;
        }
        #[cfg(target_os = "windows")]
        let asset = manifest.assets.windows;
        #[cfg(target_os = "macos")]
        let asset = manifest.assets.macos;
        let expected_name = if cfg!(target_os = "windows") {
            "agentpad-windows-x64.exe"
        } else {
            "agentpad-macos-arm64.zip"
        };
        let expected_url = expected_asset_url(&manifest.tag_name, expected_name);
        if asset.name != expected_name
            || asset.url != expected_url
            || !valid_sha256(&asset.sha256)
        {
            errors.push(format!("更新清单安装包字段无效 ({url})"));
            continue;
        }
        let body = if manifest.body.trim().is_empty() {
            format!("新版本已发布：{}", manifest.release_url)
        } else {
            manifest.body
        };
        return Ok(Some(UpdateInfo {
            tag_name: manifest.tag_name,
            version: manifest.version,
            body,
            download_url: asset.url,
            sha256: asset.sha256.to_ascii_lowercase(),
        }));
    }
    if found_current {
        Ok(None)
    } else {
        Err(format!("更新清单获取失败：{}", errors.join(" | ")))
    }
}

#[cfg(any(target_os = "windows", test))]
fn old_executable_path(exe: &Path) -> PathBuf {
    let parent = exe.parent().unwrap_or_else(|| Path::new(""));
    let name = exe
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    parent.join(format!("{name}.old"))
}

#[cfg(any(target_os = "windows", test))]
fn after_update_parent_pid(args: &[std::ffi::OsString]) -> Option<u32> {
    args.windows(2).find_map(|pair| {
        (pair[0] == AFTER_UPDATE_FLAG)
            .then(|| pair[1].to_string_lossy().parse().ok())
            .flatten()
    })
}

#[cfg(any(target_os = "windows", test))]
fn is_post_update_launch_for(exe: &Path, env_flag: bool, args: &[std::ffi::OsString]) -> bool {
    env_flag || after_update_parent_pid(args).is_some() || old_executable_path(exe).exists()
}

#[cfg(any(target_os = "windows", test))]
fn configure_relaunch(command: &mut Command, parent_pid: u32) {
    command
        .arg(AFTER_UPDATE_FLAG)
        .arg(parent_pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

pub fn is_post_update_launch() -> bool {
    #[cfg(target_os = "windows")]
    {
        let env_flag = std::env::var_os(POST_UPDATE_ENV).is_some();
        let args: Vec<_> = std::env::args_os().collect();
        return std::env::current_exe()
            .ok()
            .is_some_and(|exe| is_post_update_launch_for(&exe, env_flag, &args));
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
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

fn https_agent(timeout: Duration) -> Result<ureq::Agent, String> {
    let tls = ureq::native_tls::TlsConnector::new()
        .map_err(|e| format!("初始化系统 TLS 失败: {e}"))?;
    Ok(ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout(timeout)
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

fn download_with_progress(
    url: &str,
    dest: &Path,
    expected_sha256: &str,
    on_progress: &dyn Fn(u64, u64),
) -> Result<(), String> {
    let agent = https_agent(Duration::from_secs(600))?;
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
    let mut hasher = Sha256::new();
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("下载中断: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("写入更新文件失败: {e}"))?;
        hasher.update(&buf[..n]);
        done += n as u64;
        on_progress(done, total);
    }
    file.flush()
        .map_err(|e| format!("写入更新文件失败: {e}"))?;
    drop(file);
    if total > 0 && done != total {
        let _ = std::fs::remove_file(dest);
        return Err(format!("下载不完整（{done}/{total} 字节）"));
    }
    let actual_sha256 = format!("{:x}", hasher.finalize());
    if !actual_sha256.eq_ignore_ascii_case(expected_sha256) {
        let _ = std::fs::remove_file(dest);
        return Err(format!(
            "更新文件校验失败（期望 {expected_sha256}，实际 {actual_sha256}）"
        ));
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
    download_with_progress(&info.download_url, &tmp_file, &info.sha256, &report)?;

    *status.lock().unwrap() = UpdateStatus::Updating("正在替换程序文件...".into());
    let old_path = old_executable_path(&exe_path);
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
    let mut relaunch = Command::new(&exe_path);
    relaunch.env(POST_UPDATE_ENV, "1");
    configure_relaunch(&mut relaunch, std::process::id());
    relaunch
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
    download_with_progress(&info.download_url, &zip_path, &info.sha256, &report)?;

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
            let _ = std::fs::remove_file(old_executable_path(&exe_path));
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
    fn parses_update_manifest_and_validates_hashes() {
        let manifest: UpdateManifest = serde_json::from_str(
            r#"{
              "schema": 1,
              "tag_name": "v1.2.3",
              "version": "1.2.3",
              "release_url": "https://example.com/v1.2.3",
              "body": "notes",
              "assets": {
                "windows": {"name":"agentpad-windows-x64.exe","url":"https://example.com/a.exe","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
                "macos": {"name":"agentpad-macos-arm64.zip","url":"https://example.com/a.zip","sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},
                "android": {"name":"agentpad.apk","url":"https://example.com/a.apk","sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"}
              }
            }"#,
        )
        .unwrap();
        assert_eq!(manifest.schema, 1);
        assert_eq!(manifest.version, "1.2.3");
        assert!(valid_sha256(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ));
        assert!(!valid_sha256("not-a-sha"));
        let urls = manifest_urls();
        assert!(urls[0].contains("releases/latest/download"));
        assert!(urls[1].contains("cdn.jsdelivr.net"));
        assert!(urls[2].contains("cdn.jsdmirror.com"));
        assert_eq!(
            expected_asset_url("v1.2.3", "agentpad-macos-arm64.zip"),
            "https://github.com/MitsukiJoe/AgentPad/releases/download/v1.2.3/agentpad-macos-arm64.zip"
        );
    }

    #[test]
    fn progress_message_formats() {
        assert_eq!(download_progress_msg(50, 200), "正在下载更新... 25%");
        assert!(download_progress_msg(1, 0).starts_with("正在下载更新..."));
    }

    #[test]
    fn detects_post_update_from_flag_env_or_old_executable() {
        let dir = std::env::temp_dir().join(format!("agentpad-relaunch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("agentpad.exe");
        let old = old_executable_path(&exe);
        let no_args = vec![std::ffi::OsString::from("agentpad.exe")];
        let update_args = vec![
            std::ffi::OsString::from("agentpad.exe"),
            std::ffi::OsString::from(AFTER_UPDATE_FLAG),
            std::ffi::OsString::from("123"),
        ];
        assert_eq!(old, dir.join("agentpad.exe.old"));
        assert_eq!(after_update_parent_pid(&update_args), Some(123));
        assert_eq!(after_update_parent_pid(&no_args), None);
        assert!(!is_post_update_launch_for(&exe, false, &no_args));
        assert!(is_post_update_launch_for(&exe, false, &update_args));
        assert!(is_post_update_launch_for(&exe, true, &no_args));
        std::fs::write(&old, b"old").unwrap();
        assert!(is_post_update_launch_for(&exe, false, &no_args));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn relaunch_command_contains_parent_pid() {
        let mut command = Command::new("agentpad.exe");
        configure_relaunch(&mut command, 456);
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        assert_eq!(args, [AFTER_UPDATE_FLAG, "456"]);
    }
}
