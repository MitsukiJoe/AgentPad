use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub device_id: String,
    pub name: String,
}

pub fn data_dir() -> PathBuf {
    if let Ok(p) = std::env::var("AGENTPAD_DATA_DIR") {
        return PathBuf::from(p);
    }
    #[cfg(target_os = "macos")]
    {
        home().join("Library/Application Support/AgentPad")
    }
    #[cfg(windows)]
    {
        PathBuf::from(std::env::var("APPDATA").unwrap_or_else(|_| ".".into())).join("AgentPad")
    }
}

pub fn log_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home().join("Library/Logs/AgentPad")
    }
    #[cfg(windows)]
    {
        data_dir().join("logs")
    }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
}

pub fn load() -> Identity {
    let path = data_dir().join("identity.json");
    if let Ok(s) = std::fs::read_to_string(&path) {
        if let Ok(id) = serde_json::from_str::<Identity>(&s) {
            if !id.device_id.is_empty() {
                return id;
            }
        }
    }
    let id = Identity {
        device_id: uuid::Uuid::new_v4().to_string(),
        name: whoami::fallible::hostname().unwrap_or_else(|_| "AgentPad".into()),
    };
    let _ = save(&id);
    id
}

pub fn save(id: &Identity) -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir())?;
    std::fs::write(
        data_dir().join("identity.json"),
        serde_json::to_vec_pretty(id).unwrap(),
    )
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
        let a = load();
        let b = load();
        assert_eq!(a.device_id, b.device_id);
        assert!(!a.device_id.is_empty());
        save_theme("dark");
        assert_eq!(load_theme(), "dark");
        save_theme("nope");
        assert_eq!(load_theme(), "system");
        let _ = std::fs::remove_dir_all(dir);
        std::env::remove_var("AGENTPAD_DATA_DIR");
    }
}
