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

  # The smallest English model, so a VM can transcribe without a download.
  whisperTiny = pkgs.fetchurl {
    url = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en-q5_1.bin";
    hash = "sha256-x3xXZvHO8JtrfUfyG1Rsvd1BV4hrO11tT3CekeZsfCs=";
  };

  dictationConfig = pkgs.writeText "sonori-dictate.toml" ''
    [general_config]
    model = "tiny.en"
    language = "en"

    [backend_config]
    backend = "whisper_cpp"
    quantization_level = "low"
    gpu_enabled = false

    [portal_config]
    paste_shortcut = "auto"
    output_mode = "Paste"

    [ui_config]
    display_mode = "Cursor"
  '';

  # Speaks a WAV file into a null sink whose monitor is the default microphone,
  # then pastes into foot, which writes what it gets to /tmp/pasted.
  dictationChecks = sway: ''
    with subtest("dictation, paste check, and clipboard restore"):
        machine.succeed(as_user("mkdir -p ~/.cache/speechcore/models && cp -f /etc/sonori-test/ggml-tiny.en-q5_1.bin ~/.cache/speechcore/models/"))
        machine.succeed(as_user("pactl list short sinks | grep -q fakemic || pactl load-module module-null-sink sink_name=fakemic"))
        machine.succeed(as_user("pactl set-default-source fakemic.monitor"))
        sentences = "".join(
            f"<s>Sentence {n} is about the clipboard and the paste check.</s><break time='1500ms'/>"
            for n in range(1, 11)
        )
        machine.succeed(f"cat > /tmp/long.ssml <<'EOF'\n<speak>{sentences}</speak>\nEOF")
        machine.succeed("espeak-ng -m -f /tmp/long.ssml -w /tmp/long.wav")
        machine.succeed("espeak-ng -w /tmp/short.wav 'This text goes nowhere.'")
        machine.succeed("espeak-ng -w /tmp/entry.wav 'Paste into the entry.'")
        print(machine.succeed("soxi -D /tmp/long.wav || ls -l /tmp/long.wav"))

        machine.succeed(as_user("${sway} swaymsg exec \"foot sh -c 'stty -icanon -echo; cat > /tmp/pasted'\""))
        machine.wait_for_file("/tmp/pasted")
        # A real session gives Sonori the compositor socket; the paste shortcut needs it.
        start_sonori("dictate", "${dictationConfig}", "${sway}")
        machine.wait_until_succeeds("grep -q 'Backend loaded' /tmp/sonori-dictate.log", timeout=120)

        def dictate(wav):
            machine.succeed(as_user("sonori start"))
            machine.sleep(1)
            machine.succeed(as_user(f"paplay --device=fakemic {wav}"))
            machine.sleep(2)
            machine.succeed(as_user("sonori stop"))

        machine.succeed(as_user("wl-copy OLD-CLIPBOARD > /dev/null 2>&1"))
        dictate("/tmp/long.wav")
        try:
            machine.wait_until_succeeds("test -s /tmp/pasted", timeout=120)
        except Exception:
            check(False, "the transcript did not reach foot")
        machine.sleep(2)
        pasted = machine.succeed("cat /tmp/pasted")
        print(f"pasted: {pasted!r}")
        check(len(pasted) > 40, "foot got too little text")
        log_text = machine.succeed("cat /tmp/sonori-dictate.log")
        first_chunk = log_text.find("Transcribing manual chunk")
        stop = log_text.find("Recording state changed: true -> false")
        check(0 <= first_chunk < stop, "no chunk was transcribed while recording")
        clip = machine.succeed(as_user("wl-paste --no-newline"))
        check(clip == "OLD-CLIPBOARD", f"the clipboard was not restored: {clip!r}")
        check('Paste shortcut: Ctrl+Shift+V for app Some("foot")' in log_text, "foot did not get Ctrl+Shift+V")

        # A GTK entry is not a terminal: it gets Ctrl+V.
        machine.succeed(as_user("${sway} swaymsg exec \"zenity --entry --text=Dictate > /tmp/entry\""))
        machine.wait_until_succeeds(as_user("${sway} swaymsg -t get_tree | grep -qi zenity"), timeout=30)
        machine.sleep(1)
        dictate("/tmp/entry.wav")
        try:
            machine.wait_until_succeeds("grep -q 'Paste shortcut: Ctrl+V for app' /tmp/sonori-dictate.log", timeout=60)
        except Exception:
            print(machine.succeed("tail -40 /tmp/sonori-dictate.log"))
            print(machine.succeed(as_user("${sway} swaymsg -t get_tree | grep -E 'app_id|focused'")))
            machine.screenshot("zenity")
            check(False, "no Ctrl+V paste for the GTK entry")
        machine.sleep(2)
        machine.succeed(as_user("wtype -k Return"))
        machine.wait_until_succeeds("test -s /tmp/entry", timeout=10)
        entry = machine.succeed("cat /tmp/entry")
        print(f"entry: {entry!r}")
        check("entry" in entry.lower(), "the GTK entry did not get the text")

        # Nothing in focus: the paste goes nowhere, and the transcript stays.
        machine.succeed(as_user("${sway} swaymsg '[app_id=foot] kill'"))
        machine.sleep(1)
        dictate("/tmp/short.wav")
        try:
            machine.wait_until_succeeds("grep -q 'no app read the text' /tmp/sonori-dictate.log", timeout=60)
        except Exception:
            check(False, "Sonori did not notice the paste that no app took")
        clip = machine.succeed(as_user("wl-paste --no-newline"))
        print(f"clipboard: {clip!r}")
        check("nowhere" in clip.lower(), "the transcript is not in the clipboard")
        types = machine.succeed(as_user("wl-paste --list-types"))
        check("x-kde-passwordManagerHint" in types, "the transcript has no history hint")
        machine.screenshot("not-pasted")
        machine.copy_from_vm("/tmp/sonori-dictate.log", "")
        machine.copy_from_vm("/tmp/pasted", "")
  '';

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
    # dotool drives a virtual mouse inside the VM, for clicks on Sonori.
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
      extraChecks ? "",
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

      def start_sonori(display, config, env=""):
          machine.succeed(f"install -o alice -m 644 {config} /tmp/sonori-{display}.toml")
          machine.succeed(as_user(
              f"{env} SONORI_CONFIG_PATH=/tmp/sonori-{display}.toml "
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
                try:
                    machine.wait_until_succeeds("grep -q 'startup notice' /tmp/sonori-cursor.log", timeout=60)
                    machine.screenshot("cursor-notice")
                    # Clicks along the notice's top row (y 581 at 1280x800). The close
                    # button's x depends on the hint text; clicks beside it reach the
                    # desktop. The virtual mouse lives in the VM only.
                    sweep = "; ".join(
                        f"echo 'mouseto {x / 1280:.4f} {581 / 800:.4f}'; sleep 0.06; echo 'click left'; sleep 0.06"
                        for x in range(880, 991, 6)
                    )
                    machine.succeed(f"{{ sleep 0.4; {sweep}; }} | dotool")
                    closed = machine.execute("grep -q 'bubble closed' /tmp/sonori-cursor.log")[0] == 0
                    machine.sleep(1)
                    machine.screenshot("notice-closed")
                    check(closed, "the close button did not remove the notice")
                except Exception as e:
                    check(False, f"the startup notice did not appear ({e})")
                if sonori_ready("cursor"):
                    # No pointer report: the menu must open in the screen centre.
                    status, out = machine.execute(as_user("sonori menu"))
                    print(f"sonori menu: {out}")
                    check(status == 0, f"sonori menu failed: {out}")
                try:
                    machine.wait_for_text("Magic Mode", timeout=30)
                except Exception:
                    check(False, "the quick menu did not appear")
                try:
                    machine.wait_for_text("Recent", timeout=10)
                except Exception:
                    check(False, "the quick menu has no Recent row")
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

      ${extraChecks}

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
      # Reads the screen, so a test can check that the menu really shows.
      enableOCR = true;
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
          # The headless backend lets the test add a second output.
          environment.variables.WLR_BACKENDS = "drm,libinput,headless";
          environment.variables.WLR_HEADLESS_OUTPUTS = "0";
          environment.etc."sonori-test/ggml-tiny.en-q5_1.bin".source = whisperTiny;
          environment.systemPackages = [
            pkgs.foot
            pkgs.grim
            pkgs.tesseract
            pkgs.imagemagick
            pkgs.zenity
            pkgs.wtype
            pkgs.espeak-ng
            pkgs.pulseaudio
            pkgs.sox
            pkgs.wl-clipboard
          ];
        }
      )
      {
        waylandDisplay = "wayland-1";
        layerShell = true;
        waitForDesktop = ''machine.wait_for_file("/run/user/${uid}/wayland-1")'';
        extraChecks = ''
          with subtest("drag a bubble that did not paste"):
              # The desktop is empty, so no app takes the paste and the bubble stays.
              machine.succeed(as_user("mkdir -p ~/.cache/speechcore/models && cp -f /etc/sonori-test/ggml-tiny.en-q5_1.bin ~/.cache/speechcore/models/"))
              machine.succeed(as_user("pactl load-module module-null-sink sink_name=fakemic"))
              machine.succeed(as_user("pactl set-default-source fakemic.monitor"))
              machine.succeed("espeak-ng -w /tmp/short.wav 'This text goes nowhere.'")
              start_sonori("drag", "${dictationConfig}", "SWAYSOCK=$(ls /run/user/${uid}/sway-ipc.*.sock)")
              machine.wait_until_succeeds("grep -q 'Backend loaded' /tmp/sonori-drag.log", timeout=120)
              machine.succeed(as_user("sonori start"))
              machine.sleep(1)
              machine.succeed(as_user("paplay --device=fakemic /tmp/short.wav"))
              machine.sleep(2)
              machine.succeed(as_user("sonori stop"))
              try:
                  machine.wait_until_succeeds("grep -q 'no app read the text' /tmp/sonori-drag.log", timeout=60)
              except Exception:
                  check(False, "the paste did not fail")
              machine.sleep(2)

              def screen_word(name, wanted):
                  machine.succeed(as_user(f"grim /tmp/{name}.png"))
                  machine.copy_from_vm(f"/tmp/{name}.png", "")
                  # Upscaled 3x, so OCR reads the small text on Sonori's glass.
                  machine.succeed(f"magick /tmp/{name}.png -resize 300% -colorspace Gray -negate /tmp/{name}-ocr.png")
                  rows = [
                      line.split("\t")
                      for line in machine.succeed(f"tesseract /tmp/{name}-ocr.png - --psm 11 tsv 2>/dev/null").splitlines()
                  ]
                  words = [r for r in rows if len(r) == 12 and r[0] == "5" and r[11].strip()]
                  print(f"OCR {name}:", " ".join(r[11] for r in words))
                  # OCR reads the glass poorly; any one of the wanted words is enough.
                  for r in words:
                      word = r[11].strip(".,").lower()
                      if word in wanted:
                          left, top, width, height = (int(v) for v in r[6:10])
                          return word, (left + width / 2) / 3, (top + height / 2) / 3
                  return None

              before = screen_word("bubble-before", ["text", "goes", "nowhere", "pasted", "clipboard"])
              if before is None:
                  check(False, "OCR found no failed-paste bubble")
              else:
                  word, x, y = before
                  # The virtual mouse lives in the VM only.
                  steps = "; ".join(
                      f"echo 'mouseto {(x - 30 * i) / 1280:.4f} {(y - 30 * i) / 800:.4f}'; sleep 0.03"
                      for i in range(1, 11)
                  )
                  machine.succeed(
                      f"{{ sleep 0.4; echo 'mouseto {x / 1280:.4f} {y / 800:.4f}'; sleep 0.3; "
                      f"echo 'buttondown left'; sleep 0.2; {steps}; sleep 0.2; echo 'buttonup left'; }} | dotool"
                  )
                  machine.sleep(1)
                  machine.screenshot("bubble-dragged")
                  after = screen_word("bubble-after", [word])
                  print(f"bubble word moved from {before} to {after}")
                  check(
                      after is not None and abs(after[1] - (x - 300)) < 30 and abs(after[2] - (y - 300)) < 30,
                      "the bubble did not follow the drag",
                  )
                  check(machine.execute("grep -q 'bubble closed' /tmp/sonori-drag.log")[0] != 0, "the drag closed the bubble")
              # A new session removes the pinned bubble.
              machine.succeed(as_user("sonori start"))
              machine.sleep(1)
              machine.succeed(as_user("sonori cancel"))
              machine.sleep(2)
              machine.copy_from_vm("/tmp/sonori-drag.log", "")

          with subtest("type through the input method, quick menu, and Recent"):
              # The virtual mouse lives in the VM only.
              def click(x, y):
                  machine.succeed(f"{{ sleep 0.3; echo 'mouseto {x / 1280:.4f} {y / 800:.4f}'; sleep 0.3; echo 'click left'; }} | dotool")
                  machine.sleep(1)

              def menu_click(shot, wanted):
                  hit = screen_word(shot, wanted)
                  check(hit is not None, f"OCR found none of {wanted} in the quick menu")
                  if hit is not None:
                      click(hit[1], hit[2])
                  return hit is not None

              machine.succeed("espeak-ng -w /tmp/typed.wav 'Typed through the input method.'")
              machine.succeed(as_user("SWAYSOCK=$(ls /run/user/${uid}/sway-ipc.*.sock) swaymsg exec \"foot sh -c 'stty -icanon -echo; cat > /tmp/typed'\""))
              machine.wait_for_file("/tmp/typed")
              machine.sleep(1)
              machine.succeed(as_user("wl-copy KEEP-CLIPBOARD > /dev/null 2>&1"))

              # The quick menu switches the output to Type.
              machine.succeed(as_user("sonori menu"))
              machine.sleep(2)
              if menu_click("menu-output", ["type"]):
                  machine.screenshot("menu-type")
                  click(60, 760)
                  check(machine.execute("grep -q 'output_mode = \"Type\"' /tmp/sonori-drag.toml")[0] == 0, "the menu did not switch the output to Type")

              machine.succeed(as_user("sonori start"))
              machine.sleep(1)
              machine.succeed(as_user("paplay --device=fakemic /tmp/typed.wav"))
              machine.sleep(2)
              machine.succeed(as_user("sonori stop"))
              try:
                  machine.wait_until_succeeds("grep -q 'inserted through the input method' /tmp/sonori-drag.log", timeout=60)
              except Exception:
                  print(machine.succeed("grep -E 'Type:|Typing' /tmp/sonori-drag.log || true"))
                  check(False, "Type did not use the input method")
              machine.sleep(1)
              typed = machine.succeed("cat /tmp/typed")
              print(f"typed: {typed!r}")
              check("method" in typed.lower(), "foot did not get the text")
              clip = machine.succeed(as_user("wl-paste --no-newline"))
              check(clip == "KEEP-CLIPBOARD", f"Type changed the clipboard: {clip!r}")

              # Recent delivers the transcript again, with the current output mode.
              machine.sleep(6)
              machine.succeed(as_user("sonori menu"))
              machine.sleep(2)
              if menu_click("menu-rows", ["recent"]) and menu_click("menu-recent", ["through", "method", "input"]):
                  machine.screenshot("recent-chosen")
                  try:
                      machine.wait_until_succeeds("test $(grep -c 'inserted through the input method' /tmp/sonori-drag.log) -ge 2", timeout=30)
                  except Exception:
                      check(False, "Recent did not deliver the transcript")
                  machine.sleep(1)
                  typed = machine.succeed("cat /tmp/typed")
                  print(f"typed after Recent: {typed!r}")
                  check(typed.lower().count("method") >= 2, "foot did not get the Recent text")
              machine.succeed(as_user("SWAYSOCK=$(ls /run/user/${uid}/sway-ipc.*.sock) swaymsg '[app_id=foot] kill'"))
              machine.copy_from_vm("/tmp/sonori-drag.log", "")
              machine.copy_from_vm("/tmp/typed", "")

          with subtest("model cards in Settings"):
              # Before the second output exists, so the virtual mouse maps to the screen.
              machine.succeed(as_user("sonori settings"))
              machine.sleep(3)
              # OCR finds the Speech tab; the virtual mouse lives in the VM only.
              machine.succeed(as_user("grim /tmp/settings.png"))
              words = [
                  line.split("\t")
                  for line in machine.succeed("tesseract /tmp/settings.png - tsv 2>/dev/null").splitlines()
              ]
              print("OCR:", " ".join(w[11] for w in words if len(w) == 12 and w[11].strip()))
              speech = [w for w in words if len(w) == 12 and w[11].strip() == "Speech"]
              if speech:
                  left, top, width, height = (int(v) for v in speech[0][6:10])
                  x, y = (left + width / 2) / 1280, (top + height / 2) / 800
                  machine.succeed(f"{{ sleep 0.4; echo 'mouseto {x:.4f} {y:.4f}'; sleep 0.2; echo 'click left'; }} | dotool")
                  machine.sleep(2)
                  machine.screenshot("settings-speech")
                  for name in ["Nemotron", "Whisper turbo", "Moonshine base"]:
                      try:
                          machine.wait_for_text(name, timeout=20)
                      except Exception:
                          check(False, f"no {name} card in Settings")
              else:
                  machine.screenshot("settings")
                  check(False, "OCR found no Speech tab in Settings")

          with subtest("cursor mode on the focused output"):
              machine.succeed(as_user("SWAYSOCK=$(ls /run/user/${uid}/sway-ipc.*.sock) swaymsg create_output"))
              machine.succeed(as_user("SWAYSOCK=$(ls /run/user/${uid}/sway-ipc.*.sock) swaymsg focus output HEADLESS-1"))
              start_sonori("focus", "${sonoriConfig "Cursor"}")
              try:
                  machine.wait_until_succeeds("grep -q 'focused output is HEADLESS-1' /tmp/sonori-focus.log", timeout=60)
              except Exception:
                  check(False, "Sonori did not find the focused output")
              print(machine.execute(as_user("SWAYSOCK=$(ls /run/user/${uid}/sway-ipc.*.sock) swaymsg -t get_outputs | grep -E 'name|focused'"))[1])
              machine.copy_from_vm("/tmp/sonori-focus.log", "")

          ${dictationChecks "SWAYSOCK=$(ls /run/user/${uid}/sway-ipc.*.sock)"}

        '';
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
