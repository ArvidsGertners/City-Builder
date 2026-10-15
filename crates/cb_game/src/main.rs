//! City Builder entry point: parses the command line and hands over to [`cb_game::run`].

use std::process::ExitCode;

use cb_game::bootstrap::cli;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let parsed = match cli::parse(args.iter().map(String::as_str)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}\n\n{}", cli::HELP);
            return ExitCode::from(2);
        }
    };
    if parsed.help {
        print!("{}", cli::HELP);
        return ExitCode::SUCCESS;
    }
    if parsed.version {
        println!("{}", cb_game::version_line());
        return ExitCode::SUCCESS;
    }
    cb_game::run(parsed)
}
