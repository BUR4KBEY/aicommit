{
  pkgs,
  lib,
  config,
  inputs,
  ...
}: {
  env.DEVSHELL_NAME = "aicommit";
  languages.rust.enable = true;
}
