#!/usr/bin/env python3

"""CLI client for the kube-workspace-operator workspace manager.
Allows starting and stopping workspaces on a Kubernetes cluster.
"""

from __future__ import annotations

import os
import typing
import getpass
import argparse
import sys
import json
import urllib.request
import time
from urllib.parse import urlparse

AnyDict = typing.Dict[str, typing.Any]

# CLI config file.
class ConfigFile:
    """Parsed config file settings."""

    username: typing.Optional[str]
    ssh_key_path: typing.Optional[str]
    api_url: typing.Optional[str]

    def __init__(
        self,
        user: typing.Optional[str],
        ssh_key_path: typing.Optional[str],
        api_url: typing.Optional[str],
    ):
        self.username = user
        self.ssh_key_path = ssh_key_path
        self.api_url = api_url

    @staticmethod
    def user_path() -> str:
        """Get the default config file path for the current user"""

        return os.path.expanduser("~/.config/kube-workspaces/config.json")

    # Prompts for config options and creates the config file.
    @staticmethod
    def initialize() -> ConfigFile:
        """Initialize a config file by prompting the user for settings and
        writing the file to the default location.
        """

        url: typing.Optional[str] = None
        while not url:
            url = input("API URL (http://DOMAIN.com[:port]): ")
            try:
                res = urlparse(url)
                if not res.scheme in ["http", "https"]:
                    url = ""
            except:
                url = ""

        user: typing.Optional[str] = None
        user = input("Username (leave empty to use current system user): ").strip()
        if not user:
            user = None

        default_ssh_key_path = os.path.expanduser("~/.ssh/id_rsa.pub")
        ssh_path: typing.Optional[str]
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

        config = ConfigFile(user=user, ssh_key_path=ssh_path, api_url=url)

        path = ConfigFile.user_path()
        config_dir = os.path.dirname(path)
        if not os.path.isdir(config_dir):
            os.makedirs(config_dir)
        with open(path, mode="w") as f:
            json.dump(config.__dict__, f)
        print(f"Config written to {path}")
        return config

    @staticmethod
    def load(
        auto_initialize: bool, custom_path: typing.Optional[str] = None
    ) -> ConfigFile:
        """Load config from disk.
        If auto_initialize is true, prompts the user for config options.
        Otherwise, it returns an empty config if file does not exist.
        """

        path: str = custom_path or ConfigFile.user_path()
        if os.path.isfile(path):
            data: typing.Optional[typing.Dict[str, str]] = None
            with open(path) as file:
                data = json.load(file)
            if not data:
                return ConfigFile(None, None, None)
            return ConfigFile(
                user=data["username"],
                ssh_key_path=data["ssh_key_path"],
                api_url=data["api_url"],
            )
        if auto_initialize:
            return ConfigFile.initialize()
        return ConfigFile(None, None, None)


class Config:
    """Materialized CLI config."""

    username: str
    ssh_key: str
    api_url: str

    def __init__(self, username: str, ssh_key: str, api_url: str):
        self.username = username
        self.ssh_key = ssh_key
        self.api_url = api_url

    def api_endpoint(self) -> str:
        """Compute query endpoint."""
        return self.api_url + "/api/query"


class SshAddress(typing.TypedDict):
    """API type for an ssh address and port."""

    address: str
    port: int


class PodStatus(typing.TypedDict):
    """Api response for the PodStart and PodStatus query."""

    is_ready: bool
    ssh_address: typing.Optional[SshAddress]


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
        res = urllib.request.urlopen(req)
        res_data = json.load(res)
        if "Ok" in res_data:
            data = res["Ok"]
            assert isinstance(data, object)
            return data
        if "Error" in res_data:
            msg = res_data["Error"]["message"]
            raise Exception("API request failed: " + msg)
        raise Exception("Invalid API response")

    def pod_start(self) -> PodStatus:
        """Start a workspace."""

        output = self.query(
            {
                "PodStart": {
                    "username": self.config.username,
                    "ssh_public_key": self.config.ssh_key,
                },
            }
        )["PodStart"]
        return typing.cast(PodStatus, output)

    def pod_status(self) -> PodStatus:
        """Get the current status of a workspace."""

        data = self.query(
            {
                "PodStatus": {
                    "username": self.config.username,
                    "ssh_public_key": self.config.ssh_key,
                },
            }
        )
        return typing.cast(PodStatus, data["PodStatus"])

    def pod_stop(self) -> None:
        """Stop a workspace."""
        self.query(
            {
                "PodStop": {
                    "username": self.config.username,
                    "ssh_public_key": self.config.ssh_key,
                },
            }
        )


def run_start(api: Api) -> None:
    """Run the `start` commmand."""

    print("Starting pod...")
    res = api.pod_start()
    print("Started. Waiting for pod to become reachable...")
    while True:
        res = api.pod_status()
        if res["is_ready"] and res["ssh_address"]:
            break
        print("*", end="", flush=True)
        time.sleep(1)

    print("\nPod is ready!")

    user_prefix = (
        api.config.username + "@" if current_username() != api.config.username else ""
    )
    assert isinstance(res["ssh_address"], dict)
    ssh = res["ssh_address"]
    if ssh:
        port = ssh["port"]
        addr = ssh["address"]
        print(f"Connect via ssh -p {port} {user_prefix}{addr}")
    else:
        print("SSH not ready yet - call `start` again")


def run_stop(api: Api) -> None:
    """Run the `stop` command."""

    print("Stopping pod...")
    api.pod_stop()
    # TODO: poll until termination is complete
    # while True:
    #     res = api.pod_status()
    #     if res['is_ready'] and res['ssh_address']:
    #         break
    #     print('*', end='')
    #     time.sleep(1)
    print("Pod was deleted")
    print("Run workspaces.py start to start it again")


def current_username() -> str:
    """Get the current username from the OS."""
    return getpass.getuser()


class Args(typing.TypedDict):
    """Parsed command line arguments."""

    command: str
    user: typing.Optional[str]
    ssh_key_path: typing.Optional[str]
    api_url: typing.Optional[str]
    config_path: typing.Optional[str]


def parse_args() -> Args:
    """Parse CLI arguments with argparse."""

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

    args = parser.parse_args()
    assert isinstance(args.subcommand, str)
    return {
        "command": args.subcommand,
        "user": args.user,
        "ssh_key_path": args.ssh_key_path,
        "api_url": args.api,
        "config_path": args.config,
    }


def run() -> None:
    """Run the CLI."""

    args = parse_args()
    file = ConfigFile.load(not args["api_url"], custom_path=args["config_path"])

    user = args["user"] or file.username or current_username()
    ssh_key_path = args["ssh_key_path"] or file.ssh_key_path
    if not ssh_key_path:
        ssh_key_path = os.path.expanduser("~/.ssh/id_rsa.pub")

    if os.path.isfile(ssh_key_path):
        with open(ssh_key_path) as file:
            ssh_key = file.read().strip()
    else:
        print(
            "Error: Could not determine ssh key path to use: no file at " + ssh_key_path
        )
        print("Configure key in config or with --ssh-key-path=PATH")
        sys.exit(1)

    url = args["api_url"] or file.api_url
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

    cmd = args["command"]
    if cmd == "start":
        run_start(api)
    elif cmd == "stop":
        run_stop(api)
    else:
        raise Exception("Invalid subcommand")


if __name__ == "__main__":
    run()
