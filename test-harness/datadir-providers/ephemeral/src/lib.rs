use std::path::PathBuf;
use synapto::config::DataDirProvider;

pub struct EphemeralDir;

impl DataDirProvider for EphemeralDir {
    fn get_data_dir() -> PathBuf {
        tempfile::tempdir()
            .unwrap_or_else(|e| panic!("Failed to create temporary directory: {:?}", e))
            .keep()
    }
}
