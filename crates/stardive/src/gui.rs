use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use eframe::{App, Frame, NativeOptions, egui};
use reqwest::blocking::multipart::{Form, Part};
use serde_json::Value;
use stardive_core::types::FileListResponse;

pub fn run_file_gui(base_url: String, api_key: Option<String>) -> Result<()> {
    let options = NativeOptions::default();
    eframe::run_native(
        "Stardive File Manager",
        options,
        Box::new(|_cc| Ok(Box::new(FileGuiApp::new(base_url, api_key)))),
    )
    .map_err(|err| anyhow!("failed to launch gui: {err}"))?;
    Ok(())
}

struct FileGuiApp {
    base_url: String,
    api_key: Option<String>,
    pending_files: Vec<PathBuf>,
    files: Vec<String>,
    status: String,
    download_target: String,
}

impl FileGuiApp {
    fn new(base_url: String, api_key: Option<String>) -> Self {
        Self {
            base_url,
            api_key,
            pending_files: Vec::new(),
            files: Vec::new(),
            status: "Drop files into this window, then click upload".to_string(),
            download_target: ".".to_string(),
        }
    }

    fn auth_client(&self) -> Result<reqwest::blocking::Client> {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(key) = &self.api_key {
            headers.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("Bearer {key}"))?,
            );
        }
        reqwest::blocking::Client::builder()
            .default_headers(headers)
            .build()
            .context("failed to build gui http client")
    }

    fn refresh_files(&mut self) -> Result<()> {
        let client = self.auth_client()?;
        let url = format!("{}/v1/files", self.base_url.trim_end_matches('/'));
        let response = client
            .get(url)
            .send()
            .context("failed to fetch file list")?;
        if !response.status().is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!("list request failed: {}", body));
        }
        let list = response
            .json::<FileListResponse>()
            .context("invalid file list response")?;
        self.files = list
            .files
            .iter()
            .map(|f| format!("{} ({}, {} bytes)", f.id, f.original_name, f.size))
            .collect();
        Ok(())
    }

    fn upload_pending(&mut self) -> Result<()> {
        let client = self.auth_client()?;
        let url = format!("{}/v1/files", self.base_url.trim_end_matches('/'));

        let files = std::mem::take(&mut self.pending_files);
        for path in files {
            let name = path
                .file_name()
                .map(|v| v.to_string_lossy().to_string())
                .ok_or_else(|| anyhow!("invalid path"))?;
            let bytes = std::fs::read(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let part = Part::bytes(bytes).file_name(name);
            let form = Form::new().part("file", part);

            let resp = client
                .post(&url)
                .multipart(form)
                .send()
                .context("upload request failed")?;
            if !resp.status().is_success() {
                let body = resp.text().unwrap_or_default();
                return Err(anyhow!("upload failed: {}", body));
            }
        }

        Ok(())
    }

    fn download_file(&mut self, id: &str) -> Result<()> {
        let client = self.auth_client()?;
        let url = format!("{}/v1/files/{}", self.base_url.trim_end_matches('/'), id);
        let resp = client.get(url).send().context("download request failed")?;
        if !resp.status().is_success() {
            let body = resp.text().unwrap_or_default();
            return Err(anyhow!("download failed: {}", body));
        }
        let bytes = resp.bytes().context("failed to read download")?;
        let target = PathBuf::from(&self.download_target).join(id);
        std::fs::write(&target, bytes)
            .with_context(|| format!("failed to write {}", target.display()))?;
        Ok(())
    }
}

impl App for FileGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in dropped {
            if let Some(path) = file.path {
                self.pending_files.push(path);
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Stardive File Manager");
            ui.label("Drag and drop files into the window, then upload.");

            ui.horizontal(|ui| {
                if ui.button("Refresh").clicked() {
                    match self.refresh_files() {
                        Ok(_) => self.status = "file list refreshed".to_string(),
                        Err(err) => self.status = err.to_string(),
                    }
                }

                if ui.button("Upload Pending").clicked() {
                    match self.upload_pending() {
                        Ok(_) => self.status = "upload complete".to_string(),
                        Err(err) => self.status = err.to_string(),
                    }
                }
            });

            ui.separator();
            ui.label("Pending uploads:");
            for path in &self.pending_files {
                ui.label(path.display().to_string());
            }

            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Download target dir:");
                ui.text_edit_singleline(&mut self.download_target);
            });

            ui.separator();
            ui.label("Remote files:");
            for line in self.files.clone() {
                ui.horizontal(|ui| {
                    ui.label(&line);
                    if ui.button("Download").clicked() {
                        let id = line
                            .split_whitespace()
                            .next()
                            .unwrap_or_default()
                            .to_string();
                        match self.download_file(&id) {
                            Ok(_) => self.status = format!("downloaded {}", id),
                            Err(err) => self.status = err.to_string(),
                        }
                    }
                });
            }

            ui.separator();
            ui.label(format!("Status: {}", self.status));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gui_app_initializes() {
        let app = FileGuiApp::new("https://api.stardive.space".to_string(), None);
        assert!(app.pending_files.is_empty());
    }

    #[test]
    fn parse_id_from_label_format() {
        let value = "abc123 (name.txt, 10 bytes)";
        let parsed = value.split_whitespace().next().unwrap_or_default();
        assert_eq!(parsed, "abc123");
    }

    #[test]
    fn value_type_is_available_for_future_gui_extensions() {
        let _v = Value::Null;
    }
}
