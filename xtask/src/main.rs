type DynError = Box<dyn std::error::Error>;

/// Run a command and return `Err` on non-zero status codes.
fn run(cmd: &str, args: &[&str]) -> Result<(), DynError> {
    eprintln!("Running '{} {}'", cmd, args.join(" "));
    let status = std::process::Command::new(cmd).args(args).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{} failed with status: {}", cmd, status))?
    }
}

fn lint_rust() -> Result<(), DynError> {
    run("cargo", &["check"])?;
    run("cargo", &["fmt", "--", "--check"])
}

fn lint_cli() -> Result<(), DynError> {
    run("mypy", &["--strict", "workspaces.py"])?;
    run("pylint", &["workspaces.py"])?;
    run("black", &["--check", "workspaces.py"])
}

fn lint() -> Result<(), DynError> {
    lint_rust()?;
    lint_cli()
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

fn docker_image_publish() -> Result<(), DynError> {
    let name = docker_image_build()?;
    eprintln!("Image built. Publishing {}", name);
    run("docker", &["push", &name])?;
    Ok(())
}

fn main() -> Result<(), DynError> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let args_str: Vec<_> = args.iter().map(|x| x.as_str()).collect();

    match args_str.as_slice() {
        ["fmt"] | ["format"] => format(),
        ["lint-rust"] => lint_rust(),
        ["lint-cli"] => lint_cli(),
        ["lint"] => lint(),
        ["docker-build"] => docker_image_build().map(|_| ()),
        ["docker-publish"] => docker_image_publish(),
        other => Err(format!("Unknown arguments: {:?}", other))?,
    }
}
