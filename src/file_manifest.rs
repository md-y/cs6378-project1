use log::info;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    error::Error,
    fs,
    path::{self, Path},
};

#[derive(Deserialize, Serialize)]
pub struct FileManifest {
    files: HashMap<String, String>,
}

impl Default for FileManifest {
    fn default() -> Self {
        return Self {
            files: HashMap::new(),
        };
    }
}

impl FileManifest {
    pub fn read_file<P: AsRef<Path>>(file_path: &P) -> Result<Self, Box<dyn Error>> {
        let manifest_str = fs::read_to_string(file_path)?;
        let manifest: Self = toml::from_str(&manifest_str)?;
        return Ok(manifest);
    }

    pub fn read_or_generate<P: AsRef<Path>>(file_path: &P) -> Result<Self, Box<dyn Error>> {
        match Self::read_file(&file_path) {
            Err(err) => {
                info!(target: "FILE MANIFEST", "Failed to read file manifest, so generating new one. {}", err);
                let manifest = Self::default();
                manifest.write(&file_path)?;
                return Ok(manifest);
            }
            Ok(manifest) => return Ok(manifest),
        }
    }

    pub fn write<P: AsRef<Path>>(&self, file_path: &P) -> Result<(), Box<dyn Error>> {
        let manifest_str = toml::to_string_pretty(self)?;
        if let Some(parent) = file_path.as_ref().parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(file_path, manifest_str)?;
        return Ok(());
    }

    pub fn has_file(&self, file_name: &String) -> bool {
        return self.files.contains_key(file_name);
    }
}
