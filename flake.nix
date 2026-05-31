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

                  install -Dm0644 ${./packaging/systemd/user/xdg-thumbnail-prune.timer} \
                    "$out/share/systemd/user/xdg-thumbnail-prune.timer"
                  install -Dm0644 ${./packaging/systemd/user/xdg-thumbnail-prune.service.in} \
                    "$out/share/systemd/user/xdg-thumbnail-prune.service.in"
                  sed "s|@bindir@|$out/bin|g" \
                    "$out/share/systemd/user/xdg-thumbnail-prune.service.in" \
                    > "$out/share/systemd/user/xdg-thumbnail-prune.service"
                  rm "$out/share/systemd/user/xdg-thumbnail-prune.service.in"
                ''}
              '';
            }
          );

          packagedPruneSystemdUserUnits =
            pkgs.runCommand "xdg-thumbnail-prune-systemd-user-units"
              {
                nativeBuildInputs = [
                  pkgs.systemd
                ];
              }
              ''
                service="${package}/share/systemd/user/xdg-thumbnail-prune.service"
                timer="${package}/share/systemd/user/xdg-thumbnail-prune.timer"

                test -f "$service"
                test -f "$timer"

                grep -Fx "Type=oneshot" "$service"
                grep -Fx "ExecStart=${package}/bin/xdg-thumbnail-prune --delete" "$service"

                grep -Fx "Unit=xdg-thumbnail-prune.service" "$timer"
                grep -Fx "OnCalendar=daily" "$timer"
                grep -Fx "Persistent=true" "$timer"
                grep -Fx "RandomizedDelaySec=1h" "$timer"
                grep -Fx "WantedBy=timers.target" "$timer"

                fixture="$TMPDIR/systemd-user-fixtures"
                mkdir -p "$fixture"
                printf "[Unit]\nDescription=Basic Target\n" > "$fixture/basic.target"
                printf "[Unit]\nDescription=Timers Target\n" > "$fixture/timers.target"

                runtime="$TMPDIR/systemd-runtime"
                mkdir -p "$runtime"
                chmod 700 "$runtime"

                XDG_RUNTIME_DIR="$runtime" \
                  SYSTEMD_UNIT_PATH="${package}/share/systemd/user:$fixture" \
                  systemd-analyze --user --root=/ --man=no verify "$service" "$timer"

                mkdir -p "$out"
              '';
        in
        {
          checks = {
            default = package;
          }
          // lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
            inherit packagedPruneSystemdUserUnits;
          };
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
