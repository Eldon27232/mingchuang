//! 检测本机已安装的"开放型"应用 — 用户挑已装软件设默认打开方式,不用手输路径
//!
//! 白名单 + 扫描:
//!  - 一份"开放型软件"清单(我们推荐的可信替代,不是国产流氓)
//!  - 每个软件给 candidate 路径列表,逐个 Test-Path
//!  - 命中即返回

use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct InstalledApp {
    pub key: String,
    pub display_name: String,
    pub category: String,    // music / video / archive / image / doc / browser
    pub exe_path: String,
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

const CANDIDATES: &[Candidate] = &[
    // ===== 视频 =====
    Candidate {
        key: "potplayer",
        display_name: "PotPlayer",
        category: "video",
        paths: &[
            r"%PROGRAMFILES%\DAUM\PotPlayer\PotPlayerMini64.exe",
            r"%PROGRAMFILES(x86)%\DAUM\PotPlayer\PotPlayerMini.exe",
            r"%PROGRAMFILES%\PotPlayer\PotPlayerMini64.exe",
        ],
    },
    Candidate {
        key: "mpc-hc",
        display_name: "MPC-HC",
        category: "video",
        paths: &[
            r"%PROGRAMFILES%\MPC-HC\mpc-hc64.exe",
            r"%PROGRAMFILES(x86)%\MPC-HC\mpc-hc.exe",
        ],
    },
    Candidate {
        key: "vlc",
        display_name: "VLC",
        category: "video",
        paths: &[
            r"%PROGRAMFILES%\VideoLAN\VLC\vlc.exe",
            r"%PROGRAMFILES(x86)%\VideoLAN\VLC\vlc.exe",
        ],
    },
    Candidate {
        key: "mpv",
        display_name: "mpv",
        category: "video",
        paths: &[r"%PROGRAMFILES%\mpv\mpv.exe", r"%LOCALAPPDATA%\mpv\mpv.exe"],
    },
    // ===== 音乐 =====
    Candidate {
        key: "foobar2000",
        display_name: "foobar2000",
        category: "music",
        paths: &[
            r"%PROGRAMFILES%\foobar2000\foobar2000.exe",
            r"%PROGRAMFILES(x86)%\foobar2000\foobar2000.exe",
        ],
    },
    Candidate {
        key: "aimp",
        display_name: "AIMP",
        category: "music",
        paths: &[
            r"%PROGRAMFILES(x86)%\AIMP\AIMP.exe",
            r"%PROGRAMFILES%\AIMP\AIMP.exe",
        ],
    },
    // ===== 压缩 =====
    Candidate {
        key: "7zip",
        display_name: "7-Zip",
        category: "archive",
        paths: &[
            r"%PROGRAMFILES%\7-Zip\7zFM.exe",
            r"%PROGRAMFILES(x86)%\7-Zip\7zFM.exe",
        ],
    },
    Candidate {
        key: "bandizip",
        display_name: "Bandizip",
        category: "archive",
        paths: &[
            r"%PROGRAMFILES%\Bandizip\Bandizip.exe",
            r"%PROGRAMFILES(x86)%\Bandizip\Bandizip.exe",
        ],
    },
    Candidate {
        key: "winrar",
        display_name: "WinRAR",
        category: "archive",
        paths: &[
            r"%PROGRAMFILES%\WinRAR\WinRAR.exe",
            r"%PROGRAMFILES(x86)%\WinRAR\WinRAR.exe",
        ],
    },
    // ===== 图片 =====
    Candidate {
        key: "honeyview",
        display_name: "Honeyview",
        category: "image",
        paths: &[
            r"%PROGRAMFILES%\Honeyview\Honeyview.exe",
            r"%PROGRAMFILES(x86)%\Honeyview\Honeyview.exe",
        ],
    },
    Candidate {
        key: "imageglass",
        display_name: "ImageGlass",
        category: "image",
        paths: &[
            r"%PROGRAMFILES%\ImageGlass\ImageGlass.exe",
            r"%PROGRAMFILES(x86)%\ImageGlass\ImageGlass.exe",
        ],
    },
    Candidate {
        key: "irfanview",
        display_name: "IrfanView",
        category: "image",
        paths: &[
            r"%PROGRAMFILES%\IrfanView\i_view64.exe",
            r"%PROGRAMFILES(x86)%\IrfanView\i_view32.exe",
        ],
    },
    // ===== 文档 =====
    Candidate {
        key: "sumatrapdf",
        display_name: "Sumatra PDF",
        category: "doc",
        paths: &[
            r"%PROGRAMFILES%\SumatraPDF\SumatraPDF.exe",
            r"%LOCALAPPDATA%\SumatraPDF\SumatraPDF.exe",
        ],
    },
    Candidate {
        key: "notepad++",
        display_name: "Notepad++",
        category: "doc",
        paths: &[
            r"%PROGRAMFILES%\Notepad++\notepad++.exe",
            r"%PROGRAMFILES(x86)%\Notepad++\notepad++.exe",
        ],
    },
];

pub fn detect_installed() -> Vec<InstalledApp> {
    let mut out = Vec::new();
    for c in CANDIDATES {
        for p in c.paths {
            if let Some(path) = expand(p) {
                if path.is_file() {
                    out.push(InstalledApp {
                        key: c.key.to_string(),
                        display_name: c.display_name.to_string(),
                        category: c.category.to_string(),
                        exe_path: path.display().to_string(),
                    });
                    break;
                }
            }
        }
    }
    out
}
