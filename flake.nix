{
  description = "Sheprd: a native GPUI client for an existing local Herdr daemon";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # Build with exactly the toolchain rust-toolchain.toml pins, not whatever
    # rustc the nixpkgs revision happens to carry.
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
    }:
    let
      inherit (nixpkgs) lib;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems =
        f:
        lib.genAttrs systems (
          system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
            }
          )
        );
      herdrCoreData = builtins.fromJSON (builtins.readFile ./scripts/release/herdr-core.json);
      package =
        pkgs:
        let
          inherit (pkgs.stdenv.hostPlatform) isDarwin isLinux;
          toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = toolchain;
            rustc = toolchain;
          };
          # Loaded with dlopen at runtime on Linux, so the linker never records them.
          runtimeLibraries = lib.optionals isLinux [
            pkgs.vulkan-loader
            pkgs.wayland
          ];
          # herdr-core is a prebuilt binary; on Linux it targets FHS and is not patched
          # for NixOS, so Linux users supply their own daemon or PATH package. On macOS,
          # Mach-O binaries use system dyld and run natively, so we bundle it into Sheprd.app.
          herdrCoreTarget = if isDarwin && pkgs.stdenv.hostPlatform.isAarch64 then "macos-aarch64" else null;
          herdrCore =
            if herdrCoreTarget != null then
              let
                entry = herdrCoreData.targets.${herdrCoreTarget};
                tarball = pkgs.fetchurl {
                  inherit (entry) url sha256;
                };
              in
              pkgs.runCommand "herdr-core" { } ''
                mkdir -p $out/bin
                tar -xzf ${tarball} -C $out/bin
                chmod 755 $out/bin/herdr-core
              ''
            else
              null;
          nearby = pkgs.callPackage ./macos/sheprd-nearby/package.nix { };
        in
        rustPlatform.buildRustPackage {
          pname = "sheprd";
          version = "0-unstable-${lib.substring 0 8 (self.lastModifiedDate or "19700101")}";
          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./rust-toolchain.toml
              ./crates
              ./assets
              # The agent skill is compiled into the executable.
              ./skills
              ./LICENSE
              ./NOTICE
              ./scripts/release/org.sheprd.Sheprd.desktop
              # The herdr-core release a remote install pins is compiled in.
              ./scripts/release/herdr-core.json
              ./scripts/release/herdr-core-licenses.txt
            ];
          };
          cargoLock = {
            lockFile = ./Cargo.lock;
            # All Moonframe crates share this pinned Git checkout, including
            # when the optional feature is off and Cargo only vendors it.
            outputHashes."moonframe-0.3.1" = "sha256-DSroyTi58a5T15DKmZCs7v0vnYI6CtHxhaeMUw4xyYQ=";
          };
          # The app starts chats with the sheprd-acp beside its executable,
          # runs the sheprd-remote-control beside it as a user service, runs
          # automations in the sheprd-automate there, and starts sheprd-nearby
          # for nearby pairing and presence over Bluetooth.
          cargoBuildFlags = [
            "-p"
            "sheprd-gpui"
            "-p"
            "sheprd-acp"
            "-p"
            "sheprd-remote-control"
            "-p"
            "sheprd-automate"
          ]
          ++ lib.optionals isLinux [
            "-p"
            "sheprd-nearby"
          ];
          # The optimized CLI suite CI's main-branch build runs; the rest of
          # the workspace suite needs a desktop or a live daemon.
          cargoTestFlags = [
            "-p"
            "sheprd-gpui"
            "--test"
            "cli"
          ];
          nativeBuildInputs = [
            pkgs.pkg-config
            rustPlatform.bindgenHook
          ]
          ++ lib.optionals isLinux [
            pkgs.makeWrapper
          ];
          buildInputs =
            lib.optionals isLinux [
              pkgs.alsa-lib
              pkgs.fontconfig
              pkgs.freetype
              pkgs.libxkbcommon
              pkgs.libxcb
            ]
            ++ runtimeLibraries
            ++ lib.optionals isDarwin [
              pkgs.apple-sdk_14
              pkgs.libiconv
            ];

          env = lib.optionalAttrs isDarwin {
            MACOSX_DEPLOYMENT_TARGET = "14.2";
          };

          # No updater key is embedded, so this build never replaces itself;
          # Nix owns the store path and upgrades come from the flake.
          postInstall =
            if isDarwin then
              ''
                app="$out/Applications/Sheprd.app"
                mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

                # Relocate compiled executables into the macOS app bundle
                for bin in sheprd sheprd-acp sheprd-remote-control sheprd-automate; do
                  mv "$out/bin/$bin" "$app/Contents/MacOS/$bin"
                done
                install -Dm755 ${nearby}/bin/sheprd-nearby "$app/Contents/MacOS/sheprd-nearby"

                ${lib.optionalString (herdrCore != null) ''
                  install -Dm755 ${herdrCore}/bin/herdr-core "$app/Contents/MacOS/herdr-core"
                ''}

                # Legacy and helper symlinks in bundle
                ln -s sheprd "$app/Contents/MacOS/herdr-gpui"
                for helper in acp automate nearby remote-control; do
                  ln -s "sheprd-$helper" "$app/Contents/MacOS/herdr-$helper"
                done

                # Bundle metadata and resources
                cp assets/macos/Info.plist "$app/Contents/Info.plist"
                cp assets/icons/Sheprd.icns "$app/Contents/Resources/Sheprd.icns"
                cp assets/icons/Sheprd.car "$app/Contents/Resources/Assets.car"
                cp LICENSE NOTICE "$app/Contents/Resources/"
                cp crates/sheprd-gpui/SOUND-NOTICE.md "$app/Contents/Resources/"
                cp assets/icons/LICENSE-octicons "$app/Contents/Resources/"
                cp scripts/release/herdr-core-licenses.txt "$app/Contents/Resources/HERDR-CORE-LICENSES.txt"

                # Expose binaries on PATH via symlinks pointing into the bundle
                mkdir -p $out/bin
                for executable in sheprd sheprd-acp sheprd-remote-control sheprd-automate sheprd-nearby; do
                  ln -s "$app/Contents/MacOS/$executable" "$out/bin/$executable"
                done
                ln -s "$app/Contents/MacOS/sheprd" "$out/bin/herdr-gpui"
                for helper in acp automate nearby remote-control; do
                  ln -s "$app/Contents/MacOS/sheprd-$helper" "$out/bin/herdr-$helper"
                done
                ${lib.optionalString (herdrCore != null) ''
                  ln -s "$app/Contents/MacOS/herdr-core" "$out/bin/herdr-core"
                ''}
              ''
            else
              ''
                install -Dm644 scripts/release/org.sheprd.Sheprd.desktop \
                  $out/share/applications/org.sheprd.Sheprd.desktop
                install -Dm644 assets/icons/sheprd-linux.svg \
                  $out/share/icons/hicolor/scalable/apps/sheprd.svg
              '';

          postFixup = lib.optionalString isLinux ''
            patchelf --add-rpath ${lib.makeLibraryPath runtimeLibraries} $out/bin/sheprd
            wrapProgram $out/bin/sheprd \
              --prefix PATH : ${
                lib.makeBinPath [
                  pkgs.wl-clipboard
                  pkgs.xclip
                ]
              }
            # Point at the final wrapper, including its clipboard PATH setup.
            ln -s sheprd $out/bin/herdr-gpui
            for helper in acp automate nearby remote-control; do
              ln -s "sheprd-$helper" "$out/bin/herdr-$helper"
            done
          '';
          meta = {
            description = "Native GPUI client for an existing local Herdr daemon";
            homepage = "https://sheprd.org";
            license = lib.licenses.asl20;
            mainProgram = "sheprd";
            platforms = systems;
          };
        };
    in
    {
      packages = forAllSystems (pkgs: {
        default = package pkgs;
        sheprd = package pkgs;
        herdr-gpui = package pkgs;
      });
      overlays.default = final: _prev: {
        sheprd = self.packages.${final.stdenv.hostPlatform.system}.default;
        herdr-gpui = self.packages.${final.stdenv.hostPlatform.system}.default;
      };
    };
}
