//! WinRT actionable toast 通知
//!
//! 用户在 toast 上能直接点 [立即停止]/[加白名单]/[保持运行] 三个按钮。
//! 回传机制: protocol activation — 按钮 activationType="protocol",
//! arguments = "mingchuang://action=kill&pid=...", Windows 用我们注册的
//! mingchuang:// scheme 拉起 mingchuang-sentry.exe + URL 参数, sentry main 解析
//! 然后写 control.json 把动作通知主守护进程。
//!
//! 前置一次性注册: AUMID + 开始菜单 .lnk + mingchuang:// protocol scheme
//! 这些由 register_app_metadata() 在 sentry 启动时调用, idempotent。

use anyhow::{anyhow, Context, Result};
use windows::core::HSTRING;
use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

const AUMID: &str = "io.mingchuang.sentry";

/// 弹一个带按钮的 toast。
/// pid + image_name 用于按钮 protocol arguments。
pub fn show_actionable_toast(title: &str, body: &str, pid: u32, image_name: &str) -> Result<()> {
    let xml = format!(
        r#"<toast activationType="protocol" launch="mingchuang://action=open">
  <visual>
    <binding template="ToastGeneric">
      <text>{title}</text>
      <text>{body}</text>
    </binding>
  </visual>
  <actions>
    <action content="保持运行" activationType="protocol" arguments="mingchuang://action=ignore&amp;pid={pid}&amp;name={image_enc}"/>
    <action content="加入白名单" activationType="protocol" arguments="mingchuang://action=whitelist&amp;name={image_enc}"/>
    <action content="立即停止" activationType="protocol" arguments="mingchuang://action=kill&amp;pid={pid}&amp;name={image_enc}"/>
  </actions>
</toast>"#,
        title = escape_xml(title),
        body = escape_xml(body),
        image_enc = urlencode(image_name),
    );

    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(&xml))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?;
    notifier.Show(&toast)?;
    Ok(())
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.bytes() {
        match c {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(c as char)
            }
            _ => out.push_str(&format!("%{c:02X}")),
        }
    }
    out
}

// ============ 一次性注册 ============

/// idempotent: 注册 AUMID + 开始菜单 .lnk + mingchuang:// protocol
pub fn register_app_metadata() -> Result<()> {
    register_aumid()?;
    register_start_menu_shortcut()?;
    register_protocol()?;
    Ok(())
}

const ICON_REL: &str = ""; // 暂无图标资源

fn register_aumid() -> Result<()> {
    use windows_registry::CURRENT_USER;
    let path = format!("Software\\Classes\\AppUserModelId\\{AUMID}");
    let key = CURRENT_USER.create(&path).map_err(|e| anyhow!("{e}"))?;
    key.set_string("DisplayName", "明窗守护").map_err(|e| anyhow!("{e}"))?;
    if !ICON_REL.is_empty() {
        let _ = key.set_string("IconUri", ICON_REL);
    }
    Ok(())
}

fn register_protocol() -> Result<()> {
    use windows_registry::CURRENT_USER;
    let exe = std::env::current_exe().context("当前 exe")?;
    let cmd = format!("\"{}\" \"%1\"", exe.display());
    let base = "Software\\Classes\\mingchuang";
    let root = CURRENT_USER.create(base).map_err(|e| anyhow!("{e}"))?;
    root.set_string("", "URL:Mingchuang Protocol").map_err(|e| anyhow!("{e}"))?;
    root.set_string("URL Protocol", "").map_err(|e| anyhow!("{e}"))?;
    let cmd_key = CURRENT_USER
        .create(format!("{base}\\shell\\open\\command"))
        .map_err(|e| anyhow!("{e}"))?;
    cmd_key.set_string("", &cmd).map_err(|e| anyhow!("{e}"))?;
    Ok(())
}

/// 在开始菜单创建普通 .lnk 指向 sentry.exe
/// 注: AUMID 属性写入 .lnk 需要 IPropertyStore (feature Win32_UI_Shell_PropertiesSystem),
/// 简化版只创建 .lnk, toast 在 Win11 上仍能用 HKCU\Software\Classes\AppUserModelId 注册显示。
fn register_start_menu_shortcut() -> Result<()> {
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    let appdata = std::env::var("APPDATA").context("APPDATA")?;
    let lnk_dir = std::path::PathBuf::from(appdata)
        .join("Microsoft\\Windows\\Start Menu\\Programs");
    std::fs::create_dir_all(&lnk_dir).ok();
    let lnk_path = lnk_dir.join("明窗守护.lnk");
    let exe = std::env::current_exe().context("当前 exe")?;

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let did_init = hr.is_ok();

        let result: Result<()> = (|| {
            let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                .context("CoCreateInstance ShellLink")?;
            let exe_w: Vec<u16> = exe
                .to_string_lossy()
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            shell_link.SetPath(PCWSTR(exe_w.as_ptr())).context("SetPath")?;

            let persist: IPersistFile =
                Interface::cast(&shell_link).context("cast IPersistFile")?;
            let lnk_w: Vec<u16> = lnk_path
                .to_string_lossy()
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            persist.Save(PCWSTR(lnk_w.as_ptr()), true).context("Save lnk")?;
            Ok(())
        })();

        if did_init {
            CoUninitialize();
        }
        result?;
    }
    Ok(())
}
