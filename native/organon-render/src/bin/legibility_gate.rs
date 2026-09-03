//! `legibility-gate` — score a rendered frame against a legibility fixture and exit
//! 0 / 1 / 2 (PBR text T13, organon#217). A thin `main` over
//! [`organon_render::legibility_gate`], which is where the logic and the tests are;
//! `verify.sh --legibility` calls this, and so can a person over any PNG:
//!
//! ```text
//! cargo run --release -p organon-render --bin legibility-gate -- \
//!     target/verify/frames/legibility-faceplate.png \
//!     organon-render/tests/fixtures/omarchy-logo.txt \
//!     --thresholds verify/legibility/thresholds.toml --geom auto
//! ```
//!
//! No clap, no new dependency: the crate's dependency list is an acceptance test
//! (`cargo tree -p organon-render`), and eight options do not need a parser.

use organon_render::legibility_gate::{self as gate, Command, EXIT_USAGE};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = match gate::parse_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("legibility-gate: {e}\n\n{}", gate::USAGE);
            std::process::exit(EXIT_USAGE);
        }
    };
    match cmd {
        Command::Help => {
            print!("{}", gate::USAGE);
        }
        Command::EmitText(path) => {
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
                eprintln!("legibility-gate: reading {}: {e}", path.display());
                std::process::exit(EXIT_USAGE);
            });
            let fixture = organon_render::legibility::Fixture::parse(&text).unwrap_or_else(|e| {
                eprintln!("legibility-gate: fixture {}: {e}", path.display());
                std::process::exit(EXIT_USAGE);
            });
            print!("{}", gate::emit_text(&fixture));
        }
        Command::Gate(a) => {
            let mut out = String::new();
            match gate::run(&a, &mut out) {
                Ok(code) => {
                    print!("{out}");
                    std::process::exit(code);
                }
                Err(e) => {
                    print!("{out}");
                    eprintln!("legibility-gate: {e}");
                    std::process::exit(EXIT_USAGE);
                }
            }
        }
    }
}
