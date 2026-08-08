#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(error) = quick_video_maker_lib::run() {
        eprintln!("QuickVideoMaker failed: {error}");
    }
}
