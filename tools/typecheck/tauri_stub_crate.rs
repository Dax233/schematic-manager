//! tauri 的最小替身，编译成独立 crate 供命令层检查使用。
//!
//! 只覆盖命令层实际用到的符号：AppHandle、Manager::path()、
//! path::BaseDirectory 与 PathResolver::resolve。
//! #[tauri::command] 是过程宏，无法用源码替代，
//! 检查时由 gen_cmd_check.py 把该属性剥掉。

use std::path::PathBuf;

#[derive(Clone)]
pub struct AppHandle {
    _priv: (),
}

impl AppHandle {
    pub fn new_for_test() -> Self {
        Self { _priv: () }
    }
}

#[derive(Debug)]
pub struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub mod path {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum BaseDirectory {
        Audio, Cache, Config, Data, LocalData, Document, Download, Picture,
        Public, Video, Resource, Temp, AppConfig, AppData, AppLocalData,
        AppCache, AppLog, Desktop, Executable, Font, Home, Runtime, Template,
    }

    pub struct PathResolver {
        _priv: (),
    }

    impl PathResolver {
        /// 仅桩内部使用
        pub(crate) fn new() -> Self {
            Self { _priv: () }
        }

        pub fn resolve<P: AsRef<std::path::Path>>(
            &self,
            path: P,
            _dir: BaseDirectory,
        ) -> Result<std::path::PathBuf, super::Error> {
            Ok(path.as_ref().to_path_buf())
        }
    }
}

pub trait Manager {
    fn path(&self) -> path::PathResolver;
}

impl Manager for AppHandle {
    fn path(&self) -> path::PathResolver {
        path::PathResolver::new()
    }
}

pub fn stub_path(p: &str) -> PathBuf {
    PathBuf::from(p)
}
