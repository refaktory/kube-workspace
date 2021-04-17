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
    eprintln!("Checking workspaces.py with mypy:");
    run("cargo", &["check"])?;
    run("cargo", &["fmt", "--", "--check"])
}

fn lint_ui() -> Result<(), DynError> {
    run("mypy", &["--strict", "workspaces.py"])?;
    run("pylint", &["workspaces.py"])?;
    run("black", &["--check", "workspaces.py"])
}

fn lint() -> Result<(), DynError> {
    lint_rust()?;
    lint_ui()
}

fn format() -> Result<(), DynError> {
    run("cargo", &["fmt"])?;
    run("black", &["workspaces.py"])
}

fn main() -> Result<(), DynError> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let args_str: Vec<_> = args.iter().map(|x| x.as_str()).collect();

    match args_str.as_slice() {
        ["fmt"] => format(),
        ["lint-rust"] => lint_rust(),
        ["lint-ui"] => lint_ui(),
        ["lint"] => lint(),
        other => Err(format!("Unknown arguments: {:?}", other))?,
    }
}
