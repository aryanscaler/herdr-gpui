{
  lib,
  stdenv,
  swift,
  apple-sdk_14,
}:

stdenv.mkDerivation {
  pname = "sheprd-nearby";
  version = "0-unstable";
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./main.swift
      ./Framing.swift
      ./Bonjour.swift
    ];
  };
  nativeBuildInputs = [ swift ];
  buildInputs = [ apple-sdk_14 ];
  env.MACOSX_DEPLOYMENT_TARGET = "14.2";
  dontConfigure = true;
  buildPhase = ''
    runHook preBuild
    swiftc -O -swift-version 5 main.swift Framing.swift Bonjour.swift -o sheprd-nearby
    runHook postBuild
  '';
  installPhase = ''
    runHook preInstall
    install -Dm755 sheprd-nearby "$out/bin/sheprd-nearby"
    runHook postInstall
  '';
  meta.platforms = lib.platforms.darwin;
}
