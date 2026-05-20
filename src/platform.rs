use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

use crate::error::{MooseError, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppPaths {
    data_dir: PathBuf,
    cache_dir: PathBuf,
    config_dir: PathBuf,
    database_path: PathBuf,
}

impl AppPaths {
    pub fn new(app_name: &str) -> Result<Self> {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or(MooseError::MissingHomeDirectory)?;
        let data_root = xdg_path("XDG_DATA_HOME", &home, ".local/share");
        let cache_root = xdg_path("XDG_CACHE_HOME", &home, ".cache");
        let config_root = xdg_path("XDG_CONFIG_HOME", &home, ".config");

        Ok(Self::from_base_dirs(
            data_root.join(app_name),
            cache_root.join(app_name),
            config_root.join(app_name),
        ))
    }

    pub fn from_base_dirs(data_dir: PathBuf, cache_dir: PathBuf, config_dir: PathBuf) -> Self {
        let database_path = data_dir.join("moose.db");
        Self {
            data_dir,
            cache_dir,
            config_dir,
            database_path,
        }
    }

    pub fn create_all(&self) -> io::Result<()> {
        fs::create_dir_all(&self.data_dir)?;
        fs::create_dir_all(&self.cache_dir)?;
        fs::create_dir_all(&self.config_dir)
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub fn ollama_installation_dir(&self) -> PathBuf {
        self.data_dir.join("ollama_installation")
    }

    pub fn ollama_binary_path(&self) -> PathBuf {
        self.ollama_installation_dir().join("bin").join("ollama")
    }

    pub fn ollama_models_dir(&self) -> PathBuf {
        self.data_dir.join(".ollama").join("models")
    }

    pub fn ollama_log_path(&self) -> PathBuf {
        self.cache_dir.join("ollama.log")
    }

    pub fn ollama_download_cache_dir(&self) -> PathBuf {
        self.cache_dir.join("ollama-downloads")
    }
}

fn xdg_path(variable: &str, home: &Path, fallback: &str) -> PathBuf {
    env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(fallback))
}

#[cfg(test)]
mod tests {
    use super::AppPaths;
    use std::path::PathBuf;

    #[test]
    fn app_paths_keep_database_inside_data_directory() {
        let paths = AppPaths::from_base_dirs(
            PathBuf::from("/tmp/data/moose"),
            PathBuf::from("/tmp/cache/moose"),
            PathBuf::from("/tmp/config/moose"),
        );

        assert_eq!(
            paths.database_path(),
            PathBuf::from("/tmp/data/moose/moose.db")
        );
    }
}
