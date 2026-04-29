/// seed-daemon library surface.
///
/// The `wire`, `ipc`, and `event_log` modules are public so integration tests
/// can connect to a running daemon without duplicating framing code.
/// The `daemon`, `notify`, and `schedule` modules are pub for the bin target.
pub mod daemon;
pub mod event_log;
pub mod ipc;
pub mod notify;
pub mod schedule;
pub mod wire;
