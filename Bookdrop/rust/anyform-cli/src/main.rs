use std::path::PathBuf;
use std::process::ExitCode;

use anyform_core::{Options, StdLog};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("parse") => {
            let Some(input) = args.get(1) else {
                eprintln!("usage: anyform parse <input.epub>");
                return ExitCode::FAILURE;
            };
            run_parse(PathBuf::from(input))
        }
        Some("convert") => {
            let (Some(input), Some(output)) = (args.get(1), args.get(2)) else {
                eprintln!("usage: anyform convert <input> <output>");
                return ExitCode::FAILURE;
            };
            run_convert(PathBuf::from(input), PathBuf::from(output))
        }
        _ => {
            eprintln!("usage: anyform <parse|convert> ...");
            ExitCode::FAILURE
        }
    }
}

fn run_parse(input: PathBuf) -> ExitCode {
    let registry = anyform_doc::document_registry();
    let opts = Options::new();
    match registry.parse(&input, &opts, &StdLog) {
        Ok(ir) => {
            match serde_json::to_string_pretty(&ir) {
                Ok(json) => {
                    println!("{json}");
                    println!(
                        "// cover: {} bytes, content_dir: {}",
                        ir.metadata.cover.as_ref().map(Vec::len).unwrap_or(0),
                        ir.content_dir.display()
                    );
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("failed to serialize parsed document: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Err(e) => {
            eprintln!("parse failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_convert(input: PathBuf, output: PathBuf) -> ExitCode {
    let registry = anyform_doc::document_registry();
    let opts = Options::new();
    match registry.convert(&input, &output, &opts, &StdLog) {
        Ok(()) => {
            println!("wrote {}", output.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("convert failed: {e}");
            ExitCode::FAILURE
        }
    }
}
