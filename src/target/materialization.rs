//! Classification of existing materializations against their intended source.

use super::*;
type Classification = (Comparison, MaterializationState);

pub(super) fn inspect_materialization(
    path: &Path,
    enablement: &Enablement,
    resolved: Option<&crate::library::LibrarySkill>,
    scope: TargetScope,
) -> Classification {
    let source = resolved.and_then(|skill| skill.absolute_path());
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return missing_materialization(enablement, source);
        }
        Err(_) => return uninspectable(),
    };
    match enablement.materialization() {
        MaterializationKind::Linked => inspect_link(path, &metadata, source, scope),
        MaterializationKind::Copied => inspect_copy(path, &metadata, source),
    }
}

fn uninspectable() -> Classification {
    (
        Comparison::Unverifiable,
        MaterializationState::Uninspectable,
    )
}

fn missing_materialization(enablement: &Enablement, source: Option<&Path>) -> Classification {
    let ineligible = enablement.materialization() == MaterializationKind::Copied
        && source.is_some_and(|source| TreeSnapshot::read(source, true).is_err());
    (
        Comparison::Drifted,
        if ineligible {
            MaterializationState::CopyIneligible
        } else {
            MaterializationState::Missing
        },
    )
}

fn equivalent_copy(path: &Path, source: Option<&Path>) -> bool {
    let Some(source) = source else {
        return false;
    };
    match (
        TreeSnapshot::read(source, true),
        TreeSnapshot::read(path, false),
    ) {
        (Ok(source), Ok(destination)) => source == destination,
        _ => false,
    }
}

fn inspect_link(
    path: &Path,
    metadata: &fs::Metadata,
    source: Option<&Path>,
    scope: TargetScope,
) -> Classification {
    if !metadata.file_type().is_symlink() {
        let equivalent = metadata.is_dir() && equivalent_copy(path, source);
        return (
            Comparison::Drifted,
            if equivalent {
                MaterializationState::EquivalentCopy
            } else {
                MaterializationState::WrongKind
            },
        );
    }
    let Ok(stored) = fs::read_link(path) else {
        return uninspectable();
    };
    let Some(source) = source else {
        return uninspectable();
    };
    if stored == source {
        return (Comparison::InSync, MaterializationState::CanonicalLink);
    }
    inspect_noncanonical_link(path, &stored, source, scope)
}

fn inspect_noncanonical_link(
    path: &Path,
    stored: &Path,
    source: &Path,
    scope: TargetScope,
) -> Classification {
    let resolved = if stored.is_absolute() {
        stored.canonicalize()
    } else {
        path.parent()
            .unwrap_or(Path::new("."))
            .join(stored)
            .canonicalize()
    };
    match resolved {
        Ok(candidate) if candidate == source => (
            if scope == TargetScope::User {
                Comparison::InSync
            } else {
                Comparison::Drifted
            },
            MaterializationState::NoncanonicalLink,
        ),
        Ok(_) => (Comparison::Drifted, MaterializationState::MisdirectedLink),
        Err(_) => (Comparison::Drifted, MaterializationState::BrokenLink),
    }
}

fn points_to_source(path: &Path, source: Option<&Path>) -> bool {
    source.is_some_and(|source| fs::read_link(path).is_ok_and(|target| target == source))
}

fn inspect_copy(path: &Path, metadata: &fs::Metadata, source: Option<&Path>) -> Classification {
    if metadata.file_type().is_symlink() && points_to_source(path, source) {
        return (Comparison::Drifted, MaterializationState::CanonicalLink);
    }
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return (Comparison::Drifted, MaterializationState::WrongKind);
    }
    let Some(source) = source else {
        return uninspectable();
    };
    compare_copy_trees(path, source)
}

fn compare_copy_trees(path: &Path, source: &Path) -> Classification {
    let source_tree = match TreeSnapshot::read(source, true) {
        Ok(tree) => tree,
        Err(_) => return (Comparison::Drifted, MaterializationState::CopyIneligible),
    };
    let destination_tree = match TreeSnapshot::read(path, false) {
        Ok(tree) => tree,
        Err(_) => return uninspectable(),
    };
    if source_tree == destination_tree {
        (Comparison::InSync, MaterializationState::EquivalentCopy)
    } else {
        (Comparison::Drifted, MaterializationState::DivergedCopy)
    }
}
