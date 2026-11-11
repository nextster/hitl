fn main() {
    if std::env::args_os().len() != 1 {
        std::process::exit(64);
    }
    if let Err(code) = hitl_journal::run() {
        std::process::exit(code);
    }
}
