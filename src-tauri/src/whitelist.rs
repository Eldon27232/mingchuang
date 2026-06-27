//! 系统关键项白名单护栏 — 硬性拒绝命中这些路径的破坏性动作
//!
//! 设计原则:
//! - 默认全拒绝,显式允许 = 由画像的非保护路径触发
//! - 命中即返回 Some(reason),调用方必须把 reason 透出给用户/日志
//! - 不可配置,代码层禁止穿透(避免被画像写错或被攻击者钓到 "我设了允许就能删 Defender" 的洞)

/// 受保护的注册表路径前缀(不分大小写匹配)
/// 命中任一即拒绝任何 reg-delete / reg-deny-acl 动作。
const PROTECTED_REGISTRY_PREFIXES: &[&str] = &[
    // Windows 关键系统配置
    r"HKLM\SAM",
    r"HKLM\SECURITY",
    r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion",
    r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication",
    r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Setup",
    r"HKLM\SOFTWARE\Microsoft\Cryptography",
    r"HKLM\SYSTEM\CurrentControlSet\Control",
    r"HKLM\SYSTEM\CurrentControlSet\Services\Tcpip",
    r"HKLM\SYSTEM\CurrentControlSet\Services\RpcSs",
    r"HKLM\SYSTEM\CurrentControlSet\Services\LSM",
    // Defender / 安全
    r"HKLM\SOFTWARE\Microsoft\Windows Defender",
    r"HKLM\SOFTWARE\Policies\Microsoft\Windows Defender",
    r"HKLM\SYSTEM\CurrentControlSet\Services\WinDefend",
    r"HKLM\SYSTEM\CurrentControlSet\Services\WdNisSvc",
    r"HKLM\SYSTEM\CurrentControlSet\Services\SgrmBroker",
    // Windows Update
    r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate",
    r"HKLM\SYSTEM\CurrentControlSet\Services\wuauserv",
    // 内核 / 启动
    r"HKLM\SYSTEM\CurrentControlSet\Control\BootDrivers",
    r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management",
    // 资源管理器壳
    r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Shell Folders",
    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Shell Folders",
    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders",
    // 用户账户
    r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\ProfileList",
    // 本工具自己 (防止画像不小心删工具本体)
    r"HKLM\SOFTWARE\com.kuake.fuckyou",
    r"HKCU\Software\com.kuake.fuckyou",
];

/// 精确匹配(不是前缀)的禁删路径 — 防止 target 末尾空 CLSID 等场景删整棵子树
const PROTECTED_REGISTRY_EXACT: &[&str] = &[
    r"HKCU\Software\Classes\CLSID",
    r"HKLM\SOFTWARE\Classes\CLSID",
    r"HKCU\Software\Classes",
    r"HKLM\SOFTWARE\Classes",
    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\MyComputer\NameSpace",
    r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\MyComputer\NameSpace",
];

/// 检查注册表路径是否受保护。返回 Some(理由) 表示拒绝。
///
/// path 形如 `HKCU\Software\Foo\Bar` 或 `HKLM\SOFTWARE\Bar`。前缀比较不分大小写。
pub fn is_registry_protected(path: &str) -> Option<&'static str> {
    let normalized = normalize_hive(path);
    let trimmed = normalized.trim().trim_end_matches('\\').to_lowercase();
    // 精确匹配先查 — 防止"删 NameSpace 父键"等灾难
    for exact in PROTECTED_REGISTRY_EXACT {
        if trimmed == exact.to_lowercase() {
            return Some(exact);
        }
    }
    let lower = normalized.to_lowercase();
    for prefix in PROTECTED_REGISTRY_PREFIXES {
        if lower.starts_with(&prefix.to_lowercase()) {
            return Some(prefix);
        }
    }
    None
}

/// 把 HKEY_CURRENT_USER / HKCU 等所有别名归一为 HKCU/HKLM/HKCR/HKU/HKCC。
fn normalize_hive(path: &str) -> String {
    let p = path.trim();
    let lower = p.to_lowercase();
    let mapping: &[(&str, &str)] = &[
        ("hkey_current_user\\", "HKCU\\"),
        ("hkey_local_machine\\", "HKLM\\"),
        ("hkey_classes_root\\", "HKCR\\"),
        ("hkey_users\\", "HKU\\"),
        ("hkey_current_config\\", "HKCC\\"),
    ];
    for (long, short) in mapping {
        if lower.starts_with(long) {
            return format!("{short}{}", &p[long.len()..]);
        }
    }
    p.to_string()
}

/// 把 HKCU\X 拆成 (hive_short, rest_path)
pub fn split_hive(path: &str) -> Option<(&'static str, String)> {
    let normalized = normalize_hive(path);
    for hive in &["HKCU", "HKLM", "HKCR", "HKU", "HKCC"] {
        let prefix = format!("{hive}\\");
        if let Some(rest) = normalized.strip_prefix(&prefix) {
            return Some((*hive, rest.to_string()));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protects_defender() {
        assert!(is_registry_protected(r"HKLM\SOFTWARE\Microsoft\Windows Defender\Foo").is_some());
        assert!(is_registry_protected(r"HKLM\Software\Microsoft\Windows Defender").is_some()); // 不分大小写
        assert!(is_registry_protected(r"hkey_local_machine\SOFTWARE\Microsoft\Windows Defender").is_some());
    }

    #[test]
    fn allows_user_software() {
        assert!(is_registry_protected(r"HKCU\Software\123pan").is_none());
        assert!(is_registry_protected(r"HKCU\Software\Classes\CLSID\{D5BE1ADA-...}").is_none());
    }

    #[test]
    fn split_hive_works() {
        assert_eq!(split_hive(r"HKCU\Software\Foo"), Some(("HKCU", "Software\\Foo".into())));
        assert_eq!(split_hive(r"HKEY_LOCAL_MACHINE\SOFTWARE\Bar"), Some(("HKLM", "SOFTWARE\\Bar".into())));
    }
}
