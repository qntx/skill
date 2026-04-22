//! Aggregated per-scope update statistics.

/// Success / failure / discovery tallies for a single scope pass.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ScopeStats {
    pub success: u32,
    pub fail: u32,
    pub found: u32,
}

impl ScopeStats {
    /// Fold `other` into `self`.
    pub(super) const fn merge(&mut self, other: Self) {
        self.success += other.success;
        self.fail += other.fail;
        self.found += other.found;
    }
}

/// Case-insensitive skill-name filter. Empty filter matches all.
pub(super) fn matches_skill_filter(name: &str, filter: &[String]) -> bool {
    if filter.is_empty() {
        return true;
    }
    let lower = name.to_lowercase();
    filter.iter().any(|f| f.to_lowercase() == lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_stats_merge_accumulates() {
        let mut a = ScopeStats {
            success: 1,
            fail: 2,
            found: 3,
        };
        let b = ScopeStats {
            success: 10,
            fail: 20,
            found: 30,
        };
        a.merge(b);
        assert_eq!(a.success, 11);
        assert_eq!(a.fail, 22);
        assert_eq!(a.found, 33);
    }

    #[test]
    fn test_matches_skill_filter_empty_matches_all() {
        assert!(matches_skill_filter("any", &[]));
    }

    #[test]
    fn test_matches_skill_filter_case_insensitive() {
        let filter = vec!["Find-Skills".to_owned()];
        assert!(matches_skill_filter("find-skills", &filter));
        assert!(!matches_skill_filter("other", &filter));
    }
}
