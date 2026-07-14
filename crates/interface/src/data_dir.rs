use std::path::PathBuf;

pub trait DataDirProvider: Send + Sync + 'static {
    fn get_data_dir() -> PathBuf;
}

pub struct CurrentDir;

impl DataDirProvider for CurrentDir {
    fn get_data_dir() -> PathBuf {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    }
}
