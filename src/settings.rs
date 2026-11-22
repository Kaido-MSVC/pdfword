//! 使用者設定：存於 %APPDATA%\pdfword\settings.json

use crate::output::TempFile;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

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

    /// 以同資料夾的暫存檔寫入設定，成功後才取代原檔。
    ///
    /// # Errors
    /// 找不到 APPDATA，或設定無法序列化、寫入、提交時傳回錯誤。
    pub fn save(&self) -> io::Result<()> {
        let path = path()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "找不到 APPDATA 環境變數"))?;
        self.save_to(&path)
    }

    fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        let (tmp, mut file) = TempFile::create_with_suffix(path, "tmp")?;
        let written = file.write_all(&bytes);
        drop(file);
        written?;
        tmp.replace(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_test_dir(case: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pdfword-settings-{case}-{}",
            std::process::id()
        ));
        fs::create_dir(&dir).expect("無法建立設定測試資料夾");
        dir
    }

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

    #[test]
    fn save_replaces_settings_and_cleans_up_after_failed_commit() {
        let dir = settings_test_dir("blocked-directory");
        let target = dir.join("settings.json");
        let mut settings = Settings::default();
        settings.save_to(&target).expect("首次儲存設定應成功");
        settings.theme = 2;
        settings.save_to(&target).expect("再次儲存應取代設定檔");
        let loaded: Settings = serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();
        assert_eq!(loaded, settings);

        let blocked = dir.join("blocked.json");
        fs::create_dir(&blocked).unwrap();
        assert!(settings.save_to(&blocked).is_err());
        assert!(blocked.is_dir());
        let mut entries: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort_unstable();
        assert_eq!(entries, [blocked, target], "不應留下設定暫存檔");

        // 交由遞迴刪除處理整棵測試目錄，避免在 Windows 逐項刪除後立刻移除父目錄。
        fs::remove_dir_all(&dir).expect("無法清理設定測試資料夾");
    }

    #[cfg(windows)]
    #[test]
    fn failed_settings_replacement_preserves_original_file() {
        use std::os::windows::fs::OpenOptionsExt;

        let dir = settings_test_dir("locked-file");
        let target = dir.join("settings.json");
        let mut settings = Settings::default();
        settings.save_to(&target).expect("首次儲存設定應成功");
        let original = fs::read(&target).unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&target)
            .expect("無法獨占開啟現有設定檔");

        settings.theme = 2;
        settings
            .save_to(&target)
            .expect_err("設定檔仍被獨占開啟時應無法取代");
        drop(lock);
        assert_eq!(fs::read(&target).unwrap(), original, "提交失敗應保留原設定");
        let entries: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(
            entries.as_slice(),
            std::slice::from_ref(&target),
            "提交失敗不應留下設定暫存檔"
        );

        settings.save_to(&target).expect("解除鎖定後應可儲存設定");
        let loaded: Settings = serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();
        assert_eq!(loaded, settings);
        fs::remove_dir_all(&dir).expect("無法清理設定測試資料夾");
    }
}
