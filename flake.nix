{
  description = "fabric";

  inputs = {
    nixpkgs.url = github:NixOS/nixpkgs/nixos-unstable;
    flakeutils.url = "github:numtide/flake-utils";
    naersk.url = "github:nmattia/naersk";
  };

  outputs = { self, nixpkgs, flakeutils, naersk }: 
    flakeutils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages."${system}";
        naersk-lib = naersk.lib."${system}";
        pypkgs = pkgs.python38Packages;
      in rec {

        # Operator (server)
        packages.kube-workspace-operator = naersk-lib.buildPackage {
          pname = "kube-workspace-operator";
          src = builtins.filterSource
            # Filter out directories unrelated to the Rust crate.
            # This avoids rebuilds when unrelated things change.
            (path: type:
              let
                filename = (baseNameOf path);
                # TODO: only filter main path, not nested paths too
                included = !(builtins.elem filename [".github" ".gitignore" "flake.nix" "flake.lock" "cli" "deploy" "xtask" ".git"]);
              in
              included
            )
            ./.
          ;
          root = ./.;

          buildInputs = with pkgs; [
            pkgconfig
          ];
          propagatedBuildInputs = with pkgs; [
            openssl
          ];
          runtimeDependencies = with pkgs; [
            openssl
          ];
        };

        # CLI
        packages.kube-workspace-cli = pypkgs.buildPythonPackage {
          pname = "kworkspaces";
          # version = version;
          src = ./cli;

          postShellHook = ''
            mv $out/bin/kworkspaces.py $out/bin/kworkspaces
          '';

          meta = {
            homepage = "https://github.com/theduke/kube-workspaces";
            description = "CLI for kube-workspaces";
          };
        };

        # Operator Docker image.
        # To build, run `nix build .#dockerImage`.
        # This will put the image into `./result`, which can then be 
        # loaded into the Docker daemon with `docker load < ./result`.
        packages.dockerImage = pkgs.dockerTools.buildImage {
          name = "theduke/kube-workspace-operator";
          tag = "${packages.kube-workspace-operator.version}";
          config = {
            Cmd = [ "${packages.kube-workspace-operator}/bin/kube-workspace-operator" ];
            ExposedPorts = {
              "8080/tcp" = {};
            };
            Volumes = {
              "/config" = {};
            };
          };
        };

        defaultPackage = packages.kube-workspace-operator;

        apps.kube-workspace-operator = flakeutils.lib.mkApp {
          drv = packages.kube-workspace-operator;
        };

        apps.cli = flakeutils.lib.mkApp {
          drv = packages.kube-workspace-cli;
        };

        defaultApp = apps.kube-workspace-operator;

        devShell = pkgs.stdenv.mkDerivation {
            name = "kube-workspace-operator";
            src = self;
            buildInputs = with pkgs; [
              pkgconfig

              # Python formatter.
              black
              # Python type checker.
              mypy
              # Python linter.
              pypkgs.pylint

              # kind (Kubernetes in Docker) for integration tests.
              kind
            ];
            propagatedBuildInputs = with pkgs; [
              openssl
            ];
            buildPhase = "";
            installPhase = "";

            # Allow `cargo run` etc to find ssl lib.
            LD_LIBRARY_PATH = "${pkgs.openssl.out}/lib";
            RUST_BACKTRACE = "1";
            RUST_LOG = "kube_workspace_operator=trace";
            KUBE_WORKSPACE_OPERATOR_CONFIG = "./deploy/config.json";
        };

      }
    );
}  
