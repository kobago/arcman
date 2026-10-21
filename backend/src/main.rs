use std::{
    env,
    io::{BufRead, IsTerminal},
    path::PathBuf,
    process::ExitCode,
};

use arcman::{AppState, BoxResult, Config};

const DEFAULT_ADDR: &str = "127.0.0.1:3000";

const USAGE: &str = "usage: arcman <command>

commands:
  serve [--addr HOST:PORT]   start the server (default 127.0.0.1:3000)
  set-password               set the login password (reads one line from stdin when piped)

environment:
  ARCMAN_HOME                 data directory (default ~/.arcman)
  ARCMAN_ADDR                 listen address when --addr is not given
  ARCMAN_FXTWITTER_BASE_URL   initial fxtwitter API base URL";

fn config() -> BoxResult<Config> {
    let home = match env::var_os("ARCMAN_HOME") {
        Some(h) => PathBuf::from(h),
        None => PathBuf::from(env::var_os("HOME").ok_or("HOME is not set")?).join(".arcman"),
    };
    Ok(Config {
        home,
        fxtwitter_base_url_env: env::var("ARCMAN_FXTWITTER_BASE_URL")
            .ok()
            .filter(|s| !s.is_empty()),
    })
}

fn listen_addr(args: &[String], env_addr: Option<String>) -> Result<String, String> {
    match args {
        [] => Ok(env_addr
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_ADDR.into())),
        [flag, addr] if flag == "--addr" => Ok(addr.clone()),
        [flag] if flag.starts_with("--addr=") => Ok(flag["--addr=".len()..].to_string()),
        _ => Err(format!("unexpected arguments: {}", args.join(" "))),
    }
}

async fn serve(args: &[String]) -> BoxResult<()> {
    let addr = listen_addr(args, env::var("ARCMAN_ADDR").ok())?;
    let (state, queue) = AppState::open(config()?)?;
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("arcman listening on http://{addr}");
    axum::serve(listener, arcman::app(state, queue)).await?;
    Ok(())
}

fn read_password() -> BoxResult<String> {
    if !std::io::stdin().is_terminal() {
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        return Ok(line.trim_end_matches(['\r', '\n']).to_string());
    }
    let password = rpassword::prompt_password("New password: ")?;
    if rpassword::prompt_password("Confirm password: ")? != password {
        return Err("passwords do not match".into());
    }
    Ok(password)
}

fn set_password() -> BoxResult<()> {
    let password = read_password()?;
    if password.chars().count() < 8 {
        return Err("password must be at least 8 characters".into());
    }
    let config = config()?;
    std::fs::create_dir_all(&config.home)?;
    let conn = arcman::db::open(&config.home.join("arcman.db"))?;
    arcman::auth::set_password(&conn, &password)?;
    println!("password updated");
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("serve") => serve(&args[1..]).await,
        Some("set-password") => set_password(),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_addr_option_wins_over_the_environment_which_wins_over_the_default() {
        let env = || Some("0.0.0.0:4000".to_string());
        assert_eq!(
            listen_addr(&args(&["--addr", "0.0.0.0:3000"]), env()).unwrap(),
            "0.0.0.0:3000"
        );
        assert_eq!(
            listen_addr(&args(&["--addr=100.64.0.1:3000"]), env()).unwrap(),
            "100.64.0.1:3000"
        );
        assert_eq!(listen_addr(&[], env()).unwrap(), "0.0.0.0:4000");
        assert_eq!(listen_addr(&[], None).unwrap(), DEFAULT_ADDR);
    }

    #[test]
    fn unknown_serve_arguments_are_rejected() {
        assert!(listen_addr(&args(&["--port", "3000"]), None).is_err());
        assert!(listen_addr(&args(&["--addr"]), None).is_err());
    }
}
