use std::path::PathBuf;
use synapto_interface::data_dir::DataDirProvider;

pub struct ScenarioTestDir;

impl DataDirProvider for ScenarioTestDir {
    fn get_data_dir() -> PathBuf {
        PathBuf::from("tests")
    }
}

pub struct WorkspaceTestDir;

impl DataDirProvider for WorkspaceTestDir {
    fn get_data_dir() -> PathBuf {
        let mut path = std::env::current_dir().unwrap();
        loop {
            if path.join("Cargo.lock").exists() {
                return path;
            }
            if !path.pop() {
                break;
            }
        }
        std::env::current_dir().unwrap()
    }
}
