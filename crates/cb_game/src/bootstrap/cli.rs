//! Command-line arguments (no parser dependency; the set is small and fixed).
//!
//! Developer-facing: help and error texts are English (CLAUDE.md, localization exemptions).

use std::path::PathBuf;

/// Parsed arguments.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Cli {
    /// World seed for a new game.
    pub seed: Option<u64>,
    /// Sandbox mode for a new game.
    pub sandbox: bool,
    /// Log filter (overrides `CB_LOG` and settings).
    pub log: Option<String>,
    /// Panic right after bootstrap (crash-report test).
    pub debug_panic: bool,
    /// Turn Bevy system errors into panics (debug builds).
    pub strict_errors: bool,
    /// Render this many frames, report validation errors, exit.
    pub smoke_test: Option<u32>,
    /// Window size in logical pixels.
    pub resolution: Option<(u32, u32)>,
    /// Scene for smoke/screenshot runs.
    pub scene: Option<String>,
    /// Render a screenshot suite and exit.
    pub screenshot_suite: Option<String>,
    /// Output directory for screenshots.
    pub out: Option<PathBuf>,
    /// Override the UI language (`en`, `de`).
    pub language: Option<String>,
    /// Print help and exit.
    pub help: bool,
    /// Print the version and exit.
    pub version: bool,
}

/// A command-line error.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum CliError {
    /// Unknown flag.
    #[error("unknown argument `{0}` (see --help)")]
    Unknown(String),
    /// Flag needs a value.
    #[error("`{0}` needs a value")]
    MissingValue(&'static str),
    /// Value did not parse.
    #[error("invalid value `{value}` for `{flag}`: {reason}")]
    Invalid {
        /// Flag.
        flag: &'static str,
        /// Raw value.
        value: String,
        /// Why.
        reason: &'static str,
    },
}

/// Help text.
pub const HELP: &str = "\
city-builder [OPTIONS]

  --seed <u64>                 world seed for a new game
  --sandbox                    sandbox mode (unlimited money, all unlocked)
  --language <en|de>           UI language override
  --log <filter>               log filter, e.g. \"info,cb_sim=debug\" (also CB_LOG)
  --resolution <WxH>           window size in logical pixels
  --smoke-test <frames>        render N frames, report validation errors, exit
  --scene <name>               scene for smoke/screenshot runs (default: smoke)
  --screenshot-suite <name>    render a screenshot suite (e.g. M1) and exit
  --out <dir>                  output directory for --screenshot-suite
  --strict-errors              system errors panic (debug builds)
  --debug-panic                panic after start-up (tests the crash reporter)
  --version                    print version
  --help                       print this help
";

fn value<'a>(
    flag: &'static str,
    it: &mut impl Iterator<Item = &'a str>,
) -> Result<&'a str, CliError> {
    it.next().ok_or(CliError::MissingValue(flag))
}

fn parse_resolution(v: &str) -> Option<(u32, u32)> {
    let (w, h) = v.split_once(['x', 'X'])?;
    let (w, h) = (w.parse::<u32>().ok()?, h.parse::<u32>().ok()?);
    ((64..=16_384).contains(&w) && (64..=16_384).contains(&h)).then_some((w, h))
}

/// Parses arguments (without the program name). Accepts `--flag value` and `--flag=value`.
pub fn parse<'a>(args: impl IntoIterator<Item = &'a str>) -> Result<Cli, CliError> {
    let mut cli = Cli::default();
    let mut split: Vec<&str> = Vec::new();
    for a in args {
        match a.split_once('=') {
            Some((k, v)) if k.starts_with("--") => {
                split.push(k);
                split.push(v);
            }
            _ => split.push(a),
        }
    }
    let mut it = split.into_iter();
    while let Some(arg) = it.next() {
        match arg {
            "--seed" => {
                let v = value("--seed", &mut it)?;
                cli.seed = Some(v.parse().map_err(|_| CliError::Invalid {
                    flag: "--seed",
                    value: v.to_owned(),
                    reason: "expected an unsigned integer",
                })?);
            }
            "--sandbox" => cli.sandbox = true,
            "--language" => cli.language = Some(value("--language", &mut it)?.to_owned()),
            "--log" => cli.log = Some(value("--log", &mut it)?.to_owned()),
            "--resolution" => {
                let v = value("--resolution", &mut it)?;
                cli.resolution = Some(parse_resolution(v).ok_or_else(|| CliError::Invalid {
                    flag: "--resolution",
                    value: v.to_owned(),
                    reason: "expected WxH, each 64..=16384",
                })?);
            }
            "--smoke-test" => {
                let v = value("--smoke-test", &mut it)?;
                let n: u32 = v.parse().map_err(|_| CliError::Invalid {
                    flag: "--smoke-test",
                    value: v.to_owned(),
                    reason: "expected a frame count",
                })?;
                cli.smoke_test = Some(n.max(1));
            }
            "--scene" => cli.scene = Some(value("--scene", &mut it)?.to_owned()),
            "--screenshot-suite" => {
                cli.screenshot_suite = Some(value("--screenshot-suite", &mut it)?.to_owned());
            }
            "--out" => cli.out = Some(PathBuf::from(value("--out", &mut it)?)),
            "--strict-errors" => cli.strict_errors = true,
            "--debug-panic" => cli.debug_panic = true,
            "--version" | "-V" => cli.version = true,
            "--help" | "-h" => cli.help = true,
            other => return Err(CliError::Unknown(other.to_owned())),
        }
    }
    Ok(cli)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses_smoke_test_args() {
        let cli = parse(["--smoke-test", "60", "--resolution=640x360", "--seed", "42"]).unwrap();
        assert_eq!(cli.smoke_test, Some(60));
        assert_eq!(cli.resolution, Some((640, 360)));
        assert_eq!(cli.seed, Some(42));
        assert!(!cli.sandbox);
    }

    #[test]
    fn cli_rejects_bad_input() {
        assert_eq!(parse(["--bogus"]), Err(CliError::Unknown("--bogus".into())));
        assert_eq!(parse(["--seed"]), Err(CliError::MissingValue("--seed")));
        assert!(matches!(
            parse(["--resolution", "10x10"]),
            Err(CliError::Invalid { .. })
        ));
        assert!(matches!(
            parse(["--resolution", "800"]),
            Err(CliError::Invalid { .. })
        ));
        assert!(matches!(
            parse(["--seed", "-1"]),
            Err(CliError::Invalid { .. })
        ));
    }

    #[test]
    fn cli_screenshot_suite() {
        let cli = parse(["--screenshot-suite", "M1", "--out", "docs/screenshots/M1"]).unwrap();
        assert_eq!(cli.screenshot_suite.as_deref(), Some("M1"));
        assert_eq!(cli.out, Some(PathBuf::from("docs/screenshots/M1")));
    }
}
