use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use rings::cli::{Cli, HELP};
use rings::csv_export::write_csv;
use rings::json::tree_to_json;
use rings::scan::{scan_with_progress, WalkEvent, WalkOptions};
use rings::unix;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let cli = match Cli::parse_from(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(e) => {
            eprintln!("rings: {e}");
            std::process::exit(2);
        }
    };
    if cli.help {
        print!("{HELP}");
        return;
    }
    if cli.version {
        println!("rings {VERSION}");
        return;
    }
    if let Err(e) = run(cli) {
        eprintln!("rings: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), String> {
    let path = cli.scan_path();
    let opts = WalkOptions {
        one_file_system: cli.one_file_system(),
        root_dev_override: None,
    };

    if cli.json || cli.csv.is_some() {
        let tree = scan_headless(&path, opts)?;
        if cli.json {
            let json = tree_to_json(&tree, tree.root);
            io::stdout()
                .lock()
                .write_all(json.as_bytes())
                .map_err(|e| e.to_string())?;
        }
        if let Some(csv_path) = cli.csv {
            let n = write_csv(&csv_path, &tree, tree.root, &BTreeSet::new())?;
            eprintln!("wrote {n} rows to {}", csv_path.display());
        }
        return Ok(());
    }

    rings::tui::run(path, opts, cli.apparent)
}

fn scan_headless(path: &Path, opts: WalkOptions) -> Result<rings::scan::Tree, String> {
    let (tx, rx) = mpsc::channel();
    let path_owned = path.to_path_buf();
    std::thread::spawn(move || scan_with_progress(path_owned, opts, tx));

    let mut printed = false;
    loop {
        match rx.recv_timeout(Duration::from_millis(80)) {
            Ok(WalkEvent::Progress(p)) => {
                eprint!(
                    "\rscanning {:>8} files  {:>6} dirs  {:>4} errors  {}          ",
                    p.files,
                    p.dirs,
                    p.errors,
                    p.current.display()
                );
                let _ = io::stderr().flush();
                printed = true;
            }
            Ok(WalkEvent::Done(result)) => {
                if printed {
                    eprintln!();
                }
                if !unix::running_as_root() {
                    eprintln!("hint: not running as root — sudo rings / to see the whole disk");
                }
                return result;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("scanner thread stopped".into());
            }
        }
    }
}
