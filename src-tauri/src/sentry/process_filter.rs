//! 进程过滤 - 决定哪些 PID 即使超阈值也不告警
//!
//! Phase 1 简化:
//!  - PID 0 / 4 系统
//!  - 路径在 C:\Windows\ 下
//!  - image_name 命中内置白名单 / 用户白名单

use crate::whitelist::MergedWhitelist;
use std::path::PathBuf;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};

pub fn pid_alive(pid: u32) -> bool {
    unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => {
                let _ = CloseHandle(h);
                true
            }
            Err(_) => false,
        }
    }
}

pub fn image_name_for(pid: u32) -> Option<String> {
    let path = exe_path_for(pid)?;
    PathBuf::from(&path)
        .file_name()
        .and_then(|n| n.to_str())
        .map(String::from)
}

pub fn exe_path_for(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let r = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(handle);
        if r.is_err() || len == 0 {
            return None;
        }
        let s = String::from_utf16_lossy(&buf[..len as usize]);
        Some(s)
    }
}

/// 判断该 PID 是否被过滤(不应该告警)
pub fn is_filtered(pid: u32, image_name: &str, wl: &MergedWhitelist) -> bool {
    // 1. 系统 PID
    if pid == 0 || pid == 4 {
        return true;
    }
    // 2. 路径在 Windows 系统目录
    if let Some(path) = exe_path_for(pid) {
        let pl = path.to_ascii_lowercase();
        if pl.starts_with("c:\\windows\\system32\\")
            || pl.starts_with("c:\\windows\\syswow64\\")
            || pl.starts_with("c:\\windows\\winsxs\\")
            || pl.starts_with("c:\\windows\\servicing\\")
            || pl.starts_with("c:\\windows\\softwaredistribution\\")
        {
            return true;
        }
    }
    // 3. 白名单匹配 (按 image_name)
    let name_lc = image_name.to_ascii_lowercase();
    if wl.image_names.contains(&name_lc) {
        return true;
    }
    false
}
