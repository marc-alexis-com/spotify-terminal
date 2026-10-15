{
  description = "Play Spotify in your terminal";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      cargo = builtins.fromTOML (builtins.readFile ./Cargo.toml);
    in
    {
      packages = forAll (pkgs: rec {
        spotify-terminal = pkgs.rustPlatform.buildRustPackage {
          pname = "spotify-terminal";
          version = cargo.package.version;
          src = pkgs.lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.pkg-config ];
          # ALSA for the audio; librespot links its own TLS against OpenSSL.
          buildInputs = [ pkgs.alsa-lib pkgs.openssl ];
          meta = {
            description = cargo.package.description;
            homepage = "https://github.com/marc-alexis-com/spotify-terminal";
            license = pkgs.lib.licenses.mit;
            mainProgram = "spt";
            platforms = pkgs.lib.platforms.linux;
          };
        };
        default = spotify-terminal;
      });

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.system}.default ];
          packages = [ pkgs.cargo pkgs.rustc pkgs.rustfmt pkgs.clippy ];
        };
      });
    };
}
