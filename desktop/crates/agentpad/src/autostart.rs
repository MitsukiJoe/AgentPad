use std::path::{Path, PathBuf};

const PREFERENCE_FILE: &str = "autostart.txt";

/// 同步用户 Startup 项。提权进程不碰用户目录，直接返回。
/// 管理员模式下开机启动由计划任务的登录触发负责，Startup 里的旧项要撤掉，免得重复启动。
pub fn apply() {
    if !available() || this_process_elevated() {
        return;
    }
    let want = !admin_mode() && read_preference(&preference_path());
    if sync_system(want).is_err() {
        crate::logutil::write("autostart apply failed");
    }
}

pub fn enabled() -> bool {
    #[cfg(windows)]
    if admin_mode() {
        return crate::elevation::autostart_enabled();
    }
    read_preference(&preference_path())
}

pub fn set_enabled(enabled: bool) -> std::io::Result<()> {
    #[cfg(windows)]
    if admin_mode() {
        // 只有提权实例能改计划任务；它不碰用户目录。
        return if crate::elevation::set_autostart(enabled) {
            Ok(())
        } else {
            Err(std::io::Error::other("admin autostart task update failed"))
        };
    }
    if this_process_elevated() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "autostart is unchanged while elevated",
        ));
    }
    if !available() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "autostart requires the packaged app",
        ));
    }
    sync_system(enabled)?;
    let path = preference_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, enabled.to_string())
}

fn this_process_elevated() -> bool {
    crate::elevation::is_elevated()
}

#[cfg(windows)]
fn admin_mode() -> bool {
    crate::elevation::enabled()
}

#[cfg(not(windows))]
fn admin_mode() -> bool {
    false
}

/// 偏好留在普通用户目录。提权进程只读这个布尔值，写入被 `edits_allowed` 拒绝。
fn preference_path() -> PathBuf {
    let dir = if this_process_elevated() {
        crate::identity::fixed_user_data_dir()
    } else {
        crate::identity::data_dir()
    };
    dir.join(PREFERENCE_FILE)
}

pub fn available() -> bool {
    running_from_app_bundle()
}

fn read_preference(path: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > 8 {
        return false;
    }
    std::fs::read_to_string(path).is_ok_and(|value| value.trim() == "true")
}

fn sync_entry(path: &Path, contents: &str, enabled: bool) -> std::io::Result<()> {
    if enabled {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, contents)
    } else {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

fn sync_system(enabled: bool) -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    #[cfg(target_os = "macos")]
    {
        let app = app_bundle_for_exe(&exe).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "autostart requires the packaged app",
            )
        })?;
        let path = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
            .join("Library/LaunchAgents/app.agentspads.plist");
        sync_entry(&path, &macos_plist(&app), enabled)
    }
    #[cfg(windows)]
    {
        let appdata = std::env::var_os("APPDATA").ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "APPDATA is unavailable")
        })?;
        let path = PathBuf::from(appdata)
            .join("Microsoft/Windows/Start Menu/Programs/Startup/AgentsPads.bat");
        sync_entry(&path, &windows_startup_bat(&exe), enabled)
    }
}

pub(crate) fn app_bundle_for_exe(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .find(|path| path.extension().is_some_and(|ext| ext == "app"))
        .map(Path::to_path_buf)
}

pub(crate) fn running_from_app_bundle() -> bool {
    #[cfg(target_os = "macos")]
    {
        std::env::current_exe()
            .ok()
            .and_then(|exe| app_bundle_for_exe(&exe))
            .is_some()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// 启动项只指向当前这个未提权进程自己的 exe，不再读用户目录里的启动器路径。
#[cfg(any(windows, test))]
fn windows_startup_bat(exe: &Path) -> String {
    format!("@echo off\r\nstart \"\" \"{}\"\r\n", exe.display())
}

fn macos_plist(app: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>app.agentspads</string>
  <key>ProgramArguments</key>
  <array>
    <string>/usr/bin/open</string>
    <string>-g</string>
    <string>{}</string>
  </array>
  <key>RunAtLoad</key><true/>
</dict>
</plist>
"#,
        app.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "agentpad-autostart-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ))
    }

    #[test]
    fn preference_defaults_off() {
        let path = temp_path("preference");
        assert!(!read_preference(&path));
        std::fs::write(&path, "true").unwrap();
        assert!(read_preference(&path));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn entry_file_follows_toggle() {
        let path = temp_path("entry");
        sync_entry(&path, "startup", true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "startup");
        sync_entry(&path, "startup", false).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn finds_macos_app_bundle_from_executable() {
        let exe = std::path::Path::new("/Applications/AgentsPads.app/Contents/MacOS/agentpad");
        assert_eq!(
            app_bundle_for_exe(exe),
            Some(std::path::PathBuf::from("/Applications/AgentsPads.app")),
        );
        assert_eq!(
            app_bundle_for_exe(std::path::Path::new("/tmp/agentpad")),
            None,
        );
    }

    #[test]
    fn macos_plist_opens_app_bundle() {
        let body = macos_plist(std::path::Path::new("/Applications/AgentsPads.app"));
        assert!(body.contains("<string>/usr/bin/open</string>"));
        assert!(body.contains("<string>-g</string>"));
        assert!(body.contains("<string>/Applications/AgentsPads.app</string>"));
    }

    #[test]
    fn windows_startup_bat_points_at_the_current_executable_only() {
        let bat = windows_startup_bat(Path::new(r"C:\Apps\AgentsPads\agentspads.exe"));
        assert_eq!(
            bat,
            "@echo off\r\nstart \"\" \"C:\\Apps\\AgentsPads\\agentspads.exe\"\r\n"
        );
    }
}
