//! Every plugin in `examples/plugins/` must validate and pass its own tests.

mod common;

use std::path::PathBuf;

fn example_dirs() -> Vec<PathBuf> {
    let root = common::repo_root().join("examples/plugins");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", root.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.join("plugin.json").is_file())
        .collect();
    dirs.sort();
    dirs
}

#[test]
fn every_example_has_the_expected_files() {
    let dirs = example_dirs();
    assert!(dirs.len() >= 8, "expected at least 8 examples, found {}", dirs.len());
    for dir in &dirs {
        for file in ["plugin.json", "index.ts", "index.test.ts", "README.md"] {
            assert!(dir.join(file).is_file(), "{} is missing {file}", dir.display());
        }
    }
}

#[tokio::test]
async fn every_example_validates_and_passes_its_tests() {
    let (_engine, host) = common::host();
    for dir in example_dirs() {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let problems = host.validate(&dir).await;
        assert!(
            problems.is_empty(),
            "{name} does not validate:\n{}",
            problems.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n")
        );

        let report = host.test(&dir).await.unwrap_or_else(|p| panic!("{name}: {p:#?}"));
        assert_eq!(report.plugin, name, "the folder name should match plugin.json's name");
        assert!(!report.tests.is_empty(), "{name} has no tests");
        assert!(report.passed(), "{name} tests failed:\n{}", common::failures(&report.tests));
    }
}
