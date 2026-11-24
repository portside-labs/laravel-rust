//! `storage:unlink`, `auth:clear-resets`, and `package:discover`.

use std::path::Path;

use illuminate_console::{Command, Console, async_trait};
use illuminate_support::{Result, Value};

use crate::application::Application;

/// `storage:unlink` — Delete the symbolic links configured for the
/// application.
pub struct StorageUnlinkCommand;

#[async_trait]
impl Command for StorageUnlinkCommand {
    fn signature(&self) -> &str {
        "storage:unlink"
    }

    fn description(&self) -> &str {
        "Delete existing symbolic links configured for the application"
    }

    async fn handle(&self, cmd: Console) -> Result<()> {
        let app = Application::current();
        let links: Vec<String> = match app.config_repository().get("filesystems.links") {
            Value::Object(map) => map.keys().cloned().collect(),
            Value::Null => vec![app.public_path("storage")],
            _ => Vec::new(),
        };

        for link in links {
            let path = Path::new(&link);
            if !is_storage_symlink(path) {
                continue;
            }
            // Laravel reports the link after attempting deletion, even when unlink fails.
            let _ = std::fs::remove_file(path);
            cmd.components()
                .info(format!("The [{link}] link has been deleted."));
        }
        Ok(())
    }
}

#[cfg(not(windows))]
fn is_storage_symlink(path: &Path) -> bool {
    path.exists() && path.is_symlink()
}

#[cfg(windows)]
fn is_storage_symlink(path: &Path) -> bool {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FileAttributeTagInfo, GetFileInformationByHandleEx,
    };
    use windows_sys::Win32::System::SystemServices::IO_REPARSE_TAG_SYMLINK;

    // PHP's is_link() checks the reparse tag of the link itself.
    let Ok(link) = OpenOptions::new()
        .access_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
    else {
        return false;
    };

    let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: the handle stays open and the buffer is a writable FILE_ATTRIBUTE_TAG_INFO.
    let found = unsafe {
        GetFileInformationByHandleEx(
            link.as_raw_handle(),
            FileAttributeTagInfo,
            std::ptr::from_mut(&mut info).cast(),
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    found != 0 && info.ReparseTag == IO_REPARSE_TAG_SYMLINK
}

/// `auth:clear-resets` — Flush expired password reset tokens.
pub struct ClearResetsCommand;

#[async_trait]
impl Command for ClearResetsCommand {
    fn signature(&self) -> &str {
        "auth:clear-resets {name? : The name of the password broker}"
    }

    fn description(&self) -> &str {
        "Flush expired password reset tokens"
    }

    async fn handle(&self, cmd: Console) -> Result<()> {
        let name = cmd.argument("name").filter(|name| !name.is_empty());
        illuminate_auth::facades::Password::broker(name.as_deref())?
            .get_repository()
            .delete_expired()
            .await?;
        cmd.components().info("Expired reset tokens cleared successfully.");
        Ok(())
    }
}

/// `package:discover` — List the packages whose service providers are
/// discovered (registered with `discover_provider!`).
pub struct PackageDiscoverCommand;

#[async_trait]
impl Command for PackageDiscoverCommand {
    fn signature(&self) -> &str {
        "package:discover"
    }

    fn description(&self) -> &str {
        "Rebuild the cached package manifest"
    }

    async fn handle(&self, cmd: Console) -> Result<()> {
        cmd.components().info("Discovering packages");

        let mut packages: Vec<&str> = illuminate_container::discovered_providers()
            .iter()
            .map(|provider| provider.package)
            .collect();
        packages.dedup();
        for package in &packages {
            cmd.components().task(*package, || async { Ok(true) }).await?;
        }
        if !packages.is_empty() {
            cmd.new_line(1);
        }
        Ok(())
    }
}
