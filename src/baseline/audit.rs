use std::collections::BTreeMap;

use chrono::{Duration, NaiveDate};
use serde::Serialize;

use super::{BaselineFile, parse_iso_date};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct OwnerStats {
    pub total: usize,
    pub expired: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct AuditReport {
    pub total_entries: usize,
    pub by_owner: BTreeMap<String, OwnerStats>,
    pub entries_without_owner: usize,
    pub entries_without_reason: usize,
    pub entries_without_added_at: usize,
    pub expired: usize,
    pub expiring_within_30d: usize,
    pub no_expiry: usize,
    pub active: usize,
    pub has_owner_count: usize,
    pub has_reason_count: usize,
    pub has_added_at_count: usize,
}

impl AuditReport {
    pub fn has_metadata_gaps(&self) -> bool {
        self.entries_without_owner > 0
            || self.entries_without_reason > 0
            || self.entries_without_added_at > 0
    }
}

enum ExpiryState {
    Expired,
    ExpiringSoon,
    NoExpiry,
    Active,
}

pub fn audit_baseline(baseline: &BaselineFile, today: &str) -> AuditReport {
    let today = parse_iso_date(today);

    let mut report = AuditReport {
        total_entries: baseline.entries.len(),
        ..AuditReport::default()
    };

    for entry in &baseline.entries {
        if let Some(owner) = entry
            .owner
            .as_ref()
            .filter(|owner| !owner.trim().is_empty())
        {
            report.has_owner_count += 1;
            report.by_owner.entry(owner.clone()).or_default().total += 1;
        } else {
            report.entries_without_owner += 1;
        }

        if entry
            .reason
            .as_ref()
            .is_some_and(|reason| !reason.trim().is_empty())
        {
            report.has_reason_count += 1;
        } else {
            report.entries_without_reason += 1;
        }

        if entry
            .added_at
            .as_ref()
            .is_some_and(|added_at| !added_at.trim().is_empty())
        {
            report.has_added_at_count += 1;
        } else {
            report.entries_without_added_at += 1;
        }

        match classify_expiry(entry.expires_at.as_deref(), today) {
            ExpiryState::Expired => {
                report.expired += 1;
                if let Some(owner) = entry
                    .owner
                    .as_ref()
                    .filter(|owner| !owner.trim().is_empty())
                {
                    report.by_owner.entry(owner.clone()).or_default().expired += 1;
                }
            }
            ExpiryState::ExpiringSoon => report.expiring_within_30d += 1,
            ExpiryState::NoExpiry => report.no_expiry += 1,
            ExpiryState::Active => report.active += 1,
        }
    }

    report
}

fn classify_expiry(expires_at: Option<&str>, today: Option<NaiveDate>) -> ExpiryState {
    let Some(raw_date) = expires_at else {
        return ExpiryState::NoExpiry;
    };

    let Some(today) = today else {
        return ExpiryState::Active;
    };

    let Some(expires_at) = parse_iso_date(raw_date) else {
        return ExpiryState::NoExpiry;
    };

    if expires_at < today {
        return ExpiryState::Expired;
    }

    if expires_at <= today + Duration::days(30) {
        return ExpiryState::ExpiringSoon;
    }

    ExpiryState::Active
}
