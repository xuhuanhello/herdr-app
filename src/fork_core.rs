//! First install of the personal fork's Core. Later upgrades use its own hc updater.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const RELEASES: Option<&str> = option_env!("HERDR_APP_FORK_CORE_RELEASES");

pub fn build_number(version: &str) -> Option<u64> {
    let (base, build) = version.trim_start_matches("hc ").split_once("+fork.xuhuanhello.")?;
    let parts: Vec<_> = base.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|part| part.parse::<u64>().is_err())
        || build.starts_with('0') || !build.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    build.parse().ok()
}

pub fn accepts(version: &str) -> bool {
    RELEASES.is_none() || build_number(version).is_some()
}

pub fn directory(home: &Path) -> PathBuf {
    if cfg!(windows) {
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| home.join("AppData/Local"));
        local.join("Programs/herdr-core-xuhuanhello")
    } else {
        home.join(".local/share/herdr-core-xuhuanhello")
    }
}

pub fn executable(dir: &Path) -> PathBuf {
    if cfg!(windows) { dir.join("herdr-core.exe") } else { dir.join("bin/herdr-core") }
}

#[derive(serde::Deserialize)]
pub struct Manifest {
    pub version: String,
    pub generation: u64,
    targets: std::collections::HashMap<String, Archive>,
}

#[derive(serde::Deserialize)]
struct Archive {
    url: String,
    sha256: String,
}

impl Manifest {
    fn archive(&self, releases: &str, target: &str, name: &str) -> Result<&Archive, String> {
        let build = build_number(&self.version).ok_or("invalid fork Core version")?;
        if self.generation != crate::hc::GENERATION || crate::hc::semver(&self.version) < crate::hc::semver(crate::hc::core_min()) {
            return Err(format!("Core {} is incompatible with this App. Check both fork releases.", self.version));
        }
        let archive = self.targets.get(target).ok_or_else(|| format!("Core {} has no package for {target}", self.version))?;
        if archive.url != format!("{releases}/download/fork-{build}/{name}")
            || archive.sha256.len() != 64 || !archive.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("unexpected Core download URL or checksum in the fork manifest".into());
        }
        Ok(archive)
    }
}

pub fn manifest(releases: &str) -> Result<Manifest, String> {
    let text = crate::update::curl_text(&format!("{releases}/latest/download/manifest.json"))?;
    let manifest: Manifest = serde_json::from_str(&text).map_err(|e| format!("Core manifest: {e}"))?;
    if build_number(&manifest.version).is_none() {
        return Err("the Core manifest belongs to another release channel".into());
    }
    Ok(manifest)
}

pub fn install(releases: &str, dir: &Path) -> Result<PathBuf, String> {
    let bin = executable(dir);
    if crate::core_bin::usable_version(&bin).is_some() {
        return Ok(bin);
    }
    if dir.exists() {
        return Err(format!("{} already exists without a usable fork Core. Move the directory aside before retrying.", dir.display()));
    }
    let parent = dir.parent().ok_or("Core install path has no parent")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
    let scratch = parent.join(format!(".herdr-core-{}-{stamp}", std::process::id()));
    fs::create_dir(&scratch).map_err(|e| e.to_string())?;
    let result = install_from(releases, dir, &scratch);
    let _ = fs::remove_dir_all(scratch);
    result.map(|()| bin)
}

fn install_from(releases: &str, dir: &Path, scratch: &Path) -> Result<(), String> {
    let manifest = manifest(releases)?;
    let target = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let extension = if cfg!(windows) { "zip" } else { "tar.gz" };
    let name = format!("herdr-core-{target}.{extension}");
    let archive = manifest.archive(releases, &target, &name)?;
    let downloaded = scratch.join(name);
    crate::update::run(crate::update::release_curl().args(["--max-time", "600", "-o"]).arg(&downloaded).arg(&archive.url))?;
    let bytes = fs::read(&downloaded).map_err(|e| e.to_string())?;
    if !crate::update::sha256(&bytes).eq_ignore_ascii_case(&archive.sha256) {
        return Err("Core download checksum mismatch".into());
    }
    let staged = scratch.join("install");
    fs::create_dir(&staged).map_err(|e| e.to_string())?;
    let members: &[&str] = if cfg!(windows) { &["herdr-core.exe", "conpty"] } else { &["Herdr Core.app"] };
    crate::update::run(crate::spawn::command(crate::update::system("tar"))
        .arg("-xf").arg(downloaded).arg("-C").arg(&staged).args(members)
        .args(["LICENSE", "THIRD-PARTY-NOTICES"]))?;
    prepare_install(&staged)?;
    if crate::core_bin::usable_version(&executable(&staged)).as_deref() != Some(&format!("hc {}", manifest.version)) {
        return Err("the installed Core version does not match the release manifest".into());
    }
    match fs::rename(staged, dir) {
        Ok(()) => Ok(()),
        Err(_) if crate::core_bin::usable_version(&executable(dir)).is_some() => Ok(()),
        Err(e) => Err(format!("installing Core into {}: {e}", dir.display())),
    }
}

fn prepare_install(staged: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let bundle = staged.join("Herdr Core.app");
        let active = staged.join("active");
        let aliases = staged.join("bin");
        crate::update::run(std::process::Command::new("/usr/bin/codesign").args(["--verify", "--deep", "--strict"]).arg(&bundle))?;
        fs::create_dir(active.as_path()).and_then(|()| fs::create_dir(aliases.as_path()))
            .and_then(|()| fs::rename(&bundle, active.join("Herdr Core.app")))
            .and_then(|()| std::os::unix::fs::symlink("../active/Herdr Core.app/Contents/MacOS/herdr-core", aliases.join("herdr-core")))
            .and_then(|()| std::os::unix::fs::symlink("herdr-core", aliases.join("hc")))
            .map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    {
        if !staged.join("conpty/conpty.dll").is_file() || !staged.join("conpty/x64/OpenConsole.exe").is_file() {
            return Err("Core archive is missing ConPTY".into());
        }
        fs::hard_link(staged.join("herdr-core.exe"), staged.join("hc.exe")).map_err(|e| e.to_string())?;
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = staged;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fork_release_requires_the_same_owner_version_and_archive_url() {
        let releases = "https://github.com/xuhuanhello/herdr-core/releases";
        let target = "windows-x86_64";
        let name = "herdr-core-windows-x86_64.zip";
        let mut manifest: Manifest = serde_json::from_value(serde_json::json!({
            "version": "0.1.22+fork.xuhuanhello.10",
            "generation": crate::hc::GENERATION,
            "targets": {(target): { "url": format!("{releases}/download/fork-10/{name}"), "sha256": "a".repeat(64) }},
        })).unwrap();
        assert!(manifest.archive(releases, target, name).is_ok());
        manifest.targets.get_mut(target).unwrap().url = format!("https://example.org/{name}");
        assert!(manifest.archive(releases, target, name).is_err());
        assert_eq!(build_number("hc 0.1.22+fork.xuhuanhello.10"), Some(10));
        assert_eq!(build_number("hc 0.1.22+fork.other.10"), None);
        assert_eq!(build_number("hc 0.1.22"), None);
    }
}
