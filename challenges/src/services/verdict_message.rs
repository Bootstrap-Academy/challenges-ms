use std::collections::BTreeMap;

use entity::sea_orm_active_enums::ChallengesVerdict;
use schemas::challenges::coding_challenges::VerdictMessage;

/// Context required to build a human friendly verdict message.
pub struct VerdictMessageContext<'a> {
    pub verdict: ChallengesVerdict,
    pub reason: Option<&'a str>,
    pub compile_status: Option<i32>,
    pub compile_stderr: Option<&'a str>,
    pub run_status: Option<i32>,
    pub run_stderr: Option<&'a str>,
    pub run_time_ms: Option<u64>,
    pub run_memory_kib: Option<u64>,
    pub time_limit_ms: Option<u64>,
    pub memory_limit_mb: Option<u64>,
}

pub fn build_message(ctx: VerdictMessageContext<'_>) -> Option<VerdictMessage> {
    use ChallengesVerdict::*;

    let title_key = title_key(&ctx.verdict);
    let mut params = BTreeMap::new();
    let mut detail_candidates: Vec<&str> = Vec::new();

    if let Some(reason) = ctx.reason {
        detail_candidates.push(reason);
    }

    let body_key: Option<&'static str> = match ctx.verdict {
        Ok => Some("VerdictHint.OK"),
        TimeLimitExceeded => {
            if let Some(actual) = ctx.run_time_ms {
                params.insert("actual_ms".into(), actual.to_string());
                params.insert("actual_seconds".into(), format_secs(actual));
            } else {
                params.insert("actual_ms".into(), "—".into());
                params.insert("actual_seconds".into(), "—".into());
            }
            if let Some(limit) = ctx.time_limit_ms {
                params.insert("limit_ms".into(), limit.to_string());
                params.insert("limit_seconds".into(), format_secs(limit));
            } else {
                params.insert("limit_ms".into(), "—".into());
                params.insert("limit_seconds".into(), "—".into());
            }
            if let Some(stderr) = ctx.run_stderr {
                detail_candidates.push(stderr);
            }
            Some("VerdictHint.TIME_LIMIT_EXCEEDED")
        }
        MemoryLimitExceeded => {
            if let Some(actual) = ctx.run_memory_kib {
                params.insert("actual_kib".into(), actual.to_string());
                params.insert("actual_mib".into(), format_mib(actual));
            } else {
                params.insert("actual_kib".into(), "—".into());
                params.insert("actual_mib".into(), "—".into());
            }
            if let Some(limit) = ctx.memory_limit_mb {
                params.insert("limit_mib".into(), limit.to_string());
            } else {
                params.insert("limit_mib".into(), "—".into());
            }
            if let Some(stderr) = ctx.run_stderr {
                detail_candidates.push(stderr);
            }
            Some("VerdictHint.MEMORY_LIMIT_EXCEEDED")
        }
        CompilationError => {
            if let Some(status) = ctx.compile_status {
                params.insert("exit_code".into(), status.to_string());
            } else {
                params.insert("exit_code".into(), "?".into());
            }
            if let Some(stderr) = ctx.compile_stderr {
                detail_candidates.push(stderr);
            }
            Some("VerdictHint.COMPILATION_ERROR")
        }
        RuntimeError => {
            if let Some(status) = ctx.run_status {
                params.insert("exit_code".into(), status.to_string());
            } else {
                params.insert("exit_code".into(), "?".into());
            }
            if let Some(stderr) = ctx.run_stderr {
                detail_candidates.push(stderr);
            }
            Some("VerdictHint.RUNTIME_ERROR")
        }
        NoOutput => {
            if let Some(stderr) = ctx.run_stderr {
                detail_candidates.push(stderr);
            }
            if !params.contains_key("actual_seconds") {
                params.insert("actual_seconds".into(), "—".into());
            }
            Some("VerdictHint.NO_OUTPUT")
        }
        WrongAnswer => Some("VerdictHint.WRONG_ANSWER"),
        InvalidOutputFormat => {
            if let Some(stderr) = ctx.run_stderr {
                detail_candidates.push(stderr);
            }
            Some("VerdictHint.INVALID_OUTPUT_FORMAT")
        }
        PreCheckFailed => Some("VerdictHint.PRE_CHECK_FAILED"),
    };

    if matches!(
        ctx.verdict,
        WrongAnswer | PreCheckFailed | InvalidOutputFormat
    ) && detail_candidates.is_empty()
    {
        if let Some(stderr) = ctx.run_stderr {
            detail_candidates.push(stderr);
        }
        if let Some(stderr) = ctx.compile_stderr {
            detail_candidates.push(stderr);
        }
    }

    let detail = detail_candidates
        .into_iter()
        .filter_map(sanitize_detail)
        .next();

    Some(VerdictMessage {
        title_key: title_key.into(),
        body_key: body_key.map(|key| key.to_string()),
        body_params: (!params.is_empty()).then_some(params),
        detail,
    })
}

fn title_key(verdict: &ChallengesVerdict) -> &'static str {
    use ChallengesVerdict::*;
    match verdict {
        CompilationError => "Error.Verdict.COMPILATION_ERROR",
        InvalidOutputFormat => "Error.Verdict.INVALID_OUTPUT_FORMAT",
        MemoryLimitExceeded => "Error.Verdict.MEMORY_LIMIT_EXCEEDED",
        NoOutput => "Error.Verdict.NO_OUTPUT",
        Ok => "Error.Verdict.OK",
        PreCheckFailed => "Error.Verdict.PRE_CHECK_FAILED",
        RuntimeError => "Error.Verdict.RUNTIME_ERROR",
        TimeLimitExceeded => "Error.Verdict.TIME_LIMIT_EXCEEDED",
        WrongAnswer => "Error.Verdict.WRONG_ANSWER",
    }
}

fn sanitize_detail(raw: &str) -> Option<String> {
    let cleaned: Vec<_> = raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("/nix/store"))
        .filter(|line| !line.contains("sandkasten"))
        .filter(|line| !line.contains("nix/store"))
        .take(3)
        .map(String::from)
        .collect();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned.join("\n"))
    }
}

fn format_secs(ms: u64) -> String {
    format!("{:.2}", (ms as f64) / 1000.0)
}

fn format_mib(kib: u64) -> String {
    format!("{:.2}", (kib as f64) / 1024.0)
}
