use std::path::{Path, PathBuf};

const PREFERENCE_FILE: &str = "autostart.txt";

pub fn apply() {
    if !available() {
        return;
    }
    if let Err(e) = sync_system(enabled()) {
        crate::logutil::write(&format!("autostart: {e}"));
    }
}

pub fn enabled() -> bool {
    read_preference(&crate::identity::data_dir().join(PREFERENCE_FILE))
}

pub fn set_enabled(enabled: bool) -> std::io::Result<()> {
    if !available() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "autostart requires the packaged app",
        ));
    }
    sync_system(enabled)?;
    let path = crate::identity::data_dir().join(PREFERENCE_FILE);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, enabled.to_string())
}

pub fn available() -> bool {
    running_from_app_bundle()
}

fn read_preference(path: &Path) -> bool {
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
            .join("Library/LaunchAgents/app.agentpad.plist");
        sync_entry(&path, &macos_plist(&app), enabled)
    }
    #[cfg(windows)]
    {
        let appdata = std::env::var_os("APPDATA").ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "APPDATA is unavailable")
        })?;
        let path = PathBuf::from(appdata)
            .join("Microsoft/Windows/Start Menu/Programs/Startup/AgentPad.bat");
        let bat = format!("@echo off\r\nstart \"\" \"{}\"\r\n", exe.display());
        sync_entry(&path, &bat, enabled)
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

fn macos_plist(app: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>app.agentpad</string>
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
        let exe = std::path::Path::new("/Applications/AgentPad.app/Contents/MacOS/agentpad");
        assert_eq!(
            app_bundle_for_exe(exe),
            Some(std::path::PathBuf::from("/Applications/AgentPad.app")),
        );
        assert_eq!(
            app_bundle_for_exe(std::path::Path::new("/tmp/agentpad")),
            None,
        );
    }

    #[test]
    fn macos_plist_opens_app_bundle() {
        let body = macos_plist(std::path::Path::new("/Applications/AgentPad.app"));
        assert!(body.contains("<string>/usr/bin/open</string>"));
        assert!(body.contains("<string>-g</string>"));
        assert!(body.contains("<string>/Applications/AgentPad.app</string>"));
    }
}
