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

## Authentication

For ease of use authentication is handled via SSH public keys. 
The server configuration must specify a whitelist of user/public key pairs.

The public key is used as an API token.
Access to workspaces is only possible via SSH with the private key.
