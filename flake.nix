# The shell that builds VODscribe on Servo:
#
#   nix develop -c pnpm tauri:servo dev|build
#
# It is Servo's own build environment (the fork's shell.nix, copied here
# because that file fetches its inputs without hashes, which a flake can't
# do, and keeps its GStreamer to itself), GStreamer 1.28.8 with the patched
# mselib, and the Node and pnpm versions this repository pins. GTK's
# development files come from the system (libgtk-3-dev, gtk3-devel), as in
# Servo's shell: the app links against the system's loader and GTK, so it runs
# outside Nix.
{
  description = "VODscribe on Servo: its build shell";

  inputs = {
    # Servo's pins. This nixpkgs has glibc 2.42: the app built with it runs
    # on older glibc, but some of its libraries (libclang, libmount) load
    # only on glibc 2.42 or newer.
    nixpkgs.url = "github:NixOS/nixpkgs/34ab99075ac4f7e40cf037eef32cb1c360bb85e9";
    rust-overlay = {
      url = "github:oxalica/rust-overlay/87ffb7f777aef2e2647fd8fd718a5b3cbc825588";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # libclang for bindgen, from nixos-24.05 (glibc 2.39): see LIBCLANG_PATH.
    nixpkgs-libclang.url = "github:NixOS/nixpkgs/b134951a4c9f3c995fd7be05f3243f8ecd65d798";
    # Node 24.21.0 (.nvmrc), newer than Servo's nixpkgs has.
    nixpkgs-node.url = "github:NixOS/nixpkgs/4fe25e12eb9f4c744966dd3de5a05246cf098601";
    # GStreamer 1.28.8 with the MSE fixes (branch mse-fixes-1.28).
    gstreamer = {
      url = "github:Airodonack/gstreamer/5ce1c32454cf2ac6195ee8fc717ca3d3e1c8070c";
      flake = false;
    };
  };

  outputs =
    {
      nixpkgs,
      rust-overlay,
      nixpkgs-node,
      nixpkgs-libclang,
      gstreamer,
      ...
    }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };
      lib = pkgs.lib;
      llvmPackages = pkgs.llvmPackages_20;
      llvmPackagesBindgen = nixpkgs-libclang.legacyPackages.${system}.llvmPackages_18;

      # The toolchain of Servo's rust-toolchain.toml.
      rust = pkgs.rust-bin.stable."1.99.1".default.override {
        extensions = [
          "rustfmt"
          "clippy"
        ];
      };

      nodejs = nixpkgs-node.legacyPackages.${system}.nodejs_24;
      # package.json's packageManager, as pnpm releases it (a static binary).
      pnpm = pkgs.stdenvNoCC.mkDerivation {
        pname = "pnpm";
        version = "12.11.2";
        src = pkgs.fetchurl {
          url = "https://github.com/pnpm/pnpm/releases/download/v12.11.2/pnpm-linux-x64-musl.tar.gz";
          hash = "sha256-ZONf4IVElNxWwkMR2X8Tz+c/z9jjy2W3wPXDWdpqD9g=";
        };
        sourceRoot = ".";
        dontFixup = true;
        installPhase = ''
          mkdir -p $out/libexec/pnpm $out/bin
          cp -r pnpm dist $out/libexec/pnpm/
          ln -s $out/libexec/pnpm/pnpm $out/bin/pnpm
        '';
      };

      # GStreamer 1.28.8: nixpkgs' derivations with the version and source
      # moved, as in Servo's shell.nix, and gst-plugins-bad built from the
      # fork, whose mselib is the patched one.
      bump = name: hash: _: {
        version = "1.28.8";
        src = pkgs.fetchurl {
          url = "https://gstreamer.freedesktop.org/src/${name}/${name}-1.28.8.tar.xz";
          inherit hash;
        };
      };
      gst = pkgs.gst_all_1.overrideScope (
        final: prev: {
          gstreamer = prev.gstreamer.overrideAttrs (
            bump "gstreamer" "sha256-uqjoaYWtZBfMCtbnt5/yiExnecixx6YkDnEhgiyG5SQ="
          );
          gst-plugins-base = prev.gst-plugins-base.overrideAttrs (
            bump "gst-plugins-base" "sha256-recOM4H8uwJPHyCqnt6bVlSu7TnVvXCxqY9TAV5QERc="
          );
          gst-plugins-good = prev.gst-plugins-good.overrideAttrs (
            bump "gst-plugins-good" "sha256-p2cO2QDT0DvnRNJcGHOyOYlumoLObRf4/rxv/7rulSE="
          );
          gst-plugins-bad = prev.gst-plugins-bad.overrideAttrs {
            version = "1.28.8";
            src = "${gstreamer}/subprojects/gst-plugins-bad";
            # mselib's unit tests, which come with the fixes. They run the
            # test binary itself: meson's test environment lacks
            # gst-plugins-good, whose demuxers the tests parse media with.
            doCheck = true;
            checkPhase = ''
              runHook preCheck
              ninja tests/check/libs_mse
              GST_REGISTRY=$TMPDIR/registry.bin GST_REGISTRY_FORK=no GST_PLUGIN_SYSTEM_PATH_1_0= \
                GST_PLUGIN_PATH_1_0=$PWD:${final.gstreamer.out}/lib/gstreamer-1.0:${final.gst-plugins-base}/lib/gstreamer-1.0:${final.gst-plugins-good}/lib/gstreamer-1.0 \
                CK_DEFAULT_TIMEOUT=60 GST_STATE_IGNORE_ELEMENTS= tests/check/libs_mse
              runHook postCheck
            '';
          };
        }
      );
      # Empty libraries under WebKitGTK's names (see the shell's SYSTEM_DEPS_*).
      webkitStandIns = pkgs.runCommandCC "webkitgtk-stand-ins" { } ''
        mkdir -p $out/lib
        echo > empty.c
        for name in webkit2gtk-4.1 javascriptcoregtk-4.1 soup-3.0; do
          $CC -shared -o $out/lib/lib$name.so empty.c
        done
      '';
      gstPackages = [
        gst.gstreamer
        gst.gst-plugins-base
        gst.gst-plugins-good
        gst.gst-plugins-bad
      ];
    in
    {
      packages.${system} = {
        inherit (gst)
          gstreamer
          gst-plugins-base
          gst-plugins-good
          gst-plugins-bad
          ;
      };

      devShells.${system}.default = pkgs.mkShell.override { stdenv = llvmPackages.stdenv; } {
        name = "vodscribe-servo";

        buildInputs = gstPackages ++ [
          # Servo's native dependencies, as its shell.nix lists them.
          pkgs.fontconfig
          pkgs.freetype
          pkgs.libunwind
          pkgs.libxcb
          pkgs.libx11
          pkgs.udev
          pkgs.dbus
          # The app's: its audio.
          pkgs.alsa-lib

          rust
          llvmPackages.bintools
          pkgs.cmake
          pkgs.gcc
          pkgs.git
          pkgs.pkg-config
          pkgs.which
          llvmPackages.llvm
          pkgs.perl
          pkgs.yasm
          pkgs.m4
          pkgs.python311
          pkgs.gnumake

          nodejs
          pnpm
          # scripts/prepare-sidecars.sh, and the AppImage's bundler.
          pkgs.patchelf
          pkgs.file
          pkgs.curl
          pkgs.bzip2
        ];

        # Tauri builds its WebKitGTK bindings on Linux even without its wry
        # runtime, and nothing in the Servo build calls them. They link
        # against empty stand-ins instead of WebKitGTK, libsoup and
        # JavaScriptCore, which --as-needed then drops: a call into any of
        # them would fail to link rather than make the app need WebKitGTK.
        SYSTEM_DEPS_WEBKIT2GTK_4_1_NO_PKG_CONFIG = "1";
        SYSTEM_DEPS_WEBKIT2GTK_4_1_LIB = "webkit2gtk-4.1";
        SYSTEM_DEPS_WEBKIT2GTK_4_1_SEARCH_NATIVE = "${webkitStandIns}/lib";
        SYSTEM_DEPS_JAVASCRIPTCOREGTK_4_1_NO_PKG_CONFIG = "1";
        SYSTEM_DEPS_JAVASCRIPTCOREGTK_4_1_LIB = "javascriptcoregtk-4.1";
        SYSTEM_DEPS_JAVASCRIPTCOREGTK_4_1_SEARCH_NATIVE = "${webkitStandIns}/lib";
        SYSTEM_DEPS_LIBSOUP_3_0_NO_PKG_CONFIG = "1";
        SYSTEM_DEPS_LIBSOUP_3_0_LIB = "soup-3.0";
        SYSTEM_DEPS_LIBSOUP_3_0_SEARCH_NATIVE = "${webkitStandIns}/lib";

        # Build scripts link against the system's loader (shellHook), and
        # bindgen's load libclang into them. This nixpkgs' libclang needs
        # glibc 2.42, so it comes from one built on glibc 2.39 (Ubuntu
        # 24.04's, the CI runner's).
        # The clang beside it gives bindgen its headers.
        LIBCLANG_PATH = "${llvmPackagesBindgen.libclang.lib}/lib";
        CLANG_PATH = "${llvmPackagesBindgen.clang}/bin/clang";
        SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
        TERMINFO = "${pkgs.ncurses.out}/share/terminfo";

        # Libraries the app loads but doesn't link against (Servo's list),
        # then GStreamer 1.28.8's: the app uses the system's loader, and
        # its other libraries (GTK, GLib) are the system's.
        LD_LIBRARY_PATH = lib.makeLibraryPath (
          [
            pkgs.wayland
            pkgs.libxcursor
            pkgs.libxrandr
            pkgs.libxi
            pkgs.libxkbcommon
            pkgs.vulkan-loader
            pkgs.libGL
          ]
          ++ gstPackages
        );

        # For scripts/tauri-servo.sh: this shell.
        VODSCRIBE_SERVO_SHELL = "flake";

        shellHook = ''
          # Servo's shell.nix: link for the system's loader, without Nix
          # library paths, so the binaries run outside Nix too.
          unset AS
          if ! [ -e /etc/NIXOS ]; then
            set -- $NIX_LDFLAGS
            for i; do
              shift
              if [ "$i" = -rpath ]; then
                shift
              else
                set -- "$@" "$i"
              fi
            done
            export NIX_DYNAMIC_LINKER=$(patchelf --print-interpreter /usr/bin/env)
            export NIX_DONT_SET_RPATH=1
            export NIX_LDFLAGS="$@"
          fi
          # GStreamer 1.28's plugins have a registry of their own, apart from
          # the system GStreamer's.
          export GST_REGISTRY_1_0=''${XDG_CACHE_HOME:-$HOME/.cache}/vodscribe-servo/gstreamer-1.28.8-registry.bin
        '';
      };
    };
}
