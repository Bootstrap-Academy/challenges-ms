//! Preserve transport status before trusting execution results. The upstream
//! client still handles metadata; execution failures must never become grades.
use std::ops::Deref;

use reqwest::StatusCode;
use sandkasten_client::{
    schemas::{
        programs::{BuildRunError, BuildRunRequest, BuildRunResult, RunResult},
        ErrorResponse,
    },
    SandkastenClient,
};
use thiserror::Error;

#[derive(Clone)]
pub struct SandboxClient {
    metadata: SandkastenClient,
    url: reqwest::Url,
    http: reqwest::Client,
}

impl SandboxClient {
    pub fn new(url: reqwest::Url) -> Self {
        Self {
            metadata: SandkastenClient::new(url.clone()),
            url,
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("valid sandbox HTTP client configuration"),
        }
    }

    pub async fn build_and_run(&self, request: &BuildRunRequest) -> Result<BuildRunResult, Error> {
        let response = self
            .http
            .post(self.url.join("run").map_err(|_| Error::InvalidUrl)?)
            .json(request)
            .send()
            .await?;
        let status = response.status();
        // A proxy/service error may contain an otherwise valid compilation
        // payload. Its HTTP status takes precedence; do not interpret its body.
        match status {
            StatusCode::OK => {
                let output: BuildRunResult = response.json().await?;
                if !valid_limits(&output.run)
                    || output
                        .build
                        .as_ref()
                        .is_some_and(|build| !valid_limits(build) || build.status != 0)
                {
                    return Err(Error::InvalidExecution);
                }
                Ok(output)
            }
            StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND => {
                let error: Box<ErrorResponse<BuildRunError>> = response.json().await?;
                // Only the endpoint's documented status/body pairs may reach
                // the judge's learner-compilation branch.
                match (status, error.as_ref()) {
                    (
                        StatusCode::BAD_REQUEST,
                        ErrorResponse::Inner(BuildRunError::CompileError(result)),
                    ) => {
                        if technical_compilation(result) {
                            Err(Error::CompilationUnavailable)
                        } else {
                            Err(Error::Rejected(error))
                        }
                    }
                    (
                        StatusCode::NOT_FOUND,
                        ErrorResponse::Inner(BuildRunError::EnvironmentNotFound),
                    )
                    | (
                        StatusCode::BAD_REQUEST,
                        ErrorResponse::Inner(
                            BuildRunError::InvalidFileNames
                            | BuildRunError::InvalidEnvVars
                            | BuildRunError::CompileLimitsExceeded(_)
                            | BuildRunError::RunLimitsExceeded(_),
                        ),
                    ) => Err(Error::Rejected(error)),
                    _ => Err(Error::InvalidExecution),
                }
            }
            _ => Err(Error::HttpStatus(status)),
        }
    }
}

impl Deref for SandboxClient {
    type Target = SandkastenClient;

    fn deref(&self) -> &Self::Target {
        &self.metadata
    }
}

fn valid_limits(result: &RunResult) -> bool {
    let limits = &result.limits;
    limits.cpus > 0
        && limits.time > 0
        && limits.memory > 0
        && limits.filesize > 0
        && limits.file_descriptors > 0
        && limits.processes > 0
}

/// A regular compiler exit with valid resource metadata is necessary for a
/// learner error. No learner code runs while compiling, so a killed, missing
/// or crashed compiler, exhausted compiler limits and host/toolchain
/// diagnostics are technical failures, including HTTP 400 ENOSPC responses.
pub fn technical_compilation(result: &RunResult) -> bool {
    !valid_limits(result)
        || !(1..124).contains(&result.status)
        || result.resource_usage.time >= result.limits.time.saturating_mul(1000)
        || result.resource_usage.memory >= result.limits.memory.saturating_mul(1024)
        || infrastructure_diagnostic(&result.stderr)
        || infrastructure_diagnostic(&result.stdout)
}

/// nsjail could not start the learner program (status 255 plus its own fatal
/// log line, e.g. `[F][2026-10-04T03:49:06+0200][1] runChild():506 Launching
/// child process failed`). The learner code never ran.
pub fn launcher_failure(result: &RunResult) -> bool {
    result.status == 255 && result.stderr.lines().any(nsjail_fatal_line)
}

fn nsjail_fatal_line(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("[F][") else {
        return false;
    };
    let Some((timestamp, rest)) = rest.split_once("][") else {
        return false;
    };
    let Some((pid, _)) = rest.split_once(']') else {
        return false;
    };
    let date = timestamp.as_bytes();
    date.len() >= 19
        && date[..4].iter().all(u8::is_ascii_digit)
        && date[4] == b'-'
        && date[10] == b'T'
        && !pid.is_empty()
        && pid.bytes().all(|byte| byte.is_ascii_digit())
}

/// Only filesystems, hosts and toolchains print these. No reporter prefix is
/// required: storage exhaustion (ENOSPC, EDQUOT, EROFS) and the other messages
/// count anywhere on a line outside learner text (see below).
const INFRASTRUCTURE_MESSAGES: &[&str] = &[
    "no space left on device",
    "enospc",
    "errno 28",
    "os error 28",
    "disk quota exceeded",
    "read-only file system",
    "cannot allocate memory",
    "out of memory",
    "outofmemoryerror",
    "could not reserve enough space",
    "internal compiler error",
    "the compiler unexpectedly panicked",
    "unable to create native thread",
    "resource temporarily unavailable",
    "failed to create thread",
    "error while loading shared libraries",
    "nsjail error",
];

/// Learners control some diagnostic text: `#error`, `static_assert`,
/// `compile_error!`, import paths and string literals. Compilers print that
/// text quoted, in source excerpts or in a diagnostic located in the learner's
/// file. A message counts only outside all of these.
fn infrastructure_diagnostic(diagnostic: &str) -> bool {
    let lines: Vec<String> = diagnostic.lines().map(str::to_ascii_lowercase).collect();
    lines.iter().enumerate().any(|(index, line)| {
        let next = lines.get(index + 1).map_or("", String::as_str);
        !source_excerpt(line, next)
            && INFRASTRUCTURE_MESSAGES.iter().any(|message| {
                line.match_indices(message).any(|(start, _)| {
                    let end = start + message.len();
                    standalone(line, start, end)
                        && !quoted(line, start, end)
                        && !learner_located(line, start, next)
                })
            })
    })
}

fn standalone(line: &str, start: usize, end: usize) -> bool {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    !line[..start].chars().next_back().is_some_and(word)
        && !line[end..].chars().next().is_some_and(word)
}

/// Inside a cited string literal, e.g. a GHC note `namely ‘"…"’`. Tools quote
/// system errors with single quotes (GNU as: `…: 'No space left on device'`);
/// single quotes therefore only exclude text at a learner location.
fn quoted(line: &str, start: usize, end: usize) -> bool {
    let quote = |c: char| matches!(c, '"' | '“' | '”');
    line[..start].contains(quote) && line[end..].contains(quote)
}

/// Gutter excerpts (`  1 | code`, GCC/Clang/Rust/GHC), Rust sub-diagnostics
/// (`= note:`) and raw echoes above a caret marker (javac, kotlinc).
fn source_excerpt(line: &str, next: &str) -> bool {
    let trimmed = line.trim_start();
    let gutter = trimmed.trim_start_matches(|c: char| c.is_ascii_digit());
    gutter.trim_start().starts_with('|') || trimmed.starts_with("= ") || marker(next)
}

fn marker(line: &str) -> bool {
    let line = line.trim();
    line.contains(['^', '~']) && line.chars().all(|c| "^~-+| ".contains(c))
}

/// A diagnostic about the learner's own source file (`code.c:1:2:`,
/// `/box/code.go:2:8:`, `/tmp/code.cs(1,8):`, or a Rust header above
/// `--> code.rs:1:1`) carries learner text unless the compiler fixes the
/// wording: a GCC internal compiler error or javac's failed class-file write.
fn learner_located(line: &str, start: usize, next: &str) -> bool {
    let Some((extension, rest)) = learner_location(line) else {
        return next
            .trim_start()
            .strip_prefix("-->")
            .is_some_and(|location| learner_location(location).is_some());
    };
    let rest = rest.trim_start();
    if rest.starts_with("internal compiler error:") {
        return false;
    }
    // javac: "error while writing <class>: <I/O error>"; class names have no spaces.
    let javac_write = extension == "java"
        && rest
            .strip_prefix("error: error while writing ")
            .and_then(|tail| tail.split_once(": "))
            .is_some_and(|(class, io_error)| {
                !class.is_empty()
                    && !class.contains(char::is_whitespace)
                    && start >= line.len() - io_error.len()
            });
    !javac_write
}

/// Extension and remainder after a location in the learner's file `code.<ext>`:
/// `path/code.ext:LINE[:COLUMN]:` or `path/code.ext(LINE,COLUMN):`. Every
/// Sandkasten environment names the main file `code.<ext>`
/// (`default_main_file_name`), and the judge never overrides it.
fn learner_location(line: &str) -> Option<(&str, &str)> {
    let line = line.trim_start();
    let token_end = line.find(char::is_whitespace).unwrap_or(line.len());
    let file_end = line[..token_end].find(['(', ':'])?;
    let name = line[..file_end].rsplit('/').next()?;
    let extension = name.strip_prefix("code.")?;
    if extension.is_empty() || !extension.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
    let rest = &line[file_end..];
    if let Some(position) = rest.strip_prefix('(') {
        let (inner, rest) = position.split_once(')')?;
        let valid = !inner.is_empty() && inner.bytes().all(|b| b.is_ascii_digit() || b == b',');
        return valid.then(|| (extension, rest.trim_start_matches(':')));
    }
    let rest = rest.strip_prefix(':')?;
    let line_digits = digits(rest);
    if line_digits == 0 {
        return None;
    }
    let mut rest = &rest[line_digits..];
    if let Some(column) = rest.strip_prefix(':') {
        rest = &column[digits(column)..];
    }
    Some((extension, rest.trim_start_matches(':')))
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("sandbox URL cannot be joined to the execution endpoint")]
    InvalidUrl,
    #[error("sandbox transport or response decoding failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("sandbox returned HTTP {0}")]
    HttpStatus(StatusCode),
    #[error("sandbox returned inconsistent execution metadata")]
    InvalidExecution,
    #[error("compilation could not finish because the sandbox or compiler is unavailable")]
    CompilationUnavailable,
    #[error("sandbox rejected execution: {0:?}")]
    Rejected(Box<ErrorResponse<BuildRunError>>),
}

#[cfg(test)]
pub(crate) mod tests;
