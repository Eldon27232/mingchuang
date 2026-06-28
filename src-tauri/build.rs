fn main() {
    // Windows 下给主 binary 打 UAC 标记 = requireAdministrator。
    //
    // 历史: 原先用 embed-manifest crate 嵌完整 manifest, Rust 1.81+ 之后
    // rustc 自己也会嵌一个默认 manifest, 两个 <assemblyIdentity name="1">
    // 撞到, CVTRES 报"资源重复". 解法: 不嵌完整 manifest, 只追加 /MANIFESTUAC
    // 链接器开关 → 由 link.exe 在 rustc 的默认 manifest 基础上叠加 UAC 字段,
    // 不冲突。
    //
    // 只对主 binary 加 (cargo:rustc-link-arg-bin=<name>=...), sentry 是
    // 后台监控守护进程, 在用户上下文跑就行, 不需要弹 UAC。
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some()
        && std::env::var("PROFILE").as_deref() == Ok("release")
    {
        println!(
            "cargo:rustc-link-arg-bin=mingchuang=/MANIFESTUAC:level='requireAdministrator' uiAccess='false'"
        );
    }
    tauri_build::build()
}
