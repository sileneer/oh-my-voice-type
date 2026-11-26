use crate::keys::CredentialStore;
use ovt_core::config::{
    AsrProfile, CredentialMetadata, CredentialRole, LegacySettings, LlmProfile, Settings,
};
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
    pub credentials: Vec<CredentialMetadata>,
    pub active_asr_profile_id: Option<String>,
    pub active_llm_profile_id: Option<String>,
}

pub struct ConfigStore {
    path: PathBuf,
    settings: Mutex<Settings>,
    vault: Arc<dyn CredentialStore>,
}

fn lock(settings: &Mutex<Settings>) -> Result<std::sync::MutexGuard<'_, Settings>, String> {
    settings.lock().map_err(|_| "settings lock poisoned".into())
}

fn now_ms() -> u64 {
    ovt_core::dictation::unix_now_ms()
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
                        if secret.is_empty() {
                            continue;
                        }
                        vault.store(new_id, secret).map_err(|e| e.to_string())?;
                        let timestamp = now_ms();
                        if let Some(metadata) = migrated
                            .credentials
                            .iter_mut()
                            .find(|credential| credential.id == new_id)
                        {
                            metadata.hint = key_hint(secret);
                            metadata.created_at_ms = timestamp;
                            metadata.updated_at_ms = timestamp;
                        }
                    }
                }
                save_path(&path, &migrated)?;
                migrated
            }
            Some(version) if version.as_u64() == Some(2) => {
                serde_json::from_value(value).map_err(|e| format!("invalid settings: {e}"))?
            }
            Some(version) => {
                return Err(format!(
                    "unsupported settings schema {}; this app supports version 2",
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
        profile: AsrProfile,
        confirm_host_change: bool,
    ) -> Result<Settings, String> {
        let mut settings = lock(&self.settings)?;
        let previous = settings.clone();
        let mut profile = profile;
        if profile.id.is_empty() {
            profile.id = uuid::Uuid::new_v4().to_string();
        }
        if let Some(existing) = settings
            .asr_profiles
            .iter()
            .find(|item| item.id == profile.id)
        {
            let host_changed = endpoint_host(existing.base_url.as_deref())
                != endpoint_host(profile.base_url.as_deref());
            if host_changed
                && (existing.credential_id.is_some() || profile.credential_id.is_some())
                && !confirm_host_change
            {
                return Err(
                    "endpoint host changed; confirm sending the bound key to the new host".into(),
                );
            }
        }
        if let Some(index) = settings
            .asr_profiles
            .iter()
            .position(|item| item.id == profile.id)
        {
            settings.asr_profiles[index] = profile;
        } else {
            settings.asr_profiles.push(profile);
        }
        self.commit_locked(&mut settings, previous)
    }

    pub fn save_llm_profile(
        &self,
        profile: LlmProfile,
        confirm_host_change: bool,
    ) -> Result<Settings, String> {
        let mut settings = lock(&self.settings)?;
        let previous = settings.clone();
        let mut profile = profile;
        if profile.id.is_empty() {
            profile.id = uuid::Uuid::new_v4().to_string();
        }
        if let Some(existing) = settings
            .llm_profiles
            .iter()
            .find(|item| item.id == profile.id)
        {
            let host_changed = endpoint_host(existing.base_url.as_deref())
                != endpoint_host(profile.base_url.as_deref());
            if host_changed
                && (existing.credential_id.is_some() || profile.credential_id.is_some())
                && !confirm_host_change
            {
                return Err(
                    "endpoint host changed; confirm sending the bound key to the new host".into(),
                );
            }
        }
        if let Some(index) = settings
            .llm_profiles
            .iter()
            .position(|item| item.id == profile.id)
        {
            settings.llm_profiles[index] = profile;
        } else {
            settings.llm_profiles.push(profile);
        }
        self.commit_locked(&mut settings, previous)
    }

    pub fn delete_profile(&self, role: ProfileRole, id: String) -> Result<Settings, String> {
        let mut settings = lock(&self.settings)?;
        let previous = settings.clone();
        match role {
            ProfileRole::Asr => {
                settings.asr_profiles.retain(|profile| profile.id != id);
                if settings.active_asr_profile_id.as_deref() == Some(&id) {
                    settings.active_asr_profile_id = None;
                }
            }
            ProfileRole::Llm => {
                settings.llm_profiles.retain(|profile| profile.id != id);
                if settings.active_llm_profile_id.as_deref() == Some(&id) {
                    settings.active_llm_profile_id = None;
                }
            }
        }
        self.commit_locked(&mut settings, previous)
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

    pub fn create_credential(
        &self,
        label: String,
        role: CredentialRole,
        secret: String,
    ) -> Result<Settings, String> {
        let secret = secret.trim();
        if secret.is_empty() {
            return Err("API key must not be empty".into());
        }
        if label.trim().is_empty() {
            return Err("credential label must not be empty".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let timestamp = now_ms();
        let metadata = CredentialMetadata {
            id: id.clone(),
            label: label.trim().to_string(),
            role,
            hint: key_hint(secret),
            created_at_ms: timestamp,
            updated_at_ms: timestamp,
        };
        self.vault.store(&id, secret).map_err(|e| e.to_string())?;
        let mut settings = lock(&self.settings)?;
        let previous = settings.clone();
        settings.credentials.push(metadata);
        if let Err(error) = self.save_locked(&settings) {
            *settings = previous;
            if let Err(cleanup) = self.vault.delete(&id) {
                return Err(format!("{error}; cleanup also failed: {cleanup}"));
            }
            return Err(error);
        }
        Ok(settings.clone())
    }

    pub fn rename_credential(&self, id: String, label: String) -> Result<Settings, String> {
        if label.trim().is_empty() {
            return Err("credential label must not be empty".into());
        }
        let mut settings = lock(&self.settings)?;
        let previous = settings.clone();
        let credential = settings
            .credentials
            .iter_mut()
            .find(|credential| credential.id == id)
            .ok_or("credential not found")?;
        credential.label = label.trim().to_string();
        credential.updated_at_ms = now_ms();
        self.commit_locked(&mut settings, previous)
    }

    pub fn delete_credential(&self, id: String) -> Result<Settings, String> {
        let mut settings = lock(&self.settings)?;
        let previous = settings.clone();
        if !settings
            .credentials
            .iter()
            .any(|credential| credential.id == id)
        {
            return Err("credential not found".into());
        }
        if settings
            .asr_profiles
            .iter()
            .any(|profile| profile.credential_id.as_deref() == Some(&id))
            || settings
                .llm_profiles
                .iter()
                .any(|profile| profile.credential_id.as_deref() == Some(&id))
        {
            return Err("credential is still bound to a profile; unbind it first".into());
        }
        self.vault.delete(&id).map_err(|e| e.to_string())?;
        settings
            .credentials
            .retain(|credential| credential.id != id);
        self.commit_locked(&mut settings, previous)
    }

    pub fn replace_credential(
        &self,
        id: String,
        secret: String,
        profile_id: Option<String>,
        replace_all: bool,
    ) -> Result<Settings, String> {
        let secret = secret.trim();
        if secret.is_empty() {
            return Err("API key must not be empty".into());
        }
        let mut settings = lock(&self.settings)?;
        let previous = settings.clone();
        let old = settings
            .credentials
            .iter()
            .find(|credential| credential.id == id)
            .cloned()
            .ok_or("credential not found")?;
        if !replace_all {
            let profile_id = profile_id
                .clone()
                .ok_or("profile_id is required when not replacing all")?;
            let bound = settings.asr_profiles.iter().any(|profile| {
                profile.id == profile_id && profile.credential_id.as_deref() == Some(&id)
            }) || settings.llm_profiles.iter().any(|profile| {
                profile.id == profile_id && profile.credential_id.as_deref() == Some(&id)
            });
            if !bound {
                return Err("selected profile does not use this credential".into());
            }
        }
        let new_id = uuid::Uuid::new_v4().to_string();
        let timestamp = now_ms();
        let mut metadata = old;
        metadata.id = new_id.clone();
        metadata.hint = key_hint(secret);
        metadata.updated_at_ms = timestamp;
        self.vault
            .store(&new_id, secret)
            .map_err(|e| e.to_string())?;
        for profile in settings.asr_profiles.iter_mut() {
            if profile.credential_id.as_deref() == Some(&id)
                && (replace_all || profile_id.as_deref() == Some(profile.id.as_str()))
            {
                profile.credential_id = Some(new_id.clone());
            }
        }
        for profile in settings.llm_profiles.iter_mut() {
            if profile.credential_id.as_deref() == Some(&id)
                && (replace_all || profile_id.as_deref() == Some(profile.id.as_str()))
            {
                profile.credential_id = Some(new_id.clone());
            }
        }
        settings.credentials.push(metadata);
        if let Err(error) = self.save_locked(&settings) {
            *settings = previous;
            if let Err(cleanup) = self.vault.delete(&new_id) {
                return Err(format!("{error}; cleanup also failed: {cleanup}"));
            }
            return Err(error);
        }
        Ok(settings.clone())
    }

    pub fn cleanup_legacy_keys(&self) -> Result<Vec<String>, String> {
        let settings = lock(&self.settings)?;
        let mut cleaned = Vec::new();
        for (old_id, new_id) in [
            ("asr", "legacy-asr-credential"),
            ("llm", "legacy-llm-credential"),
        ] {
            if !settings
                .credentials
                .iter()
                .any(|credential| credential.id == new_id)
            {
                continue;
            }
            if self
                .vault
                .load(new_id)
                .map_err(|e| e.to_string())?
                .is_some()
            {
                self.vault.delete(old_id).map_err(|e| e.to_string())?;
                cleaned.push(old_id.to_string());
            }
        }
        Ok(cleaned)
    }

    pub fn export_data(&self) -> Result<ConfigExport, String> {
        let settings = lock(&self.settings)?;
        settings.validate().map_err(|e| e.to_string())?;
        Ok(ConfigExport {
            kind: "oh-my-voice-type/config".into(),
            version: 1,
            asr_profiles: settings.asr_profiles.clone(),
            llm_profiles: settings.llm_profiles.clone(),
            credentials: settings.credentials.clone(),
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
        if data.kind != "oh-my-voice-type/config" || data.version != 1 {
            return Err("unsupported config export format".into());
        }
        let timestamp = now_ms();
        let mut credential_ids = HashMap::new();
        for credential in &mut data.credentials {
            let new_id = uuid::Uuid::new_v4().to_string();
            credential_ids.insert(credential.id.clone(), new_id.clone());
            credential.id = new_id;
            credential.hint = None;
            credential.created_at_ms = timestamp;
            credential.updated_at_ms = timestamp;
        }
        let mut profile_ids = HashMap::new();
        for profile in data.asr_profiles.iter_mut() {
            let new_id = uuid::Uuid::new_v4().to_string();
            profile_ids.insert(profile.id.clone(), new_id.clone());
            profile.id = new_id;
            profile.credential_id = profile
                .credential_id
                .as_ref()
                .map(|id| {
                    credential_ids
                        .get(id)
                        .cloned()
                        .ok_or("invalid credential reference")
                })
                .transpose()?;
        }
        for profile in data.llm_profiles.iter_mut() {
            let new_id = uuid::Uuid::new_v4().to_string();
            profile_ids.insert(profile.id.clone(), new_id.clone());
            profile.id = new_id;
            profile.credential_id = profile
                .credential_id
                .as_ref()
                .map(|id| {
                    credential_ids
                        .get(id)
                        .cloned()
                        .ok_or("invalid credential reference")
                })
                .transpose()?;
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
        settings.credentials.extend(data.credentials);
        if settings.active_asr_profile_id.is_none() {
            settings.active_asr_profile_id = active_asr_profile_id;
        }
        if settings.active_llm_profile_id.is_none() {
            settings.active_llm_profile_id = active_llm_profile_id;
        }
        self.commit_locked(&mut settings, previous)
    }
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
        std::env::temp_dir().join(format!(
            "ovt-config-test-{}-{name}.json",
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn failed_credential_write_does_not_change_settings() {
        let path = temp_path("write-failure");
        let vault = Arc::new(MemoryVault {
            values: Mutex::new(BTreeMap::new()),
            fail_writes: true,
        });
        let store = ConfigStore::load(path, vault).unwrap();
        assert!(store
            .create_credential("Test".into(), CredentialRole::Asr, "secret".into())
            .is_err());
        assert!(store.snapshot().unwrap().credentials.is_empty());
    }

    #[test]
    fn bound_credential_cannot_be_deleted() {
        let path = temp_path("delete-bound");
        let vault = MemoryVault::new();
        let store = ConfigStore::load(path, vault).unwrap();
        let settings = store
            .create_credential("Test".into(), CredentialRole::Asr, "secret".into())
            .unwrap();
        let id = settings.credentials[0].id.clone();
        let profile = AsrProfile {
            id: "p1".into(),
            label: "ASR".into(),
            credential_id: Some(id.clone()),
            ..AsrProfile::default()
        };
        store.save_asr_profile(profile, false).unwrap();
        assert!(store.delete_credential(id).is_err());
    }

    #[test]
    fn export_contains_no_secret_and_import_requires_reentry() {
        let path = temp_path("export-source");
        let vault = MemoryVault::new();
        let store = ConfigStore::load(path, vault).unwrap();
        let canary = "sk-canary-do-not-export";
        let settings = store
            .create_credential("Test".into(), CredentialRole::Asr, canary.into())
            .unwrap();
        let id = settings.credentials[0].id.clone();
        store
            .save_asr_profile(
                AsrProfile {
                    id: "p1".into(),
                    label: "ASR".into(),
                    credential_id: Some(id),
                    ..AsrProfile::default()
                },
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

        let target_path = temp_path("import-target");
        let target = ConfigStore::load(target_path, MemoryVault::new()).unwrap();
        let imported = target.import_from(&export_path).unwrap();
        assert_eq!(imported.asr_profiles.len(), 1);
        assert!(target.status().unwrap().asr.state == ReadinessState::MissingKey);
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
        assert_eq!(settings.schema_version, 2);
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
        assert_eq!(reloaded.credentials.len(), settings.credentials.len());
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
        store.save_asr_profile(profile.clone(), false).unwrap();
        let invalid = AsrProfile {
            label: String::new(),
            ..profile
        };
        assert!(store.save_asr_profile(invalid, false).is_err());
        assert_eq!(store.snapshot().unwrap().asr_profiles[0].label, "ASR");
    }

    #[test]
    fn invalid_v2_settings_are_rejected_without_overwriting_file() {
        let path = temp_path("invalid-v2");
        let raw = serde_json::json!({
            "schema_version": 2,
            "asr_profiles": [{ "id": "", "label": "Broken" }]
        })
        .to_string();
        std::fs::write(&path, &raw).unwrap();
        assert!(ConfigStore::load(path.clone(), MemoryVault::new()).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), raw);
    }
}
