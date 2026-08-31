//! Argument parsing with std only. A handful of flags do not need clap.

use std::path::PathBuf;

use crate::logo::LOGO;

/// Every TUI binding, shared by `rings help`, `--help`, and the help overlay.
pub const KEY_LINES: &[&str] = &[
    "j k  ↑ ↓  PgUp PgDn   move selection",
    "Enter                 drill into the selected directory",
    "h Backspace ←         go up one directory  ·  -  back to the picker",
    "Space  d              mark or unmark for the delete collector",
    "f                     Temp & cache findings",
    "c                     delete collector",
    "x                     confirm delete (from collector)",
    "e                     export current view as rings-export.csv",
    "?  F1                 this help",
    "q                     quit",
    "Picker (rings, no PATH)  j k move · Enter open · h up · s scan · Esc back",
    "Right-click            context menu: open, mark, delete this file or dir",
    "Mouse  click slice/row to select · double-click to drill · click footer",
    "Delete is never automatic. The collector shows paths and total size.",
    "As root/Administrator, type DELETE to unlink. Deletes are logged.",
    "Esc / ? / F1 / h      close this help",
];

const USAGE: &str = "\
rings — disk usage sunburst for Linux, macOS, and Windows

rings scans a path, then shows a DaisyDisk-style radial sunburst and a
largest-first list. Mark waste for deletion only after an explicit
confirm. Nothing is removed while you browse.

USAGE:
    rings [OPTIONS] [PATH]
    rings help

ARGS:
    [PATH]    Directory or file to scan. With no PATH an interactive TUI
              opens the directory picker in the current directory; pick a
              directory there and press s to scan it. Piped or scripted
              runs with no PATH still scan the current directory.

OPTIONS:
    --json               Write the analyzed tree as JSON to stdout and exit
    --csv <FILE>         Write findings CSV to FILE and exit (temp file, then rename)
    --plain, --no-tui    Print a parseable table to stdout and exit
    --offline            Skip the GitHub Release update check
    --one-file-system    Stay on one filesystem (default)
    --all-filesystems    Descend into other mounted filesystems
    --apparent           Size by apparent bytes (st_size) instead of used
    -h, --help           Print help
    -V, --version        Print version

When stdout is not a terminal (piped or redirected), rings prints the
plain table instead of opening the TUI. --csv, --json, and --plain
never enter the TUI, even in a terminal.

An interactive TUI launch checks GitHub Releases for a newer rings and
offers to install it. --offline or RINGS_NO_UPDATE=1 skips the check.
Pipes, --plain, --json, --csv, --help, and --version never check.

Full-disk scan:

    sudo rings /                 # Linux, Raspberry Pi OS, macOS
    rings.exe C:\\               # Windows PowerShell (as Administrator)
    rings --plain /

Without root / Administrator, rings still works on what you can read.

CSV and plain export include every directory, every temp/cache/log/journal/crash
hit, and every file of 1 MiB or more. Tiny ordinary files are omitted.

KEYS:
";

/// Logo + usage + the same key list the TUI overlay shows.
pub fn help_text() -> String {
    let mut out = String::from(LOGO);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
    out.push_str(USAGE);
    for line in KEY_LINES {
        if line.is_empty() {
            out.push('\n');
        } else {
            out.push_str("    ");
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cli {
    pub path: Option<PathBuf>,
    pub json: bool,
    pub csv: Option<PathBuf>,
    pub plain: bool,
    pub all_filesystems: bool,
    pub apparent: bool,
    pub offline: bool,
    pub help: bool,
    pub version: bool,
}

impl Cli {
    pub fn parse_from<I: IntoIterator<Item = String>>(args: I) -> Result<Cli, String> {
        let mut cli = Cli::default();
        let mut it = args.into_iter();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--json" => cli.json = true,
                "--csv" => {
                    let file = it
                        .next()
                        .ok_or_else(|| "--csv requires a file argument".to_string())?;
                    cli.csv = Some(PathBuf::from(file));
                }
                "--plain" | "--no-tui" => cli.plain = true,
                "--offline" => cli.offline = true,
                "--one-file-system" => cli.all_filesystems = false,
                "--all-filesystems" => cli.all_filesystems = true,
                "--apparent" => cli.apparent = true,
                "-h" | "--help" | "help" => cli.help = true,
                "-V" | "--version" => cli.version = true,
                s if s.starts_with("--csv=") => {
                    cli.csv = Some(PathBuf::from(&s["--csv=".len()..]));
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(format!("unknown option: {s} (see rings --help)"));
                }
                _ => {
                    if cli.path.is_some() {
                        return Err(format!("unexpected extra argument: {arg}"));
                    }
                    cli.path = Some(PathBuf::from(arg));
                }
            }
        }
        Ok(cli)
    }

    pub fn scan_path(&self) -> PathBuf {
        self.path.clone().unwrap_or_else(|| PathBuf::from("."))
    }

    pub fn one_file_system(&self) -> bool {
        !self.all_filesystems
    }

    /// TUI only when stdout is a TTY and no scripting flag is set.
    pub fn wants_tui(&self, stdout_is_tty: bool) -> bool {
        if self.json || self.csv.is_some() || self.plain {
            return false;
        }
        stdout_is_tty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, String> {
        Cli::parse_from(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_flags_and_path() {
        let cli = parse(&["--csv", "out.csv", "--apparent", "/var"]).unwrap();
        assert_eq!(cli.csv, Some(PathBuf::from("out.csv")));
        assert!(cli.apparent);
        assert_eq!(cli.path, Some(PathBuf::from("/var")));
        assert!(cli.one_file_system());

        let cli = parse(&["--csv=x.csv"]).unwrap();
        assert_eq!(cli.csv, Some(PathBuf::from("x.csv")));

        let cli = parse(&["--all-filesystems", "--json"]).unwrap();
        assert!(!cli.one_file_system());
        assert!(cli.json);

        let cli = parse(&["--plain", "/"]).unwrap();
        assert!(cli.plain);
        assert_eq!(cli.path, Some(PathBuf::from("/")));

        let cli = parse(&["--no-tui"]).unwrap();
        assert!(cli.plain);

        let cli = parse(&["--offline", "/"]).unwrap();
        assert!(cli.offline);
        assert_eq!(cli.path, Some(PathBuf::from("/")));

        let cli = parse(&["help"]).unwrap();
        assert!(cli.help);
    }

    #[test]
    fn rejects_unknown_and_extra() {
        assert!(parse(&["--frobnicate"]).is_err());
        assert!(parse(&["a", "b"]).is_err());
        assert!(parse(&["--csv"]).is_err());
    }

    #[test]
    fn default_path_is_cwd() {
        let cli = parse(&[]).unwrap();
        assert_eq!(cli.scan_path(), PathBuf::from("."));
    }

    #[test]
    fn no_path_is_the_picker_signal() {
        assert!(
            parse(&[]).unwrap().path.is_none(),
            "no PATH means the TUI opens the directory picker"
        );
        assert_eq!(
            parse(&["--offline"]).unwrap().path,
            None,
            "flags alone still leave the path unset"
        );
        assert_eq!(parse(&["/var"]).unwrap().path, Some(PathBuf::from("/var")));
    }

    #[test]
    fn help_text_contains_logo_and_key_bindings() {
        let text = help_text();
        assert!(
            text.contains(LOGO.trim_end()),
            "help must start from the shared logo"
        );
        for needle in [
            "j k",
            "Enter",
            "Backspace",
            "Space",
            "f                     Temp",
            "c                     delete collector",
            "x                     confirm delete",
            "e                     export",
            "?  F1",
            "q                     quit",
            "Mouse",
            "double-click",
            "Right-click",
            "Picker (rings, no PATH)",
            "back to the picker",
            "--plain",
            "--no-tui",
            "--offline",
            "GitHub Release",
            "RINGS_NO_UPDATE",
            "rings help",
        ] {
            assert!(text.contains(needle), "help missing {needle:?}:\n{text}");
        }
        for line in KEY_LINES {
            if line.is_empty() {
                continue;
            }
            assert!(text.contains(line), "help missing key line {line:?}");
        }
    }

    #[test]
    fn non_tty_and_script_flags_never_want_tui() {
        let interactive = Cli::default();
        assert!(
            interactive.wants_tui(true),
            "a TTY with no flags still opens the TUI"
        );
        assert!(
            !interactive.wants_tui(false),
            "piped stdout must not require a terminal"
        );

        let mut plain = Cli::default();
        plain.plain = true;
        assert!(!plain.wants_tui(true));
        assert!(!plain.wants_tui(false));

        let mut json = Cli::default();
        json.json = true;
        assert!(!json.wants_tui(true));

        let mut csv = Cli::default();
        csv.csv = Some(PathBuf::from("out.csv"));
        assert!(!csv.wants_tui(true));
    }
}
