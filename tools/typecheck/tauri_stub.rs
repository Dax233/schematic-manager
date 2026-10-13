//! tauri 的最小替身，仅供离线类型检查使用。
//!
//! 覆盖范围严格限定为命令层实际用到的符号：AppHandle、Manager::path()、
//! path::BaseDirectory 与 PathResolver::resolve。
//!
//! 不使用真实的 tauri：它的 #[tauri::command] 是过程宏，无法用普通 Rust
//! 源码替代；检查时会把该属性剥掉（见 gen_cmd_check.py），
//! 因此这里只需要类型与 trait，不需要宏。

use std::path::PathBuf;

/// 应用句柄
#[derive(Clone)]
pub struct AppHandle {
    _priv: (),
}

impl AppHandle {
    /// 仅测试用构造
    pub fn new_for_test() -> Self {
        Self { _priv: () }
    }
}

/// 路径解析错误
#[derive(Debug)]
pub struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub mod path {
    /// 基准目录，取值与真实 tauri 对应
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum BaseDirectory {
        Audio,
        Cache,
        Config,
        Data,
        LocalData,
        Document,
        Download,
        Picture,
        Public,
        Video,
        Resource,
        Temp,
        AppConfig,
        AppData,
        AppLocalData,
        AppCache,
        AppLog,
        Desktop,
        Executable,
        Font,
        Home,
        Runtime,
        Template,
    }

    /// 路径解析器
    pub struct PathResolver {
        _priv: (),
    }

    impl PathResolver {
        pub fn resolve<P: AsRef<std::path::Path>>(
            &self,
            path: P,
            _dir: BaseDirectory,
        ) -> Result<std::path::PathBuf, super::Error> {
            Ok(path.as_ref().to_path_buf())
        }

        /// 应用配置目录。界面状态就存在这里（见 commands::ui_state_path）。
        ///
        /// 真实 tauri 由平台约定决定具体路径，这里只需给出一个合法且确定的
        /// 位置：离线检查只验证类型与调用形态，不验证平台路径规则。
        pub fn app_config_dir(&self) -> Result<std::path::PathBuf, super::Error> {
            Ok(std::path::PathBuf::from("config"))
        }
    }
}

/// Manager trait，命令层用它拿到路径解析器
pub trait Manager {
    fn path(&self) -> path::PathResolver;
}

impl Manager for AppHandle {
    fn path(&self) -> path::PathResolver {
        path::PathResolver { _priv: () }
    }
}

/// 便于测试中构造返回值
pub fn stub_resource_path(p: &str) -> PathBuf {
    PathBuf::from(p)
}
