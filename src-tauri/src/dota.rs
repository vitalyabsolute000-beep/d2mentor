use std::fs;
use std::path::{Path, PathBuf};

const CFG_FILE: &str = "gamestate_integration_d2mentor.cfg";

/// Finds the Dota 2 `gamestate_integration` folder by reading Steam's library config.
pub fn find_cfg_dir() -> Option<PathBuf> {
    let mut steam_roots: Vec<PathBuf> = Vec::new();

    if let Ok(key) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey("Software\\Valve\\Steam")
    {
        if let Ok(p) = key.get_value::<String, _>("SteamPath") {
            steam_roots.push(PathBuf::from(p.replace('/', "\\")));
        }
    }
    steam_roots.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
    steam_roots.push(PathBuf::from(r"C:\Program Files\Steam"));

    let mut libraries = steam_roots.clone();
    for root in &steam_roots {
        libraries.extend(read_library_folders(root));
    }

    for root in libraries {
        let dir = root
            .join("steamapps")
            .join("common")
            .join("dota 2 beta")
            .join("game")
            .join("dota")
            .join("cfg")
            .join("gamestate_integration");
        if dir.is_dir() {
            return Some(dir);
        }
    }

    None
}

fn read_library_folders(steam_root: &Path) -> Vec<PathBuf> {
    let vdf = steam_root.join("steamapps").join("libraryfolders.vdf");
    let Ok(text) = fs::read_to_string(vdf) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("\"path\"") {
            if let Some(value) = nth_quoted(line, 1) {
                out.push(PathBuf::from(value.replace("\\\\", "\\")));
            }
        }
    }
    out
}

fn nth_quoted(s: &str, n: usize) -> Option<String> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut count = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end] != b'"' {
                end += 1;
            }
            if end >= bytes.len() {
                return None;
            }
            if count == n {
                return Some(s[start..end].to_string());
            }
            count += 1;
            i = end + 1;
        } else {
            i += 1;
        }
    }
    None
}

pub fn cfg_path() -> Option<PathBuf> {
    find_cfg_dir().map(|dir| dir.join(CFG_FILE))
}

pub fn install_gsi(token: &str, port: u16) -> Result<PathBuf, String> {
    let dir = find_cfg_dir()
        .ok_or_else(|| "Не нашёл папку Dota 2 (gamestate_integration)".to_string())?;
    let path = dir.join(CFG_FILE);

    let cfg = format!(
        r#""D2Mentor"
{{
  "uri" "http://127.0.0.1:{port}/gsi"
  "timeout" "5.0"
  "buffer" "0.1"
  "throttle" "0.5"
  "heartbeat" "30.0"
  "auth"
  {{
    "token" "{token}"
  }}
  "data"
  {{
    "provider" "1"
    "map" "1"
    "player" "1"
    "hero" "1"
    "abilities" "1"
    "items" "1"
    "buildings" "1"
    "events" "1"
    "draft" "1"
    "neutralitems" "1"
    "roshan" "1"
    "couriers" "1"
    "minimap" "1"
  }}
}}
"#
    );

    fs::write(&path, cfg)
        .map_err(|e| format!("Не удалось записать {}: {e}", path.display()))?;
    Ok(path)
}

pub fn remove_gsi() -> Result<(), String> {
    let Some(path) = cfg_path() else {
        return Ok(());
    };
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|e| format!("Не удалось удалить {}: {e}", path.display()))?;
    }
    Ok(())
}
