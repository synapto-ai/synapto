use std::path::PathBuf;
use synapto_interface::data_dir::DataDirProvider;

pub struct ScenarioTestDir;

impl DataDirProvider for ScenarioTestDir {
    fn get_data_dir() -> PathBuf {
        PathBuf::from("tests")
    }
}
