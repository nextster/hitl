//! `hitl`: a chat's plans, questions, results and requests, answered by a person in one window on the Mac.
//!
//! The binary of the library `hitl` (lib.rs), what his team installs. hitl's own program (`rust/hitl-host`)
//! is built on the same library and adds what only the reviewer's Mac has.

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    hitl::common::exit(hitl::dispatch(arguments))
}
