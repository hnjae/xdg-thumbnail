# SPDX-FileCopyrightText: 2026 KIM Hyunjae
# SPDX-License-Identifier: AGPL-3.0-or-later

{
  description = "Freedesktop thumbnail cache tools and library";

  inputs = {
    crane.url = "github:ipetkov/crane";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    {
      crane,
      nixpkgs,
      ...
    }:
    let
      lib = nixpkgs.lib;
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      forAllSystems = lib.genAttrs systems;
      perSystem = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          craneLib = crane.mkLib pkgs;
          src = craneLib.cleanCargoSource ./.;

          commonArgs = {
            pname = "xdg-thumbnail";
            version = "0.1.0";

            inherit src;

            cargoExtraArgs = "--locked --workspace --all-features";
            strictDeps = true;

            meta = {
              description = "Freedesktop thumbnail cache tools and library";
              homepage = "https://github.com/hnjae/xdg-thumbnail";
              license = [
                lib.licenses.agpl3Plus
                lib.licenses.mpl20
              ];
              mainProgram = "xdg-thumbnail-generate";
              platforms = lib.platforms.unix;
            };
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          lintAndTest = craneLib.mkCargoDerivation (
            commonArgs
            // {
              pname = "xdg-thumbnail-lint-and-test";
              inherit cargoArtifacts;

              nativeBuildInputs = [
                pkgs.clippy
              ];

              buildPhaseCargoCommand = # sh
                ''
                  cargo clippy --profile release --locked --workspace --all-targets --all-features -- -D warnings
                  cargo test --profile release --locked --workspace --all-features
                '';

              installPhaseCommand = "mkdir -p $out";
            }
          );

          package = craneLib.buildPackage (
            commonArgs
            // {
              cargoArtifacts = lintAndTest;
              doCheck = false;

              nativeBuildInputs = [
                pkgs.installShellFiles
              ]
              ++ lib.optionals pkgs.stdenv.hostPlatform.isLinux [
                pkgs.makeWrapper
              ];

              postInstall = ''
                installCliMetadata() {
                  local name="$1"
                  local artifacts
                  artifacts="$(mktemp -d)"

                  "$out/bin/$name" --generate-completion bash > "$artifacts/$name.bash"
                  "$out/bin/$name" --generate-completion fish > "$artifacts/$name.fish"
                  "$out/bin/$name" --generate-completion zsh > "$artifacts/_$name"
                  installShellCompletion --cmd "$name" \
                    --bash "$artifacts/$name.bash" \
                    --fish "$artifacts/$name.fish" \
                    --zsh "$artifacts/_$name"

                  "$out/bin/$name" --generate-manpage > "$artifacts/$name.1"
                  installManPage "$artifacts/$name.1"
                }

                installCliMetadata xdg-thumbnail-generate
                installCliMetadata xdg-thumbnail-prune

                ${lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
                  wrapProgram "$out/bin/xdg-thumbnail-generate" \
                    --prefix PATH : ${lib.makeBinPath [ pkgs.bubblewrap ]}
                ''}
              '';
            }
          );
        in
        {
          checks.default = package;
          packages = {
            default = package;
            xdg-thumbnail = package;
          };
        }
      );
    in
    {
      checks = forAllSystems (system: perSystem.${system}.checks);
      packages = forAllSystems (system: perSystem.${system}.packages);
    };
}
