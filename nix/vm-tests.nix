# VM tests: boot one desktop per test in QEMU, run `sonori doctor`, start
# Sonori in overlay and cursor mode, and keep the report, logs, and
# screenshots in the build result. Run one with:
#   nix build .#checks.x86_64-linux.vm-sway -L
{ pkgs, sonori }:
let
  inherit (pkgs) lib;
  uid = "1000";

  sonoriConfig =
    display:
    pkgs.writeText "sonori-${lib.toLower display}.toml" ''
      [ui_config]
      display_mode = "${display}"
    '';

  # The VM has no network; Sonori exits if it cannot fetch this model.
  sileroVad = pkgs.fetchurl {
    url = "https://github.com/snakers4/silero-vad/raw/v5.1.2/src/silero_vad/data/silero_vad.onnx";
    hash = "sha256-JiOilT9v89LB5hdAxs23FoEzR5smff7xFKSjzFvdeI8=";
  };

  common = {
    imports = [ "${pkgs.path}/nixos/tests/common/user-account.nix" ];
    virtualisation.memorySize = 4096;
    virtualisation.cores = 4;
    hardware.graphics.enable = true;
    environment.systemPackages = [
      sonori
      pkgs.vulkan-tools
      pkgs.dotool
    ];
    # dotool drives a virtual pointer inside the VM.
    hardware.uinput.enable = true;
    # The VM has no GPU, so Mesa's software Vulkan driver (lavapipe) renders.
    environment.variables.VK_ICD_FILENAMES = "/run/opengl-driver/share/vulkan/icd.d/lvp_icd.x86_64.json";
    services.pipewire = {
      enable = true;
      alsa.enable = true;
      pulse.enable = true;
    };
  };

  # Starts Sonori in both display modes and saves the evidence. `layerShell`
  # says if the desktop can show Sonori at all.
  script =
    {
      waylandDisplay,
      layerShell,
      waitForDesktop,
    }:
    ''
      import shlex

      ENV = (
          "XDG_RUNTIME_DIR=/run/user/${uid} "
          "WAYLAND_DISPLAY=${waylandDisplay} "
          "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/${uid}/bus"
      )

      def as_user(command):
          return "su - alice -c " + shlex.quote(f"{ENV} {command}")

      def start_sonori(display, config):
          machine.succeed(f"install -o alice -m 644 {config} /tmp/sonori-{display}.toml")
          machine.succeed(as_user(
              f"SONORI_CONFIG_PATH=/tmp/sonori-{display}.toml "
              f"sonori > /tmp/sonori-{display}.log 2>&1 &"
          ))

      # Collect failures and fail at the end, so every run keeps all evidence.
      failures = []

      def check(ok, message):
          if not ok:
              print(f"FAILED: {message}")
              failures.append(message)

      def sonori_ready(display):
          try:
              machine.wait_until_succeeds("test -S /run/user/${uid}/sonori/control.sock", timeout=60)
              return True
          except Exception:
              check(False, f"Sonori ({display}) did not open its control socket")
              return False

      start_all()
      machine.succeed("install -D -o alice -m 644 ${sileroVad} /home/alice/.cache/speechcore/models/silero_vad.onnx")
      machine.succeed("chown -R alice /home/alice/.cache")
      ${waitForDesktop}
      machine.sleep(5)
      machine.screenshot("desktop")

      with subtest("vulkan devices"):
          print(machine.execute(as_user("vulkaninfo --summary 2>&1 | grep -E 'deviceName|driverInfo'"))[1])

      with subtest("sonori doctor"):
          status, _ = machine.execute(as_user("sonori doctor > /tmp/sonori-doctor.txt 2>&1"))
          machine.copy_from_vm("/tmp/sonori-doctor.txt", "")
          print(machine.succeed("cat /tmp/sonori-doctor.txt"))
          # A compositor without outputs runs but shows nothing; a pass would be hollow.
          check(machine.execute("grep -qE 'Outputs: [1-9]' /tmp/sonori-doctor.txt")[0] == 0, "the compositor has no outputs")
          ${
            if layerShell then
              ''check(status == 0, f"doctor reported problems (exit {status})")''
            else
              ''
                check(status == 1, f"doctor should fail without layer shell (exit {status})")
                check(machine.execute("grep -q 'no layer shell' /tmp/sonori-doctor.txt")[0] == 0, "doctor did not name the missing layer shell")
              ''
          }

      ${
        if layerShell then
          ''
            with subtest("overlay mode"):
                start_sonori("overlay", "${sonoriConfig "Overlay"}")
                sonori_ready("overlay")
                machine.sleep(8)
                machine.screenshot("overlay")
                machine.copy_from_vm("/tmp/sonori-overlay.log", "")

            with subtest("cursor mode menu"):
                # A new instance replaces the running one.
                start_sonori("cursor", "${sonoriConfig "Cursor"}")
                machine.sleep(8)
                if sonori_ready("cursor"):
                    # A virtual mouse inside the VM moves its pointer, so Sonori
                    # knows where to open the menu. The host pointer is not touched.
                    status, out = machine.execute(as_user("sonori menu"))
                    print(f"sonori menu: {out}")
                    check(status == 0, f"sonori menu failed: {out}")
                    machine.succeed("{ sleep 2; echo 'mouseto 0.4 0.4'; sleep 0.5; echo 'mouseto 0.45 0.4'; sleep 1; } | dotool")
                machine.sleep(2)
                machine.screenshot("cursor-menu")
                machine.copy_from_vm("/tmp/sonori-cursor.log", "")
          ''
        else
          ''
            with subtest("sonori exits with a clear message"):
                start_sonori("overlay", "${sonoriConfig "Overlay"}")
                machine.sleep(10)
                machine.screenshot("overlay")
                machine.copy_from_vm("/tmp/sonori-overlay.log", "")
          ''
      }

      for mode in ["overlay", "cursor"]:
          print(machine.execute(f"cat /tmp/sonori-{mode}.log")[1])
      assert not failures, "; ".join(failures)
    '';

  # Logs in on tty1 and starts `command` as the session.
  ttySession = command: {
    services.getty.autologinUser = "alice";
    programs.bash.loginShellInit = ''
      if [ "$(tty)" = "/dev/tty1" ]; then
        exec ${command}
      fi
    '';
    # The default -vga std device cannot run a Wayland compositor.
    virtualisation.qemu.options = [ "-vga none -device virtio-gpu-pci" ];
  };

  mkTest =
    name: node: args:
    pkgs.testers.runNixOSTest {
      name = "sonori-${name}";
      nodes.machine = {
        imports = [
          common
          node
        ];
      };
      testScript = script args;
    };
in
{
  vm-sway =
    mkTest "sway"
      (
        lib.recursiveUpdate (ttySession "sway") {
          programs.sway.enable = true;
          # GLES does not run in the VM; wlroots draws with Pixman.
          environment.variables.WLR_RENDERER = "pixman";
        }
      )
      {
        waylandDisplay = "wayland-1";
        layerShell = true;
        waitForDesktop = ''machine.wait_for_file("/run/user/${uid}/wayland-1")'';
      };

  vm-hyprland =
    mkTest "hyprland"
      (
        lib.recursiveUpdate (ttySession "Hyprland") {
          programs.hyprland.enable = true;
        }
      )
      {
        waylandDisplay = "wayland-1";
        layerShell = true;
        waitForDesktop = ''machine.wait_for_file("/run/user/${uid}/wayland-1")'';
      };

  vm-plasma =
    mkTest "plasma"
      {
        services.displayManager.sddm = {
          enable = true;
          wayland.enable = true;
        };
        services.desktopManager.plasma6.enable = true;
        services.displayManager.autoLogin = {
          enable = true;
          user = "alice";
        };
      }
      {
        waylandDisplay = "wayland-0";
        layerShell = true;
        waitForDesktop = ''machine.wait_until_succeeds("pgrep plasmashell")'';
      };

  vm-gnome =
    mkTest "gnome"
      {
        services.displayManager.gdm.enable = true;
        services.desktopManager.gnome.enable = true;
        services.displayManager.autoLogin = {
          enable = true;
          user = "alice";
        };
      }
      {
        waylandDisplay = "wayland-0";
        layerShell = false;
        waitForDesktop = ''machine.wait_until_succeeds("pgrep gnome-shell")'';
      };
}
