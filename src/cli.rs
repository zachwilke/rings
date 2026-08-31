//! Argument parsing with std only. Five flags and a path do not need clap.

use std::path::PathBuf;

pub const HELP: &str = "\
rings — disk usage sunburst for Linux servers (SSH TUI)

rings scans a path, then shows a DaisyDisk-style radial sunburst and a
largest-first list. Mark waste for deletion only after an explicit
confirm. Nothing is removed while you browse.

USAGE:
    rings [OPTIONS] [PATH]

ARGS:
    [PATH]    Directory or file to scan (default: current directory)

OPTIONS:
    --json               Write the analyzed tree as JSON to stdout and exit
    --csv <FILE>         Write findings CSV to FILE and exit (temp file, then rename)
    --one-file-system    Stay on one filesystem (default)
    --all-filesystems    Descend into other mounted filesystems
    --apparent           Size by apparent bytes (st_size) instead of used
    -h, --help           Print help
    -V, --version        Print version

Full-disk scan (normal on a server):

    sudo rings /

Without root, rings still works on what you can read and tells you when
permission errors mean you need sudo.

CSV export includes every directory, every temp/cache/log/journal/crash
hit, and every file of 1 MiB or more. Tiny ordinary files are omitted.
";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cli {
    pub path: Option<PathBuf>,
    pub json: bool,
    pub csv: Option<PathBuf>,
    pub all_filesystems: bool,
    pub apparent: bool,
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
                "--one-file-system" => cli.all_filesystems = false,
                "--all-filesystems" => cli.all_filesystems = true,
                "--apparent" => cli.apparent = true,
                "-h" | "--help" => cli.help = true,
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
}
