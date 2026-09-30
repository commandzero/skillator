use std::fs;
use std::path::Path;

pub(super) fn write_skill(home: &Path, relative: &str, content: &str) {
    let path = home.join(relative);
    fs::create_dir_all(&path).unwrap();
    fs::write(
        path.join("SKILL.md"),
        format!("---\nname: demo\ndescription: A demonstration skill\n---\n{content}\n"),
    )
    .unwrap();
}

pub(super) fn configure_library(home: &Path, location: &str) {
    fs::create_dir_all(home.join(".skillator")).unwrap();
    fs::write(
        home.join(".skillator/library.yaml"),
        format!("version: 1\nlocations:\n  - path: '~/{location}'\n"),
    )
    .unwrap();
}
