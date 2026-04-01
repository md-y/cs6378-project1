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
    path: PathBuf,
}

impl FileManifest {
    pub fn read_file<P: AsRef<Path>>(file_path: &P) -> Result<Self, Box<dyn Error>> {
        let manifest_str = fs::read_to_string(file_path)?;
        let manifest: RawFileManifest = toml::from_str(&manifest_str)?;
        return Ok(Self {
            files: Mutex::new(manifest.files),
            path: file_path.as_ref().into(),
        });
    }

    pub async fn read_or_generate<P: AsRef<Path>>(file_path: &P) -> Result<Self, Box<dyn Error>> {
        match Self::read_file(&file_path) {
            Err(err) => {
                info!(target: "FILE MANIFEST", "Failed to read file manifest, so generating new one. {}", err);
                let manifest = Self {
                    files: Mutex::new(Vec::new()),
                    path: file_path.as_ref().into(),
                };
                manifest.write().await?;
                return Ok(manifest);
            }
            Ok(manifest) => return Ok(manifest),
        }
    }

    fn get_path_dir(&self) -> Result<&Path, Box<dyn Error>> {
        return match self.path.parent() {
            Some(par) => Ok(par),
            None => Err(format!("Could not find parent path for: {:?}", self.path).into()),
        };
    }

    pub async fn write(&self) -> Result<(), Box<dyn Error>> {
        let files = self.files.lock().await;
        let manifest_str = toml::to_string_pretty(&RawFileManifest {
            files: files.clone(),
        })?;
        if let Ok(parent) = self.get_path_dir() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(&self.path, manifest_str)?;
        return Ok(());
    }

    pub async fn has_file(&self, file_name: &String) -> bool {
        let files = self.files.lock().await;
        return files.contains(file_name);
    }

    pub fn get_file_data(&self, file_name: &String) -> Result<Vec<u8>, Box<dyn Error>> {
        let file_dir = self.get_path_dir()?;
        let path = file_dir.join(file_name);
        match fs::read(path) {
            Ok(data) => return Ok(data),
            Err(err) => Err(Box::new(err)),
        }
    }

    pub async fn write_file_data(
        &self,
        file_name: &String,
        data: &Vec<u8>,
    ) -> Result<(), Box<dyn Error>> {
        let file_dir = self.get_path_dir()?;
        let path = file_dir.join(file_name);
        match fs::write(path, data) {
            Err(err) => return Err(Box::new(err)),
            _ => {}
        };

        let mut files = self.files.lock().await;
        files.push(file_name.clone());
        drop(files);

        self.write().await?;

        return Ok(());
    }
}

#[derive(Deserialize, Serialize)]
pub struct RawFileManifest {
    files: Vec<String>,
}
