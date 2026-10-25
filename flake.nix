{
  description = "Coxswain: a two-panel file manager, in the terminal and on the desktop";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  # The packages build their dependencies apart from the workspace (packaging/nix/package.nix).
  inputs.crane.url = "github:ipetkov/crane";

  outputs =
    { self, nixpkgs, crane }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAll (
        pkgs:
        {
          coxswain = pkgs.callPackage ./packaging/nix/package.nix { craneLib = crane.mkLib pkgs; };
          default = self.packages.${pkgs.stdenv.hostPlatform.system}.coxswain;
        }
        // nixpkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
          coxswain-gui = pkgs.callPackage ./packaging/nix/gui.nix { craneLib = crane.mkLib pkgs; };
        }
      );

      apps = forAll (
        pkgs:
        let
          p = self.packages.${pkgs.stdenv.hostPlatform.system};
          app = drv: {
            type = "app";
            program = nixpkgs.lib.getExe drv;
          };
        in
        {
          default = app p.coxswain;
          coxswain = app p.coxswain;
        }
        // nixpkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux { coxswain-gui = app p.coxswain-gui; }
      );
    };
}
