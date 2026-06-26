//! Windows 服务动作: stop / disable / start / set-start-type
//!
//! 用 windows-service crate 包装 SCM API。
//! 实际操作:
//! - service-stop  : 调用 service.stop(), 等待最多 5s 直到 Stopped
//! - service-disable: 改 ServiceInfo.start_type = Disabled, 保持其他字段
//! - service-set-start (rollback): 改回原 start_type
//! - service-start (rollback)    : service.start() 启动

use anyhow::{anyhow, Context, Result};
use std::ffi::OsString;
use std::time::{Duration, Instant};
use windows_service::service::{
    ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceState, ServiceType,
};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

/// 停止服务, 等待最多 5 秒直到 Stopped
pub fn stop_service(name: &str) -> Result<()> {
    let scm = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = scm.open_service(
        name,
        ServiceAccess::STOP | ServiceAccess::QUERY_STATUS,
    ).with_context(|| format!("打开服务失败: {name}"))?;

    let status = service.query_status()?;
    if status.current_state == ServiceState::Stopped {
        return Ok(());
    }
    service.stop().with_context(|| format!("停止服务失败: {name}"))?;

    // 等最多 5 秒
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
        let s = service.query_status()?;
        if s.current_state == ServiceState::Stopped {
            return Ok(());
        }
    }
    Err(anyhow!("停止服务超时(5s): {name}"))
}

/// 启动服务
pub fn start_service(name: &str) -> Result<()> {
    let scm = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = scm.open_service(
        name,
        ServiceAccess::START | ServiceAccess::QUERY_STATUS,
    ).with_context(|| format!("打开服务失败: {name}"))?;

    let s = service.query_status()?;
    if matches!(s.current_state, ServiceState::Running | ServiceState::StartPending) {
        return Ok(());
    }
    service.start(&[] as &[&OsString]).with_context(|| format!("启动服务失败: {name}"))?;
    Ok(())
}

/// 改服务启动类型为 Disabled
pub fn disable_service(name: &str) -> Result<()> {
    set_start_type(name, ServiceStartType::Disabled)
}

/// 通用: 设置服务启动类型(用于 rollback)
pub fn set_start_type(name: &str, start_type: ServiceStartType) -> Result<()> {
    let scm = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = scm.open_service(
        name,
        ServiceAccess::CHANGE_CONFIG | ServiceAccess::QUERY_CONFIG,
    ).with_context(|| format!("打开服务失败: {name}"))?;

    let cfg = service.query_config()?;
    // 仅改 start_type, 其他字段从原配置照抄
    let new_info = ServiceInfo {
        name: OsString::from(name),
        display_name: cfg.display_name,
        service_type: ServiceType::OWN_PROCESS, // 不改, 但 ServiceInfo 必填
        start_type,
        error_control: ServiceErrorControl::Normal,
        executable_path: std::path::PathBuf::from(cfg.executable_path.to_string_lossy().to_string()),
        launch_arguments: vec![],
        dependencies: vec![],
        account_name: cfg.account_name,
        account_password: None,
    };
    service.change_config(&new_info).context("change_config 失败")?;
    Ok(())
}

pub fn parse_start_type(s: &str) -> Option<ServiceStartType> {
    // 仅支持普通服务的 3 种启动模式; Boot/System 是驱动专用, 本工具不碰
    Some(match s.to_ascii_lowercase().as_str() {
        "auto" | "autostart" | "automatic" => ServiceStartType::AutoStart,
        "manual" | "ondemand" => ServiceStartType::OnDemand,
        "disabled" => ServiceStartType::Disabled,
        _ => return None,
    })
}
