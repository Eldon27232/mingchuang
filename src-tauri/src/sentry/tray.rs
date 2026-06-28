//! 系统托盘图标 — sentry 守护进程的"我还活着"信号
//!
//! 用 Win32 Shell_NotifyIcon 直接实现(避免引入 winit/tao 等大依赖)。
//! 隐藏窗口 + WindowProc 处理 WM_USER+1 (托盘消息) 和 WM_COMMAND (菜单)。
//!
//! 菜单:
//!   - 打开明窗 (启动 GUI)
//!   - 暂停 1 小时
//!   - 退出守护

use anyhow::{anyhow, Result};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DispatchMessageW,
    GetCursorPos, GetMessageW, LoadIconW, PostMessageW, PostQuitMessage, RegisterClassW,
    SetForegroundWindow, TrackPopupMenu, TranslateMessage, IDI_APPLICATION, MF_SEPARATOR,
    MF_STRING, MSG, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_COMMAND,
    WM_DESTROY, WM_RBUTTONUP, WM_USER, WNDCLASSW, WS_OVERLAPPED,
};

const WM_TRAYICON: u32 = WM_USER + 1;
const IDM_OPEN_GUI: u32 = 100;
const IDM_PAUSE: u32 = 101;
const IDM_QUIT: u32 = 102;

pub fn run() -> Result<()> {
    unsafe {
        let h_inst = GetModuleHandleW(None)?;

        let class_name = w!("MingchuangSentryTrayWnd");
        let wc = WNDCLASSW {
            hInstance: h_inst.into(),
            lpszClassName: class_name,
            lpfnWndProc: Some(wnd_proc),
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("MingchuangSentry"),
            WS_OVERLAPPED,
            0, 0, 0, 0,
            None,
            None,
            Some(h_inst.into()),
            None,
        )?;

        // 添加托盘图标
        let icon = LoadIconW(None, IDI_APPLICATION).unwrap_or_default();
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_TRAYICON,
            hIcon: icon,
            ..Default::default()
        };
        // szTip 是 [u16; 128], 拷贝 "明窗守护" 进去
        let tip: Vec<u16> = "明窗守护 - 正在监控网络上行".encode_utf16().chain(std::iter::once(0)).collect();
        for (i, &c) in tip.iter().enumerate().take(127) {
            nid.szTip[i] = c;
        }
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);

        // 消息循环
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // 清理
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
    Ok(())
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match msg {
            WM_TRAYICON => {
                let evt = (lparam.0 & 0xFFFF) as u32;
                if evt == WM_RBUTTONUP {
                    show_context_menu(hwnd);
                }
            }
            WM_COMMAND => {
                let id = (wparam.0 & 0xFFFF) as u32;
                match id {
                    IDM_OPEN_GUI => {
                        // 启动 GUI exe (假设在同目录)
                        if let Ok(exe) = std::env::current_exe() {
                            if let Some(dir) = exe.parent() {
                                let gui = dir.join("mingchuang.exe");
                                if gui.is_file() {
                                    let _ = crate::sys_cmd_local::cmd(gui.to_str().unwrap_or_default()).spawn();
                                }
                            }
                        }
                    }
                    IDM_PAUSE => {
                        let mut c = crate::state_io::read_control();
                        c.paused_until = Some(chrono::Utc::now() + chrono::Duration::hours(1));
                        let _ = crate::state_io::write_control(&c);
                    }
                    IDM_QUIT => {
                        let mut c = crate::state_io::read_control();
                        c.stop_requested = true;
                        let _ = crate::state_io::write_control(&c);
                        PostQuitMessage(0);
                    }
                    _ => {}
                }
            }
            WM_DESTROY => {
                PostQuitMessage(0);
            }
            _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
    LRESULT(0)
}

unsafe fn show_context_menu(hwnd: HWND) {
    unsafe {
        let menu = match CreatePopupMenu() {
            Ok(m) => m,
            Err(_) => return,
        };
        let _ = AppendMenuW(menu, MF_STRING, IDM_OPEN_GUI as usize, w!("打开明窗"));
        let _ = AppendMenuW(menu, MF_STRING, IDM_PAUSE as usize, w!("暂停 1 小时"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, IDM_QUIT as usize, w!("退出守护"));

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(
            menu,
            TPM_BOTTOMALIGN | TPM_LEFTALIGN | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            None,
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), 0, WPARAM(0), LPARAM(0));
    }
}
