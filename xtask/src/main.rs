type DynError = Box<dyn std::error::Error>;

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
        Err(format!("{} failed with status: {}", cmd, status))?
    }
}

/// Run a command with env vars and return `Err` on non-zero status codes.
fn run(cmd: &str, args: &[&str]) -> Result<(), DynError> {
    run_env(cmd, args, &[])
}

fn lint_rust() -> Result<(), DynError> {
    // Run clippy with warnings set to deny.
    let a = run("cargo", &["clippy", "--", "-D", "warnings"]);
    // rustfmt check.
    let b = run("cargo", &["fmt", "--", "--check"]);
    a.and(b)
}

fn test_rust() -> Result<(), DynError> {
    run("cargo", &["test", "--all-features"])
}

fn lint_cli() -> Result<(), DynError> {
    run("mypy", &["--strict", "./cli"])?;
    run("pylint", &["cli/kworkspaces", "cli/setup.py"])?;
    run("black", &["--check", "cli/kworkspaces"])
}

fn lint() -> Result<(), DynError> {
    let a = lint_rust();
    let b = lint_cli();
    a.and(b)
}

fn test() -> Result<(), DynError> {
    test_rust()
}

fn format() -> Result<(), DynError> {
    run("cargo", &["fmt"])?;
    run("black", &["workspaces.py"])
}

/// Build docker image and load it into the local daemon.
/// Returns image name. (repo/image:tag)
fn docker_image_build() -> Result<String, DynError> {
    eprintln!("Building docker image...");
    run("nix", &["build", ".#dockerImage"])?;
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
        return Err("`docker load` failed")?;
    }

    let stdout = std::str::from_utf8(&out.stdout)?;
    let name = stdout
        .lines()
        .find(|line| line.trim().starts_with("Loaded image:"))
        .and_then(|x| x.splitn(2, ':').nth(1))
        .ok_or_else(|| "Could not parse output")?
        .trim();

    eprintln!("Built and loaded docker image '{}'", name);

    Ok(name.to_string())
}

/// Publish a previously built docker image.
fn docker_image_publish() -> Result<(), DynError> {
    let name = docker_image_build()?;
    eprintln!("Image built. Publishing {}", name);
    run("docker", &["push", &name])?;
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
        ["docker-build"] => docker_image_build().map(|_| ()),
        ["docker-publish"] => docker_image_publish(),
        ["ci-rust"] => ci_rust(),
        ["ci-cli"] => ci_cli(),
        ["ci"] => ci(),
        other => Err(format!("Unknown arguments: {:?}", other))?,
    }
}
