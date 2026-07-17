//! 应用预设 —— 默认配置、路径常量、模型定义、设置持久化
//!
//! 集中管理所有默认值，避免散落在各模块中。

pub mod app;
pub mod models;
pub mod settings;

pub use app::*;
pub use models::*;
pub use settings::*;
