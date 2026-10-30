//! 使用者設定：存於 %APPDATA%\pdfword\settings.json

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// 上次加入檔案的資料夾
    pub last_input_dir: Option<PathBuf>,
    /// 輸出資料夾；None 表示與來源相同
    pub output_dir: Option<PathBuf>,
    /// 0 跟隨系統、1 淺色、2 深色
    pub theme: i32,
    /// 0 自動加序號、1 覆寫、2 略過
    pub conflict: i32,
    pub threads: i32,
}

impl Default for Settings {
    fn default() -> Self {
        let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        Settings { last_input_dir: None, output_dir: None, theme: 0, conflict: 0, threads: cpus.clamp(1, 4) as i32 }
    }
}

pub fn path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("pdfword").join("settings.json"))
}

impl Settings {
    /// 讀不到或格式錯誤時回傳預設值
    pub fn load() -> Self {
        path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(&tmp, &p);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_missing_fields() {
        let s = Settings { output_dir: Some(PathBuf::from("D:/輸出")), theme: 2, ..Default::default() };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(s, back);
        // 舊版設定檔缺欄位時用預設值
        let partial: Settings = serde_json::from_str(r#"{"theme":1}"#).unwrap();
        assert_eq!(partial.theme, 1);
        assert_eq!(partial.conflict, 0);
    }
}
