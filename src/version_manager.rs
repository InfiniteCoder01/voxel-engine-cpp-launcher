use std::{
    fs::File,
    sync::{Arc, Mutex},
};

pub struct VersionManager {
    runtime: tokio::runtime::Runtime,
    toasts: Arc<Mutex<egui_notify::Toasts>>,
    pub versions: Arc<Mutex<Vec<Arc<Version>>>>,
    pub progress: Arc<Mutex<Option<f32>>>,
}

impl VersionManager {
    pub fn new(toasts: Arc<Mutex<egui_notify::Toasts>>) -> VersionManager {
        let this = Self {
            runtime: tokio::runtime::Runtime::new().unwrap(),
            toasts,
            versions: Arc::new(Mutex::new(Vec::new())),
            progress: Arc::new(Mutex::new(None)),
        };
        this.update();
        this
    }

    pub fn update(&self) {
        let versions = self.versions.clone();
        let toasts = self.toasts.clone();
        self.runtime.spawn(async move {
            *versions.lock().unwrap() = match octocrab::instance()
                .repos("MihailRis", "VoxelEngine-Cpp")
                .releases()
                .list()
                .send()
                .await
            {
                Ok(versions) => versions
                    .into_iter()
                    .filter_map(|release| Some(Arc::new(Version::parse(release)?)))
                    .collect(),
                Err(err) => {
                    toasts.lock().unwrap().warning(format!(
                        "Failed to fetch versions from github: {}",
                        err.to_string().split('\n').next().unwrap()
                    ));
                    let mut local_versions = Vec::new();
                    if let Ok(dir) = std::fs::read_dir(get_versions_path()) {
                        for local_version in dir.flatten() {
                            let name = local_version.file_name();
                            let name = name.to_string_lossy();
                            let name = name.as_ref();
                            if get_version_path(name).join(executable_name()).exists() {
                                local_versions.push(Arc::new(Version {
                                    name: name.to_string(),
                                    source: Arc::new(Mutex::new(VersionData::Local)),
                                }));
                            }
                        }
                    }
                    local_versions
                }
            }
        });
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VersionData {
    GitLatest,
    Binary { url: String, unzip: bool },
    Source { zipball_url: String },
    Local,
    NotFound,
}

#[derive(Clone, Debug)]
pub struct Version {
    pub name: String,
    pub source: Arc<Mutex<VersionData>>,
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Version {
    pub fn parse(release: octocrab::models::repos::Release) -> Option<Self> {
        let name = release.name?;
        let source = if get_version_path(&name).join(executable_name()).exists() {
            VersionData::Local
        } else if let Some(binary_url) = release
            .assets
            .iter()
            .find(|asset| find_platform_version(asset))
            .map(|asset| asset.browser_download_url.to_string())
        {
            VersionData::Binary {
                url: binary_url,
                unzip: cfg!(windows),
            }
        } else if let Some(source_url) = release.zipball_url.map(|url| url.to_string()) {
            VersionData::Source {
                zipball_url: source_url,
            }
        } else {
            VersionData::NotFound
        };
        Some(Self {
            name,
            source: Arc::new(Mutex::new(source)),
        })
    }

    pub fn play(&self, manager: &VersionManager) {
        let verpath = get_version_path(&self.name);
        let binpath = verpath.join(executable_name());
        let toasts = manager.toasts.clone();
        let progress_bar = manager.progress.clone();
        let name = self.name.clone();
        let source = self.source.clone();
        let src = {
            let source = source.lock().unwrap().clone();
            source
        };
        match src {
            VersionData::GitLatest => {
                //
            }
            VersionData::Binary { url, unzip } => {
                manager.runtime.spawn(async move {
                    progress_bar.lock().unwrap().replace(0.0);
                    std::fs::create_dir_all(&verpath).unwrap();
                    toasts.lock().unwrap().info("Downloading version binary");

                    let bytes = match reqwest::get(url).await {
                        Ok(mut response) => {
                            let progress_bar = progress_bar.clone();
                            let download = || async move {
                                let mut bytes = Vec::new();
                                let mut progress = 0;
                                let content_length = response.content_length();
                                while let Some(chunk) = response.chunk().await? {
                                    bytes.extend_from_slice(&chunk);
                                    progress += chunk.len();
                                    if let Some(length) = content_length {
                                        progress_bar
                                            .lock()
                                            .unwrap()
                                            .replace(progress as f32 / length as f32);
                                    }
                                }
                                Ok(bytes)
                            };
                            download().await
                        }
                        Err(err) => Err(err),
                    };
                    let bytes = match bytes {
                        Ok(bytes) => bytes,
                        Err(err) => {
                            toasts
                                .lock()
                                .unwrap()
                                .error(format!("Failed to download binary: {}", err));
                            return;
                        }
                    };

                    if unzip {
                        toasts.lock().unwrap().info("Unpacking version files");
                        zip_extract::extract(std::io::Cursor::new(bytes), &verpath, true).unwrap();
                    } else {
                        let mut binfile = File::create(&binpath).unwrap();
                        std::io::copy(&mut std::io::Cursor::new(bytes), &mut binfile).unwrap();
                        drop(binfile);
                    }

                    #[cfg(target_os = "linux")]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        std::fs::set_permissions(binpath, std::fs::Permissions::from_mode(0o755))
                            .unwrap();
                    }

                    *progress_bar.lock().unwrap() = None;
                    *source.lock().unwrap() = VersionData::Local;

                    run_version_binary(&name, toasts)
                });
            }
            VersionData::Source { zipball_url: _ } => {
                //
            }
            VersionData::Local => run_version_binary(&self.name, toasts),
            VersionData::NotFound => {
                toasts
                    .lock()
                    .unwrap()
                    .error("Version files not found or it's not supported on your platform");
            }
        }
    }
}

fn find_platform_version(asset: &octocrab::models::repos::Asset) -> bool {
    if cfg!(windows) {
        asset.name.contains("win64")
    } else if cfg!(unix) {
        asset.name.contains("AppImage")
    } else {
        false
    }
}

fn executable_name() -> String {
    if cfg!(windows) {
        "VoxelEngine.exe".to_string()
    } else {
        "VoxelEngine.AppImage".to_string()
    }
}

fn get_versions_path() -> std::path::PathBuf {
    std::path::Path::new("versions").to_path_buf()
}

fn get_version_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new("versions").join(name)
}

fn run_version_binary(name: &str, toasts: Arc<Mutex<egui_notify::Toasts>>) {
    toasts.lock().unwrap().info("Running the game");
    let verpath = get_version_path(name);
    if let Err(err) =
        std::process::Command::new(verpath.join(executable_name()).canonicalize().unwrap())
            .current_dir(verpath)
            .spawn()
    {
        toasts
            .lock()
            .unwrap()
            .error(format!("Failed to run game executable: {}", err));
    }
}
