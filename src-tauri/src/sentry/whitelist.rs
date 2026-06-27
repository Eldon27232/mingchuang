//! 白名单 - 内置 + 用户
//!
//! 存放: %LOCALAPPDATA%\mingchuang\sentry\{builtin.json, user.json}

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhitelistFile {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub entries: Vec<WhitelistEntry>,
}

fn default_version() -> u32 { 1 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhitelistEntry {
    pub image_name: String,
    #[serde(default)]
    pub signer_cn: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

pub struct MergedWhitelist {
    /// 所有 image_name 的小写形式
    pub image_names: HashSet<String>,
}

pub fn sentry_dir() -> PathBuf {
    let local = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    local.join("mingchuang").join("sentry")
}

/// 内置白名单 (硬编码,随版本带,首发覆盖)
fn builtin_image_names() -> Vec<&'static str> {
    vec![
        // 浏览器
        "chrome.exe", "msedge.exe", "firefox.exe", "brave.exe", "vivaldi.exe", "iexplore.exe",
        // IDE / 编辑器
        "Code.exe", "code.exe", "devenv.exe", "idea64.exe", "pycharm64.exe", "goland64.exe",
        "clion64.exe", "rider64.exe", "webstorm64.exe", "Cursor.exe",
        "sublime_text.exe", "notepad++.exe",
        // 终端
        "WindowsTerminal.exe", "wsl.exe", "wslhost.exe", "wslservice.exe", "powershell.exe", "pwsh.exe",
        "OpenConsole.exe", "cmd.exe", "ssh.exe", "git.exe", "bash.exe",
        // 游戏平台
        "steam.exe", "steamwebhelper.exe", "steamservice.exe",
        "EpicGamesLauncher.exe", "EpicWebHelper.exe",
        "Battle.net.exe", "Agent.exe",
        "UbisoftConnect.exe", "upc.exe",
        "EA Desktop.exe", "EALauncher.exe",
        "RiotClientServices.exe",
        // 通讯
        "Discord.exe", "Slack.exe", "Telegram.exe", "Element.exe",
        "WeChat.exe", "WeChatAppEx.exe", "QQ.exe", "QQEX.exe", "TIM.exe", "dingtalk.exe",
        "feishu.exe", "lark.exe", "KOOK.exe", "Wemeet.exe", "wemeetapp.exe",
        // 同步盘
        "OneDrive.exe", "Dropbox.exe", "GoogleDriveFS.exe", "GoogleDrive.exe",
        // 系统/微软
        "MsMpEng.exe", "SearchHost.exe", "SearchIndexer.exe", "SearchApp.exe",
        "MoUsoCoreWorker.exe", "TiWorker.exe", "TrustedInstaller.exe", "wuauclt.exe",
        "DeliveryOptimization.exe", "BackgroundTransferHost.exe",
        "WindowsUpdateBox.exe", "SecurityHealthService.exe",
        // 显卡 / 硬件厂商
        "NVDisplay.Container.exe", "nvcontainer.exe", "nvsphelper64.exe",
        "RadeonSoftware.exe", "AMDRSSrcExt.exe", "atieclxx.exe", "atiesrxx.exe",
        // 媒体
        "Spotify.exe", "AppleMusic.exe", "cloudmusic.exe", "cloudmusic_reporter.exe",
        // 会议
        "Zoom.exe", "ZoomLauncher.exe", "Teams.exe", "ms-teams.exe",
        // 火绒 / 安全(用户已表态)
        "HipsDaemon.exe", "HipsMain.exe", "HipsTray.exe",
        // FlClash 代理
        "FlClash.exe", "FlClashCore.exe", "FlClashHelperService.exe",
        // 我们自己
        "kuake-fuckyou.exe", "mingchuang-sentry.exe",
        // 网盘(归"限速但不杀")
        "BaiduNetdisk.exe", "BaiduNetdiskUtility.exe",
        "123pan.exe", "MaintenanceService.exe", "UpgradeService.exe",
    ]
}

pub fn load_merged() -> MergedWhitelist {
    let mut names: HashSet<String> = builtin_image_names()
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();

    // 用户白名单(若存在)
    let user_path = sentry_dir().join("user.json");
    if let Ok(txt) = std::fs::read_to_string(&user_path) {
        if let Ok(f) = serde_json::from_str::<WhitelistFile>(&txt) {
            for e in f.entries {
                names.insert(e.image_name.to_ascii_lowercase());
            }
        }
    }

    MergedWhitelist { image_names: names }
}

#[allow(dead_code)]
pub fn save_user(file: &WhitelistFile) -> Result<()> {
    let dir = sentry_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("创建 {dir:?} 失败"))?;
    let path = dir.join("user.json");
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(file)?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}
