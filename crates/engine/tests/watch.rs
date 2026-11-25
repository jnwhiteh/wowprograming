//! Reload on change: with `watch`, editing a loaded addon's file reloads
//! the UI with the new code.

use wowprogramming_engine::{Engine, EngineConfig};

#[test]
fn an_edited_addon_file_reloads_the_ui() {
    let addons = tempfile::tempdir().unwrap();
    let wtf = tempfile::tempdir().unwrap();
    let dir = addons.path().join("Hello");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("Hello.toc"), "## Interface: 120105\nHello.lua\n").unwrap();
    std::fs::write(dir.join("Hello.lua"), "HelloVersion = 1").unwrap();
    let mut e = Engine::new(EngineConfig {
        wtf_dir: wtf.path().to_path_buf(),
        addons_dir: Some(addons.path().to_path_buf()),
        blizzard_addons: Vec::new(),
        only_addons: Some(vec!["Hello".into()]),
        watch: true,
        ..EngineConfig::default()
    })
    .unwrap();
    e.start();
    assert_eq!(e.eval::<i64>("return HelloVersion").unwrap(), 1);

    std::fs::write(dir.join("Hello.lua"), "HelloVersion = 2 -- edited").unwrap();
    let mut version = 1;
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        e.tick(0.1);
        version = e.eval::<i64>("return HelloVersion").unwrap();
        if version == 2 {
            break;
        }
    }
    assert_eq!(version, 2, "the UI did not reload after the edit");
}
