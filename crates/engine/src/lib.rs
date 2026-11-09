//! UCI engine layer: a typed parser, process supervision and analysis streams.
//!
//! Each process gets dedicated reader and writer-owning threads, so engine I/O can
//! never block a UI or executor thread. Consumers receive updates over async channels
//! and can await them from any executor.

pub mod handle;
pub mod log;
pub mod manager;
pub mod quality;
pub mod uci;

pub use handle::{
    EngineEvent, EngineHandle, EngineId, Limits, START_FEN, SearchRequest, position_command,
};
pub use log::{EngineLog, LogKind, LogLine};
pub use manager::{EngineConfig, EngineManager, EngineProfile, EngineStatus};
pub use quality::{Quality, classify as classify_move};
pub use uci::{
    AnalysisUpdate, EngineInfo, OptionKind, Score, UciOption, Wdl, format_hash, format_nodes,
    format_speed,
};
