//! What every part of hitl in Rust shares: where its state lives, how it speaks, how it starts programs.
//!
//! The port of what `src/hitl/common.py` and `src/hitl/processes.py` give every module, without what
//! `hitl::common` already has (JSON as Python writes it, the chat a call belongs to, history, the person who
//! answers): that is re-exported here, so a part depends on this crate alone.
//!
//! Not a copy of Python byte for byte: what chats and the reviewer see is carried over, nothing else.

pub mod chatbutton;
pub mod git;
pub mod journal;
pub mod paths;
pub mod process;
pub mod progress;

pub use hitl::common::{
    EXIT_BUSY, EXIT_REFUSED, Env, agent, chat_link, chat_prefix, chat_tag, chat_title, clock, did, dumps, dumps_line,
    env, history, history_file, items_word, list_of, minutes, new_id, now, number_of, plural, read_json, read_text,
    record, say, text_links, text_of, truthy, var, which, write_json,
};
pub use paths::{command, entry, home, root};

/// The exit codes of the command, as chats read them.
pub const EXIT_OK: i32 = 0;
pub const EXIT_FAILED: i32 = 1;
pub const EXIT_INTERRUPTED: i32 = 130;
