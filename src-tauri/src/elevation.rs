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
