//! `codefact` is a command-line program to do one thing and one thing only:
//! check if you're logged into AWS Code Artifact and get a fresh auth token if
//! you're not. It writes the token to several files to make it available to shells
//! when they're sourced, and to maven when maven does whatever horrors it does to
//! fetch packages.
//!
//! `codefact` at the moment takes no options other than environment variables.
//! It uses whatever aws profile you have set up as your default.

use std::env;
use std::io::Read as _;
use std::path::PathBuf;
use std::process::{Command, exit};

use aws_config::BehaviorVersion;
use regex::Captures;

const MAVEN_TMPL: &str = include_str!("../templates/maven.full.xml");

const FISH_FILE: &str = "codeartifact.fish";
const FISH_FULL: &str = include_str!("../templates/full.fish");
const FISH_SHORT: &str = include_str!("../templates/short.fish");

const BASH_FILE: &str = ".codeartifact.sh";
const BASH_FULL: &str = include_str!("../templates/full.bash");
const BASH_SHORT: &str = include_str!("../templates/short.bash");

/// Our overengineered struct for holding our env vars.
#[derive(Debug, Clone)]
struct EnvVars {
    homedir: PathBuf,
    domain: String,
    account_id: String,
    region: String,
    python: Option<String>,
    maven: Option<String>,
}

impl EnvVars {
    pub fn new() -> anyhow::Result<Self> {
        let homedir = home::home_dir().expect("Cannot locate a home directory to write files to.");
        let domain = env::var("AWS_DOMAIN").expect("you must set your CodeArtifact domain in the env var AWS_DOMAIN");
        let account_id =
            env::var("AWS_ACCOUNT_ID").expect("you must set your numeric AWS account id in the env var AWS_ACCOUNT_ID");
        let region = env::var("AWS_REGION").unwrap_or("us-east-1".to_string());
        let python = env::var("CODEARTIFACT_PYTHON_REPO").ok();
        let maven = env::var("CODEARTIFACT_MAVEN_REPO").ok();

        Ok(Self {
            homedir,
            domain,
            account_id,
            region,
            python,
            maven,
        })
    }
}

fn refresh_credentials() -> anyhow::Result<()> {
    // Maybe our credentials are stale? Let's try refreshing them.
    let mut child = Command::new("aws")
        .arg("sso")
        .arg("login")
        .spawn()
        .expect("Failed to execute aws sso login");
    let _exit_status = child.wait()?;
    Ok(())
}

/// Fetch a fresh auth token and write it out to the various files it needs to be in.
async fn fetch_token() -> anyhow::Result<()> {
    let envvars = EnvVars::new()?;
    let config = aws_config::defaults(BehaviorVersion::latest()).load().await;
    let sts_client = aws_sdk_sts::Client::new(&config);

    let whoami_result = sts_client.get_caller_identity().send().await;
    if whoami_result.is_err() {
        refresh_credentials()?;
    }

    let client = aws_sdk_codeartifact::Client::new(&config);
    let token_result = client
        .get_authorization_token()
        .domain(envvars.domain.as_str())
        .domain_owner(envvars.account_id.as_str())
        .send()
        .await;

    let token_output = match token_result {
        Ok(v) => v,
        Err(token_err) => {
            eprintln!("Got the following error trying to access your repository:");
            eprintln!("{token_err}");
            eprintln!("Please double-check your configuration:");
            eprintln!("    AWS_DOMAIN={}", envvars.domain);
            eprintln!("    AWS_ACCOUNT_ID={}", envvars.account_id);
            exit(1);
        }
    };

    let token = token_output
        .authorization_token()
        .expect("AWS failed to respond with an auth token.");
    let expiry_dt = token_output
        .expiration()
        .expect("AWS failed to respond with a token expiration time.");
    let expiry_ms = format!("{}", expiry_dt.to_millis()?);

    maybe_write_maven(&envvars, token)?;
    let shell = env::var("SHELL").unwrap_or("bash".to_string());
    if shell.ends_with("fish") {
        write_fish(&envvars, token, expiry_ms.as_str())
    } else {
        write_bash(&envvars, token, expiry_ms.as_str())
    }
}

const TOKEN_PATTERN: &str = "(<password>)(.+?)(</password><!--AWS_CODEARTIFACT_TOKEN-->)";

fn maybe_write_maven(envvars: &EnvVars, token: &str) -> anyhow::Result<()> {
    let Some(maven) = envvars.maven.as_ref() else {
        return Ok(());
    };

    let mut mvnpath = envvars.homedir.clone();
    mvnpath.push(".m2");
    std::fs::create_dir_all(&mvnpath)?;
    mvnpath.push("settings.xml");

    // We're going to be careful about maven config.
    if std::fs::exists(&mvnpath)? {
        // We replace only our pattern.
        let patt = regex::Regex::new(TOKEN_PATTERN)?;
        // It's not very long, and we are very lazy.
        let mut file = std::fs::File::open(&mvnpath)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        // Why parse xml when we can use a regex? I suppose this isn't very overengineered.
        let result = patt.replace(contents.as_str(), |caps: &Captures<'_>| {
            format!("{}{token}{}", &caps[1], &caps[3])
        });
        std::fs::write(&mvnpath, result.as_bytes())?;
    } else {
        // we may cheerfully write all over a file that does not exist.
        std::fs::write(
            &mvnpath,
            MAVEN_TMPL
                .replace("{domain}", envvars.domain.as_str())
                .replace("{account_id}", envvars.account_id.as_str())
                .replace("{region}", envvars.region.as_str())
                .replace("{token}", token)
                .replace("{repository}", maven),
        )?;
    }

    Ok(())
}

fn write_shell_templates(
    envvars: &EnvVars,
    token: &str,
    expiry_ms: &str,
    fpath: PathBuf,
    full: &str,
    short: &str,
) -> anyhow::Result<()> {
    if let Some(python) = envvars.python.as_ref() {
        std::fs::write(
            &fpath,
            full.replace("{domain}", envvars.domain.as_str())
                .replace("{account_id}", envvars.account_id.as_str())
                .replace("{region}", envvars.region.as_str())
                .replace("{token}", token)
                .replace("{expiry_ms}", expiry_ms)
                .replace("{repository}", python),
        )?;
    } else {
        std::fs::write(
            &fpath,
            short.replace("{token}", token).replace("{expiry_ms}", expiry_ms),
        )?;
    }
    Ok(())
}

fn write_fish(envvars: &EnvVars, token: &str, expiry_ms: &str) -> anyhow::Result<()> {
    let mut fishpath = envvars.homedir.clone();
    fishpath.push(".config/fish/");
    std::fs::create_dir_all(&fishpath)?;
    fishpath.push(FISH_FILE);
    println!(
        "source {} to get the fresh token in your environment",
        fishpath.display()
    );
    write_shell_templates(envvars, token, expiry_ms, fishpath, FISH_FULL, FISH_SHORT)
}

fn write_bash(envvars: &EnvVars, token: &str, expiry_ms: &str) -> anyhow::Result<()> {
    let mut bashpath = envvars.homedir.clone();
    bashpath.push(BASH_FILE);
    println!(
        "source {} to get the fresh token in your environment",
        bashpath.display()
    );
    write_shell_templates(envvars, token, expiry_ms, bashpath, BASH_FULL, BASH_SHORT)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let Ok(_token) = env::var("AWS_CODEARTIFACT_TOKEN") else {
        return fetch_token().await;
    };
    let Ok(expiry_str) = env::var("CODEARTIFACT_TOKEN_EXPIRY") else {
        return fetch_token().await;
    };
    let Ok(expiry) = expiry_str.parse::<i64>() else {
        return fetch_token().await;
    };

    let now = jiff::Timestamp::now().as_millisecond();
    if now > expiry {
        // consider testing the token here
        fetch_token().await
    } else {
        eprintln!("Your token is probably good.");
        Ok(())
    }
}
