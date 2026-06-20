use serde::{Deserialize, Serialize};

use crate::{
    error::{AppError, AppResult, UpstreamRequestError},
    AppState,
};

const GITHUB_LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/neogate-io/NeoGate/releases/latest";
const GITHUB_RELEASES_URL: &str = "https://github.com/neogate-io/NeoGate/releases";
const GITHUB_USER_AGENT: &str = concat!("NeoGate/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Serialize)]
pub(crate) struct VersionCheckResponse {
    current_version: &'static str,
    latest_version: String,
    latest_tag: String,
    update_available: bool,
    release_url: String,
    published_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubReleaseResponse {
    tag_name: String,
    html_url: String,
    published_at: Option<String>,
}

pub(crate) async fn check_latest_version(state: &AppState) -> AppResult<VersionCheckResponse> {
    let release = state
        .http
        .get(GITHUB_LATEST_RELEASE_API)
        .header("user-agent", GITHUB_USER_AGENT)
        .header("accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|err| {
            AppError::UpstreamRequest(UpstreamRequestError::from_reqwest("github", &err))
        })?;

    if !release.status().is_success() {
        return Err(AppError::UpstreamUnavailable(format!(
            "github latest release request failed with status {}",
            release.status()
        )));
    }

    let release = release
        .json::<GithubReleaseResponse>()
        .await
        .map_err(|err| {
            AppError::UpstreamRequest(UpstreamRequestError::from_reqwest("github", &err))
        })?;
    let latest_version = normalized_version(&release.tag_name);

    Ok(VersionCheckResponse {
        current_version: env!("CARGO_PKG_VERSION"),
        update_available: compare_versions(&latest_version, env!("CARGO_PKG_VERSION")).is_gt(),
        latest_version,
        latest_tag: release.tag_name,
        release_url: if release.html_url.is_empty() {
            GITHUB_RELEASES_URL.to_string()
        } else {
            release.html_url
        },
        published_at: release.published_at,
    })
}

fn normalized_version(version: &str) -> String {
    version.trim().trim_start_matches(['v', 'V']).to_string()
}

fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let left_parts = semver_core(left);
    let right_parts = semver_core(right);

    for index in 0..left_parts.len().max(right_parts.len()) {
        let left_value = left_parts.get(index).copied().unwrap_or_default();
        let right_value = right_parts.get(index).copied().unwrap_or_default();
        match left_value.cmp(&right_value) {
            std::cmp::Ordering::Equal => {}
            ordering => return ordering,
        }
    }

    std::cmp::Ordering::Equal
}

fn semver_core(version: &str) -> Vec<u64> {
    normalized_version(version)
        .split(['.', '-', '+'])
        .take(3)
        .map(|part| part.parse::<u64>().unwrap_or_default())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::compare_versions;

    #[test]
    fn compares_prefixed_release_tags() {
        assert!(compare_versions("v0.2.1", "0.2.0").is_gt());
        assert!(compare_versions("0.2.0", "0.2.0").is_eq());
        assert!(compare_versions("0.1.9", "0.2.0").is_lt());
    }
}
