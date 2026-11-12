//! Bounded JSON profile library, platform data paths and recoverable atomic replacement.
use super::{
    invalid, validate_profile_id, PrintSettingProfile, PrinterProfile, ProfileError, ResinMaterial,
    ResinProfile, MAX_PROFILE_BYTES,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// Typed profile envelope. Unknown kinds and semantic fields are rejected.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "profile",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ProfileDocument {
    /// Printer hardware profile.
    Printer(PrinterProfile),
    /// Version-one resin with printer-bound calibrations.
    Resin(ResinProfile),
    /// Library resin as a material (TD-106).
    Material(ResinMaterial),
    /// Library print setting for one printer and one resin (TD-106).
    PrintSetting(PrintSettingProfile),
}
impl ProfileDocument {
    /// Validate all typed semantic fields.
    /// # Errors
    /// Returns the first invalid field or calibration.
    pub fn validate(&self) -> Result<(), ProfileError> {
        match self {
            Self::Printer(p) => p.validate(),
            Self::Resin(r) => r.validate(),
            Self::Material(m) => m.validate(),
            Self::PrintSetting(s) => s.validate(),
        }
    }
    /// Profile identity.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Printer(p) => &p.id,
            Self::Resin(p) => &p.id,
            Self::Material(p) => &p.id,
            Self::PrintSetting(p) => &p.id,
        }
    }
    /// Positive persisted revision.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        match self {
            Self::Printer(p) => p.revision,
            Self::Resin(p) => p.revision,
            Self::Material(p) => p.revision,
            Self::PrintSetting(p) => p.revision,
        }
    }
    /// Profile type.
    #[must_use]
    pub const fn kind(&self) -> ProfileKind {
        match self {
            Self::Printer(_) => ProfileKind::Printer,
            Self::Resin(_) => ProfileKind::Resin,
            Self::Material(_) => ProfileKind::Material,
            Self::PrintSetting(_) => ProfileKind::PrintSetting,
        }
    }
    /// Serialize validated, deterministic UTF-8 JSON with a final newline.
    /// # Errors
    /// Rejects invalid profiles and documents exceeding the resource bound.
    pub fn to_json(&self) -> Result<Vec<u8>, ProfileError> {
        self.validate()?;
        let mut bytes = serde_json::to_vec_pretty(self)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_PROFILE_BYTES {
            return Err(invalid("serialized profile exceeds 1 MiB"));
        }
        Ok(bytes)
    }
    /// Decode a bounded typed profile without silently ignoring future fields.
    /// # Errors
    /// Rejects excessive bytes/nesting, invalid JSON/schema or physical fields.
    pub fn from_json(bytes: &[u8]) -> Result<Self, ProfileError> {
        if bytes.len() > MAX_PROFILE_BYTES {
            return Err(invalid("profile exceeds 1 MiB"));
        }
        // Bound structural nesting without decoding strings or allocating an untyped JSON tree.
        let mut depth = 0_u32;
        let mut quoted = false;
        let mut escaped = false;
        for &byte in bytes {
            if quoted {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    quoted = false;
                }
            } else {
                match byte {
                    b'"' => quoted = true,
                    b'{' | b'[' => {
                        depth += 1;
                        if depth > 32 {
                            return Err(invalid("profile nesting exceeds 32 levels"));
                        }
                    }
                    b'}' | b']' => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
        }
        let profile: Self = serde_json::from_slice(bytes)?;
        profile.validate()?;
        Ok(profile)
    }
}
/// Supported stored profile types, ordered for deterministic catalogs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProfileKind {
    /// Printer hardware.
    Printer,
    /// Version-one resin calibration.
    Resin,
    /// Library resin as a material.
    Material,
    /// Library print setting.
    PrintSetting,
}
impl ProfileKind {
    /// Every kind, in listing order.
    pub const ALL: [Self; 4] = [
        Self::Printer,
        Self::Resin,
        Self::Material,
        Self::PrintSetting,
    ];
    const fn directory(self) -> &'static str {
        match self {
            Self::Printer => "printers",
            Self::Resin => "resins",
            Self::Material => "materials",
            Self::PrintSetting => "print-settings",
        }
    }
}
/// Explicit collision and optimistic revision policy for saving.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SavePolicy {
    /// Create a profile preserving its positive revision; never overwrite an existing file.
    CreateOnly,
    /// Replace exactly this persisted revision with its successor.
    Replace {
        /// Revision read by the caller before editing.
        expected_revision: u64,
    },
}
/// Injectable host platform for path discovery tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfilePlatform {
    /// XDG Linux layout.
    Linux,
    /// Windows roaming profiles and local cache.
    Windows,
    /// macOS application support and cache directories.
    MacOs,
}
/// Separate user-owned profile data, preferences and disposable cache roots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfilePaths {
    /// Application data root; the profile stores append `profiles/v1` and `profiles/v2`.
    pub data: PathBuf,
    /// Application preferences root, separate from profile files.
    pub config: PathBuf,
    /// Disposable application cache root.
    pub cache: PathBuf,
}
impl ProfilePaths {
    /// Resolve platform directories using supplied environment values, without changing the process.
    /// # Errors
    /// Rejects unavailable required roots; relative XDG overrides are ignored per the XDG standard.
    pub fn from_environment(
        platform: ProfilePlatform,
        lookup: impl Fn(&str) -> Option<PathBuf>,
    ) -> Result<Self, ProfileError> {
        let absolute = |key: &str| lookup(key).filter(|p| p.is_absolute());
        let root = |key: &str| {
            absolute(key)
                .ok_or_else(|| invalid(format!("cannot resolve absolute {key} user directory")))
        };
        let app = |p: PathBuf| p.join("karasu-slicer");
        match platform {
            ProfilePlatform::Linux => {
                let resolve = |key: &str, suffix: &str| {
                    absolute(key).map_or_else(|| root("HOME").map(|h| h.join(suffix)), Ok)
                };
                Ok(Self {
                    data: app(resolve("XDG_DATA_HOME", ".local/share")?),
                    config: app(resolve("XDG_CONFIG_HOME", ".config")?),
                    cache: app(resolve("XDG_CACHE_HOME", ".cache")?),
                })
            }
            ProfilePlatform::Windows => Ok(Self {
                data: app(root("APPDATA")?),
                config: app(root("APPDATA")?).join("config"),
                cache: app(root("LOCALAPPDATA")?),
            }),
            ProfilePlatform::MacOs => {
                let home = root("HOME")?;
                Ok(Self {
                    data: app(home.join("Library/Application Support")),
                    config: app(home.join("Library/Preferences")),
                    cache: app(home.join("Library/Caches")),
                })
            }
        }
    }
    /// Discover this process's platform directories without creating them.
    /// # Errors
    /// Returns missing user roots or an unsupported host platform.
    pub fn discover() -> Result<Self, ProfileError> {
        let platform = if cfg!(target_os = "windows") {
            ProfilePlatform::Windows
        } else if cfg!(target_os = "macos") {
            ProfilePlatform::MacOs
        } else if cfg!(target_os = "linux") {
            ProfilePlatform::Linux
        } else {
            return Err(invalid(
                "profile path discovery is unsupported on this platform",
            ));
        };
        Self::from_environment(platform, |key| std::env::var_os(key).map(PathBuf::from))
    }
}
/// Writable user library. Built-in application profiles must be kept in a separate read-only layer.
#[derive(Clone, Debug)]
pub struct ProfileStore {
    root: PathBuf,
}
fn io(operation: &'static str, source: std::io::Error) -> ProfileError {
    ProfileError::Io { operation, source }
}
fn read_bytes(path: &Path) -> Result<Vec<u8>, ProfileError> {
    let file = File::open(path).map_err(|e| io("open profile", e))?;
    let mut bytes = Vec::new();
    file.take(u64::try_from(MAX_PROFILE_BYTES).expect("profile bound fits u64") + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io("read profile", e))?;
    if bytes.len() > MAX_PROFILE_BYTES {
        return Err(invalid("stored profile exceeds 1 MiB"));
    }
    Ok(bytes)
}
impl ProfileStore {
    /// Use an explicit absolute application data root without creating directories.
    /// # Errors
    /// Rejects a relative directory to avoid accidental working-directory storage.
    pub fn new(data_root: impl Into<PathBuf>) -> Result<Self, ProfileError> {
        Self::at(data_root, "profiles/v1")
    }
    /// The library of TD-106 (`profiles/v2`): printers, materials and print settings. The
    /// version-one store beside it is left as it was.
    /// # Errors
    /// Rejects a relative directory to avoid accidental working-directory storage.
    pub fn library(data_root: impl Into<PathBuf>) -> Result<Self, ProfileError> {
        Self::at(data_root, "profiles/v2")
    }
    /// A store whose library directory is exactly `directory`, as a migration stages one
    /// before publishing it.
    /// # Errors
    /// Rejects a relative directory to avoid accidental working-directory storage.
    pub fn in_directory(directory: impl Into<PathBuf>) -> Result<Self, ProfileError> {
        let root = directory.into();
        if !root.is_absolute() {
            return Err(invalid("profile library directory must be absolute"));
        }
        Ok(Self { root })
    }
    fn at(data_root: impl Into<PathBuf>, version: &str) -> Result<Self, ProfileError> {
        let root = data_root.into();
        if !root.is_absolute() {
            return Err(invalid("profile data root must be absolute"));
        }
        Ok(Self {
            root: root.join(version),
        })
    }
    /// Versioned profile library directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }
    fn path(&self, kind: ProfileKind, id: &str) -> Result<PathBuf, ProfileError> {
        validate_profile_id(id)?;
        Ok(self.root.join(kind.directory()).join(format!("{id}.json")))
    }
    /// Load and validate a profile, including its filename identity and type.
    /// # Errors
    /// Returns I/O, malformed profile or mismatched identity errors.
    pub fn load(&self, kind: ProfileKind, id: &str) -> Result<ProfileDocument, ProfileError> {
        let profile = ProfileDocument::from_json(&read_bytes(&self.path(kind, id)?)?)?;
        if profile.id() != id || profile.kind() != kind {
            return Err(invalid(
                "stored profile identity or kind disagrees with its path",
            ));
        }
        Ok(profile)
    }
    /// Load validated printer hardware.
    /// # Errors
    /// Returns the underlying load or validation failure.
    pub fn load_printer(&self, id: &str) -> Result<PrinterProfile, ProfileError> {
        match self.load(ProfileKind::Printer, id)? {
            ProfileDocument::Printer(p) => Ok(p),
            _ => Err(invalid("expected printer profile")),
        }
    }
    /// Load validated resin calibration.
    /// # Errors
    /// Returns the underlying load or validation failure.
    pub fn load_resin(&self, id: &str) -> Result<ResinProfile, ProfileError> {
        match self.load(ProfileKind::Resin, id)? {
            ProfileDocument::Resin(p) => Ok(p),
            _ => Err(invalid("expected resin profile")),
        }
    }
    /// List every validated profile sorted by type and identity. Missing libraries are empty.
    /// # Errors
    /// Reports unreadable directories and malformed files rather than hiding broken profiles.
    pub fn list(&self) -> Result<Vec<ProfileDocument>, ProfileError> {
        let mut profiles = Vec::new();
        for kind in ProfileKind::ALL {
            let entries = match fs::read_dir(self.root.join(kind.directory())) {
                Ok(e) => e,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(io("list profiles", e)),
            };
            let mut ids = Vec::new();
            for entry in entries {
                let path = entry
                    .map_err(|e| io("read profile directory entry", e))?
                    .path();
                if path.extension().is_some_and(|e| e == "json") {
                    let id = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .ok_or_else(|| invalid("profile filename is not UTF-8"))?;
                    ids.push(id.to_owned());
                }
            }
            ids.sort();
            for id in ids {
                profiles.push(self.load(kind, &id)?);
            }
        }
        Ok(profiles)
    }
    /// Save validated printer hardware with an explicit collision/revision policy.
    /// # Errors
    /// Returns validation, contention, stale revision or original I/O failure.
    pub fn save_printer(&self, p: &PrinterProfile, policy: SavePolicy) -> Result<(), ProfileError> {
        self.save(&ProfileDocument::Printer(p.clone()), policy)
    }
    /// Save validated resin calibration with an explicit collision/revision policy.
    /// # Errors
    /// Returns validation, contention, stale revision or original I/O failure.
    pub fn save_resin(&self, p: &ResinProfile, policy: SavePolicy) -> Result<(), ProfileError> {
        self.save(&ProfileDocument::Resin(p.clone()), policy)
    }
    /// Import typed JSON under an explicit collision/revision policy.
    /// # Errors
    /// Invalid imports never modify an existing profile.
    pub fn import(
        &self,
        bytes: &[u8],
        policy: SavePolicy,
    ) -> Result<ProfileDocument, ProfileError> {
        let p = ProfileDocument::from_json(bytes)?;
        self.save(&p, policy)?;
        Ok(p)
    }
    /// Remove exactly this persisted revision, keeping it in the revision history.
    /// # Errors
    /// Reports lock contention, an absent profile, a stale revision and filesystem failures;
    /// the profile is then left as it was.
    pub fn remove(
        &self,
        kind: ProfileKind,
        id: &str,
        expected_revision: u64,
    ) -> Result<(), ProfileError> {
        let path = self.path(kind, id)?;
        create_directory(&self.root)?;
        let _lock = self.lock()?;
        let bytes = read_bytes(&path)?;
        let old = ProfileDocument::from_json(&bytes)?;
        if old.id() != id || old.kind() != kind || old.revision() != expected_revision {
            return Err(ProfileError::Conflict(
                "profile revision conflict; reload before removing".into(),
            ));
        }
        self.preserve(kind, id, expected_revision, &bytes)?;
        fs::remove_file(&path).map_err(|e| io("remove profile", e))
    }
    fn lock(&self) -> Result<OwnedFile, ProfileError> {
        let lock_path = self.root.join(".library.lock");
        let lock_file=OpenOptions::new().write(true).create_new(true).open(&lock_path).map_err(|e|if e.kind()==std::io::ErrorKind::AlreadyExists {ProfileError::Conflict("profile library is locked; recover a stale lock explicitly only after checking no writer is active".into())}else{io("acquire profile library lock",e)})?;
        Ok(OwnedFile {
            path: lock_path,
            file: Some(lock_file),
        })
    }
    /// Keep `bytes`, revision `revision` of a profile, in its history.
    fn preserve(
        &self,
        kind: ProfileKind,
        id: &str,
        revision: u64,
        bytes: &[u8],
    ) -> Result<(), ProfileError> {
        let history = self.root.join("history").join(kind.directory()).join(id);
        create_directory(&history)?;
        let backup = history.join(format!("{revision}.json"));
        let temporary_backup = write_temporary(&backup, bytes)?;
        match fs::hard_link(&temporary_backup.path, &backup) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if read_bytes(&backup)? == bytes {
                    Ok(())
                } else {
                    Err(ProfileError::Conflict(
                        "profile revision backup differs from current bytes".into(),
                    ))
                }
            }
            Err(e) => Err(io("preserve profile revision", e)),
        }
    }
    /// Export deterministic validated JSON without modifying the library.
    /// # Errors
    /// Returns load or serialization failures.
    pub fn export(&self, kind: ProfileKind, id: &str) -> Result<Vec<u8>, ProfileError> {
        self.load(kind, id)?.to_json()
    }
    /// Serialize then atomically publish a revision under a cooperative library lock.
    ///
    /// Existing revisions are backed up before replacement. A process crash can leave the lock;
    /// explicit user recovery must establish that no writer is active before removing it. No
    /// cross-filesystem or universal power-loss durability guarantee is made. Operations are small,
    /// bounded and non-cancellable once publication starts.
    /// # Errors
    /// Reports validation, lock contention, collisions, stale revisions and filesystem failures.
    pub fn save(&self, profile: &ProfileDocument, policy: SavePolicy) -> Result<(), ProfileError> {
        let bytes = profile.to_json()?;
        let destination = self.path(profile.kind(), profile.id())?;
        create_directory(&self.root)?;
        let _lock = self.lock()?;
        create_directory(
            destination
                .parent()
                .ok_or_else(|| invalid("profile path has no parent"))?,
        )?;
        match policy {
            SavePolicy::CreateOnly => {
                if destination.exists() {
                    return Err(ProfileError::Conflict("profile already exists".into()));
                }
            }
            SavePolicy::Replace { expected_revision } => {
                let old_bytes = read_bytes(&destination)?;
                let old = ProfileDocument::from_json(&old_bytes)?;
                if old.id() != profile.id()
                    || old.kind() != profile.kind()
                    || old.revision() != expected_revision
                    || expected_revision.checked_add(1) != Some(profile.revision())
                {
                    return Err(ProfileError::Conflict(
                        "profile revision conflict; reload before replacing".into(),
                    ));
                }
                self.preserve(profile.kind(), profile.id(), expected_revision, &old_bytes)?;
            }
        }
        let temporary = write_temporary(&destination, &bytes)?;
        match policy {
            // A hard link publishes a fully written file without any overwrite race.
            SavePolicy::CreateOnly => fs::hard_link(&temporary.path, &destination)
                .map_err(|e| io("publish new profile without replacement", e))?,
            SavePolicy::Replace { .. } => fs::rename(&temporary.path, &destination)
                .map_err(|e| io("replace profile atomically", e))?,
        }
        #[cfg(unix)]
        File::open(
            destination
                .parent()
                .ok_or_else(|| invalid("profile path has no parent"))?,
        )
        .and_then(|f| f.sync_all())
        .map_err(|e| {
            io(
                "sync profile directory after publishing complete profile",
                e,
            )
        })?;
        Ok(())
    }
}
fn create_directory(path: &Path) -> Result<(), ProfileError> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|e| io("create profile directory", e))
}
struct OwnedFile {
    path: PathBuf,
    file: Option<File>,
}
impl Drop for OwnedFile {
    fn drop(&mut self) {
        self.file.take();
        let _ = fs::remove_file(&self.path);
    }
}

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
fn write_temporary(destination: &Path, bytes: &[u8]) -> Result<OwnedFile, ProfileError> {
    let mut temporary = None;
    for _ in 0..128 {
        let serial = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = destination.with_extension(format!("{}-{serial}.tmp", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => {
                temporary = Some(OwnedFile {
                    path,
                    file: Some(file),
                });
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(io("create temporary profile", e)),
        }
    }
    let mut temporary = temporary.ok_or_else(|| {
        ProfileError::Conflict("unable to reserve temporary profile filename".into())
    })?;
    let file = temporary
        .file
        .as_mut()
        .ok_or_else(|| invalid("temporary profile file is closed"))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| io("write temporary profile", e))?;
    temporary.file.take();
    Ok(temporary)
}
