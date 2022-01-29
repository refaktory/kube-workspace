#!/usr/bin/env python3

"""CLI client for the kube-workspace-operator workspace manager.
Allows starting and stopping workspaces on a Kubernetes cluster.
"""

from __future__ import annotations

import os
import getpass
import argparse
import sys
import json
import urllib.request
import time
from dataclasses import dataclass, asdict
from typing import Any, Dict, Optional
from urllib.parse import urlparse
from enum import Enum
import subprocess

AnyDict = Dict[str, Any]


def log(*args: Any, **kwargs: Any) -> None:
    """Print to stderr.
    Defers to print()"""
    print(
        *args,
        **kwargs,
        file=sys.stderr,
    )


def host_username() -> str:
    """Get the current username from the OS."""
    return getpass.getuser()


class ApiError(Exception):
    """Error response from API"""

    def __init__(self, message: str):
        self.message = message
        super().__init__(message)


# CLI config file.
@dataclass(frozen=True)
class ConfigFile:
    """Parsed config file settings."""

    username: Optional[str]
    ssh_key_path: Optional[str]
    api_url: Optional[str]

    @staticmethod
    def user_path() -> str:
        """Get the default config file path for the current user"""

        return os.path.expanduser("~/.config/kube-workspaces/config.json")

    @staticmethod
    def initialize_ssh_path() -> str:
        """Prompt for and return path to public shh key file"""
        default_ssh_key_path = os.path.expanduser("~/.ssh/id_rsa.pub")
        ssh_path: Optional[str]
        if os.path.isfile(default_ssh_key_path):
            log("Default SSH key detected at " + default_ssh_key_path)
            while True:
                ssh_path = input(
                    "Alternative key (leave empty to use default): "
                ).strip()
                if ssh_path:
                    if not os.path.isfile(ssh_path):
                        log("Path is not valid")
                    else:
                        break
                else:
                    ssh_path = default_ssh_key_path
                    break
        else:
            log("No default SSH key detected")
            while True:
                ssh_path = input("SSH key path: ").strip()
                if ssh_path:
                    if not os.path.isfile(ssh_path):
                        log("Path is not valid")
                    else:
                        break
        return ssh_path

    # Prompts for config options and creates the config file.
    @classmethod
    def initialize(cls) -> ConfigFile:
        """Initialize a config file by prompting the user for settings and
        writing the file to the default location.
        """

        url: Optional[str] = None
        while not url:
            url = input("API URL (http://DOMAIN.com[:port]): ")
            try:
                res = urlparse(url)
                if not res.scheme in ["http", "https"]:
                    url = ""
            except Exception:  # pylint: disable=broad-except
                url = ""

        current_user = host_username()
        user = input(
            f'Username: (leave empty to use current username "{current_user}")'
        )
        user = user or current_user
        if not user:
            raise Exception("Could not determine username")

        ssh_path = cls.initialize_ssh_path()

        config = cls(username=user, ssh_key_path=ssh_path, api_url=url)

        path = ConfigFile.user_path()
        config_dir = os.path.dirname(path)
        if not os.path.isdir(config_dir):
            os.makedirs(config_dir)
        with open(path, mode="w", encoding="utf8") as file:
            json.dump(asdict(config), file)
        log(f"Config written to {path}")
        return config

    @staticmethod
    def load(auto_initialize: bool, custom_path: Optional[str] = None) -> ConfigFile:
        """Load config from disk.
        If auto_initialize is true, prompts the user for config options.
        Otherwise, it returns an empty config if file does not exist.
        """

        path: str = custom_path or ConfigFile.user_path()
        if os.path.isfile(path):
            data: Optional[Dict[str, str]] = None
            with open(path, encoding="utf8") as file:
                data = json.load(file)
            if not data:
                return ConfigFile(None, None, None)
            return ConfigFile(
                username=data["username"],
                ssh_key_path=data["ssh_key_path"],
                api_url=data["api_url"],
            )
        if auto_initialize:
            return ConfigFile.initialize()
        return ConfigFile(None, None, None)


@dataclass(frozen=True)
class Config:
    """Materialized CLI config."""

    username: str
    ssh_public_key: str
    ssh_private_key_path: str
    api_url: str

    def api_endpoint(self) -> str:
        """Compute query endpoint."""
        return self.api_url + "/api/query"


@dataclass(frozen=True)
class SshAddress:
    """API type for an ssh address and port."""

    address: str
    port: int

    def build_ssh_command(self, config: Config, extra_args: list[str]) -> list[str]:
        "Build the ssh command for a specific address and user(config)."

        parts = ["ssh", "-i", config.ssh_private_key_path]
        if self.port != 22:
            parts += ["-p", str(self.port)]
        parts += extra_args

        addr_prefix = (
            config.username + "@" if config.username != host_username() else ""
        )
        addr = addr_prefix + self.address
        parts.append(addr)
        return parts


class WorkspacePhase(Enum):
    """Enum of server side WorkspacePhase"""

    NOT_FOUND = "not_found"
    STARTING = "starting"
    READY = "ready"
    TERMINATING = "terminating"
    UNKNOWN = "unknown"


@dataclass(frozen=True)
class WorkspaceStatus:
    """Api response for the PodStart and WorkspaceStatus query."""

    username: Optional[str]
    phase: WorkspacePhase
    ssh_address: Optional[SshAddress]
    info: Optional[WorkspaceInfo]

    def is_ready(self) -> bool:
        """Returns true if workspace is ready and reachable over SSH."""
        return self.phase == WorkspacePhase.READY and self.ssh_address is not None

    def render_info(self, config: Config) -> str:
        """Render for display in the terminal."""

        parts: list[tuple[str, str]] = []

        if self.info:
            parts.append(("Container Image", self.info.image))
            if self.info.memory_limit is not None:
                parts.append(("Memory Limit", self.info.memory_limit))
            if self.info.cpu_limit is not None:
                parts.append(("CPU Limit", self.info.cpu_limit))

        if self.ssh_address:
            parts.append(
                (
                    "Connect with SSH",
                    " ".join(self.ssh_address.build_ssh_command(config, extra_args=[])),
                )
            )

        out = ""
        for (name, value) in parts:
            out += f"* {name}: {value}\n"

        return out


@dataclass(frozen=True)
class WorkspaceInfo:
    """Metadata for a running workspace (container)."""

    image: str
    memory_limit: Optional[str]
    cpu_limit: Optional[str]

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> WorkspaceInfo:
        """Parse from a raw dictionary."""
        return cls(
            image=data["image"],
            memory_limit=data["memory_limit"],
            cpu_limit=data["memory_limit"],
        )


# API CLient.
class Api:
    """API Client."""

    config: Config

    def __init__(self, config: Config):
        self.config = config

    def query(self, data: AnyDict) -> AnyDict:
        """Send a query to the API."""

        data_json = json.dumps(data).encode("utf-8")
        req = urllib.request.Request(
            self.config.api_endpoint(),
            method="POST",
            headers={"content-type": "application/json"},
            data=data_json,
        )
        res = urllib.request.urlopen(req)  # pylint: disable=consider-using-with
        res_data = json.load(res)
        if "Ok" in res_data:
            data = res_data["Ok"]
            assert isinstance(data, object)
            return data
        if "Error" in res_data:
            msg = res_data["Error"]["message"]
            raise ApiError(msg)
        raise Exception("Invalid API response")

    def _query_pod(self, what: str) -> WorkspaceStatus:
        query = {
            what: {
                "username": self.config.username,
                "ssh_public_key": self.config.ssh_public_key,
            },
        }
        data = self.query(query)

        status = data[what]
        username = status.get("username")
        phase = WorkspacePhase(status.get("phase", "unknown"))
        ssh_address = (
            SshAddress(
                status["ssh_address"]["address"], int(status["ssh_address"]["port"])
            )
            if status.get("ssh_address", None)
            else None
        )
        info = (
            WorkspaceInfo.from_dict(status["info"])
            if "info" in status and status["info"] is not None
            else None
        )
        return WorkspaceStatus(
            username=username, phase=phase, ssh_address=ssh_address, info=info
        )

    def pod_start(self) -> WorkspaceStatus:
        """Start a workspace."""
        return self._query_pod("PodStart")

    def pod_status(self) -> WorkspaceStatus:
        """Get the current status of a workspace."""
        return self._query_pod("PodStatus")

    def pod_stop(self) -> None:
        """Stop a workspace."""
        self._query_pod("PodStop")


def await_workspace_available(api: Api) -> WorkspaceStatus:
    """Wait until a workspace is started and reachable over SSH.
    Provides feedback via stdout.
    """
    current_phase = WorkspacePhase.UNKNOWN
    while True:
        res = api.pod_start()
        if res.phase != current_phase:
            log(f"\n{res.phase.value}->", end="")
            current_phase = res.phase
        if res.phase == WorkspacePhase.READY and res.ssh_address is not None:
            log("")
            return res
        log("*", end="", flush=True)
        time.sleep(2)


def run_start(api: Api) -> WorkspaceStatus:
    """Run the `start` commmand."""

    status = api.pod_status()
    ssh = status.ssh_address
    if status.phase == WorkspacePhase.READY and ssh:
        log("Your workspace is already running!")
        log(status.render_info(api.config))
        return status

    curent_phase = status.phase
    log(f"Launching your workspace from phase: {curent_phase.value}")
    log("This might take a few minutes. Please be patient.")
    res = await_workspace_available(api)

    log("\nPod is ready!")
    return res


def run_connect(api: Api, port_forwards: list[str], user_command: list[str]) -> None:
    """Start a workspace and connect to it via SSH."""
    status = run_start(api)

    extra_args = []
    for forward in port_forwards:
        try:
            spec = PortForwardSpec.parse(forward)
            extra_args += spec.build_ssh_args()
        except Exception as err:  # pylint: disable=broad-except
            log(f'INVALID port-forward spec "{forward}": {err}')
            log('Consult "kworkspace connect --help"')
            sys.exit(1)

    if status.ssh_address:
        ssh_cmd = status.ssh_address.build_ssh_command(api.config, extra_args)
        cmd = ssh_cmd + user_command

        log("Connecting via SSH...")
        log("Running command: " + " ".join(cmd))
        subprocess.run(cmd, check=True)
    else:
        # Only there to satisfy the type checker.
        raise Exception("Internal error: expected SSH address to be available")


def run_stop(api: Api) -> None:
    """Run the `stop` command."""

    status = api.pod_status()
    if status.phase == WorkspacePhase.NOT_FOUND:
        log("Your workspace is already stopped")
        return

    log("Stopping workspace...")
    api.pod_stop()
    while True:
        res = api.pod_status()
        if res.phase == WorkspacePhase.NOT_FOUND:
            break
        log("*", end="")
        time.sleep(2)
    log("\nWorkspace was shut down.")
    log("Run workspaces.py start to start it again")


@dataclass
class PortForwardSpec:
    """SSH port forward specification."""

    local_port: int
    remote_port: int
    remote_host: str

    @classmethod
    def parse(cls, spec: str) -> PortForwardSpec:
        """Parse a spec from a string."""
        parts = spec.strip().split(":")
        count = len(parts)

        local_port = 0
        remote_port = 0
        remote_host = "127.0.0.1"

        if count == 1:
            remote_port = int(parts[0])
            local_port = remote_port
        elif count == 2:
            local_port = int(parts[0])
            remote_port = int(parts[1])
        elif count == 3:
            local_port = int(parts[0])
            remote_host = parts[1]
            remote_port = int(parts[2])
        else:
            raise Exception("Invalid (empty) port spec")

        return cls(
            local_port=local_port, remote_port=remote_port, remote_host=remote_host
        )

    def build_ssh_args(self) -> list[str]:
        """Build the ssh CLI arguments for this port forward."""
        return ["-L", f"{self.local_port}:{self.remote_host}:{self.remote_port}"]


@dataclass(frozen=True)
class Args:
    """Parsed command line arguments."""

    command: str
    user: Optional[str]
    ssh_key_path: Optional[str]
    api_url: Optional[str]
    config_path: Optional[str]


def arg_parser() -> argparse.ArgumentParser:
    """Create argparser for CLI arguments."""

    parser = argparse.ArgumentParser(description="Kubernetes workspace manager")
    parser.add_argument(
        "--user",
        help="Username to use. Defaults to the current OS username",
    )
    parser.add_argument(
        "--ssh-key-path",
        help="Path of SSH public key to use. Defaults to $HOME/.ssh/id_ras.pub",
    )
    parser.add_argument(
        "--api", help="The API URL. Like: http://workspace-manager.DOMAIN.com"
    )
    parser.add_argument(
        "--config",
        help="Config file path. Defaults to $HOME/.config/kube-workspaces/config.json",
    )

    subs = parser.add_subparsers(dest="subcommand", required=True)

    subs.add_parser("start", help="Start your workspace container.")

    # Connect.
    cmd_connect = subs.add_parser("connect", help="Connect to your workspace with SSH.")
    cmd_connect.add_argument(
        "-f",
        "--port-forward",
        type=str,
        nargs="*",
        action="extend",
        dest="forward",
        help=(
            "The remote specification.\n"
            + "Mirrors SSH -L format."
            + "Eg: "
            + "* '80' => forward local port 80 to remote localhost:80"
            + "* '8000:80' => forward local port 8000 to remote localhost:80"
            + "* '8000:domain.com:80' => forward local port 8000 to remote domain.com:80"
        ),
    )
    cmd_connect.add_argument(
        "command",
        nargs="*",
        action="extend",
        help="Command to execute on the remote host.",
        default=[],
    )

    subs.add_parser("stop", help="Stop your workspace container.")
    return parser


def run() -> None:
    """Run the CLI."""

    parser = arg_parser()
    namespace = parser.parse_args()
    assert isinstance(namespace.subcommand, str)
    args = Args(
        namespace.subcommand,
        namespace.user,
        namespace.ssh_key_path,
        namespace.api,
        namespace.config,
    )

    file = ConfigFile.load(not args.api_url, custom_path=args.config_path)

    user = args.user or file.username or host_username()
    ssh_key_path = args.ssh_key_path or file.ssh_key_path or ""
    ssh_public_key = ""
    if not ssh_key_path:
        ssh_key_path = os.path.expanduser("~/.ssh/id_rsa.pub")
    ssh_private_key_path = ssh_key_path.removesuffix(".pub")

    if os.path.isfile(ssh_key_path):
        with open(ssh_key_path, encoding="utf8") as keyfile:
            ssh_public_key = keyfile.read().strip()
    else:
        log(
            "Error: Could not determine ssh key path to use: no file at " + ssh_key_path
        )
        log("Configure key in config or with --ssh-key-path=PATH")
        sys.exit(1)

    url = args.api_url or file.api_url
    if not url:
        log(
            "Error: Could not determine API endpoint: specify in config or with --api=http://..."
        )
        sys.exit(1)

    config = Config(
        username=user,
        ssh_public_key=ssh_public_key,
        ssh_private_key_path=ssh_private_key_path,
        api_url=url,
    )
    api = Api(config)

    if args.command == "start":
        run_start(api)
    elif args.command == "connect":
        forwards = namespace.forward or []
        cmd = namespace.command
        run_connect(api, port_forwards=forwards, user_command=cmd)
    elif args.command == "stop":
        run_stop(api)
    else:
        raise Exception("Invalid subcommand")


if __name__ == "__main__":
    run()
