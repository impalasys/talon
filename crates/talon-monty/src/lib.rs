// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only
//
// The protocol shell follows the MIT-licensed `monty-runtime` subprocess
// entrypoint from the Pydantic Monty project.

//! Hidden stdio entrypoint used by `monty-pool` for crash-isolated execution.
//!
//! `monty-pool` starts the current Talon executable with the `subprocess`
//! argument. This module must therefore be runnable before any host runtime,
//! configuration, telemetry, or secret initialization happens.

use std::{env, ffi::OsStr, io, panic};

use monty_proto::{
    pb,
    worker::{fatal_error_event, protocol_violation, Child, EventSink, HandleOutcome},
    FrameError, FrameReader,
};

/// Runs the hidden Monty worker mode when requested by `monty-pool`.
///
/// Returns `None` for normal host startup, or the worker's process status when
/// the first positional argument is exactly `subprocess`.
pub fn run_if_requested() -> Option<i32> {
    if !is_subprocess_args(env::args_os()) {
        return None;
    }
    Some(run())
}

fn is_subprocess_args(mut args: impl Iterator<Item = std::ffi::OsString>) -> bool {
    let _program = args.next();
    args.next().as_deref() == Some(OsStr::new("subprocess"))
}

fn run() -> i32 {
    install_panic_hook();
    let mut reader = FrameReader::new(io::stdin().lock());
    let mut child = Child::default();
    let mut sink = StdoutSink;

    loop {
        match reader.read::<pb::ParentRequest>() {
            Ok(Some(request)) => match child.handle(request, &mut sink) {
                Ok(HandleOutcome::Continue) => {}
                Ok(HandleOutcome::Shutdown) => return 0,
                Ok(HandleOutcome::Fatal) => return 4,
                Err(FrameError::FrameTooLarge { len, max }) => {
                    fatal(
                        &child,
                        &mut sink,
                        &format!("response frame of {len} bytes exceeds maximum of {max} bytes"),
                    );
                    return 2;
                }
                Err(_) => return 3,
            },
            Ok(None) => return 0,
            Err(FrameError::Decode(err)) => {
                if sink
                    .send(&protocol_violation(&format!("malformed request: {err}")))
                    .is_err()
                {
                    return 3;
                }
            }
            Err(err) => {
                fatal(
                    &child,
                    &mut sink,
                    &format!("malformed request frame: {err}"),
                );
                return 2;
            }
        }
    }
}

struct StdoutSink;

impl EventSink for StdoutSink {
    fn send(&mut self, event: &pb::ChildEvent) -> Result<(), FrameError> {
        monty_proto::write_frame(&mut io::stdout(), event)
    }
}

fn fatal(child: &Child, sink: &mut impl EventSink, message: &str) {
    eprintln!("monty subprocess fatal error: {message}");
    let _ = sink.send(&child.fatal_event(message));
}

fn install_panic_hook() {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = monty_proto::write_frame(
            &mut io::stdout(),
            &fatal_error_event(&format!("child panicked: {info}")),
        );
        default_hook(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subprocess_mode_requires_exact_first_argument() {
        assert!(is_subprocess_args(
            ["talon-node", "subprocess"].into_iter().map(Into::into)
        ));
        assert!(!is_subprocess_args(
            ["talon-node", "server", "subprocess"]
                .into_iter()
                .map(Into::into)
        ));
        assert!(!is_subprocess_args(
            ["talon-node"].into_iter().map(Into::into)
        ));
    }
}
