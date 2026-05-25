use std::path::PathBuf;

use anyhow::{Context, Result};
use stardive_core::types::FileMetadata;
use tokio::{fs, sync::Mutex};

#[derive(Debug)]
pub struct FileStore {
    root_dir: PathBuf,
    index_path: PathBuf,
    metadata: Mutex<Vec<FileMetadata>>,
}

impl FileStore {
    pub async fn new(data_root: PathBuf) -> Result<Self> {
        let root_dir = data_root.join("files");
        fs::create_dir_all(&root_dir)
            .await
            .with_context(|| format!("failed to create file store dir {}", root_dir.display()))?;

        let index_path = root_dir.join("index.json");
        let existing = if index_path.exists() {
            let raw = fs::read_to_string(&index_path)
                .await
                .with_context(|| format!("failed to read index {}", index_path.display()))?;
            serde_json::from_str::<Vec<FileMetadata>>(&raw).context("invalid index json")?
        } else {
            Vec::new()
        };

        Ok(Self {
            root_dir,
            index_path,
            metadata: Mutex::new(existing),
        })
    }

    pub fn blob_path(&self, id: &str) -> PathBuf {
        self.root_dir.join(id)
    }

    pub async fn insert(&self, meta: FileMetadata) -> Result<()> {
        let mut guard = self.metadata.lock().await;
        guard.push(meta);
        guard.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        self.write_index(&guard).await?;
        Ok(())
    }

    pub async fn update(&self, id: &str, meta: FileMetadata) -> Result<Option<FileMetadata>> {
        let mut guard = self.metadata.lock().await;
        let Some(existing) = guard.iter_mut().find(|m| m.id == id) else {
            return Ok(None);
        };
        *existing = meta.clone();
        guard.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        self.write_index(&guard).await?;
        Ok(Some(meta))
    }

    pub async fn delete(&self, id: &str) -> Result<Option<FileMetadata>> {
        let mut guard = self.metadata.lock().await;
        let Some(index) = guard.iter().position(|m| m.id == id) else {
            return Ok(None);
        };
        let removed = guard.remove(index);
        self.write_index(&guard).await?;
        Ok(Some(removed))
    }

    pub async fn list(&self) -> Vec<FileMetadata> {
        self.metadata.lock().await.clone()
    }

    pub async fn get(&self, id: &str) -> Option<FileMetadata> {
        self.metadata
            .lock()
            .await
            .iter()
            .find(|m| m.id == id)
            .cloned()
    }

    async fn write_index(&self, metadata: &[FileMetadata]) -> Result<()> {
        let raw = serde_json::to_string_pretty(metadata).context("failed to encode index json")?;

        let tmp_path = self.index_path.with_extension("json.tmp");
        fs::write(&tmp_path, raw)
            .await
            .with_context(|| format!("failed to write {}", tmp_path.display()))?;
        fs::rename(&tmp_path, &self.index_path)
            .await
            .with_context(|| {
                format!(
                    "failed to move {} to {}",
                    tmp_path.display(),
                    self.index_path.display()
                )
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use tempfile::tempdir;

    use super::*;

    #[tokio::test]
    async fn writes_and_reads_metadata_index() {
        let tmp = tempdir().expect("tempdir");
        let store = FileStore::new(tmp.path().to_path_buf())
            .await
            .expect("store");
        let meta = FileMetadata {
            id: "abc".to_string(),
            original_name: "file.txt".to_string(),
            size: 7,
            mime_type: "text/plain".to_string(),
            sha256: "deadbeef".to_string(),
            created_at: Utc::now(),
        };
        store.insert(meta.clone()).await.expect("insert");

        let list = store.list().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, meta.id);
    }

    #[tokio::test]
    async fn updates_and_deletes_metadata_index() {
        let tmp = tempdir().expect("tempdir");
        let store = FileStore::new(tmp.path().to_path_buf())
            .await
            .expect("store");
        let meta = FileMetadata {
            id: "abc".to_string(),
            original_name: "file.txt".to_string(),
            size: 7,
            mime_type: "text/plain".to_string(),
            sha256: "deadbeef".to_string(),
            created_at: Utc::now(),
        };
        store.insert(meta.clone()).await.expect("insert");

        let updated = FileMetadata {
            original_name: "updated.bin".to_string(),
            size: 9,
            mime_type: "application/octet-stream".to_string(),
            sha256: "feedface".to_string(),
            ..meta
        };
        let result = store.update("abc", updated.clone()).await.expect("update");
        assert_eq!(result.expect("updated").original_name, "updated.bin");
        assert_eq!(store.get("abc").await.expect("stored").size, 9);

        let removed = store.delete("abc").await.expect("delete");
        assert_eq!(removed.expect("removed").id, "abc");
        assert!(store.get("abc").await.is_none());
    }
}
