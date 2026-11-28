use crate::keys::CredentialStore;
use ovt_core::config::{AsrProfile, LegacySettings, LlmProfile, Settings};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileRole {
    Asr,
    Llm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessState {
    Ready,
    MissingProfile,
    MissingKey,
    VaultError,
    InvalidConfig,
}

#[derive(Debug, Clone, Serialize)]
pub struct Readiness {
    pub state: ReadinessState,
    pub profile_id: Option<String>,
    pub credential_id: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiStatus {
    pub asr: Readiness,
    pub llm: Readiness,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigExport {
    pub kind: String,
    pub version: u32,
    pub asr_profiles: Vec<AsrProfile>,
    pub llm_profiles: Vec<LlmProfile>,
    #[serde(default, skip_serializing)]
    pub credentials: Vec<LegacyCredentialMetadata>,
    pub active_asr_profile_id: Option<String>,
    pub active_llm_profile_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LegacyCredentialMetadata {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub hint: Option<String>,
}

pub struct ConfigStore {
    path: PathBuf,
    settings: Mutex<Settings>,
    vault: Arc<dyn CredentialStore>,
}

fn lock(settings: &Mutex<Settings>) -> Result<std::sync::MutexGuard<'_, Settings>, String> {
    settings.lock().map_err(|_| "settings lock poisoned".into())
}

fn key_hint(secret: &str) -> Option<String> {
    let chars: Vec<char> = secret.chars().collect();
    (chars.len() >= 8).then(|| chars[chars.len() - 4..].iter().collect())
}

fn endpoint_host(url: Option<&str>) -> Option<String> {
    reqwest::Url::parse(url?)
        .ok()?
        .host_str()
        .map(|host| host.to_string())
}

/// Whether a settings file failed to load because of its content (and so may
/// be reset), as opposed to an environment failure such as a keychain error
/// during migration. Only the current schema and the unversioned legacy
/// format are judged; other versions are never considered corrupt.
fn settings_content_is_corrupt(raw: &[u8]) -> bool {
    let Ok(raw) = std::str::from_utf8(raw) else {
        return true;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return true;
    };
    match value.get("schema_version") {
        None => serde_json::from_value::<LegacySettings>(value).is_err(),
        Some(version)
            if version.as_u64() == Some(u64::from(ovt_core::config::SETTINGS_SCHEMA_VERSION)) =>
        {
            serde_json::from_value::<Settings>(value)
                .map_or(true, |settings| settings.validate().is_err())
        }
        Some(_) => false,
    }
}

fn save_path(path: &Path, settings: &Settings) -> Result<(), String> {
    settings.validate().map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create settings dir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| format!("cannot write settings: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("cannot commit settings: {e}"))
}

impl ConfigStore {
    pub fn load(path: PathBuf, vault: Arc<dyn CredentialStore>) -> Result<Self, String> {
        if !path.exists() {
            let mut settings = Settings::default();
            settings.ui.onboarding_completed = Some(false);
            save_path(&path, &settings)?;
            return Ok(Self {
                path,
                settings: Mutex::new(settings),
                vault,
            });
        }

        let raw =
            std::fs::read_to_string(&path).map_err(|e| format!("cannot read settings: {e}"))?;
        let value: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| format!("invalid settings JSON: {e}"))?;
        let settings = match value.get("schema_version") {
            None => {
                let legacy: LegacySettings = serde_json::from_value(value)
                    .map_err(|e| format!("invalid legacy settings: {e}"))?;
                let mut migrated = Settings::migrate_legacy(legacy);
                let backup = path.with_file_name("settings.v1.backup.json");
                if !backup.exists() {
                    std::fs::write(&backup, &raw)
                        .map_err(|e| format!("cannot back up legacy settings: {e}"))?;
                }
                for (old_id, new_id) in [
                    ("asr", "legacy-asr-credential"),
                    ("llm", "legacy-llm-credential"),
                ] {
                    if let Some(secret) = vault.load(old_id).map_err(|e| e.to_string())? {
                        let secret = secret.trim();
                        if !secret.is_empty() {
                            vault.store(new_id, secret).map_err(|e| e.to_string())?;
                        }
                    }
                }
                for profile in migrated.asr_profiles.iter_mut() {
                    if let Some(id) = profile.credential_id.as_deref() {
                        profile.credential_hint = vault
                            .load(id)
                            .map_err(|e| e.to_string())?
                            .as_deref()
                            .and_then(key_hint);
                    }
                }
                for profile in migrated.llm_profiles.iter_mut() {
                    if let Some(id) = profile.credential_id.as_deref() {
                        profile.credential_hint = vault
                            .load(id)
                            .map_err(|e| e.to_string())?
                            .as_deref()
                            .and_then(key_hint);
                    }
                }
                save_path(&path, &migrated)?;
                migrated
            }
            Some(version) if version.as_u64() == Some(2) => {
                let credentials: Vec<LegacyCredentialMetadata> = serde_json::from_value(
                    value
                        .get("credentials")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!([])),
                )
                .map_err(|e| format!("invalid credential metadata: {e}"))?;
                let mut migrated: Settings =
                    serde_json::from_value(value).map_err(|e| format!("invalid settings: {e}"))?;
                migrate_profile_credentials(&mut migrated, &credentials, &vault)?;
                save_path(&path, &migrated)?;
                migrated
            }
            Some(version) if version.as_u64() == Some(3) => {
                serde_json::from_value(value).map_err(|e| format!("invalid settings: {e}"))?
            }
            Some(version) => {
                return Err(format!(
                    "unsupported settings schema {}; this app supports version 3",
                    version
                ))
            }
        };
        settings.validate().map_err(|e| e.to_string())?;
        Ok(Self {
            path,
            settings: Mutex::new(settings),
            vault,
        })
    }

    /// Like [`Self::load`], but recovers from a corrupt settings file with
    /// defaults instead of failing startup (which aborted with a dialog and a
    /// panic). Returns the error that triggered the reset so the caller can
    /// tell the user. Only a file whose content cannot be parsed or validated
    /// is reset, and only after a backup copy was written. Everything else is
    /// still fatal and leaves the file alone: read errors (a sync client or
    /// antivirus lock), keychain or save failures while migrating, and any
    /// other schema version (resetting would discard settings a newer app
    /// version wrote, or a migration that failed for a transient reason).
    pub fn load_or_reset(
        path: PathBuf,
        vault: Arc<dyn CredentialStore>,
    ) -> Result<(Self, Option<String>), String> {
        match Self::load(path.clone(), Arc::clone(&vault)) {
            Ok(store) => Ok((store, None)),
            Err(error) => {
                let Ok(raw) = std::fs::read(&path) else {
                    return Err(error);
                };
                if !settings_content_is_corrupt(&raw) {
                    return Err(error);
                }
                // Never overwrite an earlier backup: it may hold the only copy
                // of the user's real settings from a previous reset.
                let mut backup = path.with_file_name("settings.invalid.backup.json");
                if backup.exists() {
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|elapsed| elapsed.as_millis())
                        .unwrap_or_default();
                    backup = path.with_file_name(format!("settings.invalid.backup-{stamp}.json"));
                }
                std::fs::write(&backup, &raw).map_err(|backup_error| {
                    format!("{error} (the settings were not reset: cannot back them up: {backup_error})")
                })?;
                std::fs::remove_file(&path).map_err(|remove_error| {
                    format!("{error} (the settings were not reset: {remove_error})")
                })?;
                let store = Self::load(path, vault)?;
                Ok((store, Some(error)))
            }
        }
    }

    pub fn snapshot(&self) -> Result<Settings, String> {
        Ok(lock(&self.settings)?.clone())
    }

    fn save_locked(&self, settings: &Settings) -> Result<(), String> {
        save_path(&self.path, settings)
    }

    fn commit_locked(
        &self,
        settings: &mut Settings,
        previous: Settings,
    ) -> Result<Settings, String> {
        if let Err(error) = self.save_locked(settings) {
            *settings = previous;
            return Err(error);
        }
        Ok(settings.clone())
    }

    pub fn save_general(&self, next: Settings) -> Result<Settings, String> {
        let mut settings = lock(&self.settings)?;
        let previous = settings.clone();
        settings.hotkey = next.hotkey;
        settings.dictation = next.dictation;
        settings.context = next.context;
        settings.history = next.history;
        settings.audio = next.audio;
        settings.ui = next.ui;
        self.commit_locked(&mut settings, previous)
    }

    pub fn status(&self) -> Result<ApiStatus, String> {
        let settings = lock(&self.settings)?;
        settings.validate().map_err(|e| e.to_string())?;
        Ok(ApiStatus {
            asr: readiness(&settings, ProfileRole::Asr, &self.vault)?,
            llm: readiness(&settings, ProfileRole::Llm, &self.vault)?,
        })
    }

    pub fn save_asr_profile(
        &self,
        mut profile: AsrProfile,
        api_key: Option<String>,
        confirm_host_change: bool,
    ) -> Result<Settings, String> {
        let mut settings = lock(&self.settings)?;
        let previous = settings.clone();
        if profile.id.is_empty() {
            profile.id = uuid::Uuid::new_v4().to_string();
        }
        let existing = settings
            .asr_profiles
            .iter()
            .find(|item| item.id == profile.id)
            .cloned();
        if existing
            .as_ref()
            .is_some_and(|existing| existing.preset != profile.preset)
            && !api_key.as_deref().is_some_and(|key| !key.trim().is_empty())
        {
            return Err("enter a new API key when changing providers".into());
        }
        let (credential_id, credential_hint) = prepare_profile_key(
            existing
                .as_ref()
                .and_then(|item| item.credential_id.as_deref()),
            existing
                .as_ref()
                .and_then(|item| item.credential_hint.clone()),
            api_key.as_deref(),
            &self.vault,
        )?;
        profile.credential_id = credential_id;
        profile.credential_hint = credential_hint;
        let old_credential_id = existing
            .as_ref()
            .and_then(|item| item.credential_id.clone());
        let new_credential_id = profile.credential_id.clone();
        let key_changed = old_credential_id != new_credential_id;
        if let Some(existing) = existing {
            let host_changed = endpoint_host(existing.resolved_base_url().as_deref())
                != endpoint_host(profile.resolved_base_url().as_deref());
            if host_changed
                && (existing.credential_id.is_some()
                    || profile.credential_id.is_some()
                    || api_key.as_deref().is_some_and(|key| !key.trim().is_empty()))
                && !confirm_host_change
            {
                if let Some(id) = profile.credential_id.as_deref() {
                    if existing.credential_id.as_deref() != Some(id) {
                        let _ = self.vault.delete(id);
                    }
                }
                return Err(
                    "endpoint host changed; confirm sending the bound key to the new host".into(),
                );
            }
        }
        let saved_id = profile.id.clone();
        if let Some(index) = settings
            .asr_profiles
            .iter()
            .position(|item| item.id == profile.id)
        {
            settings.asr_profiles[index] = profile;
        } else {
            settings.asr_profiles.push(profile);
        }
        if settings.active_asr_profile_id.is_none() {
            settings.active_asr_profile_id = Some(saved_id);
        }
        let result = self.commit_locked(&mut settings, previous);
        if result.is_err() && key_changed {
            if let Some(id) = new_credential_id {
                let _ = self.vault.delete(&id);
            }
        }
        if result.is_ok() && key_changed {
            if let Some(id) = old_credential_id {
                let _ = self.vault.delete(&id);
            }
        }
        result
    }

    pub fn save_llm_profile(
        &self,
        mut profile: LlmProfile,
        api_key: Option<String>,
        confirm_host_change: bool,
    ) -> Result<Settings, String> {
        let mut settings = lock(&self.settings)?;
        let previous = settings.clone();
        if profile.id.is_empty() {
            profile.id = uuid::Uuid::new_v4().to_string();
        }
        let existing = settings
            .llm_profiles
            .iter()
            .find(|item| item.id == profile.id)
            .cloned();
        if existing
            .as_ref()
            .is_some_and(|existing| existing.preset != profile.preset)
            && !api_key.as_deref().is_some_and(|key| !key.trim().is_empty())
        {
            return Err("enter a new API key when changing providers".into());
        }
        let (credential_id, credential_hint) = prepare_profile_key(
            existing
                .as_ref()
                .and_then(|item| item.credential_id.as_deref()),
            existing
                .as_ref()
                .and_then(|item| item.credential_hint.clone()),
            api_key.as_deref(),
            &self.vault,
        )?;
        profile.credential_id = credential_id;
        profile.credential_hint = credential_hint;
        let old_credential_id = existing
            .as_ref()
            .and_then(|item| item.credential_id.clone());
        let new_credential_id = profile.credential_id.clone();
        let key_changed = old_credential_id != new_credential_id;
        if let Some(existing) = existing {
            let host_changed = endpoint_host(existing.resolved_base_url().as_deref())
                != endpoint_host(profile.resolved_base_url().as_deref());
            if host_changed
                && (existing.credential_id.is_some()
                    || profile.credential_id.is_some()
                    || api_key.as_deref().is_some_and(|key| !key.trim().is_empty()))
                && !confirm_host_change
            {
                if let Some(id) = profile.credential_id.as_deref() {
                    if existing.credential_id.as_deref() != Some(id) {
                        let _ = self.vault.delete(id);
                    }
                }
                return Err(
                    "endpoint host changed; confirm sending the bound key to the new host".into(),
                );
            }
        }
        let saved_id = profile.id.clone();
        if let Some(index) = settings
            .llm_profiles
            .iter()
            .position(|item| item.id == profile.id)
        {
            settings.llm_profiles[index] = profile;
        } else {
            settings.llm_profiles.push(profile);
        }
        if settings.active_llm_profile_id.is_none() {
            settings.active_llm_profile_id = Some(saved_id);
        }
        let result = self.commit_locked(&mut settings, previous);
        if result.is_err() && key_changed {
            if let Some(id) = new_credential_id {
                let _ = self.vault.delete(&id);
            }
        }
        if result.is_ok() && key_changed {
            if let Some(id) = old_credential_id {
                let _ = self.vault.delete(&id);
            }
        }
        result
    }

    pub fn delete_profile(&self, role: ProfileRole, id: String) -> Result<Settings, String> {
        let mut settings = lock(&self.settings)?;
        let previous = settings.clone();
        let credential_id = match role {
            ProfileRole::Asr => {
                let position = settings
                    .asr_profiles
                    .iter()
                    .position(|profile| profile.id == id);
                let Some(position) = position else {
                    return Err("ASR profile not found".into());
                };
                let credential_id = settings.asr_profiles[position].credential_id.clone();
                settings.asr_profiles.remove(position);
                if settings.active_asr_profile_id.as_deref() == Some(&id) {
                    settings.active_asr_profile_id = None;
                }
                credential_id
            }
            ProfileRole::Llm => {
                let position = settings
                    .llm_profiles
                    .iter()
                    .position(|profile| profile.id == id);
                let Some(position) = position else {
                    return Err("LLM profile not found".into());
                };
                let credential_id = settings.llm_profiles[position].credential_id.clone();
                settings.llm_profiles.remove(position);
                if settings.active_llm_profile_id.as_deref() == Some(&id) {
                    settings.active_llm_profile_id = None;
                }
                credential_id
            }
        };
        let saved = self.commit_locked(&mut settings, previous)?;
        if let Some(credential_id) = credential_id {
            let _ = self.vault.delete(&credential_id);
        }
        Ok(saved)
    }

    pub fn set_active_profile(&self, role: ProfileRole, id: String) -> Result<Settings, String> {
        let mut settings = lock(&self.settings)?;
        let previous = settings.clone();
        match role {
            ProfileRole::Asr => {
                if !settings.asr_profiles.iter().any(|profile| profile.id == id) {
                    return Err("ASR profile not found".into());
                }
                settings.active_asr_profile_id = Some(id);
            }
            ProfileRole::Llm => {
                if !settings.llm_profiles.iter().any(|profile| profile.id == id) {
                    return Err("LLM profile not found".into());
                }
                settings.active_llm_profile_id = Some(id);
            }
        }
        self.commit_locked(&mut settings, previous)
    }

    pub fn export_data(&self) -> Result<ConfigExport, String> {
        let settings = lock(&self.settings)?;
        settings.validate().map_err(|e| e.to_string())?;
        Ok(ConfigExport {
            kind: "oh-my-voice-type/config".into(),
            version: 2,
            asr_profiles: settings.asr_profiles.clone(),
            llm_profiles: settings.llm_profiles.clone(),
            credentials: Vec::new(),
            active_asr_profile_id: settings.active_asr_profile_id.clone(),
            active_llm_profile_id: settings.active_llm_profile_id.clone(),
        })
    }

    pub fn export_to(&self, path: &Path) -> Result<(), String> {
        let data = self.export_data()?;
        let json = serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| format!("cannot export config: {e}"))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("cannot commit export: {e}"))
    }

    pub fn import_from(&self, path: &Path) -> Result<Settings, String> {
        let metadata = std::fs::metadata(path).map_err(|e| format!("cannot open export: {e}"))?;
        if metadata.len() > 1024 * 1024 {
            return Err("config export is larger than 1 MiB".into());
        }
        let raw = std::fs::read_to_string(path).map_err(|e| format!("cannot read export: {e}"))?;
        let mut data: ConfigExport =
            serde_json::from_str(&raw).map_err(|e| format!("invalid config export: {e}"))?;
        if data.kind != "oh-my-voice-type/config" || !matches!(data.version, 1 | 2) {
            return Err("unsupported config export format".into());
        }
        let hints: HashMap<String, Option<String>> = data
            .credentials
            .into_iter()
            .map(|credential| (credential.id, credential.hint))
            .collect();
        let mut profile_ids = HashMap::new();
        for profile in data.asr_profiles.iter_mut() {
            let new_id = uuid::Uuid::new_v4().to_string();
            profile_ids.insert(profile.id.clone(), new_id.clone());
            profile.id = new_id;
            profile.credential_hint = profile
                .credential_id
                .as_ref()
                .and_then(|id| hints.get(id).cloned())
                .flatten();
            profile.credential_id = None;
        }
        for profile in data.llm_profiles.iter_mut() {
            let new_id = uuid::Uuid::new_v4().to_string();
            profile_ids.insert(profile.id.clone(), new_id.clone());
            profile.id = new_id;
            profile.credential_hint = profile
                .credential_id
                .as_ref()
                .and_then(|id| hints.get(id).cloned())
                .flatten();
            profile.credential_id = None;
        }
        let remap_active = |id: Option<String>| {
            id.map(|id| {
                profile_ids
                    .get(&id)
                    .cloned()
                    .ok_or("invalid active profile reference")
            })
            .transpose()
        };
        let active_asr_profile_id = remap_active(data.active_asr_profile_id)?;
        let active_llm_profile_id = remap_active(data.active_llm_profile_id)?;
        let mut settings = lock(&self.settings)?;
        let previous = settings.clone();
        settings.asr_profiles.extend(data.asr_profiles);
        settings.llm_profiles.extend(data.llm_profiles);
        if settings.active_asr_profile_id.is_none() {
            settings.active_asr_profile_id = active_asr_profile_id;
        }
        if settings.active_llm_profile_id.is_none() {
            settings.active_llm_profile_id = active_llm_profile_id;
        }
        self.commit_locked(&mut settings, previous)
    }
}

fn prepare_profile_key(
    existing_id: Option<&str>,
    existing_hint: Option<String>,
    api_key: Option<&str>,
    vault: &Arc<dyn CredentialStore>,
) -> Result<(Option<String>, Option<String>), String> {
    let Some(secret) = api_key.map(str::trim).filter(|secret| !secret.is_empty()) else {
        return match existing_id {
            Some(id) => Ok((Some(id.to_string()), existing_hint)),
            None => Err("API key is required".into()),
        };
    };
    let id = uuid::Uuid::new_v4().to_string();
    vault.store(&id, secret).map_err(|e| e.to_string())?;
    Ok((Some(id), key_hint(secret)))
}

fn migrate_profile_credentials(
    settings: &mut Settings,
    credentials: &[LegacyCredentialMetadata],
    vault: &Arc<dyn CredentialStore>,
) -> Result<(), String> {
    settings.schema_version = ovt_core::config::SETTINGS_SCHEMA_VERSION;
    let mut counts = HashMap::new();
    for profile in settings.asr_profiles.iter() {
        if let Some(id) = profile.credential_id.as_deref() {
            *counts.entry(id.to_string()).or_insert(0usize) += 1;
        }
    }
    for profile in settings.llm_profiles.iter() {
        if let Some(id) = profile.credential_id.as_deref() {
            *counts.entry(id.to_string()).or_insert(0usize) += 1;
        }
    }
    let migrate =
        |credential_id: &mut Option<String>, hint: &mut Option<String>| -> Result<(), String> {
            let Some(id) = credential_id.as_deref() else {
                return Ok(());
            };
            let known_hint = credentials
                .iter()
                .find(|credential| credential.id == id)
                .and_then(|credential| credential.hint.clone());
            let secret = vault.load(id).map_err(|e| e.to_string())?;
            if counts.get(id).copied().unwrap_or_default() > 1 {
                let Some(secret) = secret else {
                    *hint = known_hint;
                    return Ok(());
                };
                let new_id = uuid::Uuid::new_v4().to_string();
                vault.store(&new_id, &secret).map_err(|e| e.to_string())?;
                *hint = key_hint(&secret);
                *credential_id = Some(new_id);
            } else {
                *hint = known_hint.or_else(|| secret.as_deref().and_then(key_hint));
            }
            Ok(())
        };
    for profile in &mut settings.asr_profiles {
        migrate(&mut profile.credential_id, &mut profile.credential_hint)?;
    }
    for profile in &mut settings.llm_profiles {
        migrate(&mut profile.credential_id, &mut profile.credential_hint)?;
    }
    Ok(())
}

fn readiness(
    settings: &Settings,
    role: ProfileRole,
    vault: &Arc<dyn CredentialStore>,
) -> Result<Readiness, String> {
    let (profile_id, credential_id) = match role {
        ProfileRole::Asr => (
            settings.active_asr_profile_id.clone(),
            settings
                .asr_profiles
                .iter()
                .find(|profile| Some(&profile.id) == settings.active_asr_profile_id.as_ref())
                .and_then(|profile| profile.credential_id.clone()),
        ),
        ProfileRole::Llm => (
            settings.active_llm_profile_id.clone(),
            settings
                .llm_profiles
                .iter()
                .find(|profile| Some(&profile.id) == settings.active_llm_profile_id.as_ref())
                .and_then(|profile| profile.credential_id.clone()),
        ),
    };
    let base = Readiness {
        state: ReadinessState::MissingProfile,
        profile_id,
        credential_id: credential_id.clone(),
        message: None,
    };
    let Some(credential_id) = credential_id else {
        return Ok(Readiness {
            state: if base.profile_id.is_some() {
                ReadinessState::MissingKey
            } else {
                ReadinessState::MissingProfile
            },
            ..base
        });
    };
    match vault.load(&credential_id) {
        Ok(Some(_)) => Ok(Readiness {
            state: ReadinessState::Ready,
            ..base
        }),
        Ok(None) => Ok(Readiness {
            state: ReadinessState::MissingKey,
            message: Some("bound credential is missing from the system vault".into()),
            ..base
        }),
        Err(error) => Ok(Readiness {
            state: ReadinessState::VaultError,
            message: Some(error.to_string()),
            ..base
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::CredentialError;
    use std::collections::BTreeMap;

    struct MemoryVault {
        values: Mutex<BTreeMap<String, String>>,
        fail_writes: bool,
    }

    impl MemoryVault {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                values: Mutex::new(BTreeMap::new()),
                fail_writes: false,
            })
        }
    }

    impl CredentialStore for MemoryVault {
        fn load(&self, id: &str) -> Result<Option<String>, CredentialError> {
            Ok(self.values.lock().unwrap().get(id).cloned())
        }
        fn store(&self, id: &str, secret: &str) -> Result<(), CredentialError> {
            if self.fail_writes {
                return Err(CredentialError::Vault("write denied".into()));
            }
            self.values.lock().unwrap().insert(id.into(), secret.into());
            Ok(())
        }
        fn delete(&self, id: &str) -> Result<(), CredentialError> {
            self.values.lock().unwrap().remove(id);
            Ok(())
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        // A unique directory per test, not just per file: the load-reset
        // backup uses a fixed sibling name, so shared directories would leak
        // state between tests.
        let dir = std::env::temp_dir().join(format!("ovt-config-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{name}.json"))
    }

    #[test]
    fn failed_key_write_does_not_change_settings() {
        let path = temp_path("write-failure");
        let vault = Arc::new(MemoryVault {
            values: Mutex::new(BTreeMap::new()),
            fail_writes: true,
        });
        let store = ConfigStore::load(path, vault).unwrap();
        assert!(store
            .save_asr_profile(AsrProfile::default(), Some("secret".into()), false)
            .is_err());
        assert!(store.snapshot().unwrap().asr_profiles.is_empty());
    }

    #[test]
    fn broken_settings_reset_to_defaults_and_are_backed_up() {
        let path = temp_path("broken");
        let raw = r#"{"schema_version":3,"llm_profiles":[{"id":"x","format":"openai_chat"}]}"#;
        std::fs::write(&path, raw).unwrap();
        let (store, error) = ConfigStore::load_or_reset(path.clone(), MemoryVault::new()).unwrap();
        let error = error.expect("load_or_reset should report the reset");
        assert!(error.contains("invalid settings"), "got {error:?}");
        let backup = path.with_file_name("settings.invalid.backup.json");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), raw);
        let settings = store.snapshot().unwrap();
        assert_eq!(settings.hotkey.dictate, Settings::default().hotkey.dictate);
        assert!(settings.llm_profiles.is_empty());
        assert_eq!(settings.ui.onboarding_completed, Some(false));
    }

    #[test]
    fn unreadable_settings_fail_without_reset() {
        // A directory where the file should be: load fails on the read, not
        // the content, so nothing may be deleted or reset.
        let path = temp_path("unreadable");
        std::fs::create_dir_all(&path).unwrap();
        assert!(ConfigStore::load_or_reset(path.clone(), MemoryVault::new()).is_err());
        assert!(path.is_dir());
        assert!(!path.with_file_name("settings.invalid.backup.json").exists());
    }

    #[test]
    fn invalid_json_settings_reset_to_defaults() {
        let path = temp_path("invalid-json");
        std::fs::write(&path, "{not json").unwrap();
        let (_, error) = ConfigStore::load_or_reset(path.clone(), MemoryVault::new()).unwrap();
        assert!(error.unwrap().contains("invalid settings JSON"));
        assert_eq!(
            std::fs::read_to_string(path.with_file_name("settings.invalid.backup.json")).unwrap(),
            "{not json"
        );
    }

    #[test]
    fn second_reset_keeps_the_first_backup() {
        let path = temp_path("second-reset");
        let backup = path.with_file_name("settings.invalid.backup.json");
        std::fs::write(&backup, "first").unwrap();
        std::fs::write(&path, "{second").unwrap();
        ConfigStore::load_or_reset(path.clone(), MemoryVault::new()).unwrap();
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "first");
        let stamped = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .find(|candidate| {
                candidate
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("settings.invalid.backup-"))
            })
            .expect("the second reset should write a timestamped backup");
        assert_eq!(std::fs::read_to_string(stamped).unwrap(), "{second");
    }

    #[test]
    fn newer_settings_schema_still_fails_without_reset() {
        let path = temp_path("newer-schema");
        std::fs::write(&path, r#"{"schema_version":99}"#).unwrap();
        assert!(ConfigStore::load_or_reset(path.clone(), MemoryVault::new()).is_err());
        assert!(path.exists());
        assert!(!path.with_file_name("settings.invalid.backup.json").exists());
    }

    #[test]
    fn profile_owns_its_key_and_delete_removes_it() {
        let path = temp_path("profile-key");
        let vault = MemoryVault::new();
        let store = ConfigStore::load(path, vault.clone()).unwrap();
        let settings = store
            .save_asr_profile(
                AsrProfile {
                    id: "p1".into(),
                    label: "ASR".into(),
                    ..AsrProfile::default()
                },
                Some("secret123".into()),
                false,
            )
            .unwrap();
        let id = settings.asr_profiles[0].credential_id.clone().unwrap();
        assert_eq!(
            settings.asr_profiles[0].credential_hint.as_deref(),
            Some("t123")
        );
        assert_eq!(vault.load(&id).unwrap().as_deref(), Some("secret123"));
        store
            .set_active_profile(ProfileRole::Asr, "p1".into())
            .unwrap();
        assert!(store.status().unwrap().asr.state == ReadinessState::Ready);

        let settings = store
            .save_asr_profile(settings.asr_profiles[0].clone(), None, false)
            .unwrap();
        assert_eq!(
            settings.asr_profiles[0].credential_id.as_deref(),
            Some(id.as_str())
        );
        store.delete_profile(ProfileRole::Asr, "p1".into()).unwrap();
        assert!(vault.load(&id).unwrap().is_none());
    }

    #[test]
    fn changing_provider_requires_a_new_key() {
        let path = temp_path("provider-change");
        let vault = MemoryVault::new();
        let store = ConfigStore::load(path, vault.clone()).unwrap();
        let settings = store
            .save_asr_profile(
                AsrProfile {
                    id: "p1".into(),
                    label: "Unisound".into(),
                    preset: Some("unisound-u2".into()),
                    ..AsrProfile::default()
                },
                Some("old-secret".into()),
                false,
            )
            .unwrap();
        let old_id = settings.asr_profiles[0].credential_id.clone().unwrap();
        let mut changed = settings.asr_profiles[0].clone();
        changed.preset = Some("openai".into());
        changed.label = "OpenAI".into();
        changed.engine = ovt_core::config::AsrEngine::OpenaiCompatible;
        changed.base_url = Some("https://api.openai.com/v1".into());
        changed.model = Some("whisper-1".into());
        assert!(store
            .save_asr_profile(changed.clone(), None, false)
            .is_err());
        assert_eq!(vault.load(&old_id).unwrap().as_deref(), Some("old-secret"));

        let saved = store
            .save_asr_profile(changed, Some("new-secret".into()), true)
            .unwrap();
        let new_id = saved.asr_profiles[0].credential_id.clone().unwrap();
        assert_ne!(old_id, new_id);
        assert!(vault.load(&old_id).unwrap().is_none());
        assert_eq!(vault.load(&new_id).unwrap().as_deref(), Some("new-secret"));
    }

    #[test]
    fn export_contains_no_secret_and_import_requires_reentry() {
        let path = temp_path("export-source");
        let vault = MemoryVault::new();
        let store = ConfigStore::load(path, vault).unwrap();
        let canary = "sk-canary-do-not-export";
        store
            .save_asr_profile(
                AsrProfile {
                    id: "p1".into(),
                    label: "ASR".into(),
                    ..AsrProfile::default()
                },
                Some(canary.into()),
                false,
            )
            .unwrap();
        store
            .set_active_profile(ProfileRole::Asr, "p1".into())
            .unwrap();
        let export_path = temp_path("export-data");
        store.export_to(&export_path).unwrap();
        let raw = std::fs::read_to_string(&export_path).unwrap();
        assert!(!raw.contains(canary));
        assert!(!raw.contains("credentials"));

        let target_path = temp_path("import-target");
        let target = ConfigStore::load(target_path, MemoryVault::new()).unwrap();
        let imported = target.import_from(&export_path).unwrap();
        assert_eq!(imported.asr_profiles.len(), 1);
        assert!(imported.asr_profiles[0].credential_id.is_none());
        assert!(target.status().unwrap().asr.state == ReadinessState::MissingKey);
    }

    #[test]
    fn v2_shared_credentials_are_split_between_profiles() {
        let path = temp_path("v2-migration");
        std::fs::write(
            &path,
            serde_json::json!({
                "schema_version": 2,
                "asr_profiles": [{ "id": "a1", "label": "ASR", "credential_id": "shared" }],
                "llm_profiles": [{ "id": "l1", "label": "LLM", "credential_id": "shared" }],
                "credentials": [{ "id": "shared", "label": "Shared", "role": "both", "hint": "1234" }]
            })
            .to_string(),
        )
        .unwrap();
        let vault = MemoryVault::new();
        vault.store("shared", "shared-secret").unwrap();
        let store = ConfigStore::load(path, vault.clone()).unwrap();
        let settings = store.snapshot().unwrap();
        assert_eq!(settings.schema_version, 3);
        let asr_id = settings.asr_profiles[0].credential_id.clone().unwrap();
        let llm_id = settings.llm_profiles[0].credential_id.clone().unwrap();
        assert_ne!(asr_id, llm_id);
        assert_eq!(
            vault.load(&asr_id).unwrap().as_deref(),
            Some("shared-secret")
        );
        assert_eq!(
            vault.load(&llm_id).unwrap().as_deref(),
            Some("shared-secret")
        );
    }

    #[test]
    fn legacy_settings_migrate_once_without_deleting_old_keys() {
        let path = temp_path("legacy-migration");
        std::fs::write(
            &path,
            serde_json::json!({
                "asr": {
                    "engine": "unisound_u2",
                    "base_url": "https://gateway.example/v1",
                    "model": "u2-asr",
                    "extra_options": []
                },
                "llm": {
                    "engine": "custom",
                    "format": "open_ai_chat",
                    "base_url": "https://llm.example/v1",
                    "model": "test-model",
                    "max_tokens": 512,
                    "temperature": 0.1
                }
            })
            .to_string(),
        )
        .unwrap();
        let vault = MemoryVault::new();
        vault.store("asr", "asr-canary").unwrap();
        vault.store("llm", "llm-canary").unwrap();
        let store = ConfigStore::load(path.clone(), vault.clone()).unwrap();
        let settings = store.snapshot().unwrap();
        assert_eq!(settings.schema_version, 3);
        assert_eq!(
            settings.active_asr_profile_id.as_deref(),
            Some("legacy-asr-profile")
        );
        assert_eq!(
            settings.asr_profiles[0].base_url.as_deref(),
            Some("https://gateway.example/v1")
        );
        assert_eq!(vault.load("asr").unwrap().as_deref(), Some("asr-canary"));
        assert_eq!(
            vault.load("legacy-asr-credential").unwrap().as_deref(),
            Some("asr-canary")
        );
        assert!(path.with_file_name("settings.v1.backup.json").exists());

        let reloaded = ConfigStore::load(path, vault).unwrap().snapshot().unwrap();
        assert_eq!(reloaded.asr_profiles[0].id, settings.asr_profiles[0].id);
    }

    #[test]
    fn invalid_profile_commit_rolls_back_memory() {
        let path = temp_path("rollback");
        let store = ConfigStore::load(path, MemoryVault::new()).unwrap();
        let profile = AsrProfile {
            id: "p1".into(),
            label: "ASR".into(),
            ..AsrProfile::default()
        };
        store
            .save_asr_profile(profile.clone(), Some("secret".into()), false)
            .unwrap();
        let invalid = AsrProfile {
            label: String::new(),
            ..profile
        };
        assert!(store.save_asr_profile(invalid, None, false).is_err());
        assert_eq!(store.snapshot().unwrap().asr_profiles[0].label, "ASR");
    }

    #[test]
    fn first_saved_profile_auto_activates_without_stealing_existing_active() {
        let path = temp_path("auto-activate");
        let store = ConfigStore::load(path, MemoryVault::new()).unwrap();
        let first = store
            .save_asr_profile(
                AsrProfile {
                    id: "p1".into(),
                    label: "First".into(),
                    ..AsrProfile::default()
                },
                Some("secret-one".into()),
                false,
            )
            .unwrap();
        assert_eq!(first.active_asr_profile_id.as_deref(), Some("p1"));

        let second = store
            .save_asr_profile(
                AsrProfile {
                    id: "p2".into(),
                    label: "Second".into(),
                    ..AsrProfile::default()
                },
                Some("secret-two".into()),
                false,
            )
            .unwrap();
        assert_eq!(second.active_asr_profile_id.as_deref(), Some("p1"));
    }

    #[test]
    fn invalid_v3_settings_are_rejected_without_overwriting_file() {
        let path = temp_path("invalid-v3");
        let raw = serde_json::json!({
            "schema_version": 3,
            "asr_profiles": [{ "id": "", "label": "Broken" }]
        })
        .to_string();
        std::fs::write(&path, &raw).unwrap();
        assert!(ConfigStore::load(path.clone(), MemoryVault::new()).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), raw);
    }
}
