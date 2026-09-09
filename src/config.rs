use serde::{Deserialize, Serialize};
use std::io::Write;
use std::{
    env, fs, io,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::PathBuf,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub enabled: bool,
    pub acceleration: f64,
    pub coast: bool,
    pub selected_devices: Vec<String>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            enabled: true,
            acceleration: 3.0,
            coast: false,
            selected_devices: vec![],
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("Unsupported configuration version".into());
        }
        if !self.acceleration.is_finite() || !(1.0..=8.0).contains(&self.acceleration) {
            return Err("Acceleration must be a finite number from 1 to 8".into());
        }
        if self.selected_devices.len() > 64
            || self
                .selected_devices
                .iter()
                .any(|s| s.is_empty() || s.len() > 1024)
        {
            return Err("Invalid device selection".into());
        }
        Ok(())
    }
    pub fn settings(&self) -> crate::engine::Settings {
        crate::engine::Settings {
            acceleration: self.acceleration,
            coast: self.coast,
        }
    }
}
pub fn config_path() -> io::Result<PathBuf> {
    let base = match env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        Some(p) => PathBuf::from(p),
        None => {
            PathBuf::from(env::var_os("HOME").ok_or_else(|| io::Error::other("HOME is not set"))?)
                .join(".config")
        }
    };
    if !base.is_absolute() {
        return Err(io::Error::other("Configuration directory must be absolute"));
    }
    Ok(base.join("velvet-scroll/config.json"))
}
pub fn load() -> io::Result<Config> {
    let path = config_path()?;
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(e) => return Err(e),
    };
    if file.metadata()?.len() > 65536 {
        return Err(io::Error::other("Configuration is too large"));
    }
    let config: Config = serde_json::from_reader(file)?;
    config.validate().map_err(io::Error::other)?;
    Ok(config)
}
pub fn save(config: &Config) -> io::Result<()> {
    config.validate().map_err(io::Error::other)?;
    let path = config_path()?;
    let parent = path.parent().unwrap();
    fs::create_dir_all(parent)?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    let temp = parent.join(format!(".config.{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        serde_json::to_writer_pretty(&mut file, config)?;
        file.write_all(b"\n")?;
        // Atomic replacement is enough for preferences; avoid fsync on the input loop.
        file.flush()?;
        fs::rename(&temp, &path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
pub fn runtime_dir() -> io::Result<PathBuf> {
    let base = PathBuf::from(env::var_os("XDG_RUNTIME_DIR").ok_or_else(|| {
        io::Error::other(
            "XDG_RUNTIME_DIR is not set; start Velvet Scroll inside your desktop session",
        )
    })?);
    if !base.is_absolute() {
        return Err(io::Error::other("XDG_RUNTIME_DIR must be absolute"));
    }
    let meta = fs::symlink_metadata(&base)?;
    use std::os::unix::fs::MetadataExt;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o022 != 0 {
        return Err(io::Error::other(
            "XDG_RUNTIME_DIR must be a directory owned by you, without group/other write access",
        ));
    }
    let dir = base.join("velvet-scroll");
    match fs::create_dir(&dir) {
        Ok(()) => fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let meta = fs::symlink_metadata(&dir)?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
        return Err(io::Error::other(
            "Velvet Scroll runtime directory must be owned by you with permissions 0700",
        ));
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject_bad_gain_and_version() {
        for gain in [f64::NAN, f64::INFINITY, 0.99, 8.01] {
            assert!(Config {
                acceleration: gain,
                ..Config::default()
            }
            .validate()
            .is_err());
        }
        assert!(Config {
            version: 2,
            ..Config::default()
        }
        .validate()
        .is_err());
    }
    #[test]
    fn defaults_are_precise_and_opt_in() {
        let c: Config = serde_json::from_str("{}").unwrap();
        assert!(!c.coast);
        assert!(c.selected_devices.is_empty());
        assert!(c.validate().is_ok());
        assert!(serde_json::from_str::<Config>(r#"{"typo":true}"#).is_err());
    }
}
