# Denon / Marantz AVR

Denon and Marantz AV receivers, over the telnet control port.

| Driver | Proxies |
| --- | --- |
| `denon.avr` | `receiver` |

The connection is held open rather than opened per command: the receiver pushes unsolicited
status when someone turns the volume knob on the front panel, and a controller that only spoke
when spoken to would show a stale volume until the next command.

## Building

```bash
cargo build --release
```

Releases are built by [`junohouse/driver-ci`](https://github.com/junohouse/driver-ci): push to
`main` for a beta, tag `v1.2.0` for a release. To work on this against a local core checkout,
uncomment the `[patch]` block in `Cargo.toml`.
