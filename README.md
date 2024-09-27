# discord-bridge

[![License](https://img.shields.io/badge/License-GPLv3-blue?style=for-the-badge)](https://www.gnu.org/licenses/gpl-3.0)

Bridge a Discord voice channel with an RF link.

## Getting started

This script is inspired by <https://github.com/jess-sys/dmr-bridge-discord>.

The target server is AllStarLink USRP channel.

### AllStarLink configuration
```
[1999](node-main)
rxchannel = USRP/127.0.0.1:34001:32001
duplex = 3 ; Avoid echoing back to discord
```

### Build

Make sure you have [Rust installed](https://rustup.rs/) and also Opus codec library development files installed

#### Install opus in Debian and Ubuntu
```
apt install libopus-dev
```

#### Build discord bridge
```bash
cargo build --release
# or run it directly :
# cargo run --release
```

### Install

Install binaries to `/opt/discord-bridge/bin`, default config to `/opt/discord-bridge/.env` and install systemd service to `/lib/systemd/system/discord-bridge`.

```bash
# Coming soon
make install
make install-config
make install-systemd
```

### Configure

Edit the `.env` (the same directory or in /opt/discord-bridge) file to reflect your infrastructure :

* `BOT_TOKEN` : see [this link](https://github.com/reactiflux/discord-irc/wiki/Creating-a-discord-bot-&-getting-a-token) to know how to get a token
* `BOT_PREFIX` : prefix to add before the bot's commands
* `TARGET_RX_ADDR` : your Analog Bridge IP and port
* `LOCAL_RX_ADDR` : your discord-bridge IP and port (is localhost)

### Run

#### Systemctl service

```bash
systemctl start discord-bridge.service
# or enable it at boot:
# systemctl enable discord-bridge.service --now
```

### Usage

Here are the bot's commands:

* `!join` : Make the bot join the channel (you need to be in a voice channel first)
* `!leave` : Make the bot left the channel

The bot will join the voice channel you're in after your type `!join`.

## Todo

* Option to Discord multiple voice users at once (merge audio channels)
* DV clients