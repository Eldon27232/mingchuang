//! "此电脑" Shell 命名空间扫描
//!
//! HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\MyComputer\NameSpace 下的子键即
//! 资源管理器"此电脑"里看到的命名空间项(网盘伪文件夹、系统文件夹等)。
//!
//! 实测结论(见 README "已验证的关键发现"):
//! - 国产网盘的伪文件夹注册在 HKCU,删除该子键即从"此电脑"消失。
//! - 系统文件夹(下载/图片/视频/文档/桌面/音乐)在 HKLM,且 CLSID 名称形如 ThisPC*RegFolder。
//! - 判定 is_system 时用硬编码 CLSID 白名单 + 图标指向系统 dll(双重保险)。
//!
//! 本模块只读, 真正的清理动作由 actions 模块负责(下一轮起)。

use anyhow::{Context, Result};
use serde::Serialize;
use windows_registry::CURRENT_USER;

#[derive(Debug, Clone, Serialize)]
pub struct PcNamespaceItem {
    pub clsid: String,
    pub display_name: String,
    pub default_icon: Option<String>,
    pub inproc_server: Option<String>,
    pub is_system: bool,
}

/// 系统自带的"此电脑"文件夹 CLSID 白名单(Win11 实测值)
const SYSTEM_CLSIDS: &[&str] = &[
    "{088e3905-0323-4b02-9826-5d99428e115f}", // Downloads (local)
    "{1CF1260C-4DD0-4ebb-811F-33C572699FDE}", // Music
    "{24ad3ad4-a569-4530-98e1-ab02f9417aa8}", // Pictures (local)
    "{374DE290-123F-4565-9164-39C4925E467B}", // Downloads
    "{3ADD1653-EB32-4cb0-BBD7-DFA0ABB5ACCA}", // Pictures
    "{3dfdf296-dbec-4fb4-81d1-6a3438bcf4de}", // Music (local)
    "{A0953C92-50DC-43bf-BE83-3742FED03C9C}", // Videos
    "{A8CDFF1C-4878-43be-B5FD-F8091C1C60D0}", // Documents
    "{B4BFCC3A-DB2C-424C-B029-7FE99A87C641}", // Desktop
    "{d3162b92-9365-467a-956b-92703aca08af}", // Documents (local)
    "{f86fa3ab-70d2-4fc7-9c99-fcbf05467f3a}", // Videos (local)
];

fn is_system_clsid(clsid: &str, default_icon: Option<&str>) -> bool {
    let lc = clsid.to_lowercase();
    if SYSTEM_CLSIDS.iter().any(|s| s.to_lowercase() == lc) {
        return true;
    }
    // 兜底: 图标指向系统 dll 也算系统项
    if let Some(icon) = default_icon {
        let lower = icon.to_lowercase();
        if lower.contains("imageres.dll") || lower.contains("shell32.dll") {
            return true;
        }
    }
    false
}

/// 扫描 HKCU 下的 NameSpace 子键。HKLM 副本通常只装系统项,这里不扫(留给下一轮按需扩展)。
pub fn scan_pc_namespace_items() -> Result<Vec<PcNamespaceItem>> {
    let ns = CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Explorer\MyComputer\NameSpace")
        .context("打开 HKCU NameSpace 失败")?;

    let mut items = Vec::new();
    for clsid_res in ns.keys()? {
        let clsid = clsid_res;
        if clsid.eq_ignore_ascii_case("DelegateFolders") {
            continue;
        }

        // NameSpace 子键的默认值通常是显示名
        let display_name = ns
            .open(&clsid)
            .and_then(|k| k.get_string(""))
            .unwrap_or_default();

        // 查 CLSID 本体(可能在 HKCU 也可能在 HKLM,这里只查 HKCU)
        let clsid_path = format!(r"Software\Classes\CLSID\{clsid}");
        let (default_icon, inproc_server) = match CURRENT_USER.open(&clsid_path) {
            Ok(c) => {
                let icon = c
                    .open("DefaultIcon")
                    .and_then(|k| k.get_string(""))
                    .ok();
                let inproc = c
                    .open("InProcServer32")
                    .and_then(|k| k.get_string(""))
                    .ok();
                (icon, inproc)
            }
            Err(_) => (None, None),
        };

        items.push(PcNamespaceItem {
            is_system: is_system_clsid(&clsid, default_icon.as_deref()),
            clsid,
            display_name,
            default_icon,
            inproc_server,
        });
    }

    items.sort_by(|a, b| {
        // 第三方排在前(让用户先看到),系统排在后
        a.is_system
            .cmp(&b.is_system)
            .then_with(|| a.display_name.cmp(&b.display_name))
    });

    Ok(items)
}
