//! 前台 / 可见窗口 PID 判定
//!
//! 用户可见的 PID(前台 + 任意可见非云隐藏窗口)只写日志, 不弹 toast,
//! 避免误报正在用的程序(游戏/视频通话上传 + 浏览器同步等)。

use std::collections::HashSet;
use std::ffi::c_void;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, TRUE};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindow, GetWindowLongPtrW, GetWindowThreadProcessId,
    IsWindowVisible, GWLP_HWNDPARENT, GWL_EXSTYLE, GW_OWNER, WS_EX_TOOLWINDOW,
};

pub fn foreground_pid() -> Option<u32> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 { None } else { Some(pid) }
    }
}

/// 取所有"用户能看到"的窗口对应的 PID 集合。
pub fn visible_pids() -> HashSet<u32> {
    let mut set: HashSet<u32> = HashSet::new();
    let ptr: *mut HashSet<u32> = &mut set;
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(ptr as isize));
    }
    set
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    unsafe {
        // 可见
        if !IsWindowVisible(hwnd).as_bool() {
            return TRUE;
        }
        // 没有 owner(子窗口/对话框不算独立可见)
        if !GetWindow(hwnd, GW_OWNER).unwrap_or_default().0.is_null() {
            return TRUE;
        }
        // 不是工具栏窗口
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        if ex_style & WS_EX_TOOLWINDOW.0 != 0 {
            return TRUE;
        }
        // 未被 DWM 云隐藏(UWP 后台)
        let mut cloaked: u32 = 0;
        let _ = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut _ as *mut c_void,
            std::mem::size_of::<u32>() as u32,
        );
        if cloaked != 0 {
            return TRUE;
        }

        // 取 PID
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != 0 {
            let set = &mut *(lparam.0 as *mut HashSet<u32>);
            set.insert(pid);
        }
        let _ = GWLP_HWNDPARENT; // silence unused
    }
    TRUE
}
