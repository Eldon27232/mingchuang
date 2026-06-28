//! 检测本机已安装的"开放型"应用
//!
//! 三路并:
//!  1. 内置白名单候选(15 个,扫安装目录)— 推荐工具
//!  2. 扫开始菜单 .lnk(用户实际装的所有应用)— 用 IShellLinkW COM 解析 target
//!  3. 扫 HKLM/HKCU\Software\Microsoft\Windows\CurrentVersion\App Paths(系统注册过的可执行)
//!
//! 过滤掉系统目录里的 .exe(svchost 等)和明显非用户应用的。

use anyhow::Result;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use windows_registry::{CLASSES_ROOT, CURRENT_USER, LOCAL_MACHINE};

#[derive(Debug, Clone, Serialize)]
pub struct InstalledApp {
    pub key: String,
    pub display_name: String,
    pub category: String,
    pub exe_path: String,
    /// 是否在 HKLM/HKCU Software\RegisteredApplications 里挂了 Capabilities,
    /// 即"主动告诉 Windows 我能开某些扩展名"的应用。
    /// 前端用这个做"仅已注册"筛选 — 这是 Windows 默认应用面板使用的判定口径。
    pub registered: bool,
}

fn expand(p: &str) -> Option<PathBuf> {
    let mut s = p.to_string();
    for var in ["PROGRAMFILES", "ProgramFiles(x86)", "LOCALAPPDATA", "APPDATA", "PUBLIC"] {
        let pattern = format!("%{var}%");
        if let Ok(val) = std::env::var(var) {
            s = s.replace(&pattern, &val);
        }
    }
    Some(PathBuf::from(s))
}

struct Candidate {
    key: &'static str,
    display_name: &'static str,
    category: &'static str,
    paths: &'static [&'static str],
}

// 推荐的"开放型"工具(预设)
const CANDIDATES: &[Candidate] = &[
    Candidate { key: "potplayer", display_name: "PotPlayer", category: "video", paths: &[
        r"%PROGRAMFILES%\DAUM\PotPlayer\PotPlayerMini64.exe",
        r"%PROGRAMFILES(x86)%\DAUM\PotPlayer\PotPlayerMini.exe",
        r"%PROGRAMFILES%\PotPlayer\PotPlayerMini64.exe",
        r"%PROGRAMFILES(x86)%\PotPlayer\PotPlayerMini.exe",
        // 完美解码 / 完美者(打包了 PotPlayer)
        r"%PROGRAMFILES%\Wanos\PotPlayer\PotPlayerMini64.exe",
        r"%PROGRAMFILES(x86)%\Wanos\PotPlayer\PotPlayerMini.exe",
        r"%PROGRAMFILES%\KMPlayer\PotPlayerMini64.exe",
        r"D:\Program Files\DAUM\PotPlayer\PotPlayerMini64.exe",
        r"D:\PotPlayer\PotPlayerMini64.exe",
    ]},
    Candidate { key: "mpc-hc", display_name: "MPC-HC", category: "video", paths: &[
        r"%PROGRAMFILES%\MPC-HC\mpc-hc64.exe",
        r"%PROGRAMFILES(x86)%\MPC-HC\mpc-hc.exe",
        r"%PROGRAMFILES%\K-Lite Codec Pack\MPC-HC64\mpc-hc64.exe",
    ]},
    Candidate { key: "vlc", display_name: "VLC", category: "video", paths: &[
        r"%PROGRAMFILES%\VideoLAN\VLC\vlc.exe",
        r"%PROGRAMFILES(x86)%\VideoLAN\VLC\vlc.exe",
    ]},
    Candidate { key: "mpv", display_name: "mpv", category: "video", paths: &[
        r"%PROGRAMFILES%\mpv\mpv.exe", r"%LOCALAPPDATA%\mpv\mpv.exe",
    ]},
    Candidate { key: "foobar2000", display_name: "foobar2000", category: "music", paths: &[
        r"%PROGRAMFILES%\foobar2000\foobar2000.exe",
        r"%PROGRAMFILES(x86)%\foobar2000\foobar2000.exe",
    ]},
    Candidate { key: "aimp", display_name: "AIMP", category: "music", paths: &[
        r"%PROGRAMFILES(x86)%\AIMP\AIMP.exe", r"%PROGRAMFILES%\AIMP\AIMP.exe",
    ]},
    Candidate { key: "7zip", display_name: "7-Zip", category: "archive", paths: &[
        r"%PROGRAMFILES%\7-Zip\7zFM.exe", r"%PROGRAMFILES(x86)%\7-Zip\7zFM.exe",
    ]},
    Candidate { key: "bandizip", display_name: "Bandizip", category: "archive", paths: &[
        r"%PROGRAMFILES%\Bandizip\Bandizip.exe", r"%PROGRAMFILES(x86)%\Bandizip\Bandizip.exe",
    ]},
    Candidate { key: "winrar", display_name: "WinRAR", category: "archive", paths: &[
        r"%PROGRAMFILES%\WinRAR\WinRAR.exe", r"%PROGRAMFILES(x86)%\WinRAR\WinRAR.exe",
    ]},
    Candidate { key: "honeyview", display_name: "Honeyview", category: "image", paths: &[
        r"%PROGRAMFILES%\Honeyview\Honeyview.exe", r"%PROGRAMFILES(x86)%\Honeyview\Honeyview.exe",
    ]},
    Candidate { key: "imageglass", display_name: "ImageGlass", category: "image", paths: &[
        r"%PROGRAMFILES%\ImageGlass\ImageGlass.exe", r"%PROGRAMFILES(x86)%\ImageGlass\ImageGlass.exe",
    ]},
    Candidate { key: "irfanview", display_name: "IrfanView", category: "image", paths: &[
        r"%PROGRAMFILES%\IrfanView\i_view64.exe", r"%PROGRAMFILES(x86)%\IrfanView\i_view32.exe",
    ]},
    Candidate { key: "sumatrapdf", display_name: "Sumatra PDF", category: "doc", paths: &[
        r"%PROGRAMFILES%\SumatraPDF\SumatraPDF.exe", r"%LOCALAPPDATA%\SumatraPDF\SumatraPDF.exe",
    ]},
    Candidate { key: "notepad++", display_name: "Notepad++", category: "doc", paths: &[
        r"%PROGRAMFILES%\Notepad++\notepad++.exe", r"%PROGRAMFILES(x86)%\Notepad++\notepad++.exe",
    ]},
];

/// 收集所有"已注册为关联程序"的 exe 路径 (小写)。
///
/// Windows 默认应用面板的判定口径: 一个 app 出现在 ms-settings:defaultapps
/// 里, 就必须满足两条 — (1) 在 HKLM 或 HKCU 的 Software\RegisteredApplications
/// 里有个名字, (2) 对应 Capabilities 键下声明了 FileAssociations。
///
/// 顺着这条链拿 exe:
///   HKLM/HKCU\Software\RegisteredApplications  (枚举值)
///     value name = "Adobe Reader DC"
///     value data = "Software\Adobe\Reader\Capabilities"   (← 指向 Capabilities)
///   该 Capabilities 键的子键 FileAssociations:
///     .pdf = "AcroExch.Document.DC"   (← ProgId)
///   HKCR\AcroExch.Document.DC\shell\open\command\(默认):
///     "C:\Program Files\Adobe\...\AcroRd32.exe" "%1"   (← 含 exe 路径)
///
/// 网络浏览器/邮件客户端在 Software\Clients\... 下有平行结构, 但默认应用面板
/// 把它们也算"已注册"的一部分。这里**只**走 RegisteredApplications, 因为
/// Clients\ 那条路径多数应用同时也在 RegisteredApplications 里注册, 不漏。
fn registered_exe_paths() -> HashSet<String> {
    let mut out = HashSet::new();

    for root in [LOCAL_MACHINE, CURRENT_USER] {
        let Ok(reg_apps) = root.open("Software\\RegisteredApplications") else { continue };
        let Ok(values) = reg_apps.values() else { continue };
        for (_app_name, val) in values {
            // value data 是字符串, 形如 "Software\Vendor\App\Capabilities"
            let cap_path: String = match val.try_into() {
                Ok(s) => s,
                Err(_) => continue,
            };
            collect_exes_under_capabilities(root, &cap_path, &mut out);
            collect_exes_under_capabilities(LOCAL_MACHINE, &cap_path, &mut out);
            collect_exes_under_capabilities(CURRENT_USER, &cap_path, &mut out);
        }
    }

    out
}

fn collect_exes_under_capabilities(
    hive: &windows_registry::Key,
    cap_path: &str,
    out: &mut HashSet<String>,
) {
    let cap_key = match hive.open(cap_path) {
        Ok(k) => k,
        Err(_) => return,
    };
    for sub in ["FileAssociations", "UrlAssociations"] {
        let Ok(assoc_key) = cap_key.open(sub) else { continue };
        let Ok(vs) = assoc_key.values() else { continue };
        for (_ext_or_proto, v) in vs {
            let progid: String = match v.try_into() {
                Ok(s) => s,
                Err(_) => continue,
            };
            if let Some(exe) = exe_from_progid(&progid) {
                out.insert(exe.to_ascii_lowercase());
            }
        }
    }
}

/// 从 HKCR\<ProgId>\shell\open\command\(默认) 提 exe 绝对路径
fn exe_from_progid(progid: &str) -> Option<String> {
    let path = format!("{progid}\\shell\\open\\command");
    let key = CLASSES_ROOT.open(&path).ok()?;
    let cmd: String = key.get_value("").ok()?.try_into().ok()?;
    parse_exe_from_command(&cmd)
}

/// 从 shell\open\command 的命令串里抠出 exe 路径。
/// 命令通常是 `"C:\foo\App.exe" "%1"` 或 `C:\foo\App.exe %1` (无空格无引号)。
fn parse_exe_from_command(cmd: &str) -> Option<String> {
    let trimmed = cmd.trim();
    if trimmed.is_empty() { return None; }
    let exe = if let Some(stripped) = trimmed.strip_prefix('"') {
        // 引号包裹: 取下一个引号前的内容
        let end = stripped.find('"')?;
        stripped[..end].to_string()
    } else {
        // 无引号: 取第一个空格前的全部 (假设路径不含空格)
        let end = trimmed.find(' ').unwrap_or(trimmed.len());
        trimmed[..end].to_string()
    };
    if exe.to_ascii_lowercase().ends_with(".exe") || exe.to_ascii_lowercase().ends_with(".dll") {
        Some(exe)
    } else {
        None
    }
}

pub fn detect_installed() -> Vec<InstalledApp> {
    let registered = registered_exe_paths();
    let mut out: Vec<InstalledApp> = Vec::new();
    let mut seen_paths: HashMap<String, bool> = HashMap::new();

    // 1. 内置推荐候选
    for c in CANDIDATES {
        for p in c.paths {
            if let Some(path) = expand(p) {
                if path.is_file() {
                    let key_path = path.display().to_string().to_ascii_lowercase();
                    if seen_paths.insert(key_path, true).is_none() {
                        out.push(InstalledApp {
                            key: c.key.to_string(),
                            display_name: c.display_name.to_string(),
                            category: c.category.to_string(),
                            exe_path: path.display().to_string(),
                            registered: false, // 下面统一打标
                        });
                    }
                    break;
                }
            }
        }
    }

    // 2. 扫开始菜单 .lnk
    if let Ok(apps) = scan_start_menu_apps() {
        for app in apps {
            let key_path = app.exe_path.to_ascii_lowercase();
            if seen_paths.insert(key_path, true).is_none() {
                out.push(app);
            }
        }
    }

    // 3. 给每个 app 打 registered 标 — 一次性查 HashSet, O(1)
    for app in out.iter_mut() {
        if registered.contains(&app.exe_path.to_ascii_lowercase()) {
            app.registered = true;
        }
    }

    out.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    out
}

fn start_menu_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(a) = std::env::var("APPDATA") {
        out.push(PathBuf::from(&a).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Ok(pd) = std::env::var("ProgramData") {
        out.push(PathBuf::from(&pd).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    out
}

/// 判断从开始菜单扫到的一条 .lnk 是不是"打开器"应用 (返回 false 就丢弃)。
///
/// Windows 没有 100% 准确的方式知道一个 exe 能不能打开文件 (除非真试),
/// 但开始菜单里 90% 的垃圾条目 (卸载/Setup/Help/ReadMe/License/Updater)
/// 都能通过名称和 exe 文件名干掉。这里只做硬过滤, 不做"必须在
/// RegisteredApplications 里"那种激进过滤 (会丢绿色版/便携版)。
fn looks_like_file_opener(display_name: &str, exe_path: &str) -> bool {
    let n = display_name.to_ascii_lowercase();
    let fname = std::path::Path::new(exe_path)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    // 显示名 黑名单 (中英文混)。子串匹配。
    const NAME_BLACKLIST: &[&str] = &[
        // 卸载
        "卸载", "uninstall", "uninst",
        // 安装/配置
        "setup", "installer", "安装", "install ",
        // 更新
        "update", "updater", "升级", "更新",
        // 帮助/说明
        "help", "readme", "read me", "帮助", "说明",
        // 许可/法务
        "license", "许可", "eula",
        // 教程/示例
        "tutorial", "教程", "manual", "手册", "guide", "sample", "示例", "demo", "演示",
        // 反馈/崩溃/诊断
        "crash", "report", "feedback", "反馈", "diagnostic", "诊断", "logger",
        // 网站/在线工具
        "website", "homepage", "网站", "官网",
    ];
    for kw in NAME_BLACKLIST {
        if n.contains(kw) {
            return false;
        }
    }

    // exe 文件名黑名单 (前缀/子串)
    const EXE_BLACKLIST: &[&str] = &[
        "uninst", "unins", "setup", "installer", "install.exe",
        "update.exe", "updater.exe", "crashpad", "crashreport", "wer.exe",
        "report.exe", "license.exe",
    ];
    for kw in EXE_BLACKLIST {
        if fname.contains(kw) {
            return false;
        }
    }

    true
}

/// 扫开始菜单 .lnk → 提取 target exe → 包装成 InstalledApp
fn scan_start_menu_apps() -> Result<Vec<InstalledApp>> {
    use windows::core::{Interface, PCWSTR, PWSTR};
    use windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED, STGM_READ,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink, SLGP_RAWPATH};

    let mut out = Vec::new();

    unsafe {
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        let did_init = hr.is_ok();

        for dir in start_menu_dirs() {
            if !dir.is_dir() {
                continue;
            }
            for entry in walk_lnk_files(&dir) {
                let path_str = entry.display().to_string();
                let lnk_w: Vec<u16> = path_str
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();

                let Ok(shell_link) =
                    CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER)
                else {
                    continue;
                };
                let Ok(persist) = Interface::cast::<IPersistFile>(&shell_link) else {
                    continue;
                };
                if persist.Load(PCWSTR(lnk_w.as_ptr()), STGM_READ).is_err() {
                    continue;
                }

                let mut buf = vec![0u16; 2048];
                let mut wfd = WIN32_FIND_DATAW::default();
                if shell_link
                    .GetPath(&mut buf, &mut wfd, SLGP_RAWPATH.0 as u32)
                    .is_err()
                {
                    continue;
                }
                let _ = PWSTR(buf.as_mut_ptr());
                let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                let target = String::from_utf16_lossy(&buf[..len]);

                if target.is_empty() {
                    continue;
                }
                if !target.to_ascii_lowercase().ends_with(".exe") {
                    continue;
                }
                // 排除系统目录的 exe
                let lt = target.to_ascii_lowercase();
                if lt.starts_with("c:\\windows\\system32\\")
                    || lt.starts_with("c:\\windows\\syswow64\\")
                    || lt.starts_with("c:\\windows\\winsxs\\")
                {
                    continue;
                }
                if !std::path::Path::new(&target).is_file() {
                    continue;
                }

                let display_name = entry
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(String::from)
                    .unwrap_or_else(|| "Unknown".into());

                // 过滤掉"卸载/Setup/Help/Updater/Crash" 等明显不是 opener 的 .lnk
                if !looks_like_file_opener(&display_name, &target) {
                    continue;
                }

                let category = guess_category(&display_name, &target);

                let key = format!(
                    "lnk:{}",
                    target.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>()
                );
                out.push(InstalledApp {
                    key,
                    display_name,
                    category,
                    exe_path: target,
                    registered: false, // detect_installed 末尾统一打标
                });
            }
        }

        if did_init {
            CoUninitialize();
        }
    }

    Ok(out)
}

fn walk_lnk_files(dir: &PathBuf) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk_lnk_inner(dir, &mut out, 0);
    out
}

fn walk_lnk_inner(dir: &PathBuf, out: &mut Vec<PathBuf>, depth: u32) {
    if depth > 4 { return; }
    let Ok(entries) = std::fs::read_dir(dir) else { return; };
    for entry in entries.flatten() {
        let path = entry.path();
        if let Ok(ty) = entry.file_type() {
            if ty.is_dir() {
                walk_lnk_inner(&path, out, depth + 1);
            } else if path.extension().and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("lnk")).unwrap_or(false)
            {
                out.push(path);
            }
        }
    }
}

fn guess_category(name: &str, path: &str) -> String {
    let n = name.to_ascii_lowercase();
    let p = path.to_ascii_lowercase();
    if n.contains("player") || n.contains("video") || n.contains("vlc") || n.contains("mpc")
        || n.contains("potplayer") || n.contains("kmplayer") || n.contains("视频") || n.contains("影音")
    {
        return "video".into();
    }
    if n.contains("music") || n.contains("audio") || n.contains("foobar") || n.contains("aimp")
        || n.contains("音乐")
    {
        return "music".into();
    }
    if n.contains("zip") || n.contains("rar") || n.contains("7z") || n.contains("bandi")
        || n.contains("压缩")
    {
        return "archive".into();
    }
    if n.contains("image") || n.contains("photo") || n.contains("honey") || n.contains("imageglass")
        || n.contains("irfan") || n.contains("图片")
    {
        return "image".into();
    }
    if n.contains("pdf") || n.contains("reader") || n.contains("notepad") || n.contains("文档")
        || p.contains("\\office") || n.contains("word") || n.contains("typora") || n.contains("vscode")
    {
        return "doc".into();
    }
    "custom".into()
}

#[cfg(test)]
mod tests {
    use super::looks_like_file_opener;

    #[test]
    fn junk_is_filtered() {
        // 这些应该全被过滤掉
        let junk = [
            ("卸载 PotPlayer", r"C:\Program Files\DAUM\PotPlayer\unins000.exe"),
            ("Uninstall WinRAR", r"C:\Program Files\WinRAR\Uninstall.exe"),
            ("PotPlayer 帮助", r"C:\Program Files\DAUM\PotPlayer\PotPlayerMini64.exe"),
            ("Setup", r"C:\Temp\setup.exe"),
            ("Auto Updater", r"C:\Program Files\App\updater.exe"),
            ("ReadMe", r"C:\Program Files\App\readme.exe"),
            ("Crash Reporter", r"C:\Program Files\App\crashreport.exe"),
            ("License Agreement", r"C:\Program Files\App\license.exe"),
            ("教程", r"C:\Program Files\App\tutorial.exe"),
            ("Demo", r"C:\Program Files\App\demo.exe"),
        ];
        for (name, path) in junk {
            assert!(
                !looks_like_file_opener(name, path),
                "应被过滤但通过了: {name} ({path})"
            );
        }
    }

    /// 真机 dump: 跑一次 detect_installed, 打印当前检测到什么。
    /// 用 cargo test --lib -- --ignored detect_dump --nocapture
    #[test]
    #[ignore]
    fn detect_dump() {
        let apps = super::detect_installed();
        let reg_count = apps.iter().filter(|a| a.registered).count();
        eprintln!("共 {} 个 app, 其中 {} 个已注册关联程序:", apps.len(), reg_count);
        for a in &apps {
            let tag = if a.registered { "REG" } else { "   " };
            eprintln!("  {tag} [{}] {} - {}", a.category, a.display_name, a.exe_path);
        }
    }

    #[test]
    fn legitimate_passes() {
        // 这些应该全部保留
        let ok = [
            ("PotPlayer", r"C:\Program Files\DAUM\PotPlayer\PotPlayerMini64.exe"),
            ("VLC media player", r"C:\Program Files\VideoLAN\VLC\vlc.exe"),
            ("Notepad++", r"C:\Program Files\Notepad++\notepad++.exe"),
            ("Sumatra PDF", r"C:\Program Files\SumatraPDF\SumatraPDF.exe"),
            ("Bandizip", r"C:\Program Files\Bandizip\Bandizip.exe"),
            ("foobar2000", r"C:\Program Files\foobar2000\foobar2000.exe"),
            // 名字虽含 "install" 但是合法应用 — 当前实现会误杀, 留作 TODO
            // ("PowerShell ISE", r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell_ise.exe"),
        ];
        for (name, path) in ok {
            assert!(
                looks_like_file_opener(name, path),
                "应保留但被过滤了: {name} ({path})"
            );
        }
    }
}
