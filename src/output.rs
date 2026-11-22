//! 輸出檔命名與提交：決定輸出路徑、記錄同批次已分配的名稱，並以不覆寫的方式落檔。
//!
//! 規則：
//! - 序號名稱「檔名 (n).docx」在所有模式下都會避開磁碟上已存在的檔案與同批次已分配的名稱。
//! - 只有「覆寫」模式、且目標是原始檔名時，才允許取代既有檔案。
//! - 其他情況一律以不取代既有檔的方式提交；提交時才發現撞名，略過模式就略過，其餘取下一個序號重試。

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// 輸出檔已存在時的處理方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conflict {
    /// 自動加序號「檔名 (1).docx」
    Rename,
    Overwrite,
    Skip,
}

/// 規劃好的輸出目標
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub path: PathBuf,
    dir: PathBuf,
    stem: String,
    conflict: Conflict,
    /// 允許取代既有檔案（僅覆寫模式且為原始檔名）
    replace: bool,
}

impl Target {
    /// 直接指定輸出路徑（命令列工具用）
    pub fn exact(path: &Path, conflict: Conflict) -> Target {
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "output".into());
        Target { path: path.to_path_buf(), dir, stem, conflict, replace: conflict == Conflict::Overwrite }
    }
}

/// 同一批次已分配的輸出路徑（不分大小寫）；可跨執行緒共用
#[derive(Default)]
pub struct Reservations {
    set: Mutex<HashSet<String>>,
}

fn key(p: &Path) -> String {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    abs.to_string_lossy().to_lowercase().replace('/', "\\")
}

impl Reservations {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashSet<String>> {
        self.set.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 取得第一個未被預留且不存在的序號名稱並預留
    fn next_numbered(&self, dir: &Path, stem: &str, exists: &dyn Fn(&Path) -> bool) -> PathBuf {
        let mut set = self.lock();
        let mut n = 1u32;
        loop {
            let c = dir.join(format!("{stem} ({n}).docx"));
            if !set.contains(&key(&c)) && !exists(&c) {
                set.insert(key(&c));
                return c;
            }
            n += 1;
        }
    }
}

/// 依輸入檔與設定決定每個輸出目標；`Err(既有檔)` 表示略過。
/// 同一批次內不同來源若對到同一檔名，後者改用序號，避免互相覆寫。
pub fn plan_outputs(
    inputs: &[PathBuf],
    out_dir: Option<&Path>,
    conflict: Conflict,
    res: &Reservations,
    exists: impl Fn(&Path) -> bool,
) -> Vec<Result<Target, PathBuf>> {
    inputs
        .iter()
        .map(|input| {
            let dir = out_dir.map(Path::to_path_buf).unwrap_or_else(|| input.parent().unwrap_or(Path::new(".")).to_path_buf());
            let stem = input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "output".into());
            let base = dir.join(format!("{stem}.docx"));
            let base_exists = exists(&base);
            if conflict == Conflict::Skip && base_exists {
                return Err(base);
            }
            let base_free = {
                let mut set = res.lock();
                let free = !set.contains(&key(&base)) && (conflict == Conflict::Overwrite || !base_exists);
                if free {
                    set.insert(key(&base));
                }
                free
            };
            let (path, replace) = if base_free {
                (base, conflict == Conflict::Overwrite)
            } else {
                (res.next_numbered(&dir, &stem, &exists), false)
            };
            Ok(Target { path, dir, stem, conflict, replace })
        })
        .collect()
}

/// 提交結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Committed {
    Saved(PathBuf),
    /// 略過模式下提交時才發現同名檔已存在
    Skipped(PathBuf),
}

/// 同資料夾的暫存檔；未提交就被丟棄時自動刪除，避免留下殘檔
pub struct TempFile {
    path: Option<PathBuf>,
}

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

impl TempFile {
    /// 在 `output` 同一資料夾建立新的暫存檔（不會覆用既有檔名）
    pub fn create_near(output: &Path) -> io::Result<(TempFile, fs::File)> {
        Self::create_with_suffix(output, "tmp.docx")
    }

    pub(crate) fn create_with_suffix(output: &Path, suffix: &str) -> io::Result<(Self, fs::File)> {
        let name = output
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        loop {
            let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
            let path =
                output.with_file_name(format!(".{name}.{}-{seq}.{suffix}", std::process::id()));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(f) => return Ok((TempFile { path: Some(path) }, f)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
    }

    pub fn path(&self) -> &Path {
        self.path.as_deref().expect("暫存檔已提交")
    }

    pub(crate) fn replace(self, dest: &Path) -> io::Result<()> {
        fs::rename(self.path(), dest)?;
        self.disarm();
        Ok(())
    }

    fn disarm(mut self) {
        self.path = None;
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if let Some(p) = self.path.take() {
            let _ = fs::remove_file(p);
        }
    }
}

/// 把 `from` 搬到 `to`；`to` 已存在時失敗（ErrorKind::AlreadyExists），不會取代既有檔案。
/// 同一磁碟區內為原子操作。
#[cfg(windows)]
pub fn move_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};
    let wide = |p: &Path| {
        let mut units: Vec<u16> = p.as_os_str().encode_wide().collect();
        if units.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "檔案路徑含有 NUL 字元",
            ));
        }
        units.push(0);
        Ok(units)
    };
    let (f, t) = (wide(from)?, wide(to)?);
    // 不帶 MOVEFILE_REPLACE_EXISTING：目標存在時回傳 ERROR_ALREADY_EXISTS／ERROR_FILE_EXISTS
    // SAFETY: 兩個指標各自指向有效的 Vec<u16>，以單一 NUL 結尾。
    // Vec 在同步呼叫返回前皆存活且不會被更動，Win32 只讀取路徑且不保留指標。
    let ok = unsafe { MoveFileExW(f.as_ptr(), t.as_ptr(), MOVEFILE_WRITE_THROUGH) };
    if ok == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

/// 非 Windows：hard link 在目標存在時會失敗，成功後再刪除來源
#[cfg(not(windows))]
pub fn move_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    fs::hard_link(from, to)?;
    let _ = fs::remove_file(from);
    Ok(())
}

/// 提交暫存檔到目標。`exists` 用來挑選重試用的序號名稱。
pub fn commit(tmp: TempFile, target: &Target, res: &Reservations, exists: impl Fn(&Path) -> bool) -> io::Result<Committed> {
    if target.replace {
        // 使用者明確選擇覆寫
        tmp.replace(&target.path)?;
        return Ok(Committed::Saved(target.path.clone()));
    }
    let mut dest = target.path.clone();
    // 每次撞名都換新序號；設上限避免異常環境下無限重試
    for _ in 0..1000 {
        match move_no_replace(tmp.path(), &dest) {
            Ok(()) => {
                tmp.disarm();
                return Ok(Committed::Saved(dest));
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                if target.conflict == Conflict::Skip {
                    // tmp 在此被丟棄並刪除
                    return Ok(Committed::Skipped(dest));
                }
                dest = res.next_numbered(&target.dir, &target.stem, &exists);
            }
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("找不到可用的輸出檔名"))
}

/// 以 symlink_metadata 判斷，失效的捷徑／連結也算已存在
pub fn path_exists(p: &Path) -> bool {
    p.symlink_metadata().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[cfg(windows)]
    #[test]
    fn move_rejects_path_with_embedded_nul() {
        let source = p("來源.docx");
        let destination = p("輸出\0.docx");
        let err = move_no_replace(&source, &destination).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    fn paths(v: &[Result<Target, PathBuf>]) -> Vec<Result<PathBuf, PathBuf>> {
        v.iter().map(|r| r.as_ref().map(|t| t.path.clone()).map_err(Clone::clone)).collect()
    }

    fn plan(ins: &[PathBuf], dir: &str, c: Conflict, exists: impl Fn(&Path) -> bool) -> Vec<Result<PathBuf, PathBuf>> {
        paths(&plan_outputs(ins, Some(Path::new(dir)), c, &Reservations::default(), exists))
    }

    #[test]
    fn same_folder_by_default() {
        let out = plan_outputs(&[p("C:/a/報告.pdf")], None, Conflict::Rename, &Reservations::default(), |_| false);
        assert_eq!(out[0].as_ref().unwrap().path, p("C:/a/報告.docx"));
    }

    #[test]
    fn rename_when_exists_and_within_batch() {
        // 模擬 Windows 不分大小寫的檔案系統：D:/out/x.docx 已存在
        let exists = |q: &Path| q.to_string_lossy().to_lowercase().replace('\\', "/") == "d:/out/x.docx";
        let out = plan(&[p("C:/a/x.pdf"), p("C:/b/X.pdf")], "D:/out", Conflict::Rename, exists);
        assert_eq!(out, vec![Ok(p("D:/out/x (1).docx")), Ok(p("D:/out/X (2).docx"))]);
    }

    #[test]
    fn overwrite_and_skip_when_base_exists() {
        let exists = |q: &Path| q.ends_with("x.docx");
        let ins = [p("C:/a/x.pdf"), p("C:/b/x.pdf")];
        // 覆寫只取代原始檔名；同批次第二個改用序號
        assert_eq!(plan(&ins, "D:/o", Conflict::Overwrite, exists), vec![Ok(p("D:/o/x.docx")), Ok(p("D:/o/x (1).docx"))]);
        assert_eq!(plan(&ins, "D:/o", Conflict::Skip, exists), vec![Err(p("D:/o/x.docx")), Err(p("D:/o/x.docx"))]);
    }

    /// 情境一：同批兩個同名 PDF，磁碟上都沒有輸出檔
    #[test]
    fn batch_duplicates_all_modes() {
        let ins = [p("C:/a/x.pdf"), p("C:/b/x.pdf")];
        for c in [Conflict::Rename, Conflict::Overwrite, Conflict::Skip] {
            assert_eq!(plan(&ins, "D:/o", c, |_| false), vec![Ok(p("D:/o/x.docx")), Ok(p("D:/o/x (1).docx"))], "{c:?}");
        }
    }

    /// 情境二：x (1).docx 已存在；同批第二個 x.pdf 不可選中它（審查報告 P1）
    #[test]
    fn existing_numbered_name_avoided_all_modes() {
        let ins = [p("C:/a/x.pdf"), p("C:/b/x.pdf")];
        let exists = |q: &Path| q.ends_with("x (1).docx");
        for c in [Conflict::Rename, Conflict::Overwrite, Conflict::Skip] {
            assert_eq!(plan(&ins, "D:/o", c, exists), vec![Ok(p("D:/o/x.docx")), Ok(p("D:/o/x (2).docx"))], "{c:?}");
        }
    }

    // ---- 實際檔案系統 ----

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pdfword-output-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_tmp(output: &Path, body: &str) -> TempFile {
        let (t, mut f) = TempFile::create_near(output).unwrap();
        io::Write::write_all(&mut f, body.as_bytes()).unwrap();
        t
    }

    fn leftovers(dir: &Path) -> usize {
        fs::read_dir(dir).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().ends_with(".tmp.docx")).count()
    }

    fn read(p: &Path) -> String {
        fs::read_to_string(p).unwrap()
    }

    /// 情境二（實檔）：x (1).docx 已存在，三種模式轉完兩個 x.pdf 後原檔內容不變
    #[test]
    fn existing_numbered_file_untouched_on_disk() {
        for c in [Conflict::Rename, Conflict::Overwrite, Conflict::Skip] {
            let d = temp_dir(&format!("numbered-{c:?}"));
            fs::write(d.join("x (1).docx"), "原有").unwrap();
            let res = Reservations::default();
            let ins = [p("C:/a/x.pdf"), p("C:/b/x.pdf")];
            let plan = plan_outputs(&ins, Some(&d), c, &res, path_exists);
            for (i, t) in plan.iter().enumerate() {
                let t = t.as_ref().unwrap();
                let got = commit(write_tmp(&t.path, &format!("新{i}")), t, &res, path_exists).unwrap();
                assert!(matches!(got, Committed::Saved(_)), "{c:?}: {got:?}");
            }
            assert_eq!(read(&d.join("x (1).docx")), "原有", "{c:?}");
            assert_eq!(read(&d.join("x.docx")), "新0", "{c:?}");
            assert_eq!(read(&d.join("x (2).docx")), "新1", "{c:?}");
            assert_eq!(leftovers(&d), 0);
            let _ = fs::remove_dir_all(&d);
        }
    }

    /// 情境三：規劃時目標不存在，提交前才被別的程式建立同名檔
    #[test]
    fn collision_created_before_commit() {
        for c in [Conflict::Rename, Conflict::Overwrite, Conflict::Skip] {
            let d = temp_dir(&format!("race-{c:?}"));
            let res = Reservations::default();
            let t = plan_outputs(&[p("C:/a/x.pdf")], Some(&d), c, &res, path_exists).remove(0).unwrap();
            let tmp = write_tmp(&t.path, "新");
            fs::write(d.join("x.docx"), "別人的").unwrap();
            let got = commit(tmp, &t, &res, path_exists).unwrap();
            match c {
                Conflict::Rename => {
                    assert_eq!(got, Committed::Saved(d.join("x (1).docx")));
                    assert_eq!(read(&d.join("x.docx")), "別人的");
                    assert_eq!(read(&d.join("x (1).docx")), "新");
                }
                Conflict::Skip => {
                    assert_eq!(got, Committed::Skipped(d.join("x.docx")));
                    assert_eq!(read(&d.join("x.docx")), "別人的");
                    assert!(!d.join("x (1).docx").exists());
                }
                Conflict::Overwrite => {
                    // 使用者明確選了覆寫
                    assert_eq!(got, Committed::Saved(d.join("x.docx")));
                    assert_eq!(read(&d.join("x.docx")), "新");
                }
            }
            assert_eq!(leftovers(&d), 0, "{c:?} 留下暫存檔");
            let _ = fs::remove_dir_all(&d);
        }
    }

    /// 覆寫模式下的序號名稱（同批次重名）提交時撞名也不可取代
    #[test]
    fn overwrite_numbered_never_replaces() {
        let d = temp_dir("ow-numbered");
        let res = Reservations::default();
        let plan = plan_outputs(&[p("C:/a/x.pdf"), p("C:/b/x.pdf")], Some(&d), Conflict::Overwrite, &res, path_exists);
        let t = plan[1].as_ref().unwrap();
        let tmp = write_tmp(&t.path, "新");
        fs::write(d.join("x (1).docx"), "別人的").unwrap();
        assert_eq!(commit(tmp, t, &res, path_exists).unwrap(), Committed::Saved(d.join("x (2).docx")));
        assert_eq!(read(&d.join("x (1).docx")), "別人的");
        assert_eq!(leftovers(&d), 0);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn dropped_temp_is_removed() {
        let d = temp_dir("drop");
        drop(write_tmp(&d.join("x.docx"), "半成品"));
        assert_eq!(leftovers(&d), 0);
        let _ = fs::remove_dir_all(&d);
    }
}
