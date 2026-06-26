use embed_manifest::{embed_manifest, new_manifest};
use embed_manifest::manifest::ExecutionLevel;

fn main() {
    // Windows 下嵌入 UAC manifest, 强制以管理员身份运行
    // (本工具会改 HKLM / 停服务 / 禁计划任务 / 杀进程, 没有管理员一切免谈)
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() && std::env::var("PROFILE").as_deref() == Ok("release") {
        // 仅 release 嵌入: dev 时 cargo run 不弹 UAC, 调试更顺手
        // 实际生产: 用户运行 .exe 会弹 UAC, 拒绝即退出
        embed_manifest(
            new_manifest("Kuake.Fuckyou")
                .requested_execution_level(ExecutionLevel::RequireAdministrator)
        )
        .expect("无法嵌入 UAC manifest");
    }
    tauri_build::build()
}
