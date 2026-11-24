//! A profile: in memory, or a folder only one machopo has at a time; and
//! where a person's is by default on each platform.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use browser::{Platform, Profile, ProfileError, user_cache_dir_on, user_data_dir_on};

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
/// is, and only that: not what a longer one said before.
#[test]
fn a_profiles_folder_is_made_and_says_whose_it_is() {
    let dir = scratch("made").join("deeper");
    let profile = Profile::open(&dir).unwrap();
    assert_eq!(profile.dir(), Some(dir.as_path()));
    // Let go to be read: on Windows no other handle reads a locked file.
    drop(profile);
    let lock = dir.join("lock");
    let pid = std::process::id().to_string();
    assert_eq!(std::fs::read_to_string(&lock).unwrap(), pid);

    std::fs::write(&lock, "left by a process that crashed").unwrap();
    drop(Profile::open(&dir).unwrap());
    assert_eq!(std::fs::read_to_string(&lock).unwrap(), pid);
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

/// Each platform's place for what a profile may lose, as Chromium's
/// `GetUserCacheDirectory`: its path under the user's data folder, under
/// the cache folder; the profile's own folder on Windows and for a profile
/// elsewhere.
#[test]
fn a_profiles_cache_is_where_the_platform_keeps_caches() {
    let cache = |platform, profile: &str, vars: &[(&str, &str)]| {
        user_cache_dir_on(platform, Path::new(profile), env(vars))
    };
    let home = [("HOME", "/home/a")];
    assert_eq!(
        cache(
            Platform::MacOs,
            "/home/a/Library/Application Support/machopo",
            &home
        ),
        PathBuf::from("/home/a/Library/Caches/machopo")
    );
    assert_eq!(
        cache(
            Platform::MacOs,
            "/home/a/Library/Application Support/x/y",
            &home
        ),
        PathBuf::from("/home/a/Library/Caches/x/y")
    );
    assert_eq!(
        cache(Platform::Xdg, "/home/a/.config/machopo", &home),
        PathBuf::from("/home/a/.cache/machopo")
    );
    let xdg = [
        ("HOME", "/home/a"),
        ("XDG_CONFIG_HOME", "/cfg"),
        ("XDG_CACHE_HOME", "/caches"),
    ];
    assert_eq!(
        cache(Platform::Xdg, "/cfg/machopo", &xdg),
        PathBuf::from("/caches/machopo")
    );
    // Relative and empty ones are ignored, as for the data folder.
    for relative in ["caches", ""] {
        let vars = [("HOME", "/home/a"), ("XDG_CACHE_HOME", relative)];
        assert_eq!(
            cache(Platform::Xdg, "/home/a/.config/machopo", &vars),
            PathBuf::from("/home/a/.cache/machopo"),
            "{relative:?}"
        );
        let vars = [("HOME", "/home/a"), ("XDG_CONFIG_HOME", relative)];
        assert_eq!(
            cache(Platform::Xdg, "/home/a/.config/machopo", &vars),
            PathBuf::from("/home/a/.cache/machopo"),
            "{relative:?}"
        );
    }
    // The profile's own: on Windows, for one elsewhere, for the data
    // folder itself, and where the environment doesn't say.
    let windows = "C:/Users/a/AppData/Local/machopo/User Data";
    assert_eq!(
        cache(
            Platform::Windows,
            windows,
            &[("LOCALAPPDATA", "C:/Users/a/AppData/Local")]
        ),
        PathBuf::from(windows)
    );
    for (platform, profile) in [
        (Platform::MacOs, "/tmp/profile"),
        (Platform::MacOs, "/home/a/Library/Application Support"),
        (Platform::Xdg, "/tmp/profile"),
        (Platform::Xdg, "/home/a/.config"),
    ] {
        assert_eq!(
            cache(platform, profile, &home),
            PathBuf::from(profile),
            "{platform:?} {profile}"
        );
    }
    assert_eq!(
        cache(
            Platform::MacOs,
            "/home/a/Library/Application Support/m",
            &[]
        ),
        PathBuf::from("/home/a/Library/Application Support/m")
    );
    assert_eq!(
        cache(Platform::Xdg, "/home/a/.config/m", &[]),
        PathBuf::from("/home/a/.config/m")
    );
    // A profile in memory has none; one elsewhere, its own folder.
    assert_eq!(Profile::in_memory().cache_dir(), None);
}
