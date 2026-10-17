pub mod animation;
pub mod archive;
pub mod assets;
pub mod audio;
pub mod bitmap;
pub mod bolt;
pub mod ddv;
pub mod dialogs;
pub mod dialogue;
pub mod first_start;
pub mod flac;
pub mod flight;
pub mod font;
pub mod game;
pub mod gameplay;
pub mod hpfs;
pub mod hud;
pub mod keys;
pub mod lang;
pub mod map;
pub mod media;
pub mod menu;
pub mod original;
pub mod pack;
pub mod packing;
pub mod paths;
pub mod picker;
pub mod random_map;
pub mod save;
pub mod scenario;
pub mod scene;
pub mod script;
pub mod selection;
pub mod simulation;
pub mod sprite;
pub mod terrain;
pub mod vfs;
pub mod world;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub(crate) fn invalid(message: impl Into<String>) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into()).into()
}

pub(crate) fn u32_at(data: &[u8], offset: usize) -> Result<u32> {
    let bytes = data
        .get(offset..offset.saturating_add(4))
        .ok_or_else(|| invalid("truncated u32"))?;
    Ok(u32::from_le_bytes(bytes.try_into()?))
}

/// Positional read that never moves a shared cursor, so one open file can
/// serve every thread without a lock.
pub(crate) fn read_at(file: &std::fs::File, offset: u64, len: usize) -> Result<Vec<u8>> {
    let mut buffer = vec![0; len];
    let mut done = 0;
    while done < len {
        #[cfg(windows)]
        let n = std::os::windows::fs::FileExt::seek_read(
            file,
            &mut buffer[done..],
            offset + done as u64,
        )?;
        #[cfg(unix)]
        let n =
            std::os::unix::fs::FileExt::read_at(file, &mut buffer[done..], offset + done as u64)?;
        if n == 0 {
            return Err(invalid("unexpected end of file"));
        }
        done += n;
    }
    Ok(buffer)
}

/// Replace a file only after its new content is complete. `rename` replaces
/// the destination atomically on Windows and Unix, so a crash leaves either
/// the previous or the new version, never a missing or truncated file.
pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut staging = path.as_os_str().to_owned();
    staging.push(".tmp");
    let staging = std::path::PathBuf::from(staging);
    std::fs::write(&staging, bytes)?;
    std::fs::rename(&staging, path)?;
    Ok(())
}
