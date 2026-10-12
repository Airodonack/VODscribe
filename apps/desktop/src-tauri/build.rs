// The app's own commands, as registered in lib.rs. Declaring them makes Tauri
// check each call against the capabilities, as it does for plugins: the main
// window gets them through the `app` set in permissions/app.toml. Without this,
// only pages on the app's built-in scheme could call them, not the app server's
// http://localhost origin (src/app_server.rs).
const COMMANDS: &[&str] = &[
    "setup_status",
    "data_dir",
    "open_data_dir",
    "run_setup",
    "add_vod",
    "list_vods",
    "add_files",
    "show_file",
    "open_file",
    "media_url",
    "thumbnail",
    "reorder_vods",
    "delete_vod",
    "retry_vod",
    "pause_vod",
    "retranscribe_vod",
    "resume_vod",
    "cancel_vod",
    "search",
    "transcript",
    "update_yt_dlp",
    "kick_stream",
    "get_settings",
    "set_settings",
    "gpu_status",
    "install_gpu_pack",
    "remove_gpu_pack",
    "cache_size",
    "clear_cache",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
