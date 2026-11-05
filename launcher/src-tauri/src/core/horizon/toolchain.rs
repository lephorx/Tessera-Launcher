//! The Rust toolchain native mods compile the game with: the official `rustc`, `cargo` and
//! `rust-std` (and on Windows `rust-mingw` with WinLibs' MinGW-w64), each pinned by SHA-256,
//! downloaded once into `horizon/native/rust/` with its own Cargo home. No rustup, nothing on the
//! system, and none of the player's own Rust settings reach a build. The pins are the ones
//! Horizon's launcher uses (launcher/src/rust_toolchain.rs there, MIT), so builds match its own.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};

use super::super::paths::Paths;
use super::super::launch::Progress;

pub const VERSION: &str = "1.99.1";
const DIST: &str = "https://static.rust-lang.org/dist/2026-10-15";

struct Host {
    target: &'static str,
    components: &'static [(&'static str, &'static str)],
    mingw: bool,
}

const MINGW_URL: &str = "https://github.com/brechtsanders/winlibs_mingw/releases/download/16.2.0posix-23.1.3-14.0.0-msvcrt-r3/winlibs-x86_64-posix-seh-gcc-16.2.0-mingw-w64msvcrt-14.0.0-r3.zip";
const MINGW_SHA256: &str = "62af32ea58acdea1318ea5dd8eb7017eaee694dd40b269d52a59c769dcfefb82";

const HOSTS: &[Host] = &[
    Host {
        target: "aarch64-apple-darwin",
        components: &[
            ("rustc", "cc998c1dc8df006106472357aa87838b2da3f7b25446b98a94cfd58796cb0610"),
            ("cargo", "b5cc7634f919949c3e5e131123f813416dc47f6b809a5371d12f1f593595ae13"),
            ("rust-std", "c124006d63885ba1029aca49efd59e93be6b2b171c379318ff94a00ef5eba664"),
        ],
        mingw: false,
    },
    Host {
        target: "x86_64-unknown-linux-gnu",
        components: &[
            ("rustc", "85e7c0e6f21636f714e2a42966011364a6cc56b6548126d32144d6ba90904966"),
            ("cargo", "47babc028247e26577f8a1bf390b11f447ad9b7ab93f5c9a0dda078935fc149b"),
            ("rust-std", "d1290c05994a59c029e562cb75be1bddc6fb11a4c501e1b7b0f61d119c6cbb12"),
        ],
        mingw: false,
    },
    Host {
        target: "x86_64-pc-windows-gnu",
        components: &[
            ("rustc", "43b17e9a9134ea72562bfbb23728a830f671ea9be1db6a72597a8d240e986b35"),
            ("cargo", "57e54617be4420dbb967ca8293f5ace7168104ac12efa9dc7c5c3bda112f8b36"),
            ("rust-std", "6546651238a3ac092b763511be524ca6eda6103e5d47342416923be1b3ce837c"),
            ("rust-mingw", "30263d7bb4fd0bf63a153afe36d7fc102f929a7d42781651c6b3a03a414ba428"),
        ],
        mingw: true,
    },
];

fn host() -> Option<&'static Host> {
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("windows", "x86_64") => "x86_64-pc-windows-gnu",
        _ => return None,
    };
    HOSTS.iter().find(|h| h.target == target)
}

/// Where native builds keep their things: an ASCII path the compiler's tools can read.
pub fn build_root(paths: &Paths) -> PathBuf {
    let dir = super::root(paths).join("native");
    let _ = std::fs::create_dir_all(&dir);
    // horizon-mixins refuses links: the folder itself, resolved, without Windows' \\?\ prefix
    // (MinGW's gcc then can't find its own crt2.o, kernel32 or user32)
    dunce::canonicalize(&dir).unwrap_or(dir)
}

#[derive(Clone)]
pub struct Toolchain {
    pub dir: PathBuf,
    cargo_home: PathBuf,
    tmp: PathBuf,
}

/// Variables of the player's that would change a build (another Rust, a wrapper, compiler
/// flags): left out, so builds are the release's and reuse each other.
fn scrubbed(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    if name == "CARGO_BUILD_JOBS" {
        return false;
    }
    ["RUSTUP_TOOLCHAIN", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTDOCFLAGS", "CARGO_ENCODED_RUSTDOCFLAGS", "CARGO_INCREMENTAL"].contains(&name.as_str())
        || ["CARGO_PROFILE_", "CARGO_BUILD_", "CARGO_TARGET_"].iter().any(|p| name.starts_with(p))
}

impl Toolchain {
    pub fn cargo(&self) -> PathBuf {
        self.dir.join("bin").join(super::exe("cargo"))
    }

    /// `program` with only this toolchain: its rustc and MinGW, its Cargo home, started from
    /// the disk's root (Cargo reads every `.cargo/config.toml` above the folder it starts in,
    /// the player's `~/.cargo` too), at low priority so the computer stays smooth.
    pub fn command(&self, program: &Path) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(program);
        let path = std::env::var_os("PATH").unwrap_or_default();
        let mingw = Some(self.dir.join("mingw64").join("bin")).filter(|b| b.is_dir());
        let paths = std::iter::once(self.dir.join("bin")).chain(mingw).chain(std::env::split_paths(&path));
        if let Some(root) = self.cargo_home.ancestors().last().filter(|r| r.has_root()) {
            cmd.current_dir(root);
        }
        for (name, _) in std::env::vars_os() {
            if name.to_str().is_some_and(scrubbed) {
                cmd.env_remove(name);
            }
        }
        cmd.env("PATH", std::env::join_paths(paths).unwrap_or(path)).env("RUSTC", self.dir.join("bin").join(super::exe("rustc"))).env("CARGO_HOME", &self.cargo_home);
        let _ = std::fs::create_dir_all(&self.tmp);
        cmd.env("TMPDIR", &self.tmp).env("TMP", &self.tmp).env("TEMP", &self.tmp);
        cmd.env_remove("DYLD_FALLBACK_LIBRARY_PATH").env_remove("DYLD_LIBRARY_PATH");
        low_priority(&mut cmd);
        cmd
    }
}

fn low_priority(cmd: &mut tokio::process::Command) {
    #[cfg(unix)]
    // SAFETY: between fork and exec only setpriority, which is async-signal-safe, on the child
    unsafe {
        cmd.pre_exec(|| {
            libc::setpriority(libc::PRIO_PROCESS, 0, 19);
            Ok(())
        });
    }
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000 | 0x0000_4000); // CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS
}

/// What a build needs from the computer besides Rust: the system's linker on macOS (Apple's
/// Command Line Tools) and Linux (`cc`, the C headers, and the sound and controller libraries'
/// development files the game links). Said before anything is downloaded.
pub fn check_linker(full: bool) -> Result<()> {
    let ok = |program: &str, args: &[&str]| std::process::Command::new(program).args(args).output().is_ok_and(|o| o.status.success());
    match std::env::consts::OS {
        "macos" if !ok("xcode-select", &["-p"]) => bail!("Native mods need Apple's Command Line Tools to link the game. Run `xcode-select --install` in Terminal, then play again."),
        "linux" => {
            let cc = ok("cc", &["--version"]);
            let mut missing = Vec::new();
            if !cc {
                missing.push("a C compiler (cc)");
            } else if !ok("cc", &["-fsyntax-only", "-x", "c", "-include", "stdint.h", "/dev/null"]) {
                missing.push("the C library's headers");
            }
            if full {
                let pc = ok("pkg-config", &["--version"]);
                if !pc {
                    missing.push("pkg-config");
                }
                if !(pc && ok("pkg-config", &["--exists", "alsa"])) {
                    missing.push("ALSA's development files");
                }
                if !(pc && ok("pkg-config", &["--exists", "libudev"])) {
                    missing.push("libudev's development files");
                }
            }
            if missing.is_empty() {
                return Ok(());
            }
            let os = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
            let like = |names: &[&str]| os.lines().filter_map(|l| l.strip_prefix("ID=").or_else(|| l.strip_prefix("ID_LIKE="))).flat_map(|v| v.trim_matches('"').split_whitespace().map(str::to_string).collect::<Vec<_>>()).any(|n| names.contains(&n.as_str()));
            let how = if like(&["debian", "ubuntu"]) {
                "sudo apt install build-essential pkg-config libasound2-dev libudev-dev"
            } else if like(&["fedora", "rhel"]) {
                "sudo dnf install gcc pkgconf-pkg-config alsa-lib-devel systemd-devel"
            } else if like(&["arch"]) {
                "sudo pacman -S --needed base-devel alsa-lib systemd-libs"
            } else if like(&["suse", "opensuse"]) {
                "sudo zypper install gcc pkg-config alsa-devel libudev-devel"
            } else {
                "your distribution's package manager"
            };
            bail!("Native mods need these to build the game, and this computer lacks them: {}. Install them ({how}), then play again.", missing.join(", "))
        }
        _ => Ok(()),
    }
}

/// The toolchain, downloaded, checked and unpacked once.
pub async fn install(client: &reqwest::Client, paths: &Paths, progress: Progress<'_>) -> Result<Toolchain> {
    let host = host().ok_or_else(|| anyhow!("Native mods need Rust, and there is none pinned for {} {} yet", std::env::consts::OS, std::env::consts::ARCH))?;
    let root = build_root(paths);
    let dir = root.join("rust").join(format!("{VERSION}-{}", host.target));
    let chain = Toolchain { dir: dir.clone(), cargo_home: root.join("cargo"), tmp: root.join("tmp") };
    if dir.join(".installed").is_file() {
        return Ok(chain);
    }
    let partial = root.join("rust").join(format!(".{VERSION}-{}.partial", host.target));
    let _ = std::fs::remove_dir_all(&partial);
    std::fs::create_dir_all(&partial)?;
    for (name, sha) in host.components {
        let file = format!("{name}-{VERSION}-{}.tar.gz", host.target);
        let archive = root.join("rust").join(format!(".{file}"));
        let stage = format!("Downloading Rust {VERSION} ({name})");
        super::fetch_sha256(client, &format!("{DIST}/{file}"), &archive, sha, &stage, progress).await?;
        progress(&format!("Unpacking Rust ({name})"), 0, 0);
        let (a, p) = (archive.clone(), partial.clone());
        tokio::task::spawn_blocking(move || unpack_component(&a, &p)).await??;
        let _ = std::fs::remove_file(&archive);
    }
    if host.mingw {
        let archive = root.join("rust").join(".mingw.zip");
        super::fetch_sha256(client, MINGW_URL, &archive, MINGW_SHA256, "Downloading MinGW-w64", progress).await?;
        progress("Unpacking MinGW-w64", 0, 0);
        let (a, p) = (archive.clone(), partial.clone());
        tokio::task::spawn_blocking(move || unpack_mingw(&a, &p)).await??;
        let _ = std::fs::remove_file(&archive);
    }
    std::fs::write(partial.join(".installed"), VERSION)?;
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&partial, &dir)?;
    Ok(chain)
}

/// One component archive into `prefix`, as its installer would: `<top>/<component>/<files>`
/// becomes `<files>` (the installer's own files stay out).
fn unpack_component(archive: &Path, prefix: &Path) -> Result<()> {
    use std::path::Component;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(std::fs::File::open(archive)?));
    for entry in tar.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let parts: Option<Vec<&std::ffi::OsStr>> = path.components().map(|c| match c {
            Component::Normal(n) => Some(n),
            _ => None,
        }).collect();
        let Some(parts) = parts else { continue };
        let rel: PathBuf = match parts.as_slice() {
            [_, _, rest @ ..] if !rest.is_empty() && rest != ["manifest.in"] => rest.iter().collect(),
            _ => continue,
        };
        let to = prefix.join(&rel);
        super::super::paths::ensure_parent(&to)?;
        entry.unpack(&to).with_context(|| rel.display().to_string())?;
    }
    Ok(())
}

/// MinGW-w64's `mingw64/` folder, without its documentation.
fn unpack_mingw(archive: &Path, prefix: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?)?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else { continue };
        let mut parts = rel.components();
        let top = parts.next().map(|c| c.as_os_str().to_owned());
        let first = parts.next().map(|c| c.as_os_str().to_owned());
        if top.as_deref() != Some(std::ffi::OsStr::new("mingw64")) || first.as_deref() == Some(std::ffi::OsStr::new("share")) {
            continue;
        }
        let to = prefix.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&to)?;
            continue;
        }
        super::super::paths::ensure_parent(&to)?;
        std::io::copy(&mut entry, &mut std::fs::File::create(&to)?)?;
    }
    Ok(())
}
