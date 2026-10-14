//! Every `pt plugin new` template must validate and pass its own tests.

mod common;

use post_tester_plugin_host::manifest::PLUGIN_TYPES;
use post_tester_plugin_host::scaffold;

#[tokio::test]
async fn every_template_validates_and_passes_its_tests() {
    let (_engine, host) = common::host();
    for (kind, _) in PLUGIN_TYPES {
        let dir = tempfile::tempdir().unwrap();
        let plugin_dir = dir.path().join(format!("my-{kind}"));
        let files = scaffold::create(&plugin_dir, kind, &format!("my-{kind}")).unwrap();
        assert_eq!(files, ["plugin.json", "index.ts", "index.test.ts", "README.md"]);

        let problems = host.validate(&plugin_dir).await;
        assert!(problems.is_empty(), "template `{kind}` does not validate: {problems:#?}");

        let report = host.test(&plugin_dir).await.unwrap_or_else(|p| panic!("template `{kind}`: {p:#?}"));
        assert!(!report.tests.is_empty(), "template `{kind}` has no tests");
        assert!(report.passed(), "template `{kind}` tests failed:\n{}", common::failures(&report.tests));
    }
}

#[test]
fn rejects_unknown_types_bad_names_and_non_empty_folders() {
    let dir = tempfile::tempdir().unwrap();
    let err = scaffold::create(&dir.path().join("a"), "widget", "a").unwrap_err();
    assert!(err.contains("template-function"), "{err}");

    let err = scaffold::create(&dir.path().join("b"), "hook", "Bad Name").unwrap_err();
    assert!(err.contains("not a valid plugin name"), "{err}");

    std::fs::write(dir.path().join("taken.txt"), "x").unwrap();
    let err = scaffold::create(dir.path(), "hook", "c").unwrap_err();
    assert!(err.contains("not empty"), "{err}");
}
