//! Explicit native retirement review, admission, recovery and cancellation.
#![forbid(unsafe_code)]
#[path = "tig-retire/admission.rs"]
mod admission;
#[path = "tig-retire/bookkeeping.rs"]
mod bookkeeping;
#[path = "tig-retire/cancel.rs"]
mod cancel;
#[path = "tig-retire/claim.rs"]
mod claim;
#[path = "tig-retire/codec.rs"]
mod codec;
#[path = "tig-retire/execution.rs"]
mod execution;
#[path = "tig-retire/factories.rs"]
mod factories;
#[path = "tig-retire/history.rs"]
mod history;
#[path = "tig-retire/input_identity.rs"]
mod input_identity;
#[path = "tig-retire/model.rs"]
mod model;
#[path = "tig-retire/policy_proposal.rs"]
mod policy_proposal;
#[path = "tig-retire/policy_stage.rs"]
mod policy_stage;
#[path = "tig-retire/projection.rs"]
mod projection;
#[path = "tig-retire/read_scope.rs"]
mod read_scope;
#[path = "tig-retire/removal.rs"]
mod removal;
#[path = "tig-retire/review.rs"]
mod review;
#[path = "tig-retire/session.rs"]
mod session;
#[path = "tig-retire/stage.rs"]
mod stage;
#[path = "tig-retire/steps.rs"]
mod steps;
use std::{
    fs::File,
    io::{Read, Write},
    process::ExitCode,
};
const USAGE: &str = "usage: tig-retire --review PROFILE | --propose PROFILE | --stage PROFILE | --stage-policy PROFILE | --apply-work WORK [--max-actions N] | --cancel WORK";
fn run() -> Result<u8, String> {
    let mut args = std::env::args_os().skip(1);
    let mode = args.next().ok_or(USAGE)?;
    if mode == "--help" && args.next().is_none() {
        writeln!(std::io::stdout().lock(),"{USAGE}\nReview admitted facts; stage exact consent; reconcile saved native work; stop new admission. Routine eviction and explicit final loss remain separate modes. --propose grants no approval or execution permission.")
            .map_err(|_| "cannot write help")?;
        return Ok(0);
    }
    if mode != "--review"
        && mode != "--propose"
        && mode != "--stage-policy"
        && mode != "--stage"
        && mode != "--apply-work"
        && mode != "--cancel"
    {
        return Err(USAGE.into());
    }
    let path = args
        .next()
        .ok_or("explicit native retirement input required")?;
    let maximum = if let Some(option) = args.next() {
        if mode != "--apply-work" || option != "--max-actions" {
            return Err("unexpected retirement arguments".into());
        }
        let value = args
            .next()
            .and_then(|n| n.into_string().ok())
            .ok_or("explicit invocation action limit required")?;
        let value: u32 = value
            .parse()
            .map_err(|_| "invalid invocation action limit")?;
        if !(1..=256).contains(&value) || args.next().is_some() {
            return Err("invalid invocation action limit/arguments".into());
        }
        Some(value)
    } else {
        None
    };
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "cannot open explicit retirement input")?
        .take(model::MAX_ARTIFACT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read bounded retirement input")?;
    let (out, code) = if mode == "--review" {
        (review::review(&bytes)?, 0)
    } else if mode == "--propose" {
        (policy_proposal::propose(&bytes)?, 0)
    } else if mode == "--stage-policy" {
        (policy_stage::stage(&bytes)?, 0)
    } else if mode == "--stage" {
        (stage::stage(&bytes)?, 0)
    } else if mode == "--cancel" {
        (cancel::cancel(&bytes)?, 0)
    } else if let Some(maximum) = maximum {
        execution::apply_with_limit(&bytes, maximum)?
    } else {
        execution::apply(&bytes)?
    };
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&out)
        .and_then(|_| stdout.write_all(b"\n"))
        .map_err(|_| "cannot write retirement result; reconcile exact input/work")?;
    Ok(code)
}
fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("tig-retire: {error}");
            ExitCode::from(2)
        }
    }
}
