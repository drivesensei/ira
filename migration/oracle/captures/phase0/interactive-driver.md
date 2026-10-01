# TUI oracle interactive driver

- Binary: `target/debug/ira`, built at frozen `tui-oracle-baseline` (`1cad4ce`).
- Use `exec_command` with `tty: true`; start in an isolated fixture with fresh `HOME` and `XDG_CONFIG_HOME`, and run `stty cols 120 rows 40` before executing the oracle binary.
- Allow the terminal capability probe to finish before sending keys or capturing. Under tmux on this host it took about 10–12 seconds; an early capture can be blank or input can arrive before the app event loop.
- Initial focus is Drives, where file actions such as `n` are no-ops because there is no pane folder. Press `w` to select Home before testing pane actions.
- Verified interaction: `w`, `n`, type `notes.txt`, Enter created the file under isolated HOME; `q` exited with status 0. Fixture and HOME manifests were checked after the run.
- A previous standalone `pty.fork` probe did not deliver keys reliably. Prefer the exec PTY procedure above; do not treat the failed probe as application behavior.
