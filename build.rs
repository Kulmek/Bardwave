//! Build script — embeds the Win32 resources from `assets/icon.rc` into the
//! executable.
//!
//! Why a build script instead of a crate: it keeps the dependency list at zero.
//! `rc.exe` (Windows SDK, or the copy that ships with MSVC) is located
//! automatically — from `RC`/`RC_EXE`, then `PATH`, then the installed Windows
//! Kits, then a `vswhere` lookup — compiled to a `.res`, and handed to the
//! linker. If no resource compiler is available the icon is skipped with a
//! warning rather than failing the build.
//!
//! Regenerate the icon itself with `node assets/tools/gen-icon.mjs`; the sources it
//! reads are committed under `assets/`.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    // The icon is produced by `tools/gen-icon.mjs`; rebuild when it changes.
    println!("cargo:rerun-if-changed=assets/icon.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");

    // Nothing to embed on non-Windows targets.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let assets = manifest_dir.join("assets");
    let rc_source = assets.join("icon.rc");

    if !rc_source.exists() {
        println!(
            "cargo:warning={} is missing; building without an application icon",
            rc_source.display()
        );
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let res_path = out_dir.join("soundshared.res");

    let Some(rc) = find_resource_compiler() else {
        println!(
            "cargo:warning=no resource compiler (rc.exe) found; \
             building without an application icon. Install the Windows SDK \
             or set RC to its path."
        );
        return;
    };

    // rc.exe resolves `icon.ico` relative to its working directory, so run it
    // from `assets/`.
    let output = Command::new(&rc)
        .current_dir(&assets)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res_path)
        .arg(&rc_source)
        .output();

    match output {
        Ok(out) if out.status.success() && res_path.exists() => {
            // MSVC's link.exe takes .res files directly on its command line.
            println!("cargo:rustc-link-arg-bins={}", res_path.display());
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            println!(
                "cargo:warning=rc.exe ({}) failed for {}: {}",
                rc.display(),
                rc_source.display(),
                stderr.trim()
            );
        }
        Err(err) => {
            println!("cargo:warning=could not run {}: {err}", rc.display());
        }
    }
}

/// Locates `rc.exe`, newest installation first.
fn find_resource_compiler() -> Option<PathBuf> {
    for var in ["RC", "RC_EXE", "WINDRES"] {
        if let Ok(value) = env::var(var) {
            let candidate = PathBuf::from(value);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    if let Some(found) = find_on_path("rc.exe") {
        return Some(found);
    }

    if let Some(found) = windows_kits_resource_compiler() {
        return Some(found);
    }

    vswhere_resource_compiler()
}

fn find_on_path(exe: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(exe))
        .find(|candidate| candidate.is_file())
}

/// `%ProgramFiles(x86)%\Windows Kits\10\bin\<version>\<arch>\rc.exe`.
fn windows_kits_resource_compiler() -> Option<PathBuf> {
    let kits_root = program_files_x86()?
        .join("Windows Kits")
        .join("10")
        .join("bin");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(&kits_root)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && has_version_like_name(path))
        .collect();

    // Newest SDK first: parse "10.0.26100.0" into numeric components.
    versions.sort_by_key(|path| version_key(path));
    versions.reverse();

    for version in versions {
        for arch in ["x64", "arm64", "x86"] {
            let candidate = version.join(arch).join("rc.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

fn has_version_like_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.chars().next().is_some_and(|c| c.is_ascii_digit()) && name.contains('.')
        })
}

fn version_key(path: &Path) -> Vec<u32> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    name.split('.')
        .map(|part| part.parse::<u32>().unwrap_or(0))
        .collect()
}

/// Asks the Visual Studio installer for the newest VC++ toolset.
fn vswhere_resource_compiler() -> Option<PathBuf> {
    let vswhere = program_files_x86()?
        .join("Microsoft Visual Studio")
        .join("Installer")
        .join("vswhere.exe");
    if !vswhere.is_file() {
        return None;
    }

    let output = Command::new(&vswhere)
        .args([
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let install = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_string());
    if install.as_os_str().is_empty() {
        return None;
    }

    let msvc_root = install.join("VC").join("Tools").join("MSVC");
    let mut toolsets: Vec<PathBuf> = std::fs::read_dir(&msvc_root)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    toolsets.sort_by_key(|path| version_key(path));
    toolsets.reverse();

    for toolset in toolsets {
        for host in ["Hostx64", "Hostx86"] {
            for arch in ["x64", "x86"] {
                let candidate = toolset.join("bin").join(host).join(arch).join("rc.exe");
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

fn program_files_x86() -> Option<PathBuf> {
    for var in ["ProgramFiles(x86)", "ProgramFiles", "ProgramW6432"] {
        if let Ok(value) = env::var(var) {
            let path = PathBuf::from(value);
            if path.is_dir() {
                return Some(path);
            }
        }
    }
    None
}
