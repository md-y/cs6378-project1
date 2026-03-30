use log::info;
use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};
use tokio::sync::Mutex;

pub struct FileManifest {
    files: Mutex<Vec<String>>,
}

impl Default for FileManifest {
    fn default() -> Self {
        return Self {
            files: Mutex::new(Vec::new()),
        };
    }
}

impl FileManifest {
    pub fn read_file<P: AsRef<Path>>(file_path: &P) -> Result<Self, Box<dyn Error>> {
        let manifest_str = fs::read_to_string(file_path)?;
        let manifest: RawFileManifest = toml::from_str(&manifest_str)?;
        return Ok(Self {
            files: Mutex::new(manifest.files),
        });
    }

    pub async fn read_or_generate<P: AsRef<Path>>(file_path: &P) -> Result<Self, Box<dyn Error>> {
        match Self::read_file(&file_path) {
            Err(err) => {
                info!(target: "FILE MANIFEST", "Failed to read file manifest, so generating new one. {}", err);
                let manifest = Self::default();
                manifest.write(&file_path).await?;
                return Ok(manifest);
            }
            Ok(manifest) => return Ok(manifest),
        }
    }

    pub async fn write<P: AsRef<Path>>(&self, file_path: &P) -> Result<(), Box<dyn Error>> {
        let files = self.files.lock().await;
        let manifest_str = toml::to_string_pretty(&RawFileManifest {
            files: files.clone(),
        })?;
        if let Some(parent) = file_path.as_ref().parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(file_path, manifest_str)?;
        return Ok(());
    }

    pub async fn has_file(&self, file_name: &String) -> bool {
        let files = self.files.lock().await;
        return files.contains(file_name);
    }

    pub fn get_file_data(
        &self,
        file_dir: PathBuf,
        file_name: &String,
    ) -> Result<Vec<u8>, Box<dyn Error>> {
        let path = file_dir.join(file_name);
        match fs::read(path) {
            Ok(data) => return Ok(data),
            Err(err) => Err(Box::new(err)),
        }
    }

    pub async fn write_file_data(
        &self,
        file_dir: PathBuf,
        file_name: &String,
        data: &Vec<u8>,
    ) -> Result<(), Box<dyn Error>> {
        let path = file_dir.join(file_name);
        match fs::write(path, data) {
            Err(err) => return Err(Box::new(err)),
            _ => {}
        };

        let mut files = self.files.lock().await;
        files.push(file_name.clone());
        drop(files);

        self.write(&file_dir.join("manifest.toml")).await?;

        return Ok(());
    }
}

#[derive(Deserialize, Serialize)]
pub struct RawFileManifest {
    files: Vec<String>,
}
