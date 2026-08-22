mod support;

use skillator::acquisition::{LibraryAcquisition, LibraryAcquisitionMode};
use skillator::app::{AppPaths, LibraryWorkflow, WorkflowError};
use skillator::config::{Fingerprint, LibraryConfig, LibraryLocationConfig, save_library};

#[cfg(unix)]
#[test]
fn library_acquisition_move_transfers_the_skill_to_the_local_library() {
    let fixture = AcquisitionFixture::new();

    fixture.acquire(LibraryAcquisitionMode::Move).unwrap();

    assert!(!fixture.source_skill.exists());
    assert!(fixture.destination().is_dir());
    assert!(
        !fixture
            .destination()
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[cfg(unix)]
#[test]
fn library_acquisition_copy_preserves_both_physical_skill_directories() {
    let fixture = AcquisitionFixture::new();

    fixture.acquire(LibraryAcquisitionMode::Copy).unwrap();

    assert!(fixture.source_skill.is_dir());
    assert!(fixture.destination().is_dir());
    assert!(
        !fixture
            .destination()
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[cfg(unix)]
#[test]
fn library_acquisition_link_preserves_the_source_and_links_into_the_local_library() {
    let fixture = AcquisitionFixture::new();

    fixture.acquire(LibraryAcquisitionMode::Link).unwrap();

    assert!(fixture.source_skill.is_dir());
    assert!(
        fixture
            .destination()
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fixture.destination().canonicalize().unwrap(),
        fixture.source_skill.canonicalize().unwrap()
    );
    let session = LibraryWorkflow::load(&fixture.paths).unwrap();
    let snapshot = LibraryWorkflow::snapshot(&fixture.paths, &session.config);
    assert!(
        snapshot
            .source("local/library")
            .unwrap()
            .skill("demo")
            .unwrap()
            .available()
    );
}

#[test]
fn acquisition_collision_preserves_the_source_destination_and_configuration() {
    let fixture = AcquisitionFixture::new();
    std::fs::create_dir_all(fixture.destination()).unwrap();
    std::fs::write(fixture.destination().join("mine"), "preserve").unwrap();
    let before = std::fs::read(fixture.paths.library_config()).unwrap();

    let error = fixture.acquire(LibraryAcquisitionMode::Move).unwrap_err();

    std::assert_matches!(error, WorkflowError::InvalidInput { .. });
    assert!(fixture.source_skill.is_dir());
    assert_eq!(
        std::fs::read_to_string(fixture.destination().join("mine")).unwrap(),
        "preserve"
    );
    assert_eq!(
        std::fs::read(fixture.paths.library_config()).unwrap(),
        before
    );
}

#[test]
fn stale_configuration_rolls_back_a_published_move() {
    let fixture = AcquisitionFixture::new();
    let session = LibraryWorkflow::load(&fixture.paths).unwrap();
    std::fs::write(
        fixture.paths.library_config(),
        "version: 1\nlocations: []\n",
    )
    .unwrap();

    let error = LibraryWorkflow::save_with_acquisitions(
        &fixture.paths,
        &session,
        &fixture.staged,
        &[fixture.request(LibraryAcquisitionMode::Move)],
        true,
    )
    .unwrap_err();

    std::assert_matches!(error, WorkflowError::InvalidInput { .. });
    assert!(fixture.source_skill.is_dir());
    assert!(!fixture.destination().exists());
    assert_eq!(
        std::fs::read_to_string(fixture.paths.library_config()).unwrap(),
        "version: 1\nlocations: []\n"
    );
}

struct AcquisitionFixture {
    _home: support::TestHome,
    paths: AppPaths,
    local_root: std::path::PathBuf,
    source_skill: std::path::PathBuf,
    staged: LibraryConfig,
}

impl AcquisitionFixture {
    fn new() -> Self {
        let home = support::TestHome::new();
        let paths = AppPaths::new(home.path().to_owned());
        let local_root = home.path().join(".skillator/library");
        let external_root = home.path().join("external");
        let source_skill = external_root.join("demo");
        std::fs::create_dir_all(&local_root).unwrap();
        std::fs::create_dir_all(&source_skill).unwrap();
        std::fs::write(
            source_skill.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo Skill\n---\n",
        )
        .unwrap();
        std::fs::write(source_skill.join("asset.txt"), "asset").unwrap();

        let initial = config(&external_root, false);
        save_library(&paths.library_config(), &initial, &Fingerprint::Absent).unwrap();
        let staged = config(&external_root, true);
        Self {
            _home: home,
            paths,
            local_root,
            source_skill,
            staged,
        }
    }

    fn destination(&self) -> std::path::PathBuf {
        self.local_root.join("demo")
    }

    fn request(&self, mode: LibraryAcquisitionMode) -> LibraryAcquisition {
        LibraryAcquisition::new(self.source_skill.clone(), "demo".to_owned(), mode, false)
    }

    fn acquire(&self, mode: LibraryAcquisitionMode) -> Result<(), WorkflowError> {
        let session = LibraryWorkflow::load(&self.paths)?;
        LibraryWorkflow::save_with_acquisitions(
            &self.paths,
            &session,
            &self.staged,
            &[self.request(mode)],
            true,
        )?;
        Ok(())
    }
}

fn config(external_root: &std::path::Path, _acquired: bool) -> LibraryConfig {
    LibraryConfig::new(vec![
        LibraryLocationConfig::new("./library".to_owned(), Vec::new(), false),
        LibraryLocationConfig::new(
            external_root.to_string_lossy().into_owned(),
            Vec::new(),
            false,
        ),
    ])
    .unwrap()
}
