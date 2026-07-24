use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::io::{self, IsTerminal, Write as _};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Default, PartialEq, Eq)]
pub struct LaunchOptions {
    pub fullscreen: bool,
    pub command: Option<LaunchCommand>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct LaunchCommand {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Terminal(LaunchOptions),
    Help,
    Version,
    Error(CliError),
}

#[derive(Debug, PartialEq, Eq)]
pub enum CliError {
    UnknownArgument(OsString),
    UnexpectedArgument(OsString),
    MissingCommand,
    InvalidCommand(String),
    ExecutableNotFound(String),
}

pub fn parse_env() -> Action {
    let mut args = std::env::args_os();
    let _ = args.next();
    parse(args)
}

fn parse(mut args: impl Iterator<Item = OsString>) -> Action {
    let mut launch = LaunchOptions::default();

    while let Some(argument) = args.next() {
        if argument == OsStr::new("--help") || argument == OsStr::new("-h") {
            return args
                .next()
                .map(|extra| Action::Error(CliError::UnexpectedArgument(extra)))
                .unwrap_or(Action::Help);
        }
        if argument == OsStr::new("--version") || argument == OsStr::new("-v") {
            return args
                .next()
                .map(|extra| Action::Error(CliError::UnexpectedArgument(extra)))
                .unwrap_or(Action::Version);
        }
        if argument == OsStr::new("--fullscreen") {
            launch.fullscreen = true;
            continue;
        }
        if argument == OsStr::new("--execute") || argument == OsStr::new("-e") {
            let command_arguments: Vec<_> = args.collect();
            return match parse_launch_command(command_arguments) {
                Ok(command) => {
                    launch.command = Some(command);
                    Action::Terminal(launch)
                }
                Err(error) => Action::Error(error),
            };
        }
        return Action::Error(CliError::UnknownArgument(argument));
    }

    Action::Terminal(launch)
}

fn parse_launch_command(arguments: Vec<OsString>) -> Result<LaunchCommand, CliError> {
    if arguments.is_empty() {
        return Err(CliError::MissingCommand);
    }

    let words = if arguments.len() == 1 {
        let command = arguments[0]
            .to_str()
            .ok_or_else(|| CliError::InvalidCommand("command is not valid UTF-8".to_string()))?;
        if command
            .chars()
            .any(|character| character.is_whitespace() || matches!(character, '\'' | '"' | '\\'))
        {
            split_quoted_command(command)?
        } else {
            vec![command.to_string()]
        }
    } else {
        arguments
            .into_iter()
            .map(|argument| {
                argument.into_string().map_err(|_| {
                    CliError::InvalidCommand("command argument is not valid UTF-8".to_string())
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    };

    let mut words = words.into_iter();
    let program = words
        .next()
        .filter(|program| !program.is_empty())
        .ok_or_else(|| CliError::InvalidCommand("the executable name is empty".to_string()))?;
    if program.contains('\0') || words.clone().any(|argument| argument.contains('\0')) {
        return Err(CliError::InvalidCommand(
            "commands cannot contain NUL bytes".to_string(),
        ));
    }
    Ok(LaunchCommand {
        program,
        args: words.collect(),
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Quote {
    Single,
    Double,
}

fn split_quoted_command(command: &str) -> Result<Vec<String>, CliError> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut token_started = false;
    let mut characters = command.chars();

    while let Some(character) = characters.next() {
        match quote {
            Some(Quote::Single) => {
                if character == '\'' {
                    quote = None;
                } else {
                    current.push(character);
                }
            }
            Some(Quote::Double) => match character {
                '"' => quote = None,
                '\\' => current.push(characters.next().ok_or_else(|| {
                    CliError::InvalidCommand("trailing escape in quoted command".to_string())
                })?),
                _ => current.push(character),
            },
            None => match character {
                '\'' => {
                    quote = Some(Quote::Single);
                    token_started = true;
                }
                '"' => {
                    quote = Some(Quote::Double);
                    token_started = true;
                }
                '\\' => {
                    current.push(characters.next().ok_or_else(|| {
                        CliError::InvalidCommand("trailing escape in command".to_string())
                    })?);
                    token_started = true;
                }
                character if character.is_whitespace() => {
                    if token_started {
                        words.push(std::mem::take(&mut current));
                        token_started = false;
                    }
                }
                _ => {
                    current.push(character);
                    token_started = true;
                }
            },
        }
    }

    if quote.is_some() {
        return Err(CliError::InvalidCommand(
            "unterminated quote in command".to_string(),
        ));
    }
    if token_started {
        words.push(current);
    }
    if words.is_empty() {
        return Err(CliError::MissingCommand);
    }
    Ok(words)
}

pub fn validate_command(command: &LaunchCommand) -> Result<(), CliError> {
    if executable_path(&command.program).is_some() {
        Ok(())
    } else {
        Err(CliError::ExecutableNotFound(command.program.clone()))
    }
}

fn executable_path(program: &str) -> Option<PathBuf> {
    let program_path = Path::new(program);
    if program.contains('/') {
        return is_executable(program_path).then(|| program_path.to_path_buf());
    }

    let path =
        std::env::var_os("PATH").unwrap_or_else(|| OsString::from("/usr/local/bin:/usr/bin:/bin"));
    std::env::split_paths(&path)
        .map(|directory| directory.join(program))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

pub fn execute(action: Action) -> i32 {
    match action {
        Action::Terminal(_) => 0,
        Action::Help => write_stdout(&render_help(styling_enabled(&io::stdout()))),
        Action::Version => write_stdout(&format!("Forge {VERSION}\n")),
        Action::Error(error) => {
            let message = render_error(&error, styling_enabled(&io::stderr()));
            let _ = io::stderr().lock().write_all(message.as_bytes());
            2
        }
    }
}

pub fn exit_with_error(error: CliError) -> ! {
    std::process::exit(execute(Action::Error(error)))
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

    let mut output = String::with_capacity(640);
    let _ = writeln!(output, "{bold}{title}Forge{reset} {VERSION}");
    let _ = writeln!(output, "{dim}A native Wayland terminal emulator.{reset}\n");
    let _ = writeln!(output, "{bold}{heading}Usage:{reset}");
    let _ = writeln!(output, "  {green}forge{reset} [OPTIONS]");
    let _ = writeln!(
        output,
        "  {green}forge{reset} [OPTIONS] -e <PROGRAM> [ARGUMENTS]...\n"
    );
    let _ = writeln!(output, "{bold}{heading}Options:{reset}");
    let _ = writeln!(
        output,
        "  {green}-h, --help{reset}              Display this help page"
    );
    let _ = writeln!(
        output,
        "  {green}-v, --version{reset}           Display the running Forge version"
    );
    let _ = writeln!(
        output,
        "      {green}--fullscreen{reset}        Start Forge in fullscreen mode"
    );
    let _ = writeln!(
        output,
        "  {green}-e, --execute{reset} <PROGRAM> Launch a program with all remaining arguments\n"
    );
    let _ = writeln!(output, "{bold}{heading}Examples:{reset}");
    let _ = writeln!(output, "  forge -e fish");
    let _ = writeln!(output, "  forge -e nvim README.md");
    let _ = writeln!(output, "  forge --execute \"cargo run --release\"");
    let _ = writeln!(output, "  forge --fullscreen -e btop");
    output
}

fn render_error(error: &CliError, styled: bool) -> String {
    let (reset, bold, red) = if styled {
        ("\x1b[0m", "\x1b[1m", "\x1b[31m")
    } else {
        ("", "", "")
    };
    let detail = match error {
        CliError::UnknownArgument(argument) => {
            format!("unknown argument '{}'", argument.to_string_lossy())
        }
        CliError::UnexpectedArgument(argument) => {
            format!("unexpected argument '{}'", argument.to_string_lossy())
        }
        CliError::MissingCommand => "-e/--execute requires a program".to_string(),
        CliError::InvalidCommand(reason) => format!("invalid command: {reason}"),
        CliError::ExecutableNotFound(program) => {
            format!("executable '{program}' was not found or is not executable")
        }
    };
    format!("{bold}{red}forge: error:{reset} {detail}.\nTry 'forge --help' for more information.\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Action {
        parse(args.iter().map(|argument| OsString::from(*argument)))
    }

    fn command(action: Action) -> (LaunchOptions, LaunchCommand) {
        let Action::Terminal(mut options) = action else {
            panic!("expected terminal launch")
        };
        let command = options.command.take().expect("expected launch command");
        (options, command)
    }

    #[test]
    fn no_arguments_use_terminal_startup() {
        assert_eq!(parse_strs(&[]), Action::Terminal(LaunchOptions::default()));
    }

    #[test]
    fn help_and_version_aliases_are_supported() {
        assert_eq!(parse_strs(&["--help"]), Action::Help);
        assert_eq!(parse_strs(&["-h"]), Action::Help);
        assert_eq!(parse_strs(&["--version"]), Action::Version);
        assert_eq!(parse_strs(&["-v"]), Action::Version);
    }

    #[test]
    fn execute_aliases_launch_the_same_program() {
        let (_, short) = command(parse_strs(&["-e", "fish"]));
        let (_, long) = command(parse_strs(&["--execute", "fish"]));
        assert_eq!(short, long);
        assert_eq!(short.program, "fish");
        assert!(short.args.is_empty());
    }

    #[test]
    fn unquoted_arguments_are_preserved_after_execute_boundary() {
        let (_, fish) = command(parse_strs(&["-e", "fish", "-l"]));
        assert_eq!(fish.args, ["-l"]);

        let (_, cargo) = command(parse_strs(&["-e", "cargo", "run", "--release"]));
        assert_eq!(cargo.program, "cargo");
        assert_eq!(cargo.args, ["run", "--release"]);

        let (_, nvim) = command(parse_strs(&["-e", "nvim", "file with spaces.txt"]));
        assert_eq!(nvim.args, ["file with spaces.txt"]);
    }

    #[test]
    fn single_quoted_payload_is_split_without_shell_execution() {
        let (_, cargo) = command(parse_strs(&["-e", "cargo run --release"]));
        assert_eq!(cargo.program, "cargo");
        assert_eq!(cargo.args, ["run", "--release"]);

        let (_, nvim) = command(parse_strs(&["-e", "nvim \"file with spaces.txt\""]));
        assert_eq!(nvim.program, "nvim");
        assert_eq!(nvim.args, ["file with spaces.txt"]);
    }

    #[test]
    fn fullscreen_is_parsed_before_execute_and_child_flags_are_not_parsed() {
        let (options, btop) = command(parse_strs(&["--fullscreen", "-e", "btop"]));
        assert!(options.fullscreen);
        assert_eq!(btop.program, "btop");

        let (_, child) = command(parse_strs(&["-e", "program", "--fullscreen", "--help"]));
        assert_eq!(child.args, ["--fullscreen", "--help"]);
    }

    #[test]
    fn execute_without_a_program_is_rejected() {
        assert_eq!(parse_strs(&["-e"]), Action::Error(CliError::MissingCommand));
        assert_eq!(
            parse_strs(&["--execute"]),
            Action::Error(CliError::MissingCommand)
        );
    }

    #[test]
    fn malformed_quoted_commands_are_rejected() {
        assert!(matches!(
            parse_strs(&["-e", "nvim \"unterminated"]),
            Action::Error(CliError::InvalidCommand(_))
        ));
    }

    #[test]
    fn executable_validation_rejects_missing_programs() {
        let command = LaunchCommand {
            program: "forge-program-that-does-not-exist".to_string(),
            args: Vec::new(),
        };
        assert_eq!(
            validate_command(&command),
            Err(CliError::ExecutableNotFound(command.program))
        );
    }

    #[test]
    fn plain_help_documents_execute_without_terminal_escapes() {
        let help = render_help(false);
        assert!(!help.contains('\x1b'));
        assert!(help.contains("-e, --execute"));
        assert!(help.contains("forge --fullscreen -e btop"));
        assert!(help.contains("forge --execute \"cargo run --release\""));
    }
}
