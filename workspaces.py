#!/usr/bin/env python3
# Python client CLI for Kubernetes workspaces.

from __future__ import annotations

import os, typing, getpass, argparse, sys, json, urllib.request, time
from urllib.parse import urlparse

from pprint import pprint

# CLI config file.
class ConfigFile:
    username: typing.Optional[str]
    ssh_key_path: typing.Optional[str]
    api_url: typing.Optional[str]

    def default(self, o):
        return o.__dict__

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
        return os.path.expanduser("~/.config/kube-workspaces/config.json")

    # Prompts for config options and creates the config file.
    @staticmethod
    def initialize() -> ConfigFile:
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

    # Load config from disk.
    # If auto_initialize is true, prompts the user for config options.
    # Otherwise, it returns an empty config if file does not exist.
    @staticmethod
    def load(auto_initialize: bool) -> ConfigFile:
        path = ConfigFile.user_path()
        if os.path.isfile(path):
            data: typing.Optional[typing.Dict[str, str]] = None
            with open(path) as f:
                data = json.load(f)
            if not data:
                return ConfigFile(None, None, None)
            else:
                return ConfigFile(
                    user=data["username"],
                    ssh_key_path=data["ssh_key_path"],
                    api_url=data["api_url"],
                )
        elif auto_initialize:
            return ConfigFile.initialize()
        else:
            return ConfigFile(None, None, None)


# Materialized CLI config.
class Config:
    username: str
    ssh_key: str
    api_url: str

    def __init__(self, username: str, ssh_key: str, api_url: str):
        self.username = username
        self.ssh_key = ssh_key
        self.api_url = api_url

    def api_endpoint(self) -> str:
        return self.api_url + "/api/query"


# API CLient.
class Api:
    config: Config

    def __init__(self, config: Config):
        self.config = config

    def request(self, data: object) -> object:
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
            return res_data["Ok"]
        elif "Error" in res_data:
            msg = res_data["Error"]["message"]
            raise Exception("API request failed: " + msg)
        else:
            raise Exception("Invalid API response")

    def pod_start(self):
        return self.request(
            {
                "PodStart": {
                    "username": self.config.username,
                    "ssh_public_key": self.config.ssh_key,
                },
            }
        )["PodStart"]

    def pod_status(self):
        data = self.request(
            {
                "PodStatus": {
                    "username": self.config.username,
                    "ssh_public_key": self.config.ssh_key,
                },
            }
        )
        return data["PodStatus"]

    def pod_stop(self):
        return self.request(
            {
                "PodStop": {
                    "username": self.config.username,
                    "ssh_public_key": self.config.ssh_key,
                },
            }
        )


# Run the 'start' command.
def run_start(api: Api):
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
    print(
        f"Connect via ssh -p {res['ssh_address']['port']} {user_prefix}{res['ssh_address']['address']}"
    )


# Run the 'stop' command.
def run_stop(api: Api):
    print("Stopping pod...")
    res = api.pod_stop()
    # TODO: poll until termination is complete
    # while True:
    #     res = api.pod_status()
    #     if res['is_ready'] and res['ssh_address']:
    #         break
    #     print('*', end='')
    #     time.sleep(1)
    print("Pod was deleted")
    print("Run workspaces.py start to start it again")


# Get the current username from the OS.
def current_username() -> typing.Optional[str]:
    return getpass.getuser()


def parse_args() -> [str, ConfigFile]:
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

    subs = parser.add_subparsers(dest="subcommand")

    start = subs.add_parser("start", help="Start your workspace container.")
    stop = subs.add_parser("stop", help="Stop your workspace container.")

    args = parser.parse_args()
    return [
        args.subcommand,
        ConfigFile(args.user, ssh_key_path=args.ssh_key_path, api_url=args.api),
    ]


# Run the CLI.
def run():
    [cmd, args] = parse_args()
    file = ConfigFile.load(not args.api_url)

    user = args.username or file.username or current_username()
    ssh_key_path = args.ssh_key_path or file.ssh_key_path
    if not ssh_key_path:
        ssh_key_path = os.path.expanduser("~/.ssh/id_rsa.pub")

    if os.path.isfile(ssh_key_path):
        with open(ssh_key_path) as f:
            ssh_key = f.read().strip()
    else:
        print(
            "Error: Could not determine ssh key path to use: no file at " + ssh_key_path
        )
        print("Configure key in config or with --ssh-key-path=PATH")
        sys.exit(1)

    api = args.api_url or file.api_url
    if not api:
        print(
            "Error: Could not determine API endpoint: specify in config or with --api=http://..."
        )
        sys.exit(1)

    config = Config(
        username=user,
        ssh_key=ssh_key,
        api_url=api,
    )
    api = Api(config)

    if cmd == "start":
        run_start(api)
    elif cmd == "stop":
        run_stop(api)
    else:
        raise Exception("Invalid subcommand")


if __name__ == "__main__":
    run()
