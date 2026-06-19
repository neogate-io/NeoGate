pub fn bounded_limit(limit: Option<i64>, default: i64, max: i64) -> i64 {
    limit.unwrap_or(default).clamp(1, max)
}

pub fn trimmed_non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

pub fn trimmed_non_empty_owned(value: Option<&str>) -> Option<String> {
    trimmed_non_empty(value).map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_optional_limits() {
        assert_eq!(bounded_limit(None, 50, 200), 50);
        assert_eq!(bounded_limit(Some(0), 50, 200), 1);
        assert_eq!(bounded_limit(Some(500), 50, 200), 200);
        assert_eq!(bounded_limit(Some(75), 50, 200), 75);
    }

    #[test]
    fn trims_optional_strings_and_drops_empty_values() {
        assert_eq!(trimmed_non_empty(None), None);
        assert_eq!(trimmed_non_empty(Some("   ")), None);
        assert_eq!(trimmed_non_empty(Some("  active  ")), Some("active"));
        assert_eq!(
            trimmed_non_empty_owned(Some("  project  ")).as_deref(),
            Some("project")
        );
    }
}
