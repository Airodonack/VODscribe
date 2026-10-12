//! Declares the app's own commands to Tauri, so it checks each call against the
//! capabilities as it does for plugins. Without this, only pages on the app's
//! built-in scheme could call them, not the app server's http://localhost
//! origin (src/app_server.rs).
//!
//! The commands are read from `generate_handler!` in src/lib.rs, the one list
//! of them, and granted together as the `app` set in permissions/app.toml,
//! which this writes.

use std::fs;

fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");
    let commands = commands(&fs::read_to_string("src/lib.rs").expect("read src/lib.rs"));
    let set = commands.iter().map(|c| format!("  \"allow-{}\",\n", c.replace('_', "-"))).collect::<String>();
    let toml = format!(
        "# Written by build.rs from the commands in src/lib.rs; don't edit.\n\
         # Every command of the app, for the main window (capabilities/default.json).\n\
         [[set]]\n\
         identifier = \"app\"\n\
         description = \"VODscribe's own commands.\"\n\
         permissions = [\n{set}]\n"
    );
    fs::create_dir_all("permissions").expect("create permissions/");
    if fs::read_to_string("permissions/app.toml").ok().as_deref() != Some(toml.as_str()) {
        fs::write("permissions/app.toml", toml).expect("write permissions/app.toml");
    }
    // Tauri wants the names for the life of the program; a build script ends right after.
    let commands: &'static [&'static str] = commands.into_iter().map(|c| &*c.leak()).collect::<Vec<_>>().leak();
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(commands)),
    )
    .expect("failed to run tauri-build");
}

/// The command names (each path's last segment) in `generate_handler![...]`.
fn commands(lib_rs: &str) -> Vec<String> {
    let start =
        lib_rs.find("generate_handler![").expect("generate_handler! in src/lib.rs") + "generate_handler![".len();
    let end = start + lib_rs[start..].find(']').expect("end of generate_handler!");
    let commands: Vec<String> = lib_rs[start..end]
        .split(',')
        .map(|path| path.trim().rsplit("::").next().unwrap_or("").to_string())
        .filter(|name| !name.is_empty())
        .collect();
    assert!(!commands.is_empty(), "no commands found in generate_handler!");
    commands
}
