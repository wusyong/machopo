//! How much a disk has free, as the platform says.

use ipc::platform::available_disk_space;

#[test]
fn a_disks_free_space_is_told_in_bytes() {
    let dir = std::env::temp_dir();
    let available = available_disk_space(&dir).unwrap();
    assert!(available > 0);
    // `df` says the same, in KiB, give or take what changed meanwhile.
    #[cfg(unix)]
    {
        let df = std::process::Command::new("df")
            .arg("-kP")
            .arg(&dir)
            .output()
            .unwrap();
        let df = String::from_utf8(df.stdout).unwrap();
        let line = df.lines().nth(1).unwrap();
        let kib: u64 = line.split_whitespace().nth(3).unwrap().parse().unwrap();
        let told = kib * 1024;
        let off = available.abs_diff(told);
        assert!(
            off < told / 100 + 64 * 1024 * 1024,
            "{available} against df's {told}"
        );
    }
}

#[test]
fn a_path_that_isnt_there_has_none() {
    let nowhere = std::env::temp_dir().join("machopo-nowhere/not/there");
    assert!(available_disk_space(&nowhere).is_err());
}
