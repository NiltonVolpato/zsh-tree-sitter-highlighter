use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    // Call the standard setup first (this sets up linker args, cfgs, etc.)
    zsh_module_build::setup_zsh_module();

    // Now, create symlinks for the custom library name "treeizsh"
    // (since our package name is "treeizsh-module" but our library name is "treeizsh").
    create_custom_symlinks("treeizsh");
}

fn create_custom_symlinks(module_name: &str) {
    let Ok(out_dir_str) = env::var("OUT_DIR") else {
        return;
    };
    let out_dir = PathBuf::from(out_dir_str);
    let mut profile_dir = out_dir.clone();
    for _ in 0..3 {
        profile_dir.pop();
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let ext = if target_os == "macos" { "dylib" } else { "so" };

    // Create symlinks in profile_dir (target/debug/ or target/release/)
    let future_dylib = profile_dir.join(format!("lib{}.{}", module_name, ext));
    create_symlinks_in_dir(&profile_dir, module_name, &future_dylib, &target_os);

    // Create symlinks in deps_dir (target/debug/deps/ or target/release/deps/)
    let deps_dir = profile_dir.join("deps");
    let _ = fs::create_dir_all(&deps_dir);
    let future_dylib_deps = deps_dir.join(format!("lib{}.{}", module_name, ext));
    create_symlinks_in_dir(&deps_dir, module_name, &future_dylib_deps, &target_os);
}

fn create_symlinks_in_dir(dir: &Path, module_name: &str, target_path: &Path, target_os: &str) {
    let so_symlink = dir.join(format!("{}.so", module_name));
    let _ = fs::remove_file(&so_symlink);
    let _ = std::os::unix::fs::symlink(target_path, &so_symlink);

    if target_os == "macos" {
        let bundle_symlink = dir.join(format!("{}.bundle", module_name));
        let _ = fs::remove_file(&bundle_symlink);
        let _ = std::os::unix::fs::symlink(target_path, &bundle_symlink);
    }
}
