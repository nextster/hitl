use hitl_stats::{Arguments, Error};
use std::io::Write;
fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}
fn run() -> Result<(), Error> {
    let args: Vec<_> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let mut out = std::io::stdout().lock();
    let transport = std::env::var_os("CONVEYOR_STATS_TRANSPORT").is_some();
    match hitl_stats::arguments(&args)? {
        Arguments::Help => {
            if transport {
                out.write_all(b"T").map_err(|_| Error::Output)?;
            }
            out.write_all(hitl_stats::USAGE.as_bytes())
                .map_err(|_| Error::Output)?;
        }
        Arguments::Run(options) => {
            let root = std::env::var_os("CONVEYOR_HOME")
                .map(std::path::PathBuf::from)
                .ok_or(Error::Data("use hitl stats to resolve its state root"))?;
            let errors =
                std::env::var("CONVEYOR_STATS_STDOUT_ERRORS").unwrap_or("surrogateescape".into());
            if options.json {
                if transport {
                    out.write_all(b"J").map_err(|_| Error::Output)?;
                }
                for row in hitl_stats::json_rows(&root, &options.days, now())? {
                    if hitl_stats::selected(&row, options.comments) {
                        let line = hitl_stats::json_line(&row, &errors)?;
                        out.write_all(&line).map_err(|_| Error::Output)?;
                    }
                }
            } else {
                if transport {
                    out.write_all(b"T").map_err(|_| Error::Output)?;
                }
                let report = hitl_stats::report(&root, &options.days, options.comments, now)?;
                out.write_all(&hitl_stats::output_line(&report, &errors)?)
                    .map_err(|_| Error::Output)?;
            }
        }
    }
    out.flush().map_err(|_| Error::Output)?;
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        let code = match error {
            Error::Output => {
                eprintln!("stats: stdout write failed");
                120
            }
            Error::Usage => {
                eprintln!("{}", hitl_stats::REFUSAL);
                64
            }
            Error::Data(message) => {
                eprintln!("stats: {message}");
                1
            }
        };
        std::process::exit(code);
    }
}
