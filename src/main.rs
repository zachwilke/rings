use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use rings::cli::{help_text, Cli};
use rings::csv_export::write_csv;
use rings::json::tree_to_json;
use rings::plain::render_plain;
use rings::scan::{scan_with_progress, WalkEvent, WalkOptions};
use rings::sys;

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
        print!("{}", help_text());
        return;
    }
    if cli.version {
        println!("rings {VERSION}");
        return;
    }
    rings::update::cleanup_replaced_exe();
    rings::update::maybe_offer_and_apply(&cli);
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

    if !cli.wants_tui(sys::stdout_is_tty()) {
        let show_progress = sys::stderr_is_tty() && (cli.json || cli.csv.is_some());
        let tree = scan_headless(&path, opts, show_progress)?;
        if cli.json {
            let json = tree_to_json(&tree, tree.root);
            io::stdout()
                .lock()
                .write_all(json.as_bytes())
                .map_err(|e| e.to_string())?;
        }
        if let Some(csv_path) = &cli.csv {
            let n = write_csv(csv_path, &tree, tree.root, &BTreeSet::new())?;
            eprintln!("wrote {n} rows to {}", csv_path.display());
        }
        if cli.plain || (!cli.json && cli.csv.is_none()) {
            let table = render_plain(&tree, tree.root);
            io::stdout()
                .lock()
                .write_all(table.as_bytes())
                .map_err(|e| e.to_string())?;
        }
        return Ok(());
    }

    rings::tui::run(path, opts, cli.apparent)
}

fn scan_headless(
    path: &Path,
    opts: WalkOptions,
    show_progress: bool,
) -> Result<rings::scan::Tree, String> {
    let (tx, rx) = mpsc::channel();
    let path_owned = path.to_path_buf();
    std::thread::spawn(move || scan_with_progress(path_owned, opts, tx));

    let mut printed = false;
    loop {
        match rx.recv_timeout(Duration::from_millis(80)) {
            Ok(WalkEvent::Progress(p)) => {
                if show_progress {
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
            }
            Ok(WalkEvent::Done(result)) => {
                if printed {
                    eprintln!();
                }
                if show_progress && !sys::running_as_root() {
                    eprintln!("hint: {}", sys::full_disk_hint());
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
