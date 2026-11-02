//! Public static guidance; never interpolate private error text or walk sources.
use std::error::Error;
use tig_store::source_claims::Error as JournalError;

pub(super) fn explain(error: &(dyn Error + 'static)) -> (&'static str, &'static str) {
    if let Some(error) = error.downcast_ref::<JournalError>() {
        return match error {
            JournalError::Stale => (
                "source-stale",
                "Inspect current source state and review a new plan; preserve the original request for reconciliation.",
            ),
            JournalError::Owner => (
                "source-owner-mismatch",
                "Check the admitted source owner; do not substitute another owner to bypass refusal.",
            ),
            JournalError::Conflict => (
                "operation-conflict",
                "Reconcile the original operation and exact saved request before proposing another operation.",
            ),
            JournalError::Rollback => (
                "checkpoint-refused",
                "Check the enrolled authority and trusted checkpoint; do not weaken a known minimum.",
            ),
            JournalError::Pending => (
                "source-pending",
                "Inspect the unresolved source transition before proposing a separate change.",
            ),
            JournalError::Missing => (
                "reference-unavailable",
                "Inspect the exact source and reference; absence does not establish successful cleanup.",
            ),
            JournalError::Budget => (
                "journal-budget",
                "Review explicit bounds and retained history; do not discard obligations to make room.",
            ),
            JournalError::InvalidReference => (
                "reference-invalid",
                "Check exact source, reference and request shape against the admitted contract.",
            ),
            JournalError::Busy => (
                "authority-busy",
                "Re-observe the scoped authority before an explicit retry; do not remove locks or alter the request.",
            ),
            JournalError::Corrupt | JournalError::NeedsRecovery => (
                "authority-recovery-required",
                "Preserve the authority and saved requests for explicit recovery; do not recreate or reset it.",
            ),
            JournalError::Io | JournalError::Sql | JournalError::Injected => fallback(),
        };
    }
    // Classify by type only. Provider/OS/JSON messages can contain private data.
    if error.is::<serde_json::Error>() || error.is::<tig_core::identity::Error>() {
        return (
            "profile-invalid",
            "Check the explicit profile or plan against its schema; keep private details out of public logs.",
        );
    }
    fallback()
}

fn fallback() -> (&'static str, &'static str) {
    (
        "operation-refused",
        "State may already have changed; reconcile any saved exact request with acknowledge before a separate apply. No cleanup is authorized.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_journal_categories_have_bounded_static_guidance() {
        for (error, code) in [
            (JournalError::Stale, "source-stale"),
            (JournalError::Owner, "source-owner-mismatch"),
            (JournalError::Conflict, "operation-conflict"),
            (JournalError::Rollback, "checkpoint-refused"),
            (JournalError::Pending, "source-pending"),
            (JournalError::Missing, "reference-unavailable"),
            (JournalError::Budget, "journal-budget"),
            (JournalError::InvalidReference, "reference-invalid"),
            (JournalError::Busy, "authority-busy"),
            (JournalError::Corrupt, "authority-recovery-required"),
            (JournalError::NeedsRecovery, "authority-recovery-required"),
            (JournalError::Io, "operation-refused"),
            (JournalError::Sql, "operation-refused"),
            (JournalError::Injected, "operation-refused"),
        ] {
            let (actual, hint) = explain(&error);
            assert_eq!(actual, code);
            assert!(actual.len() + hint.len() < 256);
            assert!(!hint.chars().any(char::is_control));
        }
    }

    #[test]
    fn private_or_decorated_errors_never_select_guidance_by_text() {
        for message in [
            "Stale",
            "Rollback private-owner C:/private/token=secret",
            "source-stale\u{1b}[31m secret",
            "operation-conflict https://secret@example.invalid/private",
        ] {
            let error = std::io::Error::other(message);
            assert_eq!(explain(&error), fallback());
        }
        let error =
            serde_json::from_str::<serde_json::Value>("{\"private-secret\":bad}").unwrap_err();
        assert_eq!(explain(&error).0, "profile-invalid");
        assert!(!format!("{:?}", explain(&error)).contains("private-secret"));
    }
}
