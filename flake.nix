{
  description = "Coxswain: a two-panel file manager, in the terminal and on the desktop";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAll (
        pkgs:
        {
          coxswain = pkgs.callPackage ./packaging/nix/package.nix { };
          default = self.packages.${pkgs.stdenv.hostPlatform.system}.coxswain;
        }
        // nixpkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
          coxswain-gui = pkgs.callPackage ./packaging/nix/gui.nix { };
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
