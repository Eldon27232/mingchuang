//! 服务状态快照与还原
//!
//! 保存:
//! - 原 ServiceState (Running / Stopped / Paused)
//! - 原 ServiceStartType (AutoStart / OnDemand / Disabled / SystemStart / BootStart)
//! 不保存 PathName/Type/账户等 — 我们不删服务,只 stop+disable,改回原 start_type 即还原。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use windows_service::service::{ServiceAccess, ServiceState};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceSnapshot {
    pub name: String,
    pub original_start_type: String, // 字符串化, 便于跨进程序列化
    pub original_state: String,
}

pub fn snapshot(name: &str) -> Result<ServiceSnapshot> {
    let scm = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT,
    )
    .context("打开 SCM 失败")?;
    let service = scm
        .open_service(
            name,
            ServiceAccess::QUERY_STATUS | ServiceAccess::QUERY_CONFIG,
        )
        .with_context(|| format!("打开服务失败: {name}"))?;

    let config = service.query_config().context("query_config 失败")?;
    let status = service.query_status().context("query_status 失败")?;

    Ok(ServiceSnapshot {
        name: name.to_string(),
        original_start_type: format!("{:?}", config.start_type),
        original_state: state_to_string(status.current_state),
    })
}

pub fn state_to_string(s: ServiceState) -> String {
    match s {
        ServiceState::Stopped => "Stopped",
        ServiceState::StartPending => "StartPending",
        ServiceState::StopPending => "StopPending",
        ServiceState::Running => "Running",
        ServiceState::ContinuePending => "ContinuePending",
        ServiceState::PausePending => "PausePending",
        ServiceState::Paused => "Paused",
    }
    .to_string()
}
