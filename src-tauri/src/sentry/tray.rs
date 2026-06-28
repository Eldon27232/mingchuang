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
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DispatchMessageW, GetCursorPos, GetMessageW, HICON, LoadIconW, LR_DEFAULTCOLOR, PostMessageW,
    PostQuitMessage, RegisterClassW, SetForegroundWindow, TrackPopupMenu, TranslateMessage,
    IDI_APPLICATION, MF_SEPARATOR, MF_STRING, MSG, TPM_BOTTOMALIGN, TPM_LEFTALIGN,
    TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_COMMAND, WM_DESTROY, WM_RBUTTONUP, WM_USER, WNDCLASSW,
    WS_OVERLAPPED,
};

/// 编译期内联 app icon.ico, 给托盘用。
/// (sentry 是独立 binary, tauri-build 只给主 exe 嵌 icon, sentry 自己不嵌就只能这么干)
const ICON_BYTES: &[u8] = include_bytes!("../../icons/icon.ico");

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

        // 添加托盘图标 — 优先用内联的 app icon, 失败兜底 IDI_APPLICATION
        let icon = load_app_icon().unwrap_or_else(|| {
            LoadIconW(None, IDI_APPLICATION).unwrap_or_default()
        });
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

/// 从内联的 ICO 字节里挑一个 32×32 (托盘标准尺寸) 的入口, 调
/// CreateIconFromResourceEx 转 HICON。
///
/// ICO 格式 (Win32 标准):
///   [0..6]  ICONDIR: reserved u16=0, type u16=1 (icon), count u16
///   [6..6+count*16]  ICONDIRENTRY × count:
///     [0]   width u8 (0 = 256)
///     [1]   height u8
///     [2]   colors u8
///     [3]   reserved u8
///     [4..6] planes u16
///     [6..8] bpp u16
///     [8..12] image data size u32
///     [12..16] image data offset u32
///   image data = BITMAPINFOHEADER + DIB pixels (或 PNG, 这里假设是 BMP DIB)
unsafe fn load_app_icon() -> Option<HICON> {
    let bytes = ICON_BYTES;
    if bytes.len() < 6 {
        return None;
    }
    let reserved = u16::from_le_bytes([bytes[0], bytes[1]]);
    let img_type = u16::from_le_bytes([bytes[2], bytes[3]]);
    let count = u16::from_le_bytes([bytes[4], bytes[5]]);
    if reserved != 0 || img_type != 1 || count == 0 {
        return None;
    }

    // 挑离 32 最近的入口 (倾向大于 32 而不是小于)
    const TARGET: u32 = 32;
    let mut best: Option<(u32, u32, u32)> = None; // (score, size, offset)
    for i in 0..count as usize {
        let entry = 6 + i * 16;
        if entry + 16 > bytes.len() {
            break;
        }
        let w_raw = bytes[entry];
        let actual_w = if w_raw == 0 { 256 } else { w_raw as u32 };
        let size =
            u32::from_le_bytes([bytes[entry + 8], bytes[entry + 9], bytes[entry + 10], bytes[entry + 11]]);
        let offset = u32::from_le_bytes([
            bytes[entry + 12],
            bytes[entry + 13],
            bytes[entry + 14],
            bytes[entry + 15],
        ]);
        // score: 越接近 TARGET 越小; 比 TARGET 小的多罚一倍
        let score = if actual_w >= TARGET {
            actual_w - TARGET
        } else {
            (TARGET - actual_w) * 2
        };
        match best {
            None => best = Some((score, size, offset)),
            Some((bs, _, _)) if score < bs => best = Some((score, size, offset)),
            _ => {}
        }
    }

    let (_, size, offset) = best?;
    let size = size as usize;
    let offset = offset as usize;
    if offset + size > bytes.len() || size == 0 {
        return None;
    }
    let img = &bytes[offset..offset + size];

    // 0x00030000 = ICON 资源版本号 (Win32 magic)
    CreateIconFromResourceEx(img, true, 0x00030000, 32, 32, LR_DEFAULTCOLOR).ok()
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
                        // 启动 GUI exe (在同目录)。
                        // 用 ShellExecuteW("open") 而不是 Command::new spawn:
                        //  - mingchuang.exe 有 requireAdministrator manifest, CreateProcess
                        //    从非提权 sentry 直接 spawn 会因 ERROR_ELEVATION_REQUIRED 失败
                        //    或环境变量传递异常 (这是 AI key 从托盘启动时丢失的根因)
                        //  - ShellExecute("open") 走 Windows shell, 会正确触发 UAC 弹窗,
                        //    且新进程继承用户标准环境 (APPDATA 不漂)
                        if let Ok(exe) = std::env::current_exe() {
                            if let Some(dir) = exe.parent() {
                                let gui = dir.join("mingchuang.exe");
                                if gui.is_file() {
                                    use windows::core::{w, PCWSTR};
                                    use windows::Win32::UI::Shell::ShellExecuteW;
                                    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
                                    let path_wide: Vec<u16> = gui.as_os_str()
                                        .to_string_lossy()
                                        .encode_utf16()
                                        .chain(std::iter::once(0))
                                        .collect();
                                    let dir_wide: Vec<u16> = dir.as_os_str()
                                        .to_string_lossy()
                                        .encode_utf16()
                                        .chain(std::iter::once(0))
                                        .collect();
                                    ShellExecuteW(
                                        None,
                                        w!("open"),
                                        PCWSTR(path_wide.as_ptr()),
                                        PCWSTR::null(),
                                        PCWSTR(dir_wide.as_ptr()),
                                        SW_SHOWNORMAL,
                                    );
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
