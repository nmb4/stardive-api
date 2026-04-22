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
        let raw = serde_json::to_string_pretty(&*guard).context("failed to encode index json")?;

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
}
