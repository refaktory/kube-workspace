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
        system = "x86_64-linux";
        pkgs = nixpkgs.legacyPackages."${system}";
        naersk-lib = naersk.lib."${system}";
      in rec {

        packages.kube-workspace-operator = naersk-lib.buildPackage {
          pname = "kube-workspace-operator";
          src = self;
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

        defaultPackage = packages.kube-workspace-operator;

        apps.kube-workspace-operator = flakeutils.lib.mkApp {
          drv = packages.kube-workspace-operator;
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
              python38Packages.pylint

              # kind (Kubernetes in Docker) for integration tests.
              # Custom package because no official one exists in nixpks.
              (pkgs.stdenv.mkDerivation rec {
                name = "kind";

                executable = fetchurl {
                  url = "https://github.com/kubernetes-sigs/kind/releases/download/v0.10.0/kind-linux-amd64";
                  sha256 = "74767776488508d847b0bb941212c1cb76ace90d9439f4dee256d8a04f1309c6";
                };

                phases = [ "installPhase" ];

                installPhase = ''
                  mkdir -p $out/bin
                  cp ${executable} $out/bin/kind
                  chmod +x $out/bin/kind
                '';
              })
            ];
            propagatedBuildInputs = with pkgs; [
              openssl
            ];
            buildPhase = "";
            installPhase = "";

            # Allow `cargo run` etc to find ssl lib.
            LD_LIBRARY_PATH = "${pkgs.openssl.out}/lib";
            RUST_BACKTRACE = "1";
            # Use lld linker for speedup.
            RUSTFLAGS = "-C link-arg=-fuse-ld=lld";
            RUST_LOG = "kube_workspace_operator=trace";
        };

        dockerImage = pkgs.dockerTools.buildImage {
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
      }
    );
}  