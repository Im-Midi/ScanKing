// 桌面端入口（Android 走 lib.rs 的 mobile_entry_point）
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    scanking_lib::run()
}
