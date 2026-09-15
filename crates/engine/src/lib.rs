//! Replay state: where the cursor is, what it is allowed to see, and how a
//! session is persisted, and how trades are simulated against it.

pub mod export;
pub mod fills;
pub mod gaps;
pub mod journal;
pub mod order;
pub mod replay;
pub mod session;
pub mod sim;
pub mod stats;

pub use gaps::{Gap, GapKind};
pub use journal::JournalEntry;
pub use order::{Order, OrderKind, Position, Reason, Role, Side, Trade};
pub use replay::Replay;
pub use session::Session;
pub use sim::{SimConfig, SimState};
pub use stats::{equity_curve, round_trips, summary, Equity, Summary};
