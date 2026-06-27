//! 运行时检测当前进程是否以管理员权限运行
//!
//! 用 OpenProcessToken + GetTokenInformation(TokenElevation) 这条标准 Win32 路径。
//! 比检测 BUILTIN\\Administrators 组成员更准确(后者在 UAC 限制下不可靠)。

use serde::Serialize;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

#[derive(Debug, Clone, Serialize)]
pub struct ElevationStatus {
    pub is_elevated: bool,
    /// 给用户看的人话提示
    pub message: String,
}

pub fn check_elevation() -> ElevationStatus {
    let elevated = unsafe { is_token_elevated() };
    let message = if elevated {
        "已以管理员身份运行,可执行所有动作。".into()
    } else {
        "未以管理员身份运行。改 HKLM、停服务、禁计划任务等动作会失败。请右键 → 以管理员身份运行 重新打开本工具。".into()
    };
    ElevationStatus {
        is_elevated: elevated,
        message,
    }
}

/// 以管理员身份重新拉起本进程,自身退出。
/// 调 ShellExecute(NULL, "runas", path, NULL, NULL, SW_NORMAL) — Windows 会弹 UAC。
pub fn relaunch_as_admin() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("{e:#}"))?;
    let exe_wide: Vec<u16> = exe
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let verb_wide: Vec<u16> = "runas\0".encode_utf16().collect();

    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_NORMAL;

    let h = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb_wide.as_ptr()),
            PCWSTR(exe_wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_NORMAL,
        )
    };
    // ShellExecuteW 返回值 > 32 表示成功
    if (h.0 as usize) > 32 {
        std::process::exit(0);
    } else {
        Err(format!(
            "ShellExecuteW 失败 (返回 {:?}), 用户可能取消了 UAC",
            h.0 as usize
        ))
    }
}

unsafe fn is_token_elevated() -> bool {
    let mut token = HANDLE::default();
    let opened = OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token);
    if opened.is_err() {
        return false;
    }
    let mut elevation = TOKEN_ELEVATION::default();
    let mut size: u32 = std::mem::size_of::<TOKEN_ELEVATION>() as u32;
    let r = GetTokenInformation(
        token,
        TokenElevation,
        Some(&mut elevation as *mut _ as *mut _),
        size,
        &mut size,
    );
    let _ = CloseHandle(token);
    r.is_ok() && elevation.TokenIsElevated != 0
}
