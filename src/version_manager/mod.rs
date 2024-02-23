use super::*;
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    sync::{Arc, Mutex},
};

pub mod utils;
pub mod version;
pub use version::{Version, VersionData};

pub struct VersionManager {
    interface: Arc<Interface>,

    pub versions: Arc<Mutex<Vec<Arc<Version>>>>,
}

impl VersionManager {
    pub fn new(interface: Arc<Interface>) -> VersionManager {
        let this = Self {
            interface,

            versions: Arc::new(Mutex::new(Vec::new())),
        };
        this.update();
        this
    }

    pub fn update(&self) {
        let versions = self.versions.clone();
        let interface = self.interface.clone();
        utils::spawn(async move {
            *versions.lock().unwrap() = match octocrab::instance()
                .repos("MihailRis", "VoxelEngine-Cpp")
                .releases()
                .list()
                .send()
                .await
            {
                Ok(versions) => versions
                    .into_iter()
                    .filter_map(|release| {
                        Some(Arc::new(Version::parse(release, interface.clone())?))
                    })
                    .collect(),
                Err(err) => {
                    interface.warning(format!(
                        "Failed to fetch versions from github: {}",
                        err.to_string().split('\n').next().unwrap()
                    ));
                    let mut local_versions = Vec::new();
                    if let Ok(dir) = std::fs::read_dir(utils::get_versions_path()) {
                        for local_version in dir.flatten() {
                            let name = local_version.file_name();
                            let name = name.to_string_lossy();
                            let name = name.as_ref();
                            let verfilepath = utils::get_version_path(name).join("version.ron");
                            if verfilepath.exists() {
                                match ron::from_str::<VersionData>(
                                    &std::fs::read_to_string(verfilepath).unwrap(),
                                ) {
                                    Ok(version_data) => {
                                        local_versions.push(Arc::new(Version {
                                            name: name.to_string(),
                                            data: Arc::new(Mutex::new(version_data)),
                                        }));
                                    }
                                    Err(err) => {
                                        interface.warning(format!(
                                            "Corrupted version {:?}: {}",
                                            name, err
                                        ));
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                    local_versions
                }
            };
            versions.lock().unwrap().insert(
                0,
                Arc::new(Version {
                    name: "Latest (Git)".to_owned(),
                    data: Arc::new(Mutex::new(VersionData::GitLatest)),
                }),
            );
        });
    }

    pub fn try_find(&self, name: &str) -> Option<Arc<Version>> {
        self.versions
            .lock()
            .unwrap()
            .iter()
            .find(|version| version.name == name)
            .cloned()
    }
}
