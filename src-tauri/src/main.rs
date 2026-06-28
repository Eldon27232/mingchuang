// 始终隐藏控制台窗口 (dev 和 release 都隐藏, 避免管理员模式弹黑框)
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    mingchuang_lib::run()
}
