# Downtime for Windows

A simple version of macOS's [Downtime](https://support.apple.com/guide/mac-help/manage-downtime-in-screen-time-mchl69510069/mac), which allows you to block certain websites at certain times, built for Windows.

This crate works by editing the `etc/hosts` file and closing your browser until the configured time. During that time any websites that you configure will be blocked.

In the future I might make this work on other OS's, but I only use a mac and windows machine, and mac already has this feature, so... maybe not

See [`settings.toml`](./settings.toml)

## Todo

- Make it run at startup
- Prevent killing the app
- Allow it to be bypassed by swearing on your mother that you're not gonna regret turning it off

## License

Downtime for Windows is distributed under the terms of the MIT license. See [`LICENSE`](./LICENSE) for details.
