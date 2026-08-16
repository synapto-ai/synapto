use std::path::PathBuf;

use synapto_interface::data_dir::DataDirProvider;

pub struct CurrentDir;

impl DataDirProvider for CurrentDir {
    fn get_data_dir() -> PathBuf {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    }
}
