mod codec;
use hitl_compat_json as json;
mod general;
mod store;
pub use store::Writer;

/// Reads journal events from stdin until EOF and writes them to the SQLite journal under `CONVEYOR_HOME`; the Err is
/// the exit code of the program. The program `hitl-journal` is this and nothing else; the command
/// `hitl journal _stream` runs it in the process of `hitl` itself.
pub fn run() -> Result<(), i32> {
    store::run()
}
