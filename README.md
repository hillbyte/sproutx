# sproutx

Type less, grow more.

sproutx watches your keyboard and turns short triggers into longer text —
system-wide, on Wayland, X11, or a bare TTY. No GUI, no per-app fiddling. It
just types for you.

## Try it

Either install from source (needs Rust), or grab a prebuilt binary from the
Releases page:

```sh
cargo install --git https://github.com/hillbyte/sproutx --tag v0.1.0
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
sproutx init          sample config
sproutx add :hi  hello there      add a rule
sproutx list          show rules
sproutx reload        pick up config changes
sproutx status        is it running?
sproutx test "say :hi"      dry-run a line
sproutx install       autostart as a systemd user service
sproutx uninstall     remove service + config
```

## Config

Rules live in `~/.config/sproutx/config.yaml`:

```yaml
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

While the daemon runs, `sproutx reload` picks up saved changes.

## How it works

It reads `/dev/input` directly, so it needs the `input` group and a writable
`/dev/uinput`. See `sproutx inspect` for a friendly report of what it can and
cannot touch.

That's it. Go grow some text.