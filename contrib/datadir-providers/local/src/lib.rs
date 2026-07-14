#![allow(incomplete_features)]
#![feature(adt_const_params)]
#![feature(unsized_const_params)]

use std::path::PathBuf;
use synapto_interface::data_dir::DataDirProvider;

pub struct DataLocalDir<const SUBDIR: &'static str>;

impl<const SUBDIR: &'static str> DataDirProvider for DataLocalDir<SUBDIR> {
    fn get_data_dir() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(SUBDIR)
    }
}
