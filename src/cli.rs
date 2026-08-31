//! Argument parsing with std only. A handful of flags do not need clap.

use std::path::PathBuf;

use crate::logo::LOGO;

/// One titled group of bindings. Shared by `rings help`, `--help`, and the
/// help overlay so the three never drift apart.
pub struct KeyGroup {
    pub title: &'static str,
    pub keys: &'static [(&'static str, &'static str)],
    /// Muted line under the group; explains a rule, not a key.
    pub note: Option<&'static str>,
}

/// Keys stay within `HELP_KEY_W`, descriptions within `HELP_DESC_W`, and
/// notes within `HELP_COL_W`; the test below holds the table to it.
pub const KEY_GROUPS: &[KeyGroup] = &[
    KeyGroup {
        title: "Navigate",
        keys: &[
            ("j k  ↑ ↓", "move selection"),
            ("PgUp PgDn", "move by a page"),
            ("g  G", "first / last"),
            ("Enter", "drill into directory"),
            ("h Backspace ←", "go up one directory"),
            ("-", "back to the picker"),
        ],
        note: None,
    },
    KeyGroup {
        title: "Views",
        keys: &[
            ("f", "Temp & cache findings"),
            ("c", "delete collector"),
            ("e", "export view to CSV"),
            ("?  F1", "this help"),
            ("q", "quit"),
        ],
        note: None,
    },
    KeyGroup {
        title: "Delete",
        keys: &[
            ("Space  d", "mark or unmark"),
            ("x", "confirm from collector"),
        ],
        note: Some("Never automatic · root types DELETE"),
    },
    KeyGroup {
        title: "Picker",
        keys: &[
            ("Enter  l", "open directory"),
            ("h Backspace", "go up"),
            ("s", "scan highlighted dir"),
            ("Esc", "back to the scan"),
        ],
        note: Some("Opens with no PATH, or on -"),
    },
    KeyGroup {
        title: "Mouse",
        keys: &[
            ("click", "select · click footer"),
            ("double-click", "drill in"),
            ("right-click", "context menu"),
            ("wheel", "scroll"),
            ("hover", "highlight · slice info"),
        ],
        note: None,
    },
];

/// Help overlay geometry: key caps, descriptions, and the column they make.
/// Two columns plus a 2-cell gap fit inside an 80-column box with a margin.
pub const HELP_KEY_W: usize = 13;
pub const HELP_DESC_W: usize = 22;
pub const HELP_COL_W: usize = HELP_KEY_W + 2 + HELP_DESC_W;

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
    --theme <NAME>       Color theme: rings (default), nord, gruvbox, dracula,
                         solarized-dark, mono
    -h, --help           Print help
    -V, --version        Print version

When stdout is not a terminal (piped or redirected), rings prints the
plain table instead of opening the TUI. --csv, --json, and --plain
never enter the TUI, even in a terminal.

An interactive TUI launch checks GitHub Releases for a newer rings and
offers to install it. --offline or RINGS_NO_UPDATE=1 skips the check.
Pipes, --plain, --json, --csv, --help, and --version never check.

Colors follow the terminal: 24-bit where COLORTERM says so, else 256, else
16. NO_COLOR disables color; RINGS_COLORS=16|256|truecolor overrides.

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
    for (i, group) in KEY_GROUPS.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str("  ");
        out.push_str(&group.title.to_ascii_uppercase());
        out.push('\n');
        for (key, desc) in group.keys {
            let pad = HELP_KEY_W.saturating_sub(key.chars().count());
            out.push_str(&format!("    {key}{}  {desc}\n", " ".repeat(pad)));
        }
        if let Some(note) = group.note {
            out.push_str(&format!("    {note}\n"));
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
    pub theme: Option<String>,
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
                "--theme" => {
                    let name = it
                        .next()
                        .ok_or_else(|| "--theme requires a name".to_string())?;
                    cli.theme = Some(name);
                }
                s if s.starts_with("--theme=") => {
                    cli.theme = Some(s["--theme=".len()..].to_string());
                }
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

        let cli = parse(&["--theme", "nord", "/"]).unwrap();
        assert_eq!(cli.theme.as_deref(), Some("nord"));
        let cli = parse(&["--theme=mono"]).unwrap();
        assert_eq!(cli.theme.as_deref(), Some("mono"));
        assert!(parse(&["--theme"]).is_err());
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
            "NAVIGATE",
            "VIEWS",
            "DELETE",
            "PICKER",
            "MOUSE",
            "types DELETE",
            "--plain",
            "--no-tui",
            "--offline",
            "--theme",
            "solarized-dark",
            "NO_COLOR",
            "GitHub Release",
            "RINGS_NO_UPDATE",
            "rings help",
        ] {
            assert!(text.contains(needle), "help missing {needle:?}:\n{text}");
        }
        for group in KEY_GROUPS {
            for (key, desc) in group.keys {
                assert!(text.contains(key), "help missing key {key:?}");
                assert!(text.contains(desc), "help missing {desc:?}");
            }
        }
    }

    #[test]
    fn key_table_fits_two_columns_on_eighty_cols() {
        assert!(
            2 * HELP_COL_W + 2 + 2 <= 78,
            "two columns, a tight gap, and a margin must fit an 80-col box"
        );
        for group in KEY_GROUPS {
            for (key, desc) in group.keys {
                assert!(key.chars().count() <= HELP_KEY_W, "{key:?} is too wide");
                assert!(desc.chars().count() <= HELP_DESC_W, "{desc:?} is too wide");
            }
            if let Some(note) = group.note {
                assert!(note.chars().count() <= HELP_COL_W, "{note:?} is too wide");
            }
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
