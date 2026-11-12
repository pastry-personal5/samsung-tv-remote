//! Profile selection is made by executable composition. Production has no
//! caller-supplied path; development can select only its fake namespace.
use super::{development::DevelopmentProfile, production::ProductionProfile};
use crate::infrastructure::private_files::{FileError, Namespace, RuntimeDirectory};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
#[derive(Debug, Clone)]
pub enum Profile {
    Development(PathBuf),
    Control,
}
impl Profile {
    pub fn development(base: &Path) -> Result<Self, FileError> {
        if !base.is_absolute() {
            return Err(FileError::UnsafePath);
        }
        Ok(Self::Development(base.into()))
    }
    pub fn namespace(&self) -> Namespace {
        match self {
            Self::Development(_) => Namespace::Development,
            Self::Control => Namespace::Control,
        }
    }
    pub fn base(&self) -> Result<PathBuf, FileError> {
        match self {
            Self::Development(base) => Ok(base.clone()),
            Self::Control => ProductionProfile::base(),
        }
    }
    pub fn directory(&self) -> Result<Arc<RuntimeDirectory>, FileError> {
        match self {
            Self::Development(base) => DevelopmentProfile::directory(base),
            Self::Control => ProductionProfile::directory(),
        }
    }
}
