# Home-manager module for CASE speech-to-text
#
# Provides a systemd user service for autostart.
# Usage: imports = [ case.homeManagerModules.default ];
#        services.case.enable = true;
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.case;
in
{
  options.services.case = {
    enable = lib.mkEnableOption "CASE speech-to-text user service";

    package = lib.mkOption {
      type = lib.types.package;
      defaultText = lib.literalExpression "case.packages.\${system}.case";
      description = "The CASE package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.user.services.case = {
      Unit = {
        Description = "CASE speech-to-text";
        After = [ "graphical-session.target" ];
        PartOf = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = "${cfg.package}/bin/case";
        Restart = "on-failure";
        RestartSec = 5;
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };
  };
}
