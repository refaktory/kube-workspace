# kube-workspace-operator

Kubernetes operator that allows managing user-specific "workspace" pods that can
be accessed via SSH.

A command line interface is proved that allows starting and stopping a workspace.

The primary use case is providing a standardized development environment for 
machine learning practitioners.


## Features

* Start and stop workspaces via the CLI
* Automatic shutdown of workspaces without active SSH connections and near zero
  CPU usage (WIP)
* Custom Kubernetes `Pod` templated for customizing the workspace

## Authentication

For ease of use authentication is handled via SSH public keys. 
The server configuration must specify a whitelist of users and public keys.
The public key is used as an API token.

Access to workspaces is only possible via SSH with the private key.
