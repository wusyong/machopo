//! A profile: in memory, or a folder only one machopo has at a time; and
//! where a person's is by default on each platform.

use std::ffi::OsString;
use std::path::PathBuf;

use browser::{Platform, Profile, ProfileError, user_data_dir_on};

/// A new, empty folder for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("browser-profile")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn a_profile_in_memory_has_no_folder() {
    assert_eq!(Profile::in_memory().dir(), None);
    assert_eq!(Profile::default().dir(), None);
}

/// Its folder is made if it isn't there, and its lock file says whose it
/// is.
#[test]
fn a_profiles_folder_is_made_and_says_whose_it_is() {
    let dir = scratch("made").join("deeper");
    let profile = Profile::open(&dir).unwrap();
    assert_eq!(profile.dir(), Some(dir.as_path()));
    let said = std::fs::read_to_string(dir.join("lock")).unwrap();
    assert_eq!(said, std::process::id().to_string());
}

/// While one has it (any of its clones), another can't: it is in use, the
/// error naming the folder (and the process, where the lock file can be
/// read). Once the last clone has gone, it can.
#[test]
fn a_profiles_folder_is_one_machopos_at_a_time() {
    let dir = scratch("one-at-a-time");
    let first = Profile::open(&dir).unwrap();
    let clone = first.clone();
    drop(first);
    match Profile::open(&dir) {
        Err(ProfileError::InUse { dir: in_use, pid }) => {
            assert_eq!(in_use, dir);
            if !cfg!(windows) {
                assert_eq!(pid, Some(std::process::id()));
            }
            let message = ProfileError::InUse { dir: in_use, pid }.to_string();
            assert!(message.contains(&dir.display().to_string()), "{message}");
            assert!(message.contains("--user-data-dir"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    drop(clone);
    let again = Profile::open(&dir).unwrap();
    assert_eq!(again.dir(), Some(dir.as_path()));
}

/// A folder that can't be made is an error naming it.
#[test]
fn a_folder_that_cant_be_made_is_an_error() {
    let file = scratch("a-file");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "not a folder").unwrap();
    match Profile::open(file.join("profile")) {
        Err(error @ ProfileError::Io { .. }) => {
            assert!(error.to_string().contains("a-file"), "{error}");
        }
        other => panic!("{other:?}"),
    }
}

/// The environment, as `user_data_dir_on` reads it.
fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let vars: Vec<(String, String)> = vars
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    move |name| {
        vars.iter()
            .find(|(n, _)| n == name)
            .map(|(_, value)| value.into())
    }
}

/// Each platform's place for a user's data, as Chromium's
/// `GetDefaultUserDataDirectory`: none where the environment doesn't say.
#[test]
fn a_persons_profile_is_where_the_platform_keeps_a_users_data() {
    let home = [("HOME", "/home/a")];
    assert_eq!(
        user_data_dir_on(Platform::MacOs, env(&home)),
        Some(PathBuf::from("/home/a/Library/Application Support/machopo"))
    );
    assert_eq!(
        user_data_dir_on(Platform::Xdg, env(&home)),
        Some(PathBuf::from("/home/a/.config/machopo"))
    );
    assert_eq!(
        user_data_dir_on(
            Platform::Xdg,
            env(&[("HOME", "/home/a"), ("XDG_CONFIG_HOME", "/cfg")])
        ),
        Some(PathBuf::from("/cfg/machopo"))
    );
    // A relative one is ignored, as the XDG specification says; an empty
    // one too.
    for xdg in ["cfg", ""] {
        assert_eq!(
            user_data_dir_on(
                Platform::Xdg,
                env(&[("HOME", "/home/a"), ("XDG_CONFIG_HOME", xdg)])
            ),
            Some(PathBuf::from("/home/a/.config/machopo")),
            "{xdg:?}"
        );
    }
    assert_eq!(
        user_data_dir_on(
            Platform::Windows,
            env(&[("LOCALAPPDATA", "C:/Users/a/AppData/Local")])
        ),
        Some(
            PathBuf::from("C:/Users/a/AppData/Local")
                .join("machopo")
                .join("User Data")
        )
    );
    for platform in [Platform::MacOs, Platform::Xdg, Platform::Windows] {
        assert_eq!(user_data_dir_on(platform, env(&[])), None, "{platform:?}");
        assert_eq!(
            user_data_dir_on(platform, env(&[("HOME", ""), ("LOCALAPPDATA", "")])),
            None
        );
    }
    assert_eq!(user_data_dir_on(Platform::Windows, env(&home)), None);
}
