# sproutx

Type less, grow more.

sproutx watches your keyboard and turns short triggers into longer text —
system-wide, on Wayland, X11, or a bare TTY. No GUI, no per-app fiddling. It
just types for you.

## Try it

### Prebuilt binary (no Rust needed)

Download `sproutx-v0.1.3-linux-x86_64.tar.gz` from the Releases page, then:

```sh
mkdir -p ~/.local/bin
tar -xzf sproutx-v0.1.3-linux-x86_64.tar.gz -C ~/.local/bin
```

No libraries to install — desktops already ship `libxkbcommon`. Only x86_64
binaries are published; other architectures should build from source.

### From source (needs Rust + libxkbcommon-dev)

```sh
cargo install --git https://github.com/hillbyte/sproutx --tag v0.1.3
```

Then:

```sh
sproutx init                                  # writes ~/.config/sproutx/config.yaml
sudo usermod -aG input $USER                  # once, so it can read your keyboard
# log out and back in, then:
sproutx daemon
```

Now open anything, type `:today`, press space... a date appears. magic.

## Everyday use

```
sproutx daemon         run the expander
sproutx init           sample config
sproutx add :hi  hello there      add a rule
sproutx list           show rules
sproutx reload         pick up config changes
sproutx status         is it running?
sproutx stop           stop the daemon
sproutx inspect        report input devices + permissions
sproutx test "say :hi" dry-run a line
sproutx install        autostart as a systemd user service
sproutx uninstall      remove service + config (keep it with --keep-config)
```
`uninstall` keeps the binary — remove that with `cargo uninstall sproutx`.

## Config

Rules live in `~/.config/sproutx/config.yaml`:

```yaml
layout: ~       # XKB layout: "us", "fr(azerty)", "us,de". ~ = auto-detect.
depth: 64       # max trigger length to scan for
delay_ms: 12    # pause between injected keystrokes

rules:
  - trigger: ":hi"
    replace: "hi there!"
```

Anything inside `{{ }}` is dynamic: `{{today}}`, `{{time}}`, `{{now}}`,
`{{today+1}}`, `{{today:%A, %d %B %Y}}`. Multi-line replacements use a `|`:

```yaml
  - trigger: ":sign"
    replace: |
      Jane
      jane@example.com
```

While the daemon runs, `sproutx reload` picks up saved changes — rules,
`depth`, and `delay_ms` reload live. A `layout` change still needs a daemon
restart.

## Matching

A trigger fires at a word boundary: it must start after a space (or the start
of a line/app). `going:fast` stays text, `going :fast` expands. Type the
trigger, then finish it with a delimiter — space, Enter, or punctuation.
When one trigger is a prefix of another (say `:t` and `:time`), the shorter one
waits for a delimiter so it never hijacks the longer one.

Shortcut chords (Ctrl/Caps-less Super/Alt + a key) are treated as gestures,
not text, so copy/paste/workspace switches never interfere with triggers.

## How it works

It reads `/dev/input` directly, so it needs the `input` group and a writable
`/dev/uinput`. See `sproutx inspect` for a friendly report of what it can and
cannot touch.

That's it. Go grow some text.