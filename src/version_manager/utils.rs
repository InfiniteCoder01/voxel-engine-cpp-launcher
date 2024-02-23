use super::*;
use std::sync::Arc;
use std::sync::Mutex;

pub async fn download(url: &str, interface: &Arc<Interface>, name: &str) -> Option<Vec<u8>> {
    let bytes = match reqwest::ClientBuilder::new()
        .user_agent("VoxelLauncherWGET/1.0")
        .build()
        .unwrap()
        .get(url)
        .send()
        .await
    {
        Ok(mut response) => {
            let download = || async move {
                let mut bytes = Vec::new();
                let mut progress = 0;
                let content_length = response.content_length();
                while let Some(chunk) = response.chunk().await? {
                    bytes.extend_from_slice(&chunk);
                    progress += chunk.len();
                    if let Some(length) = content_length {
                        interface.replace_progress(progress as f32 / length as f32);
                    }
                }
                Ok(bytes)
            };
            download().await
        }
        Err(err) => Err(err),
    };
    bytes
        .map_err(|err| {
            interface.error(format!("Failed to download {}: {}", name, err));
        })
        .ok()
}

pub fn unpack(bytes: &[u8], path: &std::path::Path, interface: &Arc<Interface>) -> bool {
    if let Err(err) = zip_extract::extract(std::io::Cursor::new(bytes), path, true) {
        interface.error(format!("Failed to unpack version sources: {}", err));
        false
    } else {
        true
    }
}

pub async fn run_command(
    command: &str,
    args: &[&str],
    path: Option<&std::path::Path>,
    interface: &Arc<Interface>,
    mut line_callback: impl FnMut(&str),
) -> bool {
    use std::process::Stdio;
    use tokio::process::Command;
    use tokio_process_stream::ProcessLineStream;
    use tokio_stream::StreamExt;
    let mut command = Command::new(command);
    command
        .args(args)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(path) = path {
        command.current_dir(path);
    }
    let mut procstream = match ProcessLineStream::try_from(command) {
        Ok(procstream) => procstream,
        Err(err) => {
            interface.error(format!("Failed to run command: {}", err));
            return false;
        }
    };
    while let Some(item) = procstream.next().await {
        use tokio_process_stream::Item;
        match item {
            Item::Stdout(line) => line_callback(&line),
            Item::Stderr(err) => {
                if !err.contains("Cloning into") {
                    interface.log().push(RichText::new(err).color(Color32::RED));
                }
            }
            Item::Done(status) => match status {
                Ok(status) => {
                    if !status.success() {
                        interface.error("Failed to run command!");
                        return false;
                    }
                }
                Err(err) => {
                    interface.error(format!("Failed to run command: {}", err));
                    return false;
                }
            },
        }
    }
    true
}

pub fn find_platform_version(asset: &octocrab::models::repos::Asset) -> bool {
    if cfg!(windows) {
        asset.name.contains("win64")
    } else if cfg!(unix) {
        asset.name.contains("AppImage")
    } else {
        false
    }
}

pub fn downloaded_name() -> String {
    if cfg!(windows) {
        "VoxelEngine.exe".to_string()
    } else {
        "VoxelEngine.AppImage".to_string()
    }
}

pub fn binary_name() -> String {
    if cfg!(windows) {
        "VoxelEngine.exe".to_string()
    } else {
        "VoxelEngine".to_string()
    }
}

pub fn get_versions_path() -> std::path::PathBuf {
    std::path::Path::new("versions").to_path_buf()
}

pub fn get_version_path(name: &str) -> std::path::PathBuf {
    get_versions_path().join(name)
}

pub fn get_lua_path() -> std::path::PathBuf {
    home::home_dir().unwrap().join(".luajit")
}

pub fn spawn(f: impl Future<Output = ()> + Send + 'static) {
    static RUNTIME: Mutex<Option<tokio::runtime::Runtime>> = Mutex::new(None);

    let mut runtime = RUNTIME.lock().unwrap();
    if runtime.is_none() {
        *runtime = Some(tokio::runtime::Runtime::new().unwrap());
    }
    runtime.as_ref().unwrap().spawn(f);
}
