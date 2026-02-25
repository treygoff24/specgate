pub fn tool_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn git_sha() -> &'static str {
    option_env!("SPECGATE_GIT_SHA")
        .or(option_env!("VERGEN_GIT_SHA"))
        .unwrap_or("unknown")
}
