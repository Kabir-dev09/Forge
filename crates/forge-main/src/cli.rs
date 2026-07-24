use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::io::{self, IsTerminal, Write as _};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Terminal,
    Help,
    Version,
    Invalid(OsString),
}

pub fn parse_env() -> Action {
    let mut args = std::env::args_os();
    let _ = args.next();
    parse(args)
}

fn parse(mut args: impl Iterator<Item = OsString>) -> Action {
    let Some(first) = args.next() else {
        return Action::Terminal;
    };

    let action = if first == OsStr::new("--help") || first == OsStr::new("-h") {
        Action::Help
    } else if first == OsStr::new("--version") || first == OsStr::new("-v") {
        Action::Version
    } else {
        return Action::Invalid(first);
    };

    args.next().map(Action::Invalid).unwrap_or(action)
}

pub fn execute(action: Action) -> i32 {
    match action {
        Action::Terminal => 0,
        Action::Help => write_stdout(&render_help(styling_enabled(&io::stdout()))),
        Action::Version => write_stdout(&format!("Forge {VERSION}\n")),
        Action::Invalid(argument) => {
            let message = render_invalid_argument(&argument, styling_enabled(&io::stderr()));
            let _ = io::stderr().lock().write_all(message.as_bytes());
            2
        }
    }
}

fn styling_enabled(stream: &impl IsTerminal) -> bool {
    stream.is_terminal()
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var_os("TERM").is_none_or(|term| term != OsStr::new("dumb"))
}

fn write_stdout(output: &str) -> i32 {
    match io::stdout().lock().write_all(output.as_bytes()) {
        Ok(()) => 0,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(error) => {
            let _ = writeln!(
                io::stderr().lock(),
                "forge: failed to write output: {error}"
            );
            1
        }
    }
}

fn render_help(styled: bool) -> String {
    let (reset, bold, dim, title, heading, green) = if styled {
        (
            "\x1b[0m",
            "\x1b[1m",
            "\x1b[2m",
            "\x1b[38;2;255;231;165m",
            "\x1b[38;2;196;167;231m",
            "\x1b[32m",
        )
    } else {
        ("", "", "", "", "", "")
    };

    let mut output = String::with_capacity(280);
    let _ = writeln!(output, "{bold}{title}Forge{reset} {VERSION}");
    let _ = writeln!(output, "{dim}A native Wayland terminal emulator.{reset}\n");
    let _ = writeln!(output, "{bold}{heading}Usage:{reset}");
    let _ = writeln!(output, "  {green}forge{reset} [OPTIONS]\n");
    let _ = writeln!(output, "{bold}{heading}Options:{reset}");
    let _ = writeln!(
        output,
        "  {green}-h, --help{reset}       Display this help page"
    );
    let _ = writeln!(
        output,
        "  {green}-v, --version{reset}    Display the running Forge version"
    );
    output
}

fn render_invalid_argument(argument: &OsStr, styled: bool) -> String {
    let (reset, bold, red) = if styled {
        ("\x1b[0m", "\x1b[1m", "\x1b[31m")
    } else {
        ("", "", "")
    };
    format!(
        "{bold}{red}forge: error:{reset} unknown argument '{}'.\nTry 'forge --help' for more information.\n",
        argument.to_string_lossy()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Action {
        parse(args.iter().map(|argument| OsString::from(*argument)))
    }

    #[test]
    fn no_arguments_use_terminal_startup() {
        assert_eq!(parse_strs(&[]), Action::Terminal);
    }

    #[test]
    fn help_aliases_are_supported() {
        assert_eq!(parse_strs(&["--help"]), Action::Help);
        assert_eq!(parse_strs(&["-h"]), Action::Help);
    }

    #[test]
    fn version_aliases_are_supported() {
        assert_eq!(parse_strs(&["--version"]), Action::Version);
        assert_eq!(parse_strs(&["-v"]), Action::Version);
    }

    #[test]
    fn unknown_and_trailing_arguments_are_rejected() {
        assert_eq!(
            parse_strs(&["--unknown"]),
            Action::Invalid(OsString::from("--unknown"))
        );
        assert_eq!(
            parse_strs(&["--help", "extra"]),
            Action::Invalid(OsString::from("extra"))
        );
    }

    #[test]
    fn plain_help_is_readable_without_terminal_styling() {
        let help = render_help(false);
        assert!(!help.contains('\x1b'));
        assert!(help.contains("forge [OPTIONS]"));
        assert!(help.contains("-h, --help"));
        assert!(help.contains("-v, --version"));
    }

    #[test]
    fn styled_help_uses_restrained_terminal_styling() {
        let help = render_help(true);
        assert!(help.contains("\x1b[1m"));
        assert!(help.contains("\x1b[38;2;255;231;165m"));
        assert!(help.contains("\x1b[38;2;196;167;231m"));
        assert!(help.contains("\x1b[32m"));
        assert!(!help.contains("\x1b[4m"));
    }

    #[test]
    fn invalid_argument_points_to_help() {
        let message = render_invalid_argument(OsStr::new("--bad"), false);
        assert!(!message.contains('\x1b'));
        assert!(message.contains("unknown argument '--bad'"));
        assert!(message.contains("forge --help"));
    }
}
