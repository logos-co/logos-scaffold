{
  description = "{{project_title}} QML plugin for Logos Basecamp";

  inputs = {
    logos-module-builder.url = "{{module_builder_flake_url}}";
  };

  outputs = inputs@{ logos-module-builder, ... }:
    logos-module-builder.lib.mkLogosQmlModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
    };
}
