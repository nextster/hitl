//! What each file is checked by, the first pattern that matches. "*" is every suite, "" none. The suites are the
//! Swift and shell ones (`package`, `builder`, `iphone`) and `cargo-NAME`, the `cargo test` of a crate; a file under
//! `rust/NAME/` is not listed here: `testing.rs` adds the crate and what depends on it by itself.

pub const SUITES_OF: &[(&str, &str)] = &[
    ("rust/*", ""),
    ("ConveyorCheck/*", "package"),
    ("tests/iphone-tests.sh", "iphone"),
    ("tests/ios-run-tests.sh", "builder"),
    ("ios-run.sh", "builder iphone"),
    ("skills/*", "cargo-hitl-tools"),
    ("window/*", "cargo-hitl cargo-hitl-phone"),
    ("relay/*", "cargo-hitl-phone"),
    ("tests/*.swift", "cargo-hitl-phone"),
    ("hitl", "*"),
    ("*.md", ""),
    ("docs/*", ""),
    ("demo/*", ""),
    (".gitattributes", ""),
    (".gitignore", ""),
    ("tests/merge-for-real.sh", ""),
];
