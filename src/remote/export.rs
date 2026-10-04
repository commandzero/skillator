//! Disposable projection of the leader's discovered skills for ordinary rsync.

use super::{EXPORT_PREFIX, Error, MARKER_CONTENT, MARKER_NAME, Result};
use crate::app::AppPaths;
use crate::config::{LoadResult, load_library};
use crate::domain::SkillPath;
use crate::library::{SkillValidity, scan_library};
use crate::materialization::validate_internal_symlink;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::fs::{self, File, FileTimes};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tempfile::{Builder, TempDir};

struct SkillExport<'a> {
    source: &'a Path,
    destination: PathBuf,
    location_relative: PathBuf,
    location_index: usize,
}

pub(super) fn prepare(paths: &AppPaths) -> Result<TempDir> {
    let config_path = paths.library_config();
    let loaded = load_library(&config_path)
        .map_err(|error| Error::input(format!("{}: {error}", config_path.display())))?;
    let config = match &loaded {
        LoadResult::Missing => {
            return Err(Error::input(
                "leader Library Configuration is missing; configure the library before syncing",
            ));
        }
        LoadResult::Valid(loaded) => loaded.value(),
        LoadResult::Unsupported { version, .. } => {
            return Err(Error::input(format!(
                "{}: unsupported library configuration version {version}",
                config_path.display()
            )));
        }
        LoadResult::Invalid { issues } => {
            return Err(Error::input(format!(
                "{}: invalid library configuration: {}",
                config_path.display(),
                issues
                    .iter()
                    .map(|issue| format!("{}: {}", issue.path, issue.message))
                    .collect::<Vec<_>>()
                    .join("; ")
            )));
        }
    };
    let snapshot = scan_library(config, &config_path, paths.home(), paths.environment());
    if let Some(diagnostic) = snapshot
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.code != "overlapping_locations_allowed")
    {
        return Err(Error::input(format!(
            "leader library discovery incomplete ({}): {}{}",
            diagnostic.code,
            diagnostic.message,
            diagnostic
                .path
                .as_ref()
                .map(|path| format!(" ({})", path.display()))
                .unwrap_or_default()
        )));
    }
    for location in snapshot.locations() {
        if !location.available() {
            return Err(Error::input(format!(
                "leader library location `{}` is unavailable; restore it before syncing",
                location.expression()
            )));
        }
    }
    let mut exclusions = Vec::with_capacity(config.locations().len());
    for (location, configured) in snapshot.locations().iter().zip(config.locations()) {
        let root = location.resolved().ok_or_else(|| {
            Error::input(format!(
                "leader library location `{}` has no available source folder",
                location.expression()
            ))
        })?;
        let mut builder = GitignoreBuilder::new(root);
        for pattern in configured.exclusions() {
            builder
                .add_line(None, pattern)
                .map_err(|error| Error::input(format!("invalid exclusion `{pattern}`: {error}")))?;
        }
        exclusions.push(builder.build().map_err(Error::input_display)?);
    }

    // Check the entire discovered inventory before publishing even a temporary export.
    // Path::starts_with compares components, not textual prefixes (a/b != a/bb).
    let mut skills = Vec::<SkillExport<'_>>::new();
    for source in snapshot.sources() {
        if !source.available() || source.key_collision() || source.root().is_none() {
            return Err(Error::input(format!(
                "leader library source `{}` is unavailable or has a source-key collision; resolve its configured locations before syncing",
                source.key()
            )));
        }
        for skill in source.skills() {
            if !skill.available() || skill.validity() != SkillValidity::Valid {
                return Err(Error::input(format!(
                    "invalid leader skill `{}/{}`: {}",
                    source.key(),
                    skill.path(),
                    skill.diagnostics().join("; ")
                )));
            }
            let relative = SkillPath::parse(skill.path()).map_err(|error| {
                Error::input(format!(
                    "unsupported leader skill path `{}`: {error}",
                    skill.path()
                ))
            })?;
            let root = skill.absolute_path().ok_or_else(|| {
                Error::input(format!(
                    "cannot inspect leader skill `{}/{}`",
                    source.key(),
                    skill.path()
                ))
            })?;
            let mut destination = PathBuf::from(source.key().as_str()).join("_skills");
            if relative.as_str() != "." {
                destination.push(relative.as_str());
            }
            if skills.iter().any(|existing| {
                destination.starts_with(&existing.destination)
                    || existing.destination.starts_with(&destination)
            }) {
                return Err(Error::input(format!(
                    "overlapping leader skill exports at `{}`; remove nested or duplicate skill roots",
                    destination.display()
                )));
            }
            let mut location_relative = PathBuf::new();
            if source.relative_path() != "." {
                location_relative.push(source.relative_path());
            }
            if relative.as_str() != "." {
                location_relative.push(relative.as_str());
            }
            skills.push(SkillExport {
                source: root,
                destination,
                location_relative,
                location_index: source.location_index(),
            });
        }
    }

    let export = Builder::new()
        .prefix(EXPORT_PREFIX)
        .tempdir()
        .map_err(|error| Error::input(format!("cannot create temporary leader export: {error}")))?;
    let result = (|| {
        fs::set_permissions(export.path(), fs::Permissions::from_mode(0o700))
            .map_err(|error| io_error(export.path(), error))?;
        for skill in skills {
            let destination = export.path().join(skill.destination);
            fs::create_dir_all(destination.parent().expect("skill export has a parent"))
                .map_err(|error| io_error(&destination, error))?;
            copy_skill(
                skill.source,
                &destination,
                &skill.location_relative,
                &exclusions[skill.location_index],
            )?;
        }
        fs::write(export.path().join(MARKER_NAME), MARKER_CONTENT)
            .map_err(|error| io_error(&export.path().join(MARKER_NAME), error))?;
        Ok(())
    })();
    if let Err(error) = result {
        let path = export.keep();
        return match cleanup(&path) {
            Ok(()) => Err(error),
            Err(cleanup_error) => Err(Error::input(format!(
                "{error}; could not remove temporary export `{}`: {cleanup_error}",
                path.display()
            ))),
        };
    }
    Ok(export)
}

/// Remove a private export without modifying the permissions of hard-linked source files.
pub(super) fn cleanup(path: &Path) -> io::Result<()> {
    fn grant_directory_access(path: &Path) -> io::Result<()> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.file_type().is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "export directory is not a physical directory: {}",
                    path.display()
                ),
            ));
        }
        let mode = metadata.permissions().mode();
        if mode & 0o700 != 0o700 {
            fs::set_permissions(path, fs::Permissions::from_mode(mode | 0o700))?;
        }
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                grant_directory_access(&entry.path())?;
            }
        }
        Ok(())
    }

    grant_directory_access(path)?;
    fs::remove_dir_all(path)
}

fn io_error(path: &Path, error: std::io::Error) -> Error {
    Error::input(format!("cannot export `{}`: {error}", path.display()))
}

fn copy_skill(
    source: &Path,
    destination: &Path,
    location_relative: &Path,
    exclusions: &Gitignore,
) -> Result<()> {
    let canonical_root = source
        .canonicalize()
        .map_err(|error| io_error(source, error))?;
    let source_metadata = fs::metadata(source).map_err(|error| io_error(source, error))?;
    if !source_metadata.is_dir() {
        return Err(Error::input(format!(
            "cannot export `{}`: skill root is not a directory",
            source.display()
        )));
    }
    fs::create_dir(destination).map_err(|error| io_error(destination, error))?;
    copy_children(
        source,
        source,
        &canonical_root,
        destination,
        location_relative,
        exclusions,
    )?;
    // A link into an omitted .git tree (or a link which ultimately reaches one)
    // is valid in the original tree but would dangle in this projection.
    let canonical_destination = destination
        .canonicalize()
        .map_err(|error| io_error(destination, error))?;
    validate_export_links(&canonical_destination, &canonical_destination)?;
    fs::set_permissions(destination, source_metadata.permissions())
        .map_err(|error| io_error(destination, error))?;
    Ok(())
}

fn copy_children(
    root: &Path,
    directory: &Path,
    canonical_root: &Path,
    destination: &Path,
    location_relative: &Path,
    exclusions: &Gitignore,
) -> Result<()> {
    let entries = fs::read_dir(directory).map_err(|error| io_error(directory, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| io_error(directory, error))?;
        if entry.file_name() == ".git" {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| io_error(&path, error))?;
        let kind = metadata.file_type();
        let child_relative = location_relative.join(entry.file_name());
        if exclusions
            .matched_path_or_any_parents(&child_relative, kind.is_dir())
            .is_ignore()
        {
            continue;
        }
        let target = destination.join(entry.file_name());
        if kind.is_dir() {
            fs::create_dir(&target).map_err(|error| io_error(&target, error))?;
            copy_children(
                root,
                &path,
                canonical_root,
                &target,
                &child_relative,
                exclusions,
            )?;
            fs::set_permissions(&target, metadata.permissions())
                .map_err(|error| io_error(&target, error))?;
        } else if kind.is_symlink() {
            let link_target = fs::read_link(&path).map_err(|error| io_error(&path, error))?;
            validate_internal_symlink(root, canonical_root, &path, &link_target)
                .map_err(Error::input_display)?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(link_target, &target)
                .map_err(|error| io_error(&target, error))?;
            crate::fs_safety::preserve_symlink_times(&target, &metadata)
                .map_err(|error| io_error(&target, error))?;
            #[cfg(not(unix))]
            return Err(Error::input(format!(
                "cannot export `{}`: symbolic links are unsupported on this platform",
                path.display()
            )));
        } else if kind.is_file() {
            // Opening the original also detects unreadable content even when the filesystem
            // permits creating a hard link. No second in-memory content snapshot is needed.
            let mut input = File::open(&path).map_err(|error| io_error(&path, error))?;
            match fs::hard_link(&path, &target) {
                Ok(()) => {}
                Err(_) => {
                    // Copy data only: platform clone APIs can inherit immutable flags.
                    let mut output =
                        File::create_new(&target).map_err(|error| io_error(&target, error))?;
                    std::io::copy(&mut input, &mut output)
                        .map_err(|error| io_error(&path, error))?;
                    let mut times = FileTimes::new();
                    if let Ok(accessed) = metadata.accessed() {
                        times = times.set_accessed(accessed);
                    }
                    if let Ok(modified) = metadata.modified() {
                        times = times.set_modified(modified);
                    }
                    output
                        .set_times(times)
                        .and_then(|()| output.set_permissions(metadata.permissions()))
                        .map_err(|error| io_error(&target, error))?;
                }
            }
        } else {
            return Err(Error::input(format!(
                "cannot export `{}`: only regular files, directories, and self-contained symbolic links are supported",
                path.display()
            )));
        }
    }
    Ok(())
}

fn validate_export_links(root: &Path, directory: &Path) -> Result<()> {
    for entry in fs::read_dir(directory).map_err(|error| io_error(directory, error))? {
        let entry = entry.map_err(|error| io_error(directory, error))?;
        let path = entry.path();
        let kind = fs::symlink_metadata(&path)
            .map_err(|error| io_error(&path, error))?
            .file_type();
        if kind.is_symlink() {
            let target = fs::read_link(&path).map_err(|error| io_error(&path, error))?;
            validate_internal_symlink(root, root, &path, &target).map_err(Error::input_display)?;
        } else if kind.is_dir() {
            validate_export_links(root, &path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LibraryConfig, LibraryConfigCodec, LibraryLocationConfig};
    use tempfile::tempdir;

    fn fixture(locations: &[&str]) -> (TempDir, AppPaths) {
        let home = tempdir().unwrap();
        let paths = AppPaths::new(home.path().to_owned());
        fs::create_dir_all(home.path().join(".skillator")).unwrap();
        let config = LibraryConfig::new(
            locations
                .iter()
                .map(|path| LibraryLocationConfig::new((*path).into(), vec![], false))
                .collect(),
        )
        .unwrap();
        fs::write(
            paths.library_config(),
            LibraryConfigCodec::render(&config).unwrap(),
        )
        .unwrap();
        (home, paths)
    }

    fn skill(path: &Path, name: &str) {
        fs::create_dir_all(path).unwrap();
        fs::write(
            path.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Sample skill\n---\nBody\n"),
        )
        .unwrap();
    }

    fn invalid(paths: &AppPaths) {
        let error = prepare(paths).unwrap_err();
        assert_eq!(error.code, 3);
    }

    #[test]
    fn exports_complete_skills_by_source_without_unrelated_files() {
        let (home, paths) = fixture(&["~/first", "~/second"]);
        for name in ["first", "second"] {
            let location = home.path().join(name);
            skill(&location.join("demo"), "demo");
            fs::write(location.join("demo").join("data.txt"), name).unwrap();
            fs::write(location.join("unrelated.txt"), b"not a skill").unwrap();
        }
        let export = prepare(&paths).unwrap();
        assert_eq!(
            fs::read(export.path().join(MARKER_NAME)).unwrap(),
            MARKER_CONTENT.as_bytes()
        );
        for name in ["first", "second"] {
            let skill_root = export.path().join(format!("local/{name}/_skills/demo"));
            assert_eq!(
                fs::read(skill_root.join("data.txt")).unwrap(),
                name.as_bytes()
            );
            assert!(skill_root.join("SKILL.md").is_file());
            assert!(
                !export
                    .path()
                    .join(format!("local/{name}/unrelated.txt"))
                    .exists()
            );
        }
    }

    #[test]
    fn private_acquisition_root_and_nested_directory_modes_survive_export() {
        use std::os::unix::fs::symlink;

        let (home, paths) = fixture(&["~/library"]);
        let origin = home.path().join("origin/demo");
        skill(&origin, "demo");
        let nested = origin.join("private");
        fs::create_dir(&nested).unwrap();
        let data = nested.join("data.txt");
        fs::write(&data, b"private content").unwrap();
        symlink("data.txt", nested.join("alias.txt")).unwrap();
        fs::set_permissions(&data, fs::Permissions::from_mode(0o640)).unwrap();
        fs::set_permissions(&nested, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(&origin, fs::Permissions::from_mode(0o700)).unwrap();
        let library = home.path().join("library");
        fs::create_dir(&library).unwrap();
        symlink(&origin, library.join("demo")).unwrap();

        let original_root = fs::metadata(&origin).unwrap().permissions().mode();
        let original_nested = fs::metadata(&nested).unwrap().permissions().mode();
        let original_data = fs::metadata(&data).unwrap().permissions().mode();
        let export = prepare(&paths).unwrap();
        let projected = export.path().join("local/library/_skills/demo");
        assert_eq!(
            fs::read(projected.join("private/alias.txt")).unwrap(),
            b"private content"
        );
        assert_eq!(
            fs::metadata(&projected).unwrap().permissions().mode(),
            original_root
        );
        assert_eq!(
            fs::metadata(projected.join("private"))
                .unwrap()
                .permissions()
                .mode(),
            original_nested
        );
        assert_eq!(
            fs::metadata(&origin).unwrap().permissions().mode(),
            original_root
        );
        assert_eq!(
            fs::metadata(&nested).unwrap().permissions().mode(),
            original_nested
        );
        assert_eq!(
            fs::metadata(&data).unwrap().permissions().mode(),
            original_data
        );
    }

    #[test]
    fn cleanup_removes_readonly_export_directories_without_changing_source_files() {
        use std::os::unix::fs::symlink;

        let (home, paths) = fixture(&["~/library"]);
        let source = home.path().join("library/demo");
        skill(&source, "demo");
        let nested = source.join("nested");
        fs::create_dir(&nested).unwrap();
        let data = nested.join("data.txt");
        fs::write(&data, b"source content").unwrap();
        fs::set_permissions(&data, fs::Permissions::from_mode(0o444)).unwrap();
        fs::set_permissions(&nested, fs::Permissions::from_mode(0o555)).unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o555)).unwrap();

        let export = prepare(&paths).unwrap();
        let projected = export.path().join("local/library/_skills/demo");
        assert_eq!(
            fs::metadata(&projected).unwrap().permissions().mode() & 0o777,
            0o555
        );
        assert_eq!(
            fs::metadata(projected.join("nested"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o555
        );
        // Cleanup must not follow links out of the export or chmod hard-linked files.
        symlink(&source, export.path().join("source-link")).unwrap();
        let original_file = fs::metadata(&data).unwrap();
        let path = export.keep();
        cleanup(&path).unwrap();
        assert!(!path.exists());
        let remaining_file = fs::metadata(&data).unwrap();
        assert_eq!(
            remaining_file.permissions().mode(),
            original_file.permissions().mode()
        );
        assert_eq!(
            remaining_file.modified().unwrap(),
            original_file.modified().unwrap()
        );
        assert_eq!(
            remaining_file.accessed().unwrap(),
            original_file.accessed().unwrap()
        );
        assert_eq!(fs::read(&data).unwrap(), b"source content");
        assert_eq!(
            fs::metadata(&source).unwrap().permissions().mode() & 0o777,
            0o555
        );
        assert_eq!(
            fs::metadata(&nested).unwrap().permissions().mode() & 0o777,
            0o555
        );
        fs::set_permissions(&source, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&nested, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn partial_export_with_readonly_child_is_removed_on_validation_failure() {
        use std::os::unix::fs::symlink;

        let (home, paths) = fixture(&["~/library"]);
        let source = home.path().join("library/demo");
        skill(&source, "demo");
        let nested = source.join("nested");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join("data.txt"), b"source content").unwrap();
        fs::set_permissions(&nested, fs::Permissions::from_mode(0o555)).unwrap();
        fs::write(source.join("omitted.txt"), b"excluded").unwrap();
        symlink("omitted.txt", source.join("link")).unwrap();
        let config = LibraryConfig::new(vec![LibraryLocationConfig::new(
            "~/library".into(),
            vec!["demo/omitted.txt".into()],
            false,
        )])
        .unwrap();
        fs::write(
            paths.library_config(),
            LibraryConfigCodec::render(&config).unwrap(),
        )
        .unwrap();

        let error = prepare(&paths).unwrap_err();
        assert_eq!(error.code, 3);
        let message = error.to_string();
        let projected_link = message
            .strip_prefix("cannot copy link `")
            .and_then(|text| text.split_once('`'))
            .map(|(path, _)| Path::new(path))
            .expect("final link validation must reject the excluded target");
        let export = projected_link
            .ancestors()
            .find(|ancestor| {
                ancestor
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(EXPORT_PREFIX))
            })
            .unwrap();
        assert!(
            !export.exists(),
            "partial export was retained: {}",
            export.display()
        );
        assert_eq!(
            fs::read(nested.join("data.txt")).unwrap(),
            b"source content"
        );
        assert_eq!(
            fs::metadata(&nested).unwrap().permissions().mode() & 0o777,
            0o555
        );
        fs::set_permissions(&nested, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn copies_readable_files_when_same_filesystem_hard_links_are_forbidden() {
        struct ClearImmutable(PathBuf);
        impl Drop for ClearImmutable {
            fn drop(&mut self) {
                let _ = std::process::Command::new("chflags")
                    .arg("nouchg")
                    .arg(&self.0)
                    .status();
            }
        }

        let (home, paths) = fixture(&["~/library"]);
        let source = home.path().join("library/demo");
        skill(&source, "demo");
        let data = source.join("data.txt");
        fs::write(&data, b"readable immutable file").unwrap();
        fs::set_permissions(&data, fs::Permissions::from_mode(0o640)).unwrap();
        let original = fs::metadata(&data).unwrap();
        assert!(
            std::process::Command::new("chflags")
                .arg("uchg")
                .arg(&data)
                .status()
                .unwrap()
                .success()
        );
        let _clear_immutable = ClearImmutable(data.clone());
        let link_error =
            fs::hard_link(&data, home.path().join("same-filesystem-link")).unwrap_err();
        assert_eq!(link_error.raw_os_error(), Some(libc::EPERM));

        let export = prepare(&paths).unwrap();
        let copied = export.path().join("local/library/_skills/demo/data.txt");
        assert_eq!(fs::read(&copied).unwrap(), b"readable immutable file");
        assert_eq!(
            fs::metadata(&copied).unwrap().permissions().mode(),
            original.permissions().mode()
        );
        assert_eq!(
            fs::metadata(&copied).unwrap().modified().unwrap(),
            original.modified().unwrap()
        );
        assert_eq!(
            fs::metadata(&data).unwrap().permissions().mode(),
            original.permissions().mode()
        );
        assert_eq!(
            fs::metadata(&data).unwrap().modified().unwrap(),
            original.modified().unwrap()
        );
    }

    #[test]
    fn git_root_skill_exports_content_without_git_administrative_state() {
        let (home, paths) = fixture(&["~/repo"]);
        let root = home.path().join("repo");
        skill(&root, "repo");
        for arguments in [
            vec!["init", "--quiet"],
            vec![
                "remote",
                "add",
                "origin",
                "https://example.test/owner/repo.git",
            ],
        ] {
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(&root)
                    .args(arguments)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        fs::write(root.join("data.txt"), b"working-tree content").unwrap();
        let export = prepare(&paths).unwrap();
        let projected = export.path().join("owner/repo/_skills");
        assert_eq!(
            fs::read(projected.join("data.txt")).unwrap(),
            b"working-tree content"
        );
        assert!(projected.join("SKILL.md").is_file());
        assert!(!projected.join(".git").exists());
        assert!(root.join(".git/config").is_file());
    }

    #[test]
    fn location_exclusions_apply_inside_complete_skill_content() {
        let (home, paths) = fixture(&["~/library"]);
        let source = home.path().join("library/demo");
        skill(&source, "demo");
        fs::create_dir_all(source.join("ignored")).unwrap();
        fs::write(source.join("ignored/secret"), b"not exported").unwrap();
        fs::write(source.join("secret.env"), b"not exported").unwrap();
        std::os::unix::fs::symlink("kept.txt", source.join("secret-link")).unwrap();
        fs::write(source.join("kept.txt"), b"exported").unwrap();
        let config = LibraryConfig::new(vec![LibraryLocationConfig::new(
            "~/library".into(),
            vec![
                "demo/ignored".into(),
                "demo/secret.env".into(),
                "demo/secret-link".into(),
            ],
            false,
        )])
        .unwrap();
        fs::write(
            paths.library_config(),
            LibraryConfigCodec::render(&config).unwrap(),
        )
        .unwrap();
        let export = prepare(&paths).unwrap();
        let projected = export.path().join("local/library/_skills/demo");
        assert!(!projected.join("ignored").exists());
        assert!(!projected.join("secret.env").exists());
        assert!(!projected.join("secret-link").exists());
        assert_eq!(fs::read(projected.join("kept.txt")).unwrap(), b"exported");
        assert!(source.join("ignored/secret").is_file());
    }

    #[test]
    fn root_and_nested_skill_exports_are_ambiguous() {
        let (home, paths) = fixture(&["~/library"]);
        let root = home.path().join("library");
        skill(&root, "library");
        skill(&root.join("nested"), "nested");
        invalid(&paths);
        fs::remove_dir_all(root.join("nested")).unwrap();
        let export = prepare(&paths).unwrap();
        assert!(
            export
                .path()
                .join("local/library/_skills/SKILL.md")
                .is_file()
        );
    }

    #[test]
    fn invalid_and_unavailable_input_never_becomes_an_empty_export() {
        let (home, paths) = fixture(&["~/missing"]);
        invalid(&paths);
        let root = home.path().join("missing");
        skill(&root.join("demo"), "incorrect-name");
        invalid(&paths);
        fs::remove_dir_all(root.join("demo")).unwrap();
        let export = prepare(&paths).unwrap();
        assert!(!export.path().join("local").exists());
        fs::write(paths.library_config(), "version: 99\nlocations: []\n").unwrap();
        invalid(&paths);
        fs::remove_file(paths.library_config()).unwrap();
        invalid(&paths);
    }

    #[test]
    fn same_source_keys_from_distinct_locations_fail() {
        let (home, paths) = fixture(&["~/one/library", "~/two/library"]);
        skill(&home.path().join("one/library/alpha"), "alpha");
        skill(&home.path().join("two/library/beta"), "beta");
        invalid(&paths);
    }

    #[cfg(unix)]
    #[test]
    fn unsupported_skill_entries_prevent_delivery() {
        use std::os::unix::net::UnixListener;
        let (home, paths) = fixture(&["~/library"]);
        let root = home.path().join("library/demo");
        skill(&root, "demo");
        let _socket = UnixListener::bind(root.join("socket")).unwrap();
        invalid(&paths);
    }

    #[cfg(unix)]
    #[test]
    fn acquisition_root_and_internal_links_keep_data_and_source_metadata() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let (home, paths) = fixture(&["~/library"]);
        let origin = home.path().join("origin/demo");
        skill(&origin, "demo");
        let script = origin.join("run.sh");
        fs::write(&script, b"#!/bin/sh\necho unchanged\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o751)).unwrap();
        symlink("run.sh", origin.join("alias.sh")).unwrap();
        assert!(
            std::process::Command::new("touch")
                .args(["-h", "-t", "200001010000.00"])
                .arg(origin.join("alias.sh"))
                .status()
                .unwrap()
                .success()
        );
        let original_link = fs::symlink_metadata(origin.join("alias.sh")).unwrap();
        let library = home.path().join("library");
        fs::create_dir_all(&library).unwrap();
        symlink(&origin, library.join("demo")).unwrap();
        let original = fs::metadata(&script).unwrap();
        let export = prepare(&paths).unwrap();
        assert_eq!(
            fs::metadata(export.path()).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let copied = export.path().join("local/library/_skills/demo");
        assert!(copied.is_dir());
        assert_eq!(
            fs::read(copied.join("alias.sh")).unwrap(),
            fs::read(&script).unwrap()
        );
        assert_eq!(
            fs::read_link(copied.join("alias.sh")).unwrap(),
            Path::new("run.sh")
        );
        let projected = fs::metadata(copied.join("run.sh")).unwrap();
        assert_eq!(
            projected.permissions().mode(),
            original.permissions().mode()
        );
        assert_eq!(projected.modified().unwrap(), original.modified().unwrap());
        assert_eq!(
            fs::symlink_metadata(copied.join("alias.sh"))
                .unwrap()
                .modified()
                .unwrap(),
            original_link.modified().unwrap()
        );
        assert!(
            fs::symlink_metadata(library.join("demo"))
                .unwrap()
                .is_symlink()
        );
        assert_eq!(
            fs::metadata(&script).unwrap().permissions().mode(),
            original.permissions().mode()
        );
    }

    #[cfg(unix)]
    #[test]
    fn external_and_omitted_internal_links_are_rejected() {
        use std::os::unix::fs::symlink;
        let (home, paths) = fixture(&["~/library"]);
        let root = home.path().join("library/demo");
        skill(&root, "demo");
        fs::write(home.path().join("secret"), b"outside").unwrap();
        symlink(home.path().join("secret"), root.join("outside")).unwrap();
        invalid(&paths);
        fs::remove_file(root.join("outside")).unwrap();
        symlink("../../secret", root.join("outside")).unwrap();
        invalid(&paths);
        fs::remove_file(root.join("outside")).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".git/secret"), b"omitted").unwrap();
        symlink(".git/secret", root.join("omitted")).unwrap();
        invalid(&paths);
    }
}
