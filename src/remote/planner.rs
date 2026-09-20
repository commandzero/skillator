use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, Default, clap::ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConflictPolicy {
    Local,
    Remote,
    #[default]
    Ask,
}

#[derive(Debug, Clone, Copy, Default, clap::ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MissingPolicy {
    #[default]
    Copy,
    Remove,
    Ignore,
}

/// None is observed absence. A missing base is unknown history, not a deletion.
#[derive(Debug, Clone)]
pub(super) struct Observation<T> {
    pub value: Option<T>,
    pub base: Option<Option<T>>,
    pub prior_presence: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Decision<T> {
    Use(Option<T>),
    Conflict,
    Ignore,
    Unmatched,
}

pub(super) fn decide<T: Clone + Ord>(
    observations: &[Observation<T>],
    conflict: ConflictPolicy,
    missing: MissingPolicy,
    selection: bool,
) -> Decision<T> {
    decide_group(observations, conflict, missing, selection, 1)
}

pub(super) fn decide_group<T: Clone + Ord>(
    observations: &[Observation<T>],
    conflict: ConflictPolicy,
    missing: MissingPolicy,
    selection: bool,
    local_count: usize,
) -> Decision<T> {
    let Some(local) = observations.first() else {
        return Decision::Ignore;
    };
    let values: BTreeSet<_> = observations.iter().map(|o| o.value.clone()).collect();
    if values.len() == 1 {
        return Decision::Use(local.value.clone());
    }

    let changes: BTreeSet<_> = observations
        .iter()
        .filter(|o| match &o.base {
            Some(base) => base != &o.value,
            None => o.value.is_some(),
        })
        .map(|o| o.value.clone())
        .collect();
    let deletion = observations
        .iter()
        .any(|o| o.value.is_none() && matches!(&o.base, Some(Some(_))));
    let edited = observations
        .iter()
        .any(|o| o.value.is_some() && o.base.as_ref().is_none_or(|base| base != &o.value));
    let present_values: BTreeSet<_> = values.iter().filter_map(Clone::clone).collect();

    let contested = changes.len() > 1
        || (deletion && edited)
        || (changes.is_empty() && present_values.len() > 1);
    if contested {
        return resolve(observations, conflict, local_count);
    }
    if selection {
        return Decision::Use(
            changes
                .into_iter()
                .next()
                .unwrap_or_else(|| present_values.into_iter().next()),
        );
    }
    if values.contains(&None) {
        return match missing {
            MissingPolicy::Ignore => Decision::Ignore,
            MissingPolicy::Copy => Decision::Use(
                changes
                    .iter()
                    .find_map(Clone::clone)
                    .or_else(|| present_values.into_iter().next()),
            ),
            MissingPolicy::Remove => {
                if deletion && observations.iter().all(|o| o.prior_presence) {
                    Decision::Use(None)
                } else {
                    Decision::Unmatched
                }
            }
        };
    }
    Decision::Use(
        changes
            .into_iter()
            .next()
            .unwrap_or_else(|| local.value.clone()),
    )
}

pub(super) fn resolve<T: Clone + Ord>(
    observations: &[Observation<T>],
    policy: ConflictPolicy,
    local_count: usize,
) -> Decision<T> {
    match policy {
        ConflictPolicy::Local => {
            let candidates: BTreeSet<_> = observations
                .iter()
                .take(local_count)
                .map(|o| o.value.clone())
                .collect();
            if candidates.len() == 1 {
                Decision::Use(observations[0].value.clone())
            } else {
                Decision::Conflict
            }
        }
        ConflictPolicy::Ask => Decision::Conflict,
        ConflictPolicy::Remote => {
            let candidates: BTreeSet<_> = observations
                .iter()
                .skip(local_count)
                .filter(|o| o.base.as_ref().is_none_or(|base| base != &o.value))
                .map(|o| o.value.clone())
                .collect();
            if candidates.len() == 1 {
                Decision::Use(candidates.into_iter().next().unwrap())
            } else {
                Decision::Conflict
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn seen(value: Option<&str>, base: Option<Option<&str>>) -> Observation<String> {
        Observation {
            prior_presence: matches!(base, Some(Some(_))),
            value: value.map(str::to_owned),
            base: base.map(|v| v.map(str::to_owned)),
        }
    }
    fn run(values: &[Observation<String>], missing: MissingPolicy) -> Decision<String> {
        decide(values, ConflictPolicy::Ask, missing, false)
    }

    #[test]
    fn one_remote_edit_converges_independently_of_host_order() {
        let a = seen(Some("a"), Some(Some("a")));
        let b = seen(Some("b"), Some(Some("a")));
        assert_eq!(
            run(&[a.clone(), b.clone(), a.clone()], MissingPolicy::Copy),
            Decision::Use(Some("b".into()))
        );
        assert_eq!(
            run(&[a.clone(), a, b], MissingPolicy::Copy),
            Decision::Use(Some("b".into()))
        );
    }

    #[test]
    fn deletions_need_history_and_edits_take_conflict_precedence() {
        let present = seen(Some("a"), Some(Some("a")));
        let deleted = seen(None, Some(Some("a")));
        assert_eq!(
            run(&[present.clone(), deleted.clone()], MissingPolicy::Copy),
            Decision::Use(Some("a".into()))
        );
        assert_eq!(
            run(&[present.clone(), deleted.clone()], MissingPolicy::Remove),
            Decision::Use(None)
        );
        assert_eq!(
            run(&[present.clone(), seen(None, None)], MissingPolicy::Remove),
            Decision::Unmatched
        );
        for policy in [
            MissingPolicy::Copy,
            MissingPolicy::Ignore,
            MissingPolicy::Remove,
        ] {
            assert_eq!(
                run(&[seen(Some("b"), Some(Some("a"))), deleted.clone()], policy),
                Decision::Conflict
            );
        }
        assert_eq!(
            decide(
                &[present, deleted],
                ConflictPolicy::Ask,
                MissingPolicy::Copy,
                true
            ),
            Decision::Use(None)
        );
    }

    #[test]
    fn competing_remote_edits_have_no_implicit_winner() {
        let values = [
            seen(Some("a"), Some(Some("a"))),
            seen(Some("b"), Some(Some("a"))),
            seen(Some("c"), Some(Some("a"))),
        ];
        assert_eq!(
            decide(&values, ConflictPolicy::Remote, MissingPolicy::Copy, false),
            Decision::Conflict
        );
        assert_eq!(
            decide(&values, ConflictPolicy::Local, MissingPolicy::Copy, false),
            Decision::Use(Some("a".into()))
        );
        assert_eq!(
            run(
                &[seen(Some("a"), None), seen(Some("b"), None)],
                MissingPolicy::Copy
            ),
            Decision::Conflict
        );
        assert_eq!(
            run(
                &[seen(Some("a"), None), seen(None, None)],
                MissingPolicy::Copy
            ),
            Decision::Use(Some("a".into()))
        );
    }
    #[test]
    fn a_git_content_base_does_not_authorize_first_contact_removal() {
        let mut local = seen(None, Some(Some("tracked")));
        let mut remote = seen(Some("tracked"), Some(Some("tracked")));
        local.prior_presence = false;
        remote.prior_presence = false;
        assert_eq!(
            run(&[local, remote], MissingPolicy::Remove),
            Decision::Unmatched
        );
    }
}
