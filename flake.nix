{
  description = "KONTRA sampler plugin (CLAP, VST3) for NixOS";

  # The libraries the plugin links and loads come from NixOS stable: a
  # plugin runs inside the DAW's process, and one built against stable's
  # glibc loads on stable and unstable alike. Only the compiler comes from
  # unstable, for the Rust that rust-toolchain.toml pins.
  inputs = {
    nixpkgs.url = "https://channels.nixos.org/nixos-26.05/nixexprs.tar.xz";
    nixpkgs-unstable.url = "https://channels.nixos.org/nixos-unstable/nixexprs.tar.xz";
  };

  outputs = { self, nixpkgs, nixpkgs-unstable }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      unstable = nixpkgs-unstable.legacyPackages.${system};
      lib = pkgs.lib;
      rustPlatform = pkgs.makeRustPlatform { inherit (unstable) rustc cargo; };

      # What the editor opens at run time with dlopen, by name: the Vulkan
      # loader (which finds the system's GPU driver in /run/opengl-driver),
      # EGL/GL, and the X11 and Wayland client libraries. NixOS has none of
      # them on a default library path, so the plugin carries their store
      # paths in its RPATH and opens in any host without wrappers.
      runtimeLibs = with pkgs; [
        vulkan-loader
        libglvnd
        libx11
        libxcb
        libxcursor
        libxext
        libxi
        libxrandr
        libxrender
        libxkbcommon
        wayland
      ];

      kontra = rustPlatform.buildRustPackage {
        pname = "kontra";
        version = (lib.importTOML ./plugin/Cargo.toml).package.version;
        src = lib.cleanSource ./.;

        # Git dependencies (moose, MUI) are fetched at the revisions
        # Cargo.lock pins: nothing to update here when they move.
        cargoLock = {
          lockFile = ./Cargo.lock;
          allowBuiltinFetchGit = true;
        };

        nativeBuildInputs = with pkgs; [ pkg-config patchelf ];
        buildInputs = runtimeLibs ++ [ pkgs.alsa-lib ];

        cargoBuildFlags = [ "-p" "kontra-plugin" ];
        # The build has no .git; plugin/build.rs stamps this revision instead.
        GITHUB_SHA = self.rev or self.dirtyRev or "";
        # The test suite needs real libraries and a display; run it in CI.
        doCheck = false;

        installPhase = ''
          runHook preInstall
          so=$(find target -path '*/release/libkontra_plugin.so' | head -n1)
          install -Dm755 "$so" $out/lib/clap/KONTRA.clap
          install -Dm755 "$so" $out/lib/vst3/KONTRA.vst3/Contents/x86_64-linux/KONTRA.so
          runHook postInstall
        '';

        # After fixup: its RPATH shrinking keeps only directly linked
        # libraries and would drop every dlopen'd one.
        dontPatchELF = true;
        postFixup = ''
          for f in $out/lib/clap/KONTRA.clap $out/lib/vst3/KONTRA.vst3/Contents/x86_64-linux/KONTRA.so; do
            patchelf --add-rpath ${lib.makeLibraryPath runtimeLibs} "$f"
          done
        '';

        meta = {
          description = "KONTRA sampler plugin (CLAP and VST3)";
          homepage = "https://github.com/DerpcatMusic/KONTRA";
          license = lib.licenses.asl20;
          platforms = [ system ];
        };
      };
    in
    {
      packages.${system}.default = kontra;

      # `nix develop`: cargo, the system libraries, and the run-time ones on
      # LD_LIBRARY_PATH so tests and examples can open windows.
      devShells.${system}.default = pkgs.mkShell {
        inputsFrom = [ kontra ];
        packages = with unstable; [ rustc cargo clippy rustfmt rust-analyzer ];
        LD_LIBRARY_PATH = lib.makeLibraryPath runtimeLibs;
      };
    };
}
