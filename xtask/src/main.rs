use std::{path::PathBuf, process::Command};

type DynError = Box<dyn std::error::Error>;

fn main() -> Result<(), DynError> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let args_str: Vec<_> = args.iter().map(|x| x.as_str()).collect();

    match args_str.as_slice() {
        ["fmt"] | ["format"] => format(),
        ["lint-rust"] => lint_rust(),
        ["lint-cli"] => lint_cli(),
        ["lint"] => lint(),
        ["test-rust"] => test_rust(),
        ["test"] => test(),
        ["docker-build"] => cmd_docker_build().map(|_| ()),
        ["kind-install"] => kind_install(),
        ["docker-publish"] => publish_all_docker_images(),
        ["ci-rust"] => ci_rust(),
        ["ci-cli"] => ci_cli(),
        ["ci"] => ci(),
        other => return Err(format!("Unknown arguments: {:?}", other).into()),
    }
}

fn root_dir() -> Result<PathBuf, DynError> {
    std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|_| "Required env var CARGO_MANIFEST_DIR not found".into())
        .and_then(|raw| {
            PathBuf::from(raw)
                .parent()
                .map(|p| p.to_path_buf())
                .ok_or_else(|| "Could not find root".to_string().into())
        })
}

/// Run a command and return `Err` on non-zero status codes.
fn run_env(cmd: &str, args: &[&str], env: &[(&str, &str)]) -> Result<(), DynError> {
    eprintln!("Running '{} {}'", cmd, args.join(" "));
    let mut c = std::process::Command::new(cmd);

    c.args(args);
    // Set env vars.
    for (key, value) in env {
        c.env(key, value);
    }

    let status = c.status()?;
    if status.success() {
        eprintln!("\n");
        Ok(())
    } else {
        Err(format!("{} failed with status: {}", cmd, status).into())
    }
}

trait CommandExt {
    fn run_checked(&mut self) -> Result<(), DynError>;
}

impl CommandExt for &mut std::process::Command {
    fn run_checked(&mut self) -> Result<(), DynError> {
        let status = self.spawn()?.wait()?;
        if !status.success() {
            Err(format!("Command failed with exit code: {:?}", status).into())
        } else {
            Ok(())
        }
    }
}

/// Run a command with env vars and return `Err` on non-zero status codes.
fn run(cmd: &str, args: &[&str]) -> Result<(), DynError> {
    std::env::set_current_dir(root_dir()?)?;
    run_env(cmd, args, &[])
}

/// Run Rust lints (rustfmt, clippy)
fn lint_rust() -> Result<(), DynError> {
    // Run clippy with warnings set to deny.
    eprintln!("Running CLIPPY checks");
    let a = run("cargo", &["clippy", "--", "-D", "warnings"]);

    // rustfmt check.
    eprintln!("Running rustfmt check");
    let b = run("cargo", &["fmt", "--check"]);
    a.and(b)?;

    // Do the same for xtask
    // Run clippy with warnings set to deny.
    eprintln!("Running CLIPPY checks for xtasks");
    let a = Command::new("cargo")
        .current_dir(root_dir()?.join("xtask"))
        .args(&["clippy", "--", "-D", "warnings"])
        .run_checked();

    // rustfmt check.
    eprintln!("Running rustfmt check");
    let b = Command::new("cargo")
        .current_dir(root_dir()?.join("xtask"))
        .args(&["fmt", "--check"])
        .run_checked();
    a.and(b)
}

fn test_rust() -> Result<(), DynError> {
    run("cargo", &["test", "--all-features"])
}

fn lint_kubernetes() -> Result<(), DynError> {
    eprintln!("Linting helm chart...");
    Command::new("helm")
        .arg("lint")
        .arg(root_dir()?.join("deploy").join("helm"))
        .arg("--strict")
        .run_checked()?;

    Ok(())
}

fn lint_cli() -> Result<(), DynError> {
    eprintln!("Linting CLI...");
    run("mypy", &["--strict", "./cli"])?;
    run("pylint", &["cli/kworkspace", "cli/setup.py"])?;
    run("black", &["--check", "cli/kworkspace"])?;

    eprintln!("CLI lint succeeded");
    Ok(())
}

/// Lint Rust and Python CLI.
fn lint() -> Result<(), DynError> {
    let a = lint_rust();
    let b = lint_cli();
    let c = lint_kubernetes();
    a.and(b).and(c)?;

    eprintln!("All lints succeeded");
    Ok(())
}

/// Run tests for Rust and Python CLI.
fn test() -> Result<(), DynError> {
    test_rust()
}

/// Format all code. (Rust + Python)
fn format() -> Result<(), DynError> {
    run("cargo", &["fmt"])?;
    Command::new("cargo")
        .arg("fmt")
        .current_dir(root_dir()?.join("xtask"))
        .run_checked()?;

    eprintln!("Formatting python code...");
    run("black", &["cli"])?;

    eprintln!("Formatting nix code...");
    run("nixpkgs-fmt", &["flake.nix"])?;

    Ok(())
}

type ImageName = String;

fn nix_build_and_load_docker_image(flake_package_name: &str) -> Result<ImageName, DynError> {
    eprintln!("Building docker image with package {flake_package_name}...");
    run("nix", &["build", &format!(".#{flake_package_name}")])?;
    eprintln!(
        "Docker image archive created in './result'. \
         Loading with `docker load < result`..."
    );

    let out = std::process::Command::new("docker")
        .args(&["load", "--input", "./result"])
        .output()?;
    if !out.status.success() {
        eprintln!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        return Err("`docker load` failed".to_string().into());
    }

    let stdout = std::str::from_utf8(&out.stdout)?;
    let name = stdout
        .lines()
        .find(|line| line.trim().starts_with("Loaded image:"))
        .and_then(|x| x.split_once(':').map(|x| x.1))
        .ok_or_else(|| "Could not parse output".to_string())?
        .trim();

    eprintln!("Built and loaded docker image '{}'", name);

    Ok(name.to_string())
}

fn build_docker_image_operator() -> Result<ImageName, DynError> {
    nix_build_and_load_docker_image("docker-image-operator")
}

fn build_docker_image_cli() -> Result<ImageName, DynError> {
    nix_build_and_load_docker_image("docker-image-cli")
}

fn build_all_docker_images() -> Result<(CliImageName, OperatorImageName), DynError> {
    let op = build_docker_image_operator()?;
    let cli = build_docker_image_cli()?;

    Ok((op, cli))
}

/// Publish a previously built docker image.
fn publish_all_docker_images() -> Result<(), DynError> {
    let (img_operator, img_cli) = build_all_docker_images()?;

    eprintln!("Image built. Publishing {}", img_operator);
    run("docker", &["push", &img_operator])?;

    eprintln!("Image built. Publishing {}", img_cli);
    run("docker", &["push", &img_cli])?;

    Ok(())
}

type CliImageName = ImageName;
type OperatorImageName = ImageName;

/// Build all docker images and load them into the local daemon.
fn cmd_docker_build() -> Result<(), DynError> {
    build_all_docker_images()?;
    Ok(())
}

fn kind_install() -> Result<(), DynError> {
    let (image_name_operator, _image_name_cli) = build_all_docker_images()?;

    Command::new("kind")
        .arg("load")
        .arg("docker-image")
        .arg(&image_name_operator)
        .run_checked()?;

    let tag = image_name_operator
        .split(':')
        .nth(1)
        .ok_or_else(|| "Could not parse image tag".to_string())?;

    Command::new("helm")
        .args([
            "upgrade",
            "--install",
            "kube-workspace",
            "./deploy/helm",
            "--set",
            "image.pullPolicy=Never",
            "--set",
            &format!("image.tag={}", tag),
        ])
        .run_checked()?;

    Ok(())
}

/// Run CI checks for the operator.
fn ci_rust() -> Result<(), DynError> {
    let b = test_rust();
    let a = lint_rust();
    a.and(b)
}

/// Run CI checks for CLI.
fn ci_cli() -> Result<(), DynError> {
    lint_cli()
}

/// Run all CI checks for both operator and cli.
fn ci() -> Result<(), DynError> {
    let b = ci_rust();
    let a = ci_cli();
    a.and(b)
}
