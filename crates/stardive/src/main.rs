mod gui;

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

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
    Update,
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

    if let Commands::Update = cli.command {
        return run_update();
    }

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
        Commands::Update => unreachable!("handled before api client initialization"),
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum InstallMethod {
    CratesIo,
    Pkgx,
    ScriptOrUnknown,
}

impl InstallMethod {
    fn label(self) -> &'static str {
        match self {
            Self::CratesIo => "crates.io",
            Self::Pkgx => "pkgx",
            Self::ScriptOrUnknown => "script/unknown",
        }
    }
}

#[derive(Debug, Default)]
struct InstallDetection {
    crates_paths: BTreeSet<PathBuf>,
    pkgx_paths: BTreeSet<PathBuf>,
    script_paths: BTreeSet<PathBuf>,
    cargo_reports_install: bool,
    pkgx_reports_install: bool,
}

impl InstallDetection {
    fn add_path(&mut self, path: PathBuf, method: InstallMethod) {
        match method {
            InstallMethod::CratesIo => {
                self.crates_paths.insert(path);
            }
            InstallMethod::Pkgx => {
                self.pkgx_paths.insert(path);
            }
            InstallMethod::ScriptOrUnknown => {
                self.script_paths.insert(path);
            }
        }
    }

    fn available_methods(&self) -> BTreeSet<InstallMethod> {
        let mut methods = BTreeSet::new();
        if self.cargo_reports_install || !self.crates_paths.is_empty() {
            methods.insert(InstallMethod::CratesIo);
        }
        if self.pkgx_reports_install || !self.pkgx_paths.is_empty() {
            methods.insert(InstallMethod::Pkgx);
        }
        if !self.script_paths.is_empty() {
            methods.insert(InstallMethod::ScriptOrUnknown);
        }
        if methods.is_empty() {
            methods.insert(InstallMethod::ScriptOrUnknown);
        }
        methods
    }
}

fn run_update() -> Result<()> {
    let detection = detect_install_variants();
    print_detected_variants(&detection);

    let methods = detection.available_methods();
    let preferred = select_preferred_method(&methods);
    println!("selected update method: {}", preferred.label());

    match preferred {
        InstallMethod::CratesIo => update_via_crates_io(),
        InstallMethod::Pkgx => update_via_pkgx(&detection),
        InstallMethod::ScriptOrUnknown => update_via_script(),
    }
}

fn detect_install_variants() -> InstallDetection {
    let mut detection = InstallDetection::default();
    let mut candidates = BTreeSet::new();

    if let Ok(exe) = std::env::current_exe() {
        candidates.insert(exe);
    }

    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(binary_name());
            if candidate.exists() {
                candidates.insert(candidate);
            }
        }
    }

    if let Some(home) = user_home_dir() {
        candidates.insert(home.join(".cargo").join("bin").join(binary_name()));
        candidates.insert(home.join(".local").join("bin").join(binary_name()));
    }
    candidates.insert(PathBuf::from("/usr/local/bin").join(binary_name()));

    for candidate in candidates {
        if !candidate.exists() {
            continue;
        }
        let method = classify_install_method(&candidate);
        detection.add_path(candidate, method);
    }

    detection.cargo_reports_install = detect_cargo_installation();
    detection.pkgx_reports_install = detect_pkgx_installation();

    detection
}

fn print_detected_variants(detection: &InstallDetection) {
    if !detection.crates_paths.is_empty() {
        println!("detected crates.io installs:");
        for path in &detection.crates_paths {
            println!("  - {}", path.display());
        }
    }
    if !detection.pkgx_paths.is_empty() || detection.pkgx_reports_install {
        println!("detected pkgx installs:");
        for path in &detection.pkgx_paths {
            println!("  - {}", path.display());
        }
        if detection.pkgx_reports_install && detection.pkgx_paths.is_empty() {
            println!("  - reported by `pkgx pkgm list`");
        }
    }
    if !detection.script_paths.is_empty() {
        println!("detected script/unknown installs:");
        for path in &detection.script_paths {
            println!("  - {}", path.display());
        }
    }
}

fn select_preferred_method(methods: &BTreeSet<InstallMethod>) -> InstallMethod {
    if methods.contains(&InstallMethod::CratesIo) {
        return InstallMethod::CratesIo;
    }
    if methods.contains(&InstallMethod::Pkgx) {
        return InstallMethod::Pkgx;
    }
    InstallMethod::ScriptOrUnknown
}

fn classify_install_method(path: &Path) -> InstallMethod {
    if let Ok(canon) = std::fs::canonicalize(path) {
        let classified = classify_path_like(&canon);
        if classified != InstallMethod::ScriptOrUnknown {
            return classified;
        }
    }
    classify_path_like(path)
}

fn classify_path_like(path: &Path) -> InstallMethod {
    let s = path.to_string_lossy().to_lowercase();
    if s.contains("/.cargo/bin/") || s.contains("\\.cargo\\bin\\") {
        return InstallMethod::CratesIo;
    }
    if s.contains("/.pkgx/")
        || s.contains("\\.pkgx\\")
        || s.contains("/pkgm/")
        || s.contains("\\pkgm\\")
    {
        return InstallMethod::Pkgx;
    }
    InstallMethod::ScriptOrUnknown
}

fn detect_cargo_installation() -> bool {
    if !command_exists("cargo") {
        return false;
    }
    let output = Command::new("cargo")
        .args(["install", "--list"])
        .output()
        .ok();
    let Some(output) = output else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .any(|line| line.trim_start().starts_with("stardive v"))
}

fn detect_pkgx_installation() -> bool {
    if !command_exists("pkgx") {
        return false;
    }
    let output = Command::new("pkgx").args(["pkgm", "list"]).output().ok();
    let Some(output) = output else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("stardive ") || trimmed == "stardive"
    })
}

fn update_via_crates_io() -> Result<()> {
    if command_exists("cargo") {
        run_command_streaming("cargo", &["install", "--locked", "--force", "stardive"])?;
    } else {
        run_bash_command(
            "eval \"$(sh <(curl -fsS https://pkgx.sh) +rust-lang.org +curl.se)\" && cargo install --locked --force stardive",
        )?;
    }
    println!("update completed via crates.io");
    Ok(())
}

fn update_via_pkgx(detection: &InstallDetection) -> Result<()> {
    if !command_exists("pkgx") {
        println!("pkgx not available; falling back to script installer");
        return update_via_script();
    }

    let needs_sudo = detection
        .pkgx_paths
        .iter()
        .any(|path| path.starts_with("/usr/local/bin") || path.starts_with("/usr/bin"));

    if needs_sudo {
        if command_exists("sudo") {
            run_command_streaming("sudo", &["pkgx", "pkgm", "install", "stardive"])?;
        } else {
            run_command_streaming("pkgx", &["pkgm", "install", "stardive"])?;
        }
    } else {
        run_command_streaming("pkgx", &["pkgm", "install", "stardive"])?;
    }

    println!("update completed via pkgx");
    Ok(())
}

fn update_via_script() -> Result<()> {
    run_bash_command(
        "curl -fsSL https://raw.githubusercontent.com/nmb4/stardive-api/main/installers/install-stardive.sh | bash",
    )?;
    println!("update completed via script installer");
    Ok(())
}

fn run_bash_command(command: &str) -> Result<()> {
    run_command_streaming("bash", &["-lc", command])
}

fn run_command_streaming(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("failed to start `{program}`"))?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "`{}` exited with status {}",
            format!("{program} {}", args.join(" ")).trim(),
            status
        ))
    }
}

fn command_exists(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|path_var| {
            std::env::split_paths(&path_var).any(|dir| {
                let candidate = dir.join(name);
                candidate.exists()
            })
        })
        .unwrap_or(false)
}

fn user_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

fn binary_name() -> &'static str {
    if cfg!(windows) {
        "stardive.exe"
    } else {
        "stardive"
    }
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

    #[test]
    fn update_method_priority_prefers_crates() {
        let mut methods = BTreeSet::new();
        methods.insert(InstallMethod::ScriptOrUnknown);
        methods.insert(InstallMethod::Pkgx);
        methods.insert(InstallMethod::CratesIo);
        assert_eq!(select_preferred_method(&methods), InstallMethod::CratesIo);
    }

    #[test]
    fn update_method_priority_prefers_pkgx_over_script() {
        let mut methods = BTreeSet::new();
        methods.insert(InstallMethod::ScriptOrUnknown);
        methods.insert(InstallMethod::Pkgx);
        assert_eq!(select_preferred_method(&methods), InstallMethod::Pkgx);
    }

    #[test]
    fn classify_cargo_path_as_crates() {
        let method = classify_path_like(Path::new("/home/me/.cargo/bin/stardive"));
        assert_eq!(method, InstallMethod::CratesIo);
    }
}
