//! Sound groups and the persistent library model (RPG-Sounds style categories).
//!
//! This module is intentionally pure: it performs no I/O, touches no audio
//! device, and imports nothing from `egui`. It is the single source of truth
//! for the ordered list of sounds, their per-sound volume, and their group
//! membership; `crate::config::LibraryConfig` is its persisted mirror.
//!
//! Path identity rule: sound paths are compared exactly as supplied. Paths are
//! never canonicalized, because a sound file may be missing or on a
//! currently-unavailable volume and the library must still round-trip.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::config::{GroupConfig, LibraryConfig, SoundConfigEntry};

/// Stable identifier of a sound group.
pub type GroupId = Uuid;

/// Maximum accepted length of a group name, in Unicode scalar values.
///
/// The name is trimmed before validation, so surrounding whitespace never
/// counts against the limit.
pub const MAX_GROUP_NAME_LEN: usize = 32;

/// A named sound group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// Stable identifier, generated on creation and persisted.
    pub id: GroupId,
    /// Display name, stored trimmed and unique case-insensitively.
    pub name: String,
}

/// Which sounds the UI should show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupFilter {
    /// Every registered sound.
    #[default]
    All,
    /// Only sounds not assigned to any group.
    Ungrouped,
    /// Only sounds assigned to the given group.
    Group(GroupId),
}

/// A registered sound.
#[derive(Debug, Clone, PartialEq)]
pub struct LibrarySound {
    /// Path exactly as supplied by the user (never canonicalized).
    pub path: PathBuf,
    /// Display name; the UI falls back to the file name when this is empty.
    pub name: String,
    /// Per-sound volume in `0.0..=1.0`.
    pub volume: f32,
    /// Whether the sound restarts itself when it reaches the end.
    pub looped: bool,
    /// Group membership, or `None` when ungrouped.
    pub group: Option<GroupId>,
}

/// The ordered sound library plus its groups.
///
/// Insertion order is preserved for both groups and sounds, and is what
/// `to_config` writes back, so a save/load cycle never reshuffles the UI.
#[derive(Debug, Clone, Default)]
pub struct Library {
    groups: Vec<Group>,
    sounds: Vec<LibrarySound>,
    /// `path -> index into sounds`, kept in sync with `sounds`.
    by_path: HashMap<PathBuf, usize>,
    /// `group id -> index into groups`, kept in sync with `groups`.
    by_id: HashMap<GroupId, usize>,
}

impl Library {
    /// An empty library.
    pub fn new() -> Self {
        Self::default()
    }

    /// Rebuild the model from persisted config. Invalid/duplicate group ids and
    /// dangling group references are repaired (dangling => ungrouped).
    ///
    /// Repair rules:
    /// - a `GroupConfig` whose `id` is not a UUID, or that repeats an earlier
    ///   id, is skipped;
    /// - a sound whose `path` is empty is skipped, and so is a repeated path;
    /// - a sound pointing at an unknown group id becomes ungrouped.
    pub fn from_config(cfg: &LibraryConfig) -> Self {
        let mut library = Self::new();

        for persisted in &cfg.groups {
            let Ok(id) = Uuid::parse_str(persisted.id.trim()) else {
                continue;
            };
            if library.by_id.contains_key(&id) {
                continue;
            }
            // Names are persisted verbatim; only surrounding whitespace is
            // dropped, since that is what `add_group` refuses to store.
            let name = persisted.name.trim().to_string();
            if name.is_empty() || name.chars().count() > MAX_GROUP_NAME_LEN {
                continue;
            }
            library.by_id.insert(id, library.groups.len());
            library.groups.push(Group { id, name });
        }

        for persisted in &cfg.sounds {
            if persisted.path.trim().is_empty() {
                continue;
            }
            let path = PathBuf::from(&persisted.path);
            if library.by_path.contains_key(&path) {
                continue;
            }
            let group = persisted
                .group
                .as_deref()
                .and_then(|raw| Uuid::parse_str(raw.trim()).ok())
                .filter(|id| library.by_id.contains_key(id));

            library.by_path.insert(path.clone(), library.sounds.len());
            library.sounds.push(LibrarySound {
                path,
                name: persisted.name.clone(),
                volume: persisted.volume,
                looped: persisted.looped,
                group,
            });
        }

        library
    }

    /// Serialize back to config. Stable ordering (insertion order preserved).
    pub fn to_config(&self) -> LibraryConfig {
        LibraryConfig {
            groups: self
                .groups
                .iter()
                .map(|group| GroupConfig {
                    id: group.id.to_string(),
                    name: group.name.clone(),
                })
                .collect(),
            sounds: self
                .sounds
                .iter()
                .map(|sound| SoundConfigEntry {
                    path: sound.path.to_string_lossy().into_owned(),
                    name: sound.name.clone(),
                    volume: sound.volume,
                    looped: sound.looped,
                    group: sound.group.map(|id| id.to_string()),
                })
                .collect(),
        }
    }

    /// All groups, in insertion order.
    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    /// Look up a group by id.
    pub fn group(&self, id: GroupId) -> Option<&Group> {
        self.by_id
            .get(&id)
            .and_then(|index| self.groups.get(*index))
    }

    /// Look up a group's name by id.
    pub fn group_name(&self, id: GroupId) -> Option<&str> {
        self.group(id).map(|group| group.name.as_str())
    }

    /// All sounds, in insertion order.
    pub fn sounds(&self) -> &[LibrarySound] {
        &self.sounds
    }

    /// Look up a sound by its exact path.
    pub fn sound(&self, path: &Path) -> Option<&LibrarySound> {
        self.by_path
            .get(path)
            .and_then(|index| self.sounds.get(*index))
    }

    /// Look up a sound mutably by its exact path.
    pub fn sound_mut(&mut self, path: &Path) -> Option<&mut LibrarySound> {
        let index = *self.by_path.get(path)?;
        self.sounds.get_mut(index)
    }

    /// Create a group. Returns None when the trimmed name is empty, longer than
    /// `MAX_GROUP_NAME_LEN`, or already used case-insensitively.
    pub fn add_group(&mut self, name: &str) -> Option<GroupId> {
        let name = normalize_group_name(name)?;
        if self.name_taken(&name, None) {
            return None;
        }
        let id = Uuid::new_v4();
        self.by_id.insert(id, self.groups.len());
        self.groups.push(Group { id, name });
        Some(id)
    }

    /// Rename. Returns false when id is unknown or the new name is invalid/duplicate.
    ///
    /// Renaming a group to its own current name (ignoring case and surrounding
    /// whitespace) succeeds and normalizes the stored spelling.
    pub fn rename_group(&mut self, id: GroupId, name: &str) -> bool {
        let Some(name) = normalize_group_name(name) else {
            return false;
        };
        if self.name_taken(&name, Some(id)) {
            return false;
        }
        let Some(index) = self.by_id.get(&id).copied() else {
            return false;
        };
        match self.groups.get_mut(index) {
            Some(group) => {
                group.name = name;
                true
            }
            None => false,
        }
    }

    /// Delete a group; its sounds become ungrouped. Returns false if unknown.
    pub fn remove_group(&mut self, id: GroupId) -> bool {
        let Some(index) = self.by_id.get(&id).copied() else {
            return false;
        };
        self.groups.remove(index);
        self.reindex_groups();

        for sound in &mut self.sounds {
            if sound.group == Some(id) {
                sound.group = None;
            }
        }
        true
    }

    /// Register a sound path. Returns true if it was newly added, false if it
    /// already existed (in which case name/volume/group are left untouched).
    ///
    /// An empty path, and a group id that is not a known group, are rejected the
    /// same way `assign` treats them: the entry is skipped / stored as ungrouped.
    pub fn insert_sound(
        &mut self,
        path: &Path,
        name: &str,
        volume: f32,
        group: Option<GroupId>,
    ) -> bool {
        if path.as_os_str().is_empty() || self.by_path.contains_key(path) {
            return false;
        }
        let group = group.filter(|id| self.by_id.contains_key(id));
        self.by_path.insert(path.to_path_buf(), self.sounds.len());
        self.sounds.push(LibrarySound {
            path: path.to_path_buf(),
            name: name.to_string(),
            volume,
            looped: false,
            group,
        });
        true
    }

    /// Forget a sound. Returns false if it was not registered.
    pub fn remove_sound(&mut self, path: &Path) -> bool {
        let Some(index) = self.by_path.remove(path) else {
            return false;
        };
        self.sounds.remove(index);
        self.reindex_paths();
        true
    }

    /// Assign (or clear with None) a sound's group. Ignores unknown group ids
    /// (treats them as None). Returns false if the sound is unknown.
    pub fn assign(&mut self, path: &Path, group: Option<GroupId>) -> bool {
        let group = group.filter(|id| self.by_id.contains_key(id));
        match self.sound_mut(path) {
            Some(sound) => {
                sound.group = group;
                true
            }
            None => false,
        }
    }

    /// Volume is clamped to 0.0..=1.0. Returns false if the sound is unknown.
    ///
    /// A NaN volume is treated as 0.0 rather than propagated into the mixer gain.
    pub fn set_volume(&mut self, path: &Path, volume: f32) -> bool {
        let volume = clamp_volume(volume);
        match self.sound_mut(path) {
            Some(sound) => {
                sound.volume = volume;
                true
            }
            None => false,
        }
    }

    /// Turn the "repeat" flag on or off. Returns false if the sound is unknown.
    ///
    /// The flag is part of the persisted library, so a sound that is set to
    /// repeat is still set to repeat after a restart.
    pub fn set_looped(&mut self, path: &Path, looped: bool) -> bool {
        match self.sound_mut(path) {
            Some(sound) => {
                sound.looped = looped;
                true
            }
            None => false,
        }
    }

    /// Does this sound belong to the given filter?
    ///
    /// An unregistered path belongs to no filter, including `All`.
    pub fn matches(&self, path: &Path, filter: GroupFilter) -> bool {
        let Some(sound) = self.sound(path) else {
            return false;
        };
        match filter {
            GroupFilter::All => true,
            GroupFilter::Ungrouped => sound.group.is_none(),
            GroupFilter::Group(id) => sound.group == Some(id),
        }
    }

    /// Number of sounds in the filter.
    pub fn count_for(&self, filter: GroupFilter) -> usize {
        self.sounds
            .iter()
            .filter(|s| filter_accepts(filter, s))
            .count()
    }

    /// Sounds in the filter, preserving library order.
    pub fn sounds_for(&self, filter: GroupFilter) -> Vec<&LibrarySound> {
        self.sounds
            .iter()
            .filter(|sound| filter_accepts(filter, sound))
            .collect()
    }

    /// Total number of registered sounds.
    pub fn len(&self) -> usize {
        self.sounds.len()
    }

    /// True when no sound is registered (groups may still exist).
    pub fn is_empty(&self) -> bool {
        self.sounds.is_empty()
    }

    fn name_taken(&self, candidate: &str, except: Option<GroupId>) -> bool {
        let lowered = candidate.to_lowercase();
        self.groups
            .iter()
            .any(|group| Some(group.id) != except && group.name.to_lowercase() == lowered)
    }

    fn reindex_groups(&mut self) {
        self.by_id = self
            .groups
            .iter()
            .enumerate()
            .map(|(index, group)| (group.id, index))
            .collect();
    }

    fn reindex_paths(&mut self) {
        self.by_path = self
            .sounds
            .iter()
            .enumerate()
            .map(|(index, sound)| (sound.path.clone(), index))
            .collect();
    }
}

/// Trim, then reject empty / over-long names.
fn normalize_group_name(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_GROUP_NAME_LEN {
        return None;
    }
    Some(trimmed.to_string())
}

/// Clamp a gain into the legal range, folding NaN to silence.
fn clamp_volume(volume: f32) -> f32 {
    if volume.is_nan() {
        0.0
    } else {
        volume.clamp(0.0, 1.0)
    }
}

fn filter_accepts(filter: GroupFilter, sound: &LibrarySound) -> bool {
    match filter {
        GroupFilter::All => true,
        GroupFilter::Ungrouped => sound.group.is_none(),
        GroupFilter::Group(id) => sound.group == Some(id),
    }
}
