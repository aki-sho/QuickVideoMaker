#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if quick_video_maker_lib::run_analysis_worker() {
        return;
    }
    if let Err(error) = quick_video_maker_lib::run() {
        eprintln!("QuickVideoMaker failed: {error}");
    }
}
