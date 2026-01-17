//! String Equations CLI
//!
//! Usage:
//!   strql <query_file> <input_file>
//!   strql -e <query> <input_file>
//!   strql --inline <query> <input>

use std::env;
use std::fs;
use std::process;

use miette::{GraphicalReportHandler, GraphicalTheme};
use strql::error::{StrqlError, StrqlResult};
use strql::evaluate_partition;

fn main() {
    if let Err(e) = run() {
        print_error(&e);
        process::exit(1);
    }
}

fn print_error(err: &StrqlError) {
    let noder = GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor());
    let mut output = String::new();
    if let Err(_) = noder.render_report(&mut output, err) {
        // Fallback to simple error message
        eprintln!("Error: {}", err);
    } else {
        eprintln!("{}", output);
    }
}

fn run() -> StrqlResult<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_help();
        process::exit(1);
    }

    // Check for help flag
    if args[1] == "-h" || args[1] == "--help" {
        print_help();
        return Ok(());
    }

    let (query, input) = if args[1] == "--inline" {
        // --inline <query> <input>
        if args.len() < 4 {
            eprintln!("Error: --inline requires query and input arguments");
            print_help();
            process::exit(1);
        }
        (args[2].clone(), args[3].clone())
    } else if args[1] == "-e" {
        // -e <query> <input_file>
        if args.len() < 4 {
            eprintln!("Error: -e requires query and input file arguments");
            print_help();
            process::exit(1);
        }
        let input = fs::read_to_string(&args[3]).unwrap_or_else(|e| {
            eprintln!("Failed to read input file '{}': {}", args[3], e);
            process::exit(1);
        });
        (args[2].clone(), input)
    } else {
        // <query_file> <input_file>
        if args.len() < 3 {
            eprintln!("Error: missing input file argument");
            print_help();
            process::exit(1);
        }

        let query = fs::read_to_string(&args[1]).unwrap_or_else(|e| {
            eprintln!("Failed to read query file '{}': {}", args[1], e);
            process::exit(1);
        });

        let input = fs::read_to_string(&args[2]).unwrap_or_else(|e| {
            eprintln!("Failed to read input file '{}': {}", args[2], e);
            process::exit(1);
        });

        (query, input)
    };

    let result = evaluate_partition(&query, &input)?;

    println!("{}", serde_json::to_string_pretty(&result).unwrap());

    Ok(())
}

fn print_help() {
    eprintln!("link to github once project is on github")
}
