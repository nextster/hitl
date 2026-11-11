//! The window's queue in the app on the phone (what were `phone.py` and `relay.py`).
//!
//! «чтобы на мобильное приложение hitl приходила всё та же очередь вопросов и можно было тыкать с телефона всё то
//! же самое» (2026-10-10). The app «HITL» on his iPhone is a second screen of the check window: it shows the check
//! and the item the window shows, and a button pressed there is pressed in the window, by the window's own code. So an
//! answer given on the phone is the answer the window would have kept, and the verdict is written by the window as
//! ever, for the same commit. `bridge` is what carries the two to each other; `relay` the way outside home.
//!
//! A part of hitl's move to Rust (`docs/rust-move.md`): `hitl` asks `dispatch` first.

pub mod bridge;
pub mod codec;
pub mod commands;
pub mod push;
pub mod py;
pub mod relay;
pub mod shown;
pub mod tls;
pub mod word;

/// Answers `command` with its arguments, or None when this part does not have it.
pub fn dispatch(command: &str, rest: &[String]) -> Option<i32> {
    match command {
        "phone" => Some(commands::main(rest)),
        // hitl asks hitl for the phone's process when it brings the window up (`HITL_HOOK _hitl panel`).
        "_hitl" if rest.len() == 1 && rest[0] == "panel" => {
            bridge::ensure();
            Some(0)
        }
        _ => None,
    }
}
