# kube-workspace-operator

Kubernetes operator that allows managing user-specific, persistent "workspace" 
pods that can be accessed via SSH.

A command line interface is proved that allows starting and stopping a workspace.

The primary use case is providing a standardized and easily accesible
development environment for machine learning practitioners.


## Features

* Start and stop workspaces via the CLI
* Easy authentication via SSH keys
* Persistent `/home/username` storage via `PersistentVolumeClaim`s
* Kubernetes `Pod` templates for customizing the workspace
* Helm chart

Work in progress: 
* Automatic shutdown of workspaces without active SSH connections and near zero
  CPU usage
* Scheduled (automatic) and manual backup of the persistent storage
* Logging of SSH sessions

### Authentication

For ease of use authentication is handled via SSH public keys. 
The server configuration must specify a whitelist of user/public key pairs.

The public key is used as an API token.
Access to workspaces is only possible via SSH with the private key.

## Usage - CLI

The CLI provides a convenient shell interface for starting and stopping workspaces.

**NOTE**: An administrator has to add your SSH public key to the authorized keys
before you can use the CLI.

See [Deploy] below for how to configure and deploy the project to  a Kubernetes cluster.

### Installation:

* Pip:
  `python3 -m pip install -e 'git+https://github.com/refaktory/kube-workspace.git#egg=kworkspace&subdirectory=cli'`

* Nix/NixOS: 
  **Flakes must be enabled**
  Run once: `nix run github:refaktory/kube-workspace.cli`
  Shell: `nix shell -p github:refaktory/kube-workspace.cli`

### Commands:

* `kworkspace start` - Start your workspace
* `kworkspace stop`- Stop your workspace

## Development

This project uses the [xtask pattern](https://github.com/matklad/cargo-xtask) to
provide a development CLI.

Commands:

* `cargo xtask fmt` - Format all code (`cargo fmt` for Rust, `black` for CLI) 
* `cargo xtask lint` - Lint all code (Rust + Python CLI)
* `cargo xtask test` - Run all tests (Rust + Python CLI)
* `cargo xtask ci` - Run all lints and tests as executed by the **CI**
* `cargo xtask docker-build` - Build Docker image
* `cargo xtask docker-publish` - Build Docker image and publish to image registry

### Pull Request Validation

Before submitting a pull request, run `cargo xtask ci` to execute all all 
checks as they are run by the CI.

This includes lints and tests for both the CLI and the operator and will surface
any issues that would fail the CI.

### Nix

If you use `nix` with Flakes, you can run `nix develop` to get a development
shell with all required dependencies.
