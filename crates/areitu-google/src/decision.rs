#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Decision {
    NoOp,
    Upload,
    Download,
    UploadWithConflictBackup,
}

pub fn decide(remote_changed: bool, local_changed: bool) -> Decision {
    match (remote_changed, local_changed) {
        (false, false) => Decision::NoOp,
        (false, true) => Decision::Upload,
        (true, false) => Decision::Download,
        (true, true) => Decision::UploadWithConflictBackup,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neither_changed_is_noop() {
        assert_eq!(decide(false, false), Decision::NoOp);
    }

    #[test]
    fn only_local_changed_uploads() {
        assert_eq!(decide(false, true), Decision::Upload);
    }

    #[test]
    fn only_remote_changed_downloads() {
        assert_eq!(decide(true, false), Decision::Download);
    }

    #[test]
    fn both_changed_uploads_with_conflict_backup() {
        assert_eq!(decide(true, true), Decision::UploadWithConflictBackup);
    }

    #[test]
    fn decision_covers_all_four_boolean_combinations_exhaustively() {
        let all: std::collections::HashSet<Decision> = [
            decide(false, false),
            decide(false, true),
            decide(true, false),
            decide(true, true),
        ]
        .into_iter()
        .collect();
        assert_eq!(all.len(), 4, "each of the 4 input combinations must map to a distinct decision");
    }
}
