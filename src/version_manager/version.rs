use super::*;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum VersionData {
    GitLatest,
    Binary {
        url: String,
        unzip: bool,
    },
    Source {
        zipball_url: String,
    },
    Local {
        binary: std::path::PathBuf,
        origin: Box<VersionData>,
    },
    NotFound,
}

#[derive(Clone, Debug)]
pub struct Version {
    pub name: String,
    pub data: Arc<Mutex<VersionData>>,
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Version {
    pub fn parse(
        release: octocrab::models::repos::Release,
        interface: Arc<Interface>,
    ) -> Option<Self> {
        let name = release.name?;
        let source = if let Ok(Ok(version_data)) =
            std::fs::read_to_string(utils::get_version_path(&name).join("version.ron"))
                .map(|version_data| ron::from_str::<VersionData>(&version_data))
        {
            version_data
        } else if let Some(binary_url) = release
            .assets
            .iter()
            .find(|asset| utils::find_platform_version(asset))
            .map(|asset| asset.browser_download_url.to_string())
            .and_then(|asset| {
                if interface.config().use_prebuilt_when_possible {
                    Some(asset)
                } else {
                    None
                }
            })
        {
            VersionData::Binary {
                url: binary_url,
                unzip: cfg!(windows),
            }
        } else if let Some(zipball_url) = release.zipball_url.map(|url| url.to_string()) {
            VersionData::Source { zipball_url }
        } else {
            VersionData::NotFound
        };
        Some(Self {
            name,
            data: Arc::new(Mutex::new(source)),
        })
    }

    pub fn play(&self, interface: Arc<Interface>, force_refresh: bool) {
        if force_refresh {
            let mut data = self.data.lock().unwrap();
            if let VersionData::Local { origin, .. } = &*data {
                *data = origin.as_ref().clone();
            }
        }

        let this = self.clone();
        std::fs::create_dir_all(this.path()).ok();
        let data = this.data.lock().unwrap().clone();
        match data {
            VersionData::GitLatest => {
                if !interface.config().build_unsupported {
                    interface.error("This version has to be built from source");
                    interface.progress().take();
                    return;
                }
                utils::spawn(async move {
                    interface.replace_progress(0.0);
                    if !this.path().join("src").exists() {
                        interface.info("Cloning the repo");
                        let success = utils::run_command(
                            "git",
                            &[
                                "clone",
                                "https://github.com/MihailRis/VoxelEngine-Cpp",
                                this.path().to_string_lossy().as_ref(),
                            ],
                            None,
                            &interface,
                            |_| (),
                        )
                        .await;
                        if !success {
                            interface.progress().take();
                            return;
                        }
                    } else {
                        interface.info("Pulling changes from github");
                        let success = utils::run_command(
                            "git",
                            &["pull"],
                            Some(&this.path()),
                            &interface,
                            |_| (),
                        )
                        .await;
                        if !success {
                            interface.info(
                                "Failed to clone the repo. Running the latest local commit instead",
                            );
                        }
                    }

                    if !this.build(&interface, force_refresh).await {
                        interface.progress().take();
                        return;
                    }

                    interface.progress().take();
                    this.run_binary(&interface);
                });
            }
            VersionData::Binary { url, unzip } => {
                utils::spawn(async move {
                    interface.replace_progress(0.0);
                    interface.info("Downloading version binary");

                    let bytes = match utils::download(&url, &interface, "binary").await {
                        Some(bytes) => bytes,
                        None => {
                            interface.progress().take();
                            return;
                        }
                    };
                    if unzip {
                        if !utils::unpack(&bytes, &this.path(), &interface) {
                            interface.progress().take();
                            return;
                        }
                    } else {
                        let mut binfile = File::create(this.downloaded_path()).unwrap();
                        std::io::copy(&mut std::io::Cursor::new(bytes), &mut binfile).unwrap();
                        drop(binfile);
                    }

                    #[cfg(target_os = "linux")]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        std::fs::set_permissions(
                            this.downloaded_path(),
                            std::fs::Permissions::from_mode(0o755),
                        )
                        .unwrap();
                    }

                    this.finish(utils::downloaded_name(), &interface);
                });
            }
            VersionData::Source { zipball_url } => {
                if !interface.config().build_unsupported {
                    interface
                        .error("This version doesn't have prebuilt binaries for your platform");
                    interface.progress().take();
                    return;
                }
                if self.name == "v11" || self.name == "v12" {
                    interface.error("Versions 0.11 and 0.12 are not supported by the laucher");
                    return;
                }

                utils::spawn(async move {
                    interface.replace_progress(0.0);
                    interface.info("Downloading version source");

                    let bytes = match utils::download(&zipball_url, &interface, "zipball").await {
                        Some(bytes) => bytes,
                        None => {
                            interface.progress().take();
                            return;
                        }
                    };

                    interface.info("Unpacking version sources");
                    if !utils::unpack(&bytes, &this.path(), &interface) {
                        interface.progress().take();
                        return;
                    }
                    if !this.build(&interface, force_refresh).await {
                        interface.progress().take();
                        return;
                    }

                    this.finish(
                        std::path::Path::new("build").join(utils::binary_name()),
                        &interface,
                    );
                });
            }
            VersionData::Local { .. } => self.run_binary(&interface),
            VersionData::NotFound => {
                interface.error("Version files not found or it's not supported on your platform");
            }
        }
    }

    pub fn path(&self) -> std::path::PathBuf {
        utils::get_version_path(&self.name)
    }

    pub fn downloaded_path(&self) -> std::path::PathBuf {
        self.path().join(utils::downloaded_name())
    }

    pub async fn build(&self, interface: &Arc<Interface>, force_refresh: bool) -> bool {
        if interface.config().download_lua {
            if !utils::get_lua_path().join("lib").exists() {
                std::fs::remove_dir_all(utils::get_lua_path()).ok();
                std::fs::create_dir_all(utils::get_lua_path()).unwrap();
                interface.info("Downloading lua");
                let success = utils::run_command(
                    "git",
                    &[
                        "clone",
                        "https://luajit.org/git/luajit.git",
                        utils::get_lua_path().to_string_lossy().as_ref(),
                    ],
                    None,
                    interface,
                    |_| (),
                )
                .await;
                if !success {
                    return false;
                }

                interface.info("Building lua");
                let success = utils::run_command(
                    "make",
                    &[],
                    Some(&utils::get_lua_path()),
                    interface,
                    |_| (),
                )
                .await;
                if !success {
                    return false;
                }

                let success = utils::run_command(
                    "make",
                    &[
                        "install",
                        &format!("PREFIX={:?}", utils::get_lua_path().join("lib")),
                    ],
                    Some(&utils::get_lua_path()),
                    interface,
                    |_| (),
                )
                .await;
                if !success {
                    return false;
                }
            }
            if let Ok(cmake) = std::fs::read_to_string(self.path().join("CMakeLists.txt")) {
                let lua_path = utils::get_lua_path().join("lib").canonicalize().unwrap();
                std::fs::write(self.path().join("CMakeLists.txt"), cmake.replace(
                    "find_package(Lua REQUIRED)",
                    &format!(
                        "include_directories(\"{}/include/luajit-2.1/\")\nset(LUA_LIBRARIES \"{}/lib/libluajit-5.1.a\")",
                        lua_path.to_string_lossy().as_ref(), lua_path.to_string_lossy().as_ref()
                    ),
                )).unwrap();
            }
        }

        interface.info("Building the game");
        if force_refresh {
            std::fs::remove_dir_all(self.path().join("build")).ok();
        }
        std::fs::create_dir(self.path().join("build")).ok();
        let success = utils::run_command(
            "cmake",
            &["-DCMAKE_BUILD_TYPE=Release", "-Bbuild"],
            Some(&self.path()),
            interface,
            |_| (),
        )
        .await;
        if !success {
            return false;
        }

        let success = utils::run_command(
            "cmake",
            &["--build", "build"],
            Some(&self.path()),
            interface,
            |line| {
                if let Some(percentage) = line
                    .strip_prefix('[')
                    .and_then(|line| line.split_once(']'))
                    .and_then(|(percentage, _)| percentage.trim().strip_suffix('%'))
                    .and_then(|percentage| percentage.trim().parse::<i32>().ok())
                {
                    interface.set_progress(percentage as f32 / 100.0, line);
                }
            },
        )
        .await;
        if !success {
            return false;
        }

        true
    }

    pub fn finish(&self, binary: impl AsRef<std::path::Path>, interface: &Arc<Interface>) {
        {
            let mut data = self.data.lock().unwrap();
            *data = VersionData::Local {
                binary: binary.as_ref().to_path_buf(),
                origin: Box::new(data.clone()),
            };

            std::fs::write(
                self.path().join("version.ron"),
                ron::to_string(&*data).unwrap(),
            )
            .unwrap();
        }

        interface.progress().take();
        self.run_binary(interface)
    }

    pub fn run_binary(&self, interface: &Arc<Interface>) {
        interface.info("Running the game");
        let binary = match &*self.data.lock().unwrap() {
            VersionData::Local { binary, .. } => binary.to_owned(),
            VersionData::GitLatest => std::path::Path::new("build").join(utils::binary_name()),
            _ => {
                interface.error("Error: Binary not found! Use force-refresh");
                return;
            }
        };

        if let Err(err) = self.path().join(binary).canonicalize().and_then(|binpath| {
            std::process::Command::new(binpath)
                .current_dir(self.path())
                .spawn()
        }) {
            interface.error(format!("Failed to run game executable: {}", err));
        }
    }
}
