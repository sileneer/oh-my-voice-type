//! Personal dictionary persistence. The file is atomically replaced; malformed
//! existing data is surfaced without being overwritten or silently cleared.

use ovt_core::dictionary::Dictionary;
use ovt_core::Result;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const MAX_DICTIONARY_TERMS: usize = 1000;
pub const MAX_TERM_CHARS: usize = 200;

pub struct DictionaryStore {
    path: PathBuf,
    dictionary: Mutex<Dictionary>,
}

impl DictionaryStore {
    pub fn load(path: PathBuf) -> Result<Self> {
        let dictionary = if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .map_err(|error| ovt_core::Error::Config(format!("read dictionary: {error}")))?;
            serde_json::from_str::<Dictionary>(&raw).map_err(|error| {
                ovt_core::Error::Config(format!(
                    "dictionary.json is invalid and was kept unchanged: {error}"
                ))
            })?
        } else {
            Dictionary::default()
        };
        Ok(Self {
            path,
            dictionary: Mutex::new(dictionary),
        })
    }

    pub fn snapshot(&self) -> Dictionary {
        self.dictionary
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn add(&self, term: &str) -> Result<Dictionary> {
        let term = validate_term(term)?;
        let mut dictionary = self.lock();
        if !dictionary
            .entries
            .iter()
            .any(|entry| entry.term.eq_ignore_ascii_case(term))
            && dictionary.entries.len() >= MAX_DICTIONARY_TERMS
        {
            return Err(ovt_core::Error::Config(format!(
                "dictionary capacity is {MAX_DICTIONARY_TERMS} terms"
            )));
        }
        dictionary.add_term(term);
        self.commit(&dictionary)?;
        Ok(dictionary.clone())
    }

    pub fn rename(&self, from: &str, to: &str) -> Result<Dictionary> {
        let to = validate_term(to)?;
        let mut dictionary = self.lock();
        if !dictionary.rename_term(from, to) {
            return Err(ovt_core::Error::Config(
                "term was not found or the new spelling already exists".into(),
            ));
        }
        self.commit(&dictionary)?;
        Ok(dictionary.clone())
    }

    pub fn delete(&self, term: &str) -> Result<Dictionary> {
        let mut dictionary = self.lock();
        if !dictionary.delete_term(term) {
            return Err(ovt_core::Error::Config(
                "dictionary term was not found".into(),
            ));
        }
        self.commit(&dictionary)?;
        Ok(dictionary.clone())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Dictionary> {
        self.dictionary
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn commit(&self, dictionary: &Dictionary) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                ovt_core::Error::Config(format!("create dictionary directory: {error}"))
            })?;
        }
        let json = serde_json::to_vec_pretty(dictionary)
            .map_err(|error| ovt_core::Error::Config(format!("encode dictionary: {error}")))?;
        let temporary = self.path.with_extension("json.tmp");
        write_and_sync(&temporary, &json)?;
        std::fs::rename(&temporary, &self.path).map_err(|error| {
            ovt_core::Error::Config(format!("commit dictionary atomically: {error}"))
        })?;
        Ok(())
    }
}

fn validate_term(term: &str) -> Result<&str> {
    let term = term.trim();
    if term.is_empty() || term.chars().count() > MAX_TERM_CHARS {
        return Err(ovt_core::Error::Config(format!(
            "dictionary terms must contain 1–{MAX_TERM_CHARS} characters"
        )));
    }
    Ok(term)
}

fn write_and_sync(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut file = std::fs::File::create(path)
        .map_err(|error| ovt_core::Error::Config(format!("write dictionary temp file: {error}")))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| ovt_core::Error::Config(format!("sync dictionary temp file: {error}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path() -> PathBuf {
        std::env::temp_dir().join(format!("ovt-dictionary-{}.json", uuid::Uuid::new_v4()))
    }

    #[test]
    fn crud_is_atomic_and_corruption_is_not_cleared() {
        let path = path();
        let store = DictionaryStore::load(path.clone()).unwrap();
        store.add("  Kubernetes  ").unwrap();
        store.rename("Kubernetes", "K8s").unwrap();
        assert_eq!(store.snapshot().entries[0].term, "K8s");
        assert!(store.delete("k8s").unwrap().entries.is_empty());
        drop(store);

        std::fs::write(&path, "{broken").unwrap();
        assert!(DictionaryStore::load(path.clone()).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{broken");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_empty_oversized_and_duplicate_rename() {
        let path = path();
        let store = DictionaryStore::load(path.clone()).unwrap();
        assert!(store.add(" ").is_err());
        assert!(store.add(&"x".repeat(201)).is_err());
        store.add("Alpha").unwrap();
        store.add("Beta").unwrap();
        assert!(store.rename("Alpha", "beta").is_err());
        assert_eq!(store.snapshot().entries.len(), 2);
        let _ = std::fs::remove_file(path);
    }
}
