{
  description = "yori development environment";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in {
      devShells = forAllSystems (system:
        let pkgs = import nixpkgs { inherit system; };
        in {
          default = pkgs.mkShell {
            nativeBuildInputs = with pkgs; [
              pkg-config
            ];
            buildInputs = with pkgs; [
              fontconfig
              freetype
              libxkbcommon
              vulkan-loader
              wayland
              libxcb
            ];
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (with pkgs; [
              fontconfig
              freetype
              libxkbcommon
              vulkan-loader
              wayland
              libxcb
            ]);
          };
        });
    };
}
