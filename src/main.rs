use anyhow::Result;
use clap::{Parser, Subcommand};
use rowtide::workflow;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "rowtide",
    version,
    about = "An agent-native toolkit for local data"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Inspect explicit inputs and save an immutable handoff.
    Prepare {
        #[arg(long)]
        request: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Recheck changed dependencies; reuse only strongly bound results.
    Refresh {
        #[arg(long)]
        request: Option<PathBuf>,
        #[arg(long)]
        previous: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Compare explicitly mapped native consumption evidence to a handoff.
    Record {
        #[arg(long)]
        previous: PathBuf,
        #[arg(long)]
        actual: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}
fn main() {
    if let Err(err) = run() {
        eprintln!(
            "{}",
            serde_json::json!({"execution":"failed","error":format!("{err:#}")})
        );
        std::process::exit(2)
    }
}
fn run() -> Result<()> {
    let (receipt, out) = match Cli::parse().command {
        Command::Prepare { request, out } => (
            workflow::prepare(workflow::load_request(&request)?, None),
            out,
        ),
        Command::Refresh {
            request,
            previous,
            out,
        } => {
            let old = workflow::load_receipt(&previous)?;
            (
                workflow::prepare(
                    match request {
                        Some(path) => workflow::load_request(&path)?,
                        None => {
                            workflow::validate(&old.request)?;
                            old.request.clone()
                        }
                    },
                    Some((&previous, &old)),
                ),
                out,
            )
        }
        Command::Record {
            previous,
            actual,
            out,
        } => {
            let old = workflow::load_receipt(&previous)?;
            (workflow::record(&previous, &old, &actual)?, out)
        }
    };
    workflow::publish(&receipt, &out)?;
    let check_findings: Vec<_> = receipt
        .findings
        .iter()
        .filter(|f| receipt.request.checks.iter().any(|c| c.id == f.id))
        .collect();
    let recording = receipt.operation == "record";
    let reused_checks = if recording {
        0
    } else {
        check_findings.iter().filter(|f| f.reused).count()
    };
    let rechecked_checks = if recording {
        0
    } else {
        check_findings.iter().filter(|f| !f.reused).count()
    };
    let carried_checks = if recording {
        check_findings
            .iter()
            .filter(|f| f.origin == "measured")
            .count()
    } else {
        0
    };
    println!(
        "{}",
        serde_json::json!({"reused_checks":reused_checks,"rechecked_checks":rechecked_checks,"carried_checks":carried_checks,"execution":receipt.execution,"receipt":out.join("receipt.json"),"summary":out.join("SUMMARY.md"),"mismatches":receipt.findings.iter().filter(|f|f.status=="mismatch").count(),"consumption_mismatched":receipt.consumption.iter().filter(|f|f.status=="mismatched").count(),"consumption_unknown":receipt.consumption.iter().filter(|f|f.status=="unknown").count(),"unknowns":receipt.findings.iter().filter(|f|f.status=="unknown").count()})
    );
    Ok(())
}
