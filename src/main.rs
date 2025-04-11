//! `codefact` is a command-line program to do one thing and one thing only:
//! check if you're logged into AWS Code Artifact and get a fresh auth token if
//! you're not. It writes the token to several files to make it available to shells
//! when they're sourced, and to maven when maven does whatever horrors it does to
//! fetch packages.
//!
//! `codefact` at the moment takes no options other than environment variables.
//! It uses whatever aws profile you have set up as your default.

use std::env;
use std::fs::File;
use std::io::Write;
use std::process::{Command, exit};

use aws_config::BehaviorVersion;

const MAVEN_TMPL: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?><settings xmlns="http://maven.apache.org/SETTINGS/1.0.0" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="http://maven.apache.org/SETTINGS/1.0.0 https://maven.apache.org/xsd/settings-1.0.0.xsd">
    <servers>
        <server>
            <id>{domain}</id>
            <username>aws</username>
            <password>{token}</token><!--AWS_CODEARTIFACT_TOKEN-->
        </server>
    </servers>

    <profiles>
        <profile>
            <id>{domain}</id>
            <activation>
                <activeByDefault>true</activeByDefault>
            </activation>
            <repositories>
                <repository>
                    <id>{domain}</id>
                    <name>{domain}</name>
                    <url>https://{domain}-{account_id}.d.codeartifact.{region}.amazonaws.com/maven/{repository}/</url>
                </repository>
            </repositories>
        </profile>
    </profiles>
</settings>
"#;

const FISH_FILE: &str = "codeartifact.fish";
const FISH_TMPL: &str = r#"set -gx UV_DEFAULT_INDEX "https://aws:{token}@{domain}-677637302876.d.codeartifact.{region}.amazonaws.com/pypi/{repository}/simple/"
set -gx UV_PUBLISH_URL "https://{domain}-677637302876.d.codeartifact.{region}.amazonaws.com/pypi/{repository}"
set -gx UV_PUBLISH_PASSWORD "{token}"
"#;

const BASH_FILE: &str = ".codeartifact.sh";
const BASH_TMPL: &str = r#"export UV_DEFAULT_INDEX="https://aws:{token}@{domain}-677637302876.d.codeartifact.{region}.amazonaws.com/pypi/{repository}/simple/"
export UV_PUBLISH_URL="https://{domain}-677637302876.d.codeartifact.{region}.amazonaws.com/pypi/{repository}"
export UV_PUBLISH_PASSWORD="{token}"
"#;

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
    let domain = env::var("AWS_DOMAIN").expect("you must set your CodeArtifact domain in the env var AWS_DOMAIN");
    let account_id =
        env::var("AWS_ACCOUNT_ID").expect("you must set your numeric AWS account id in the env var AWS_ACCOUNT_ID");
    let region = env::var("AWS_REGION").unwrap_or("us-east-1".to_string());

    let config = aws_config::defaults(BehaviorVersion::latest()).load().await;
    let sts_client = aws_sdk_sts::Client::new(&config);

    let whoami_result = sts_client.get_caller_identity().send().await;
    if whoami_result.is_err() {
        refresh_credentials()?;
    }

    let client = aws_sdk_codeartifact::Client::new(&config);
    let token_result = client
        .get_authorization_token()
        .domain(domain.as_str())
        .domain_owner(account_id.as_str())
        .send()
        .await;

    let token_output = match token_result {
        Ok(v) => v,
        Err(token_err) => {
            eprintln!("Got the following error trying to access your repository:");
            eprintln!("{token_err}");
            eprintln!("Please double-check your configuration:");
            eprintln!("    AWS_DOMAIN={domain}");
            eprintln!("    AWS_ACCOUNT_ID={account_id}");
            exit(1);
        }
    };

    let token = token_output
        .authorization_token()
        .expect("AWS failed to respond with an auth token.");
    let expiry_dt = token_output
        .expiration()
        .expect("AWS failed to respond with a token expiration time.");
    let expiry_ms = expiry_dt.to_millis()?;

    let homedir = home::home_dir().expect("Cannot locate a home directory to write files to.");
    let mut bashpath = homedir.clone();
    bashpath.push(BASH_FILE);
    std::fs::write(
        &bashpath,
        format!(
            r#"export AWS_CODEARTIFACT_TOKEN="{token}"
export CODEARTIFACT_TOKEN_EXPIRY={expiry_ms}
"#
        ),
    )?;

    let mut fishpath = homedir.clone();
    fishpath.push(".config/fish/");
    std::fs::create_dir_all(&fishpath)?;
    fishpath.push(FISH_FILE);
    std::fs::write(
        &fishpath,
        format!(
            r#"set -gx AWS_CODEARTIFACT_TOKEN "{token}"
set -gx CODEARTIFACT_TOKEN_EXPIRY={expiry_ms}
"#
        ),
    )?;

    // We want python setup too.
    if let Ok(python_repo) = env::var("CODEARTIFACT_PYTHON_REPO") {
        let mut bashfile = File::options().append(true).open(bashpath)?;
        write!(
            bashfile,
            "{}",
            BASH_TMPL
                .replace("{domain}", domain.as_str())
                .replace("{account_id}", account_id.as_str())
                .replace("{region}", region.as_str())
                .replace("{token}", token)
                .replace("{repository}", python_repo.as_str())
        )?;

        let mut fishfile = File::options().append(true).open(fishpath)?;
        write!(
            fishfile,
            "{}",
            FISH_TMPL
                .replace("{domain}", domain.as_str())
                .replace("{account_id}", account_id.as_str())
                .replace("{region}", region.as_str())
                .replace("{token}", token)
                .replace("{repository}", python_repo.as_str())
        )?;
    }

    if let Ok(maven_repo) = env::var("CODEARTIFACT_MAVEN_REPO") {
        let mut mvnpath = homedir.clone();
        mvnpath.push(".m2");
        std::fs::create_dir_all(&mvnpath)?;
        mvnpath.push("settings.xml");
        std::fs::write(
            &mvnpath,
            MAVEN_TMPL
                .replace("{domain}", domain.as_str())
                .replace("{account_id}", account_id.as_str())
                .replace("{region}", region.as_str())
                .replace("{token}", token)
                .replace("{repository}", maven_repo.as_str()),
        )?;
    }

    Ok(())
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
