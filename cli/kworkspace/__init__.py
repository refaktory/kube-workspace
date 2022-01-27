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

AnyDict = Dict[str, Any]


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
            print("Default SSH key detected at " + default_ssh_key_path)
            while True:
                ssh_path = input(
                    "Alternative key (leave empty to use default): "
                ).strip()
                if ssh_path:
                    if not os.path.isfile(ssh_path):
                        print("Path is not valid")
                    else:
                        break
                else:
                    ssh_path = default_ssh_key_path
                    break
        else:
            print("No default SSH key detected")
            while True:
                ssh_path = input("SSH key path: ").strip()
                if ssh_path:
                    if not os.path.isfile(ssh_path):
                        print("Path is not valid")
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

        user = (
            input("Username (leave empty to use current system user): ").strip() or None
        )

        ssh_path = cls.initialize_ssh_path()

        config = cls(username=user, ssh_key_path=ssh_path, api_url=url)

        path = ConfigFile.user_path()
        config_dir = os.path.dirname(path)
        if not os.path.isdir(config_dir):
            os.makedirs(config_dir)
        with open(path, mode="w") as file:
            json.dump(asdict(config), file)
        print(f"Config written to {path}")
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
            with open(path) as file:
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
    ssh_key: str
    api_url: str

    def api_endpoint(self) -> str:
        """Compute query endpoint."""
        return self.api_url + "/api/query"


@dataclass(frozen=True)
class SshAddress:
    """API type for an ssh address and port."""

    address: str
    port: int


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

    phase: WorkspacePhase
    ssh_address: Optional[SshAddress]
    info: Optional[WorkspaceInfo]


@dataclass(frozen=True)
class WorkspaceInfo:
    image: str
    memory_limit: Optional[str]
    cpu_limit: Optional[str]


    def print_for_cli(self) -> str:
        out = f'  * Image: {self.image}'
        if self.memory_limit != None:
            out += f'\n    Memory: {self.memory_limit}'
        if self.cpu_limit != None:
            out += f'\n    CPU: {self.memory_limit}'
        return out

    @classmethod
    def from_dict(cls, data: dict) -> WorkspaceInfo:
        return cls(
            image=data['image'],
            memory_limit=data['memory_limit'],
            cpu_limit=data['memory_limit'],
        )


# API CLient.
class Api:
    """API Client."""

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
        res = urllib.request.urlopen(req)
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
                "ssh_public_key": self.config.ssh_key,
            },
        }
        data = self.query(query)

        status = data[what]
        phase = WorkspacePhase(status.get("phase", "unknown"))
        ssh_address = (
            SshAddress(
                status["ssh_address"]["address"], int(status["ssh_address"]["port"])
            )
            if status.get("ssh_address", None)
            else None
        )
        info = WorkspaceInfo.from_dict(status['info']) if 'info' in status else None
        return WorkspaceStatus(phase, ssh_address, info)

    def pod_start(self) -> WorkspaceStatus:
        """Start a workspace."""
        return self._query_pod("PodStart")

    def pod_status(self) -> WorkspaceStatus:
        """Get the current status of a workspace."""
        return self._query_pod("PodStatus")

    def pod_stop(self) -> None:
        """Stop a workspace."""
        self._query_pod("PodStop")


def run_start(api: Api) -> None:
    """Run the `start` commmand."""

    user_prefix = (
        api.config.username + "@" if current_username() != api.config.username else ""
    )

    status = api.pod_status()
    ssh = status.ssh_address
    if status.phase == WorkspacePhase.READY and ssh:
        print("Your workspace is already running!")
        port = ssh.port
        addr = ssh.address

        if status.info != None:
          print(status.info.print_for_cli())

        print(f"Connect via ssh -p {port} {user_prefix}{addr}")
        return

    curent_phase = status.phase
    print(f"Launching your workspace from phase: {curent_phase.value}")
    print("This might take a few minutes. Please be patient.")
    while True:
        res = api.pod_start()
        if res.phase != curent_phase:
            print(f"\n{res.phase.value}->", end="")
            curent_phase = res.phase
        if res.phase == WorkspacePhase.READY:
            break
        print("*", end="", flush=True)
        time.sleep(2)

    print("\nPod is ready!")

    if res.info != None:
        print(res.info.print_for_cli())
    ssh = res.ssh_address
    if ssh:
        port = ssh.port
        addr = ssh.address
        print(f"Connect via ssh -p {port} {user_prefix}{addr}")
    else:
        print("SSH not ready yet - call `start` again")


def run_stop(api: Api) -> None:
    """Run the `stop` command."""

    status = api.pod_status()
    if status.phase == WorkspacePhase.NOT_FOUND:
        print("Your workspace is already stopped")
        return

    print("Stopping workspace...")
    api.pod_stop()
    while True:
        res = api.pod_status()
        if res.phase == WorkspacePhase.NOT_FOUND:
            break
        print("*", end="")
        time.sleep(2)
    print("\nWorkspace was shut down.")
    print("Run workspaces.py start to start it again")


def current_username() -> str:
    """Get the current username from the OS."""
    return getpass.getuser()


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
        "--user", help="Username to use. Defaults to the current OS username"
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

    user = args.user or file.username or current_username()
    ssh_key_path = args.ssh_key_path or file.ssh_key_path
    if not ssh_key_path:
        ssh_key_path = os.path.expanduser("~/.ssh/id_rsa.pub")

    if os.path.isfile(ssh_key_path):
        with open(ssh_key_path) as keyfile:
            ssh_key = keyfile.read().strip()
    else:
        print(
            "Error: Could not determine ssh key path to use: no file at " + ssh_key_path
        )
        print("Configure key in config or with --ssh-key-path=PATH")
        sys.exit(1)

    url = args.api_url or file.api_url
    if not url:
        print(
            "Error: Could not determine API endpoint: specify in config or with --api=http://..."
        )
        sys.exit(1)

    config = Config(
        username=user,
        ssh_key=ssh_key,
        api_url=url,
    )
    api = Api(config)

    if args.command == "start":
        run_start(api)
    elif args.command == "stop":
        run_stop(api)
    else:
        raise Exception("Invalid subcommand")


if __name__ == "__main__":
    run()
