use std::{
    path::PathBuf,
    process::Child,
    sync::{atomic::AtomicBool, Arc, Mutex},
};

use crate::portable::PortablePaths;

pub struct ProcessControl {
    pub child: Mutex<Option<Child>>,
    pub busy: AtomicBool,
    pub shutting_down: AtomicBool,
}

impl Default for ProcessControl {
    fn default() -> Self {
        Self {
            child: Mutex::new(None),
            busy: AtomicBool::new(false),
            shutting_down: AtomicBool::new(false),
        }
    }
}

pub struct AppState {
    pub paths: PortablePaths,
    pub process: Arc<ProcessControl>,
    pub preview: Arc<Mutex<Option<PathBuf>>>,
}
