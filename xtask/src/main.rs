use std::{
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
};

use anyhow::{anyhow, bail, Context, Error as AnyError};

fn main() -> Result<(), AnyError> {
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
        ["test-end2end"] => end_to_end_test(false),
        other => return Err(anyhow!("Unknown arguments: {:?}", other)),
    }
}

fn root_dir() -> Result<PathBuf, AnyError> {
    std::env::var_os("CARGO_MANIFEST_DIR")
        .context("Required env var CARGO_MANIFEST_DIR not found")
        .and_then(|raw| {
            PathBuf::from(raw)
                .parent()
                .map(|p| p.to_path_buf())
                .context("Could not find root")
        })
}

/// Run a command and return `Err` on non-zero status codes.
fn run_env(cmd: &str, args: &[&str], env: &[(&str, &str)]) -> Result<(), AnyError> {
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
        Err(anyhow!("{} failed with status: {}", cmd, status))
    }
}

trait CommandExt {
    fn run_checked(&mut self) -> Result<(), AnyError>;
}

impl CommandExt for &mut std::process::Command {
    fn run_checked(&mut self) -> Result<(), AnyError> {
        let status = self.spawn()?.wait()?;
        if !status.success() {
            Err(anyhow!("Command failed with exit code: {:?}", status))
        } else {
            Ok(())
        }
    }
}

/// Run a command with env vars and return `Err` on non-zero status codes.
fn run(cmd: &str, args: &[&str]) -> Result<(), AnyError> {
    std::env::set_current_dir(root_dir()?)?;
    run_env(cmd, args, &[])
}

/// Run Rust lints (rustfmt, clippy)
fn lint_rust() -> Result<(), AnyError> {
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

fn test_rust() -> Result<(), AnyError> {
    run("cargo", &["test", "--all-features"])
}

fn lint_kubernetes() -> Result<(), AnyError> {
    eprintln!("Linting helm chart...");
    Command::new("helm")
        .arg("lint")
        .arg(root_dir()?.join("deploy").join("helm"))
        .arg("--strict")
        .run_checked()?;

    Ok(())
}

fn lint_cli() -> Result<(), AnyError> {
    eprintln!("Linting CLI...");
    run("mypy", &["--strict", "./cli"])?;
    run("pylint", &["cli/kworkspace", "cli/setup.py"])?;
    run("black", &["--check", "cli/kworkspace"])?;

    eprintln!("CLI lint succeeded");
    Ok(())
}

/// Lint Rust and Python CLI.
fn lint() -> Result<(), AnyError> {
    let a = lint_rust().context("Rust lints failed");
    let b = lint_cli().context("CLI lints failed");
    let c = lint_kubernetes().context("Kubernetes lints failed");
    a.and(b).and(c)?;

    eprintln!("All lints succeeded");
    Ok(())
}

/// Run tests for Rust and Python CLI.
fn test() -> Result<(), AnyError> {
    test_rust()
}

/// Format all code. (Rust + Python)
fn format() -> Result<(), AnyError> {
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

fn nix_build_and_load_docker_image(flake_package_name: &str) -> Result<ImageName, AnyError> {
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
        return Err(AnyError::msg("`docker load` failed"));
    }

    let stdout = std::str::from_utf8(&out.stdout)?;
    let name = stdout
        .lines()
        .find(|line| line.trim().starts_with("Loaded image:"))
        .and_then(|x| x.split_once(':').map(|x| x.1))
        .context("Could not parse output")?
        .trim();

    eprintln!("Built and loaded docker image '{}'", name);

    Ok(name.to_string())
}

fn build_docker_image_operator() -> Result<ImageName, AnyError> {
    nix_build_and_load_docker_image("docker-image-operator")
}

fn build_docker_image_cli() -> Result<ImageName, AnyError> {
    nix_build_and_load_docker_image("docker-image-cli")
}

fn build_all_docker_images() -> Result<(CliImageName, OperatorImageName), AnyError> {
    let op = build_docker_image_operator()?;
    let cli = build_docker_image_cli()?;

    Ok((op, cli))
}

/// Publish a previously built docker image.
fn publish_all_docker_images() -> Result<(), AnyError> {
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
fn cmd_docker_build() -> Result<(), AnyError> {
    build_all_docker_images()?;
    Ok(())
}

fn kind_install() -> Result<(), AnyError> {
    let (image_name_operator, _image_name_cli) = build_all_docker_images()?;

    eprintln!("Loading docker image into kind...");
    Command::new("kind")
        .arg("load")
        .arg("docker-image")
        .arg(&image_name_operator)
        .run_checked()
        .context("Could not load docker image")?;

    let tag = image_name_operator
        .split(':')
        .nth(1)
        .context("Could not parse image tag")?;

    eprintln!("Upgrading/installing helm chart...");
    Command::new("helm")
        .args([
            "upgrade",
            "--install",
            "--wait-for-jobs",
            "kube-workspace",
            "./deploy/helm",
            "--values",
            "./tests/fixtures/values.yaml",
            "--set",
            "image.pullPolicy=Never",
            "--set",
            &format!("image.tag={}", tag),
        ])
        .run_checked()
        .context("Could not run 'helm upgrade'")?;

    eprintln!("Restarting operator...");
    Command::new("kubectl")
        .args(&[
            "delete",
            "--namespace",
            "default",
            "pods",
            "-l",
            "app.kubernetes.io/name=kube-workspace-operator",
        ])
        .run_checked()
        .context("Could not delete operator pods")?;

    eprintln!("Operator was installed in cluster!");

    Ok(())
}

/// Run CI checks for the operator.
fn ci_rust() -> Result<(), AnyError> {
    let b = test_rust();
    let a = lint_rust();
    a.and(b)
}

/// Run CI checks for CLI.
fn ci_cli() -> Result<(), AnyError> {
    lint_cli()
}

/// Run all CI checks for both operator and cli.
fn ci() -> Result<(), AnyError> {
    let b = ci_rust();
    let a = ci_cli();
    a.and(b)
}

fn end_to_end_test(create: bool) -> Result<(), AnyError> {
    let cli_path = root_dir()?
        .join("cli")
        .join("kworkspace")
        .join("__init__.py");
    let fixtures_path = root_dir()?.join("tests").join("fixtures");
    let ssh_fixtures_path = fixtures_path.join("ssh_keys");
    let key1_path = ssh_fixtures_path.join("key1.pub");

    let api_port = 33333;

    if create {
        unimplemented!();
    }

    kind_install().context("Could not install operator into cluster")?;

    // Make operator ip accessible.
    let _t = std::thread::spawn(move || {
        let res = Command::new("kubectl")
            .args(&[
                "--namespace",
                "default",
                "port-forward",
                "service/kube-workspace-kube-workspace-operator",
                &format!("{api_port}:http"),
            ])
            .run_checked();
        if let Err(err) = res {
            eprintln!("Port forward failed! {:?}", err);
            std::process::exit(1);
        }
    });

    // sleep a bit to allow port forward to become active.
    std::thread::sleep(std::time::Duration::from_secs(3));

    eprintln!("Running 'kworkspace connect echo hello'");
    let mut proc = Command::new(&cli_path)
        .args(&[
            "--user",
            "test1",
            "--ssh-key-path",
            &key1_path.display().to_string(),
            "--api",
            &format!("http://localhost:{api_port}"),
            "connect",
            "echo",
            "hello",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;

    let mut stdout_handle = proc.stdout.take().unwrap();
    let mut stdout = Vec::new();

    stdout_handle.read_to_end(&mut stdout)?;
    let status = proc.wait()?;
    let stdout_str = String::from_utf8(stdout)?;

    if !status.success() {
        bail!("kworkspace connect failed",);
    }

    if stdout_str != "hello" {
        bail!("Expected stdout to say 'hello'");
    }

    Ok(())
}
