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
              mypy
            ];
            propagatedBuildInputs = with pkgs; [
              openssl
            ];
            buildPhase = "";
            installPhase = "";

            RUST_BACKTRACE = "1";
            RUSTFLAGS = "-C link-arg=-fuse-ld=lld";
            LD_LIBRARY_PATH = "${pkgs.openssl.out}/lib";
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
