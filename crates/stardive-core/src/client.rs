use anyhow::{Context, Result, anyhow};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Serialize;
use serde::de::DeserializeOwned;

#[derive(Clone)]
pub struct StardiveClient {
    base_url: String,
    api_key: Option<String>,
    client: reqwest::Client,
}

impl StardiveClient {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key,
            client: reqwest::Client::new(),
        }
    }

    fn auth_headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        if let Some(key) = &self.api_key {
            let value = format!("Bearer {key}");
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&value).context("invalid api key header")?,
            );
        }
        Ok(headers)
    }

    pub async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .get(url)
            .headers(self.auth_headers()?)
            .send()
            .await
            .context("request failed")?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("request failed: {}", body));
        }

        resp.json::<T>().await.context("invalid json response")
    }

    pub async fn post_json<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .post(url)
            .headers(self.auth_headers()?)
            .json(body)
            .send()
            .await
            .context("request failed")?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("request failed: {}", body));
        }

        resp.json::<T>().await.context("invalid json response")
    }

    pub async fn post_json_bytes<B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<(Vec<u8>, String)> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .post(url)
            .headers(self.auth_headers()?)
            .json(body)
            .send()
            .await
            .context("request failed")?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("request failed: {}", body));
        }

        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let bytes = resp.bytes().await.context("failed to read response body")?;
        Ok((bytes.to_vec(), content_type))
    }

    pub fn blocking_client(&self) -> Result<reqwest::blocking::Client> {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(key) = &self.api_key {
            let value = format!("Bearer {key}");
            headers.insert(
                AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&value)?,
            );
        }
        reqwest::blocking::Client::builder()
            .default_headers(headers)
            .build()
            .context("failed to build blocking client")
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}
