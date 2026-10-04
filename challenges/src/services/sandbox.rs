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
/// learner error. Resource exhaustion, signals and compiler/host diagnostics
/// are technical failures, including legacy HTTP 400 ENOSPC responses.
pub fn technical_compilation(result: &RunResult) -> bool {
    !valid_limits(result)
        || !(1..124).contains(&result.status)
        || result.resource_usage.time >= result.limits.time.saturating_mul(1000)
        || result.resource_usage.memory >= result.limits.memory.saturating_mul(1024)
        || infrastructure_diagnostic(&result.stderr)
        || infrastructure_diagnostic(&result.stdout)
}

fn infrastructure_diagnostic(diagnostic: &str) -> bool {
    let diagnostic = diagnostic.to_ascii_lowercase();
    let messages = [
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
        "unable to create native thread",
        "resource temporarily unavailable",
        "failed to create thread",
        "error while loading shared libraries",
        "nsjail error",
    ];
    diagnostic.lines().any(|line| {
        let line = line.trim();
        // Compilers echo offending source lines. A string literal in learner
        // code is not a host diagnostic, even when it mentions ENOSPC/OOM.
        if [
            "undeclared",
            "was not declared",
            "cannot find symbol",
            "unresolved reference",
        ]
        .iter()
        .any(|message| line.contains(message))
        {
            return false;
        }
        let reporter = line.starts_with("error ")
            || line.starts_with("nsjail ")
            || line.starts_with("could not reserve enough space")
            || [
                "error:",
                "fatal:",
                "ioexception:",
                "filesystemexception:",
                "oserror:",
                "ioerror:",
                "os error",
                "errno",
                "cc1:",
                "cc1plus:",
            ]
            .iter()
            .any(|prefix| {
                line.find(prefix)
                    .is_some_and(|at| !line[..at].contains(['"', '\'', '=', ';', '{', '}']))
            });
        messages
            .iter()
            .any(|message| line == *message || (reporter && line.contains(message)))
    })
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
