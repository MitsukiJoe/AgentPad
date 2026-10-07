use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub device_id: String,
    pub name: String,
    /// 长期配对密钥，随二维码给手机；重置后所有手机需重新配对。
    #[serde(default)]
    pub secret: String,
}

pub fn data_dir() -> PathBuf {
    if let Ok(p) = std::env::var("AGENTPAD_DATA_DIR") {
        return PathBuf::from(p);
    }
    #[cfg(target_os = "macos")]
    {
        home().join("Library/Application Support/AgentsPads")
    }
    #[cfg(windows)]
    {
        PathBuf::from(std::env::var("APPDATA").unwrap_or_else(|_| ".".into())).join("AgentsPads")
    }
}

pub fn log_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home().join("Library/Logs/AgentsPads")
    }
    #[cfg(windows)]
    {
        data_dir().join("logs")
    }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
}

/// 管理员实例的密钥不能放在普通进程可读的用户目录，否则同用户的普通程序读到密钥后
/// 就能借管理员实例向高权限窗口注入输入。所以按进程实际是否提权选存放位置：
/// 提权时只读写 `elevation::admin_identity_path()`（仅 Administrators/SYSTEM 可访问），
/// 失败即报错，绝不回退到普通实例的 identity.json。
#[cfg(windows)]
fn elevated() -> bool {
    crate::elevation::is_elevated()
}

pub fn load() -> std::io::Result<Identity> {
    #[cfg(windows)]
    if elevated() {
        return load_admin();
    }
    Ok(load_normal())
}

pub fn save(id: &Identity) -> std::io::Result<()> {
    #[cfg(windows)]
    if elevated() {
        return write_json(&crate::elevation::admin_identity_path()?, id);
    }
    write_json(&data_dir().join("identity.json"), id)
}

/// 只读，不写盘（管理员实例不能往用户可写目录里写文件）。
fn read_normal() -> Option<Identity> {
    let s = std::fs::read_to_string(data_dir().join("identity.json")).ok()?;
    serde_json::from_str::<Identity>(&s)
        .ok()
        .filter(|id| !id.device_id.is_empty())
}

fn load_normal() -> Identity {
    if let Some(mut id) = read_normal() {
        if id.secret.is_empty() {
            id.secret = crate::pairing::random_hex(32);
            let _ = save(&id);
        }
        return id;
    }
    let id = seed(None);
    let _ = save(&id);
    id
}

/// 保留普通实例的 device_id 与展示名（手机端仍是同一台电脑），只换新密钥。
fn seed(base: Option<Identity>) -> Identity {
    let base = base.unwrap_or_else(|| Identity {
        device_id: uuid::Uuid::new_v4().to_string(),
        name: whoami::fallible::hostname().unwrap_or_else(|_| "AgentsPads".into()),
        secret: String::new(),
    });
    Identity {
        secret: crate::pairing::random_hex(32),
        ..base
    }
}

#[cfg(windows)]
fn load_admin() -> std::io::Result<Identity> {
    let path = crate::elevation::admin_identity_path()?;
    let saved = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<Identity>(&s).ok())
        .filter(|id| !id.device_id.is_empty() && id.secret.len() == 64);
    if let Some(id) = saved {
        return Ok(id);
    }
    // 首次进入管理员模式：新密钥，手机需重新扫码。
    let id = seed(read_normal());
    write_json(&path, &id)?;
    Ok(id)
}

fn write_json(path: &std::path::Path, id: &Identity) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(id).unwrap())
}

pub fn load_theme() -> String {
    std::fs::read_to_string(data_dir().join("theme.txt"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| s == "light" || s == "dark" || s == "system")
        .unwrap_or_else(|| "system".into())
}

pub fn save_theme(theme: &str) {
    let _ = std::fs::create_dir_all(data_dir());
    let _ = std::fs::write(data_dir().join("theme.txt"), theme);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_device_id() {
        let dir = std::env::temp_dir().join(format!("agentpad-id-{}", uuid::Uuid::new_v4()));
        std::env::set_var("AGENTPAD_DATA_DIR", &dir);
        let a = load().unwrap();
        let b = load().unwrap();
        assert_eq!(a.device_id, b.device_id);
        assert!(!a.device_id.is_empty());
        assert_eq!(a.secret, b.secret);
        assert_eq!(a.secret.len(), 64);
        save_theme("dark");
        assert_eq!(load_theme(), "dark");
        save_theme("nope");
        assert_eq!(load_theme(), "system");
        let _ = std::fs::remove_dir_all(dir);
        std::env::remove_var("AGENTPAD_DATA_DIR");
    }

    #[test]
    fn admin_seed_keeps_device_id_but_not_secret() {
        let normal = Identity {
            device_id: "11111111-2222-4333-8444-555555555555".into(),
            name: "Desk".into(),
            secret: "a".repeat(64),
        };
        let admin = seed(Some(normal.clone()));
        assert_eq!(admin.device_id, normal.device_id);
        assert_eq!(admin.name, normal.name);
        assert_eq!(admin.secret.len(), 64);
        assert_ne!(admin.secret, normal.secret);
    }
}
