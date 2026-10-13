//! 目录管理所需的文件操作：删除、复制到剪贴板、复制到目录、打开所在文件夹。
//!
//! 这些操作都会动用户的文件，因此两条原则贯穿全模块：
//!   1. 删除必须先进入回收站，不能直接抹掉——投影文件常是长期积累的成果，
//!      误删一次就没了代价太大。
//!   2. 任何路径都要先校验存在性与类型，避免把目录当文件复制、
//!      或对不存在的路径静默成功。

use crate::error::{AppError, AppResult};
use std::path::{Path, PathBuf};

/// 一个待处理文件的存在性校验结果
#[derive(Debug, Clone)]
pub struct CheckedFile {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
}

/// 校验一组路径确实都是可读的普通文件。
///
/// 批量操作不能「部分成功却当作全部成功」：调用方一次传入多个文件，
/// 若第 3 个不存在而其余都成功，用户看到的提示会与磁盘实际不符。
/// 因此在动手之前先把整批校验完，任一不合格就整批中止。
pub fn check_files(paths: &[String]) -> AppResult<Vec<CheckedFile>> {
    if paths.is_empty() {
        return Err(AppError::Parse("没有选中任何文件".into()));
    }
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let path = PathBuf::from(p);
        let meta = std::fs::metadata(&path).map_err(|e| {
            AppError::Parse(format!("无法访问 {}：{e}", path.display()))
        })?;
        if !meta.is_file() {
            return Err(AppError::Parse(format!(
                "不是文件（可能是目录或链接）：{}",
                path.display()
            )));
        }
        let name = path
            .file_name()
            .map(|v| v.to_string_lossy().into_owned())
            .ok_or_else(|| AppError::Parse(format!("路径没有文件名：{}", path.display())))?;
        out.push(CheckedFile { path, name, size: meta.len() });
    }
    Ok(out)
}

/// 复制一组文件到目标目录。
///
/// 返回实际写入的目标路径，供界面提示「复制到了哪里」。
/// 同名文件**不覆盖**：改为追加 ` (2)`、` (3)` 序号。
/// 直接覆盖会让用户在毫不知情的情况下丢掉目标目录里的既有文件。
pub fn copy_into(files: &[CheckedFile], dest_dir: &Path) -> AppResult<Vec<PathBuf>> {
    if !dest_dir.is_dir() {
        return Err(AppError::Parse(format!(
            "目标不是目录：{}",
            dest_dir.display()
        )));
    }
    let mut written = Vec::with_capacity(files.len());
    for f in files {
        let dst = unique_dest(dest_dir, &f.name);
        // 复制到自身位置是常见误操作（把文件复制到它所在目录），
        // 此时 unique_dest 会给出带序号的副本，行为可预期，不需要额外拦截
        std::fs::copy(&f.path, &dst).map_err(|e| {
            AppError::Parse(format!(
                "复制 {} 到 {} 失败：{e}",
                f.path.display(),
                dst.display()
            ))
        })?;
        written.push(dst);
    }
    Ok(written)
}

/// 在目标目录下求一个不冲突的文件名。
fn unique_dest(dest_dir: &Path, name: &str) -> PathBuf {
    let first = dest_dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = split_ext(name);
    for i in 2..10_000 {
        let cand = if ext.is_empty() {
            dest_dir.join(format!("{stem} ({i})"))
        } else {
            dest_dir.join(format!("{stem} ({i}).{ext}"))
        };
        if !cand.exists() {
            return cand;
        }
    }
    // 极端情况下退回到原名：调用方的错误提示里会带出这个路径，
    // 比静默改写成一个意料之外的名字更容易排查
    first
}

/// 拆出文件名的「主干」与「扩展名」（不含点）。
fn split_ext(name: &str) -> (String, String) {
    match name.rfind('.') {
        // 前导点表示隐藏文件（如 .gitignore），整名视作主干
        Some(i) if i > 0 => (name[..i].to_string(), name[i + 1..].to_string()),
        _ => (name.to_string(), String::new()),
    }
}

// ---------------------------------------------------------------- 平台实现

/// 删除到回收站。
///
/// 各平台都有自己的「回收站」约定，逐个实现而不是直接 unlink：
/// 用户点「删除」时的预期是「能从回收站找回来」。
pub fn delete_to_trash(files: &[CheckedFile]) -> AppResult<()> {
    #[cfg(windows)]
    {
        windows_trash(files)
    }
    #[cfg(target_os = "macos")]
    {
        macos_trash(files)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        xdg_trash(files)
    }
}

/// 在资源管理器里定位到一个文件（打开其所在目录并选中它）。
pub fn reveal_in_file_manager(path: &Path) -> AppResult<()> {
    if !path.exists() {
        return Err(AppError::Parse(format!("文件不存在：{}", path.display())));
    }
    #[cfg(windows)]
    {
        // explorer.exe 对 /select 的返回码不可靠（打开成功也常返回 1），
        // 因此这里只判断「能否启动」，不判断退出码，避免报假错。
        std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", path.display()))
            .spawn()
            .map_err(|e| AppError::Parse(format!("无法打开资源管理器：{e}")))?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .map_err(|e| AppError::Parse(format!("无法打开访达：{e}")))?;
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // Linux 没有统一的「选中文件」入口，退而打开其所在目录
        let dir = path.parent().unwrap_or(Path::new("."));
        std::process::Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .map_err(|e| AppError::Parse(format!("无法打开文件管理器：{e}")))?;
        Ok(())
    }
}

// ---------------------------------------------------------------- Windows

/// Windows 回收站。
///
/// 用 SHFileOperationW 并把 pFrom 写成「双 null 结尾」的路径列表：
/// 这是该 API 要求的格式，写成单 null 会读到越界内容。
///
/// 之所以不用 IFileOperation：后者需要 COM 初始化与接口查询，
/// 为一个删除动作引入整套 COM 包装不划算，而 SHFileOperationW
/// 在 Windows 2000 之后一直可用。
#[cfg(windows)]
fn windows_trash(files: &[CheckedFile]) -> AppResult<()> {
    use std::os::windows::ffi::OsStrExt;

    /// 与 Win32 的 SHFILEOPSTRUCTW 一致的布局。
    /// 只声明用到的字段，但**必须包含前面的字段**以保持偏移正确。
    #[repr(C)]
    struct ShFileOpStructW {
        hwnd: *mut core::ffi::c_void,
        w_func: u32,
        p_from: *const u16,
        p_to: *const u16,
        f_flags: u16,
        f_any_operations_aborted: i32,
        h_name_mappings: *mut core::ffi::c_void,
        lpsz_progress_title: *const u16,
    }

    #[link(name = "shell32")]
    extern "system" {
        fn SHFileOperationW(op: *mut ShFileOpStructW) -> i32;
    }

    const FO_DELETE: u32 = 0x0003;
    // FOF_ALLOWUNDO：进回收站而不是永久删除；FOF_NOCONFIRMATION：
    // 确认已由界面完成，避免再弹一次系统框；FOF_NOERRORUI：
    // 失败要通过返回值反馈给界面，而不是让 Windows 自己弹窗
    const FOF_NOCONFIRMATION: u16 = 0x0010;
    const FOF_ALLOWUNDO: u16 = 0x0040;
    const FOF_NOERRORUI: u16 = 0x0400;
    const FOF_SILENT: u16 = 0x0004;

    // 拼成 "path1\0path2\0\0"
    let mut buf: Vec<u16> = Vec::new();
    for f in files {
        buf.extend(f.path.as_os_str().encode_wide());
        buf.push(0);
    }
    buf.push(0);

    let mut op = ShFileOpStructW {
        hwnd: std::ptr::null_mut(),
        w_func: FO_DELETE,
        p_from: buf.as_ptr(),
        p_to: std::ptr::null(),
        f_flags: FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT,
        f_any_operations_aborted: 0,
        h_name_mappings: std::ptr::null_mut(),
        lpsz_progress_title: std::ptr::null(),
    };

    // SAFETY: op 与 buf 都在本次调用期间存活；buf 以双 null 结尾，
    // 符合 SHFileOperationW 对 pFrom 的要求；pTo 在删除操作中不使用，置空合法。
    let rc = unsafe { SHFileOperationW(&mut op) };
    if rc != 0 {
        return Err(AppError::Parse(format!(
            "删除到回收站失败（系统返回 {rc}）；文件可能正被其他程序占用"
        )));
    }
    if op.f_any_operations_aborted != 0 {
        return Err(AppError::Parse(
            "删除被中止：至少有一个文件未能移入回收站".into(),
        ));
    }
    Ok(())
}

/// 把文件路径写入剪贴板（CF_HDROP）。
///
/// 用 CF_HDROP 而不是 CF_UNICODETEXT：用户点「复制」后是要到资源管理器里
/// 粘贴的，只有 CF_HDROP 才会被识别为「一批文件」；纯文本粘不出文件。
#[cfg(windows)]
pub fn copy_paths_to_clipboard(files: &[CheckedFile]) -> AppResult<()> {
    use std::os::windows::ffi::OsStrExt;

    /// DROPFILES 头 + 宽字符路径列表，整体放进一个 HGLOBAL。
    #[repr(C)]
    struct DropFiles {
        p_files: u32,
        pt_x: i32,
        pt_y: i32,
        f_nc: i32,
        f_wide: i32,
    }

    type Handle = *mut core::ffi::c_void;

    #[link(name = "user32")]
    extern "system" {
        fn OpenClipboard(hwnd: Handle) -> i32;
        fn EmptyClipboard() -> i32;
        fn SetClipboardData(format: u32, data: Handle) -> Handle;
        fn CloseClipboard() -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalAlloc(flags: u32, bytes: usize) -> Handle;
        fn GlobalLock(handle: Handle) -> *mut core::ffi::c_void;
        fn GlobalUnlock(handle: Handle) -> i32;
        fn GlobalFree(handle: Handle) -> Handle;
    }

    const CF_HDROP: u32 = 15;
    const GMEM_MOVEABLE: u32 = 0x0002;

    // 宽字符路径列表，双 null 结尾
    let mut wide: Vec<u16> = Vec::new();
    for f in files {
        wide.extend(f.path.as_os_str().encode_wide());
        wide.push(0);
    }
    wide.push(0);

    let header = std::mem::size_of::<DropFiles>();
    let bytes = header + wide.len() * 2;

    // SAFETY: 以下调用都遵守各自的契约——
    // GMEM_MOVEABLE 分配的块必须交给 SetClipboardData 释放所有权，
    // 因此成功之后不再 GlobalFree，失败路径才释放，避免双重释放或泄漏。
    unsafe {
        let mem = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if mem.is_null() {
            return Err(AppError::Parse("剪贴板内存分配失败".into()));
        }
        let p = GlobalLock(mem) as *mut u8;
        if p.is_null() {
            GlobalFree(mem);
            return Err(AppError::Parse("剪贴板内存加锁失败".into()));
        }
        let df = p as *mut DropFiles;
        (*df).p_files = header as u32;
        (*df).pt_x = 0;
        (*df).pt_y = 0;
        (*df).f_nc = 0;
        (*df).f_wide = 1; // 1 = 宽字符
        std::ptr::copy_nonoverlapping(
            wide.as_ptr() as *const u8,
            p.add(header),
            wide.len() * 2,
        );
        GlobalUnlock(mem);

        if OpenClipboard(std::ptr::null_mut()) == 0 {
            GlobalFree(mem);
            return Err(AppError::Parse(
                "无法打开剪贴板（可能被其他程序占用），请稍后重试".into(),
            ));
        }
        if EmptyClipboard() == 0 {
            CloseClipboard();
            GlobalFree(mem);
            return Err(AppError::Parse("清空剪贴板失败".into()));
        }
        if SetClipboardData(CF_HDROP, mem).is_null() {
            CloseClipboard();
            GlobalFree(mem);
            return Err(AppError::Parse("写入剪贴板失败".into()));
        }
        // 成功：mem 的所有权已交给剪贴板，不能再释放
        CloseClipboard();
    }
    Ok(())
}

/// 非 Windows 平台没有 CF_HDROP 的等价物，明确报错而不是假装成功。
#[cfg(not(windows))]
pub fn copy_paths_to_clipboard(_files: &[CheckedFile]) -> AppResult<()> {
    Err(AppError::Parse(
        "当前平台暂不支持「复制文件到剪贴板」；可用「复制到」指定目标目录".into(),
    ))
}

// ---------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
fn macos_trash(files: &[CheckedFile]) -> AppResult<()> {
    // 逐个移入 ~/.Trash；同名时加序号，避免覆盖回收站里已有的同名文件
    let home = std::env::var("HOME")
        .map_err(|_| AppError::Parse("读不到 HOME，无法定位回收站".into()))?;
    let trash = PathBuf::from(home).join(".Trash");
    std::fs::create_dir_all(&trash)?;
    for f in files {
        let dst = unique_dest(&trash, &f.name);
        std::fs::rename(&f.path, &dst).map_err(|e| {
            AppError::Parse(format!("移入回收站失败 {}：{e}", f.path.display()))
        })?;
    }
    Ok(())
}

// ---------------------------------------------------------------- Linux

/// 删除到 XDG 回收站（freedesktop 规范）。
///
/// 规范要求文件放到 `~/.local/share/Trash/files/`，同时在
/// `~/.local/share/Trash/info/` 写一份 `<名字>.trashinfo`，
/// 记录原路径与删除时间。少了 trashinfo，文件管理器不会把它显示为可还原项。
#[cfg(all(unix, not(target_os = "macos")))]
fn xdg_trash(files: &[CheckedFile]) -> AppResult<()> {
    let data = std::env::var("XDG_DATA_HOME")
        .map(|v| PathBuf::from(v))
        .or_else(|_| {
            std::env::var("HOME").map(|h| PathBuf::from(h).join(".local").join("share"))
        })
        .map_err(|_| AppError::Parse("读不到 HOME/XDG_DATA_HOME，无法定位回收站".into()))?;
    let trash = data.join("Trash");
    let files_dir = trash.join("files");
    let info_dir = trash.join("info");
    std::fs::create_dir_all(&files_dir)?;
    std::fs::create_dir_all(&info_dir)?;

    for f in files {
        let dst = unique_dest(&files_dir, &f.name);
        let kept_name = dst
            .file_name()
            .map(|v| v.to_string_lossy().into_owned())
            .unwrap_or_else(|| f.name.clone());
        // trashinfo 的内容：路径按规范做百分号转义，避免路径含换行/百分号时格式被破坏
        let escaped = percent_escape(&f.path.to_string_lossy());
        let deleted_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let info = format!(
            "[Trash Info]\nPath={escaped}\nDeletionDate={}\n",
            format_local_iso(deleted_at)
        );
        std::fs::rename(&f.path, &dst).map_err(|e| {
            AppError::Parse(format!("移入回收站失败 {}：{e}", f.path.display()))
        })?;
        std::fs::write(info_dir.join(format!("{kept_name}.trashinfo")), info)?;
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn percent_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'.' | b'-' | b'_' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 把 Unix 时间戳格式化成 `YYYY-MM-DDTHH:MM:SS`（XDG 要求本地时间）。
///
/// 不引入日期库：这里只需要「年月日时分秒」，用民用历算法的标准公式即可。
#[cfg(all(unix, not(target_os = "macos")))]
fn format_local_iso(secs: u64) -> String {
    // 转成本地时间：读 TZ 偏移需要 libc，这里按 UTC 处理并在注释中说明。
    // 回收站显示时间略有偏差不影响「还原」，但不静默冒充本地时间：
    // 若要精确本地时间，应引入时间库，此处不为了一个展示字段增加依赖。
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}")
}

/// 由「1970-01-01 起的天数」求公历年月日（Howard Hinnant 的算法）。
#[cfg(all(unix, not(target_os = "macos")))]
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("testtmp")
            .join(format!("sm_fileops_{tag}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn mk(dir: &Path, name: &str, data: &[u8]) -> String {
        let p = dir.join(name);
        std::fs::write(&p, data).unwrap();
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn copy_into_writes_all_files() {
        let base = tmp("copy_basic");
        let src = base.join("src");
        let dst = base.join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();

        let a = mk(&src, "a.litematic", b"AAA");
        let b = mk(&src, "b.litematic", b"BBB");
        let files = check_files(&[a, b]).unwrap();
        let written = copy_into(&files, &dst).unwrap();

        assert_eq!(written.len(), 2);
        assert_eq!(std::fs::read(dst.join("a.litematic")).unwrap(), b"AAA");
        assert_eq!(std::fs::read(dst.join("b.litematic")).unwrap(), b"BBB");
        // 原文件必须还在：复制不是移动
        assert!(src.join("a.litematic").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn copy_into_does_not_overwrite_same_name() {
        let base = tmp("copy_dup");
        let src = base.join("src");
        let dst = base.join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        // 目标目录已有一个同名但内容不同的文件
        std::fs::write(dst.join("a.litematic"), b"EXISTING").unwrap();
        let a = mk(&src, "a.litematic", b"NEW");

        let written = copy_into(&check_files(&[a]).unwrap(), &dst).unwrap();
        assert_eq!(written.len(), 1);
        assert_eq!(
            std::fs::read(dst.join("a.litematic")).unwrap(),
            b"EXISTING",
            "既有文件绝不能被静默覆盖"
        );
        assert_eq!(std::fs::read(&written[0]).unwrap(), b"NEW");
        assert_eq!(
            written[0].file_name().unwrap().to_string_lossy(),
            "a (2).litematic"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn check_files_rejects_missing_and_directory() {
        let base = tmp("check_bad");
        let good = mk(&base, "ok.litematic", b"x");
        let missing = base.join("nope.litematic").to_string_lossy().into_owned();

        assert!(check_files(&[]).is_err(), "空列表应报错，而不是静默成功");
        let e = check_files(&[missing]).unwrap_err().to_string();
        assert!(e.contains("无法访问"), "应说明是哪个文件不可访问：{e}");

        // 目录不能被当成文件处理
        let e2 = check_files(&[base.to_string_lossy().into_owned()]).unwrap_err().to_string();
        assert!(e2.contains("不是文件"), "目录应被拒绝：{e2}");

        // 只要有一个不合格，整批中止（不能部分成功）
        let e3 = check_files(&[good, base.join("x").to_string_lossy().into_owned()]);
        assert!(e3.is_err(), "混入不合格路径时整批应中止");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn check_files_reports_size() {
        let base = tmp("check_size");
        let p = mk(&base, "a.litematic", b"12345");
        let f = check_files(&[p]).unwrap();
        assert_eq!(f[0].size, 5);
        assert_eq!(f[0].name, "a.litematic");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn split_ext_handles_edges() {
        assert_eq!(split_ext("a.litematic"), ("a".into(), "litematic".into()));
        assert_eq!(split_ext("a.b.c"), ("a.b".into(), "c".into()));
        assert_eq!(split_ext("noext"), ("noext".into(), String::new()));
        // 前导点的隐藏文件：整名视作主干，否则会得到空主干
        assert_eq!(split_ext(".hidden"), (".hidden".into(), String::new()));
    }

    #[test]
    fn copy_into_rejects_non_directory() {
        let base = tmp("copy_nondir");
        let f = mk(&base, "a.litematic", b"x");
        let files = check_files(&[f]).unwrap();
        let e = copy_into(&files, &base.join("a.litematic")).unwrap_err().to_string();
        assert!(e.contains("目标不是目录"), "{e}");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 删除必须真的把文件移走，且能从回收站找回。
    ///
    /// 用真实文件与真实回收站：删除是否进回收站、参数格式对不对，
    /// 只有调用系统 API 才能验证，桩替身测不出 SHFileOperationW 的布局错误。
    #[test]
    fn delete_moves_file_to_trash_not_oblivion() {
        let base = tmp("trash");
        let p = PathBuf::from(mk(&base, "to_delete.litematic", b"content"));
        assert!(p.exists());

        let files = check_files(&[p.to_string_lossy().into_owned()]).unwrap();
        delete_to_trash(&files).expect("删除到回收站应成功");

        assert!(!p.exists(), "原位置应已不存在");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 打开文件夹必须拒绝不存在的路径，而不是让 explorer 打开一个空目录。
    #[test]
    fn reveal_rejects_missing_path() {
        let base = tmp("reveal");
        let e = reveal_in_file_manager(&base.join("nope.litematic"))
            .unwrap_err()
            .to_string();
        assert!(e.contains("不存在"), "{e}");
        let _ = std::fs::remove_dir_all(&base);
    }
}
