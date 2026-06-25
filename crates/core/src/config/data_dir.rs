use std::path::PathBuf;

pub trait DataDirProvider: Send + Sync + 'static {
    fn get_data_dir() -> PathBuf;
}
