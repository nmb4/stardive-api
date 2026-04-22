mod gui;

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand, ValueEnum};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use stardive_core::{
    client::StardiveClient,
    config::resolve_cli_config,
    types::{
        ExtractRequest, ExtractResponse, FileListResponse, HealthResponse, RenderFormat,
        RenderSnippetRequest, SearchRequest, SearchResponse, UploadResponse,
    },
};

#[derive(Debug, Parser)]
#[command(name = "stardive")]
#[command(about = "Stardive API companion CLI")]
struct Cli {
    #[arg(long)]
    base_url: Option<String>,
    #[arg(long)]
    api_key: Option<String>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Api {
        #[command(subcommand)]
        command: ApiCommands,
    },
    Search {
        #[command(subcommand)]
        command: SearchCommands,
    },
    Extract(ExtractArgs),
    File {
        #[command(subcommand)]
        command: FileCommands,
    },
    Render {
        #[command(subcommand)]
        command: RenderCommands,
    },
}

#[derive(Debug, Subcommand)]
enum ApiCommands {
    Health,
}

#[derive(Debug, Subcommand)]
enum SearchCommands {
    Text(SearchArgs),
    News(SearchArgs),
}

#[derive(Debug, clap::Args)]
struct SearchArgs {
    #[arg(long)]
    query: String,
    #[arg(long)]
    region: Option<String>,
    #[arg(long)]
    safesearch: Option<String>,
    #[arg(long)]
    timelimit: Option<String>,
    #[arg(long)]
    max_results: Option<u32>,
}

#[derive(Debug, clap::Args)]
struct ExtractArgs {
    #[arg(long)]
    url: String,
    #[arg(long)]
    format: Option<String>,
}

#[derive(Debug, Subcommand)]
enum FileCommands {
    Upload {
        path: PathBuf,
    },
    List,
    Download {
        id: String,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    Gui,
}

#[derive(Debug, Subcommand)]
enum RenderCommands {
    Snippet {
        #[arg(long)]
        code: String,
        #[arg(long)]
        language: Option<String>,
        #[arg(long)]
        theme: Option<String>,
        #[arg(long, value_enum)]
        format: RenderFormatArg,
        #[arg(short, long)]
        output: PathBuf,
    },
}

#[derive(Debug, Clone, ValueEnum)]
enum RenderFormatArg {
    Svg,
    Png,
}

impl From<RenderFormatArg> for RenderFormat {
    fn from(value: RenderFormatArg) -> Self {
        match value {
            RenderFormatArg::Svg => RenderFormat::Svg,
            RenderFormatArg::Png => RenderFormat::Png,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = resolve_cli_config(cli.base_url, cli.api_key)?;
    let api = StardiveClient::new(cfg.base_url.clone(), cfg.api_key.clone());

    match cli.command {
        Commands::Api { command } => match command {
            ApiCommands::Health => {
                let health = api.get_json::<HealthResponse>("/v1/health").await?;
                print_json(&health)?;
            }
        },
        Commands::Search { command } => match command {
            SearchCommands::Text(args) => {
                let req = SearchRequest {
                    query: args.query,
                    region: args.region,
                    safesearch: args.safesearch,
                    timelimit: args.timelimit,
                    max_results: args.max_results,
                };
                let response = api
                    .post_json::<_, SearchResponse>("/v1/search/text", &req)
                    .await?;
                print_json(&response)?;
            }
            SearchCommands::News(args) => {
                let req = SearchRequest {
                    query: args.query,
                    region: args.region,
                    safesearch: args.safesearch,
                    timelimit: args.timelimit,
                    max_results: args.max_results,
                };
                let response = api
                    .post_json::<_, SearchResponse>("/v1/search/news", &req)
                    .await?;
                print_json(&response)?;
            }
        },
        Commands::Extract(args) => {
            let req = ExtractRequest {
                url: args.url,
                format: args.format,
            };
            let response = api
                .post_json::<_, ExtractResponse>("/v1/extract", &req)
                .await?;
            print_json(&response)?;
        }
        Commands::File { command } => match command {
            FileCommands::Upload { path } => {
                let uploaded = upload_file(&cfg.base_url, cfg.api_key.as_deref(), path).await?;
                print_json(&uploaded)?;
            }
            FileCommands::List => {
                let list = api.get_json::<FileListResponse>("/v1/files").await?;
                print_json(&list)?;
            }
            FileCommands::Download { id, output } => {
                let path =
                    download_file(&cfg.base_url, cfg.api_key.as_deref(), &id, output).await?;
                println!("saved to {}", path.display());
            }
            FileCommands::Gui => {
                gui::run_file_gui(cfg.base_url, cfg.api_key)?;
            }
        },
        Commands::Render { command } => match command {
            RenderCommands::Snippet {
                code,
                language,
                theme,
                format,
                output,
            } => {
                let request = RenderSnippetRequest {
                    code,
                    language,
                    theme,
                    format: format.into(),
                };

                let (bytes, _content_type) =
                    api.post_json_bytes("/v1/render/snippet", &request).await?;
                tokio::fs::write(&output, bytes)
                    .await
                    .with_context(|| format!("failed to write {}", output.display()))?;
                println!("saved to {}", output.display());
            }
        },
    }

    Ok(())
}

async fn upload_file(
    base_url: &str,
    api_key: Option<&str>,
    path: PathBuf,
) -> Result<UploadResponse> {
    let bytes = tokio::fs::read(&path)
        .await
        .with_context(|| format!("failed to read {}", path.display()))?;
    let file_name = path
        .file_name()
        .map(|v| v.to_string_lossy().to_string())
        .ok_or_else(|| anyhow!("invalid file path"))?;

    let part = reqwest::multipart::Part::bytes(bytes).file_name(file_name);
    let form = reqwest::multipart::Form::new().part("file", part);

    let client = reqwest::Client::new();
    let mut req = client
        .post(format!("{}/v1/files", base_url.trim_end_matches('/')))
        .multipart(form);
    if let Some(key) = api_key {
        req = req.bearer_auth(key);
    }

    let resp = req.send().await.context("upload request failed")?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("upload failed: {}", body));
    }

    resp.json::<UploadResponse>()
        .await
        .context("invalid upload response")
}

async fn download_file(
    base_url: &str,
    api_key: Option<&str>,
    id: &str,
    output: Option<PathBuf>,
) -> Result<PathBuf> {
    let client = reqwest::Client::new();
    let mut req = client.get(format!(
        "{}/v1/files/{}",
        base_url.trim_end_matches('/'),
        id
    ));
    if let Some(key) = api_key {
        req = req.bearer_auth(key);
    }
    let resp = req.send().await.context("download request failed")?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("download failed: {}", body));
    }

    let bytes = resp
        .bytes()
        .await
        .context("failed to read download bytes")?;
    let out_path = output.unwrap_or_else(|| PathBuf::from(id));
    tokio::fs::write(&out_path, bytes)
        .await
        .with_context(|| format!("failed to write {}", out_path.display()))?;
    Ok(out_path)
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    let pretty = serde_json::to_string_pretty(value).context("failed to serialize json")?;
    println!("{}", pretty);
    Ok(())
}

pub fn auth_header_map(api_key: Option<&str>) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    if let Some(key) = api_key {
        let value = format!("Bearer {key}");
        headers.insert(AUTHORIZATION, HeaderValue::from_str(&value)?);
    }
    Ok(headers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, FromArgMatches};

    #[test]
    fn cli_parses_search_text_command() {
        let command = Cli::command();
        let matches = command
            .try_get_matches_from(["stardive", "search", "text", "--query", "rust"])
            .expect("parse");
        let parsed = Cli::from_arg_matches(&matches).expect("from matches");

        match parsed.command {
            Commands::Search {
                command: SearchCommands::Text(args),
            } => {
                assert_eq!(args.query, "rust");
            }
            _ => panic!("unexpected command parsed"),
        }
    }

    #[test]
    fn auth_header_includes_bearer() {
        let headers = auth_header_map(Some("secret")).expect("headers");
        let auth = headers.get(AUTHORIZATION).expect("auth header");
        assert_eq!(auth.to_str().expect("str"), "Bearer secret");
    }
}
