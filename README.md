# Reaction Toplist Bot

Discord bot for building toplists for configured emoji reactions
for the given ISO calendar week (Mon-Sun)
and posting them in threads.


## Usage

You must set the `DISCORD_TOKEN` environment variable
and invite the bot to your server.

You may use the following URL
to invite [a bot from your applications](https://discordapp.com/developers/applications/me)
to your server:

```
https://discord.com/api/oauth2/authorize?client_id=<insert_client_id_here>&scope=bot&permissions=309237989376
```

The permissions are as follows:

- Create Public Threads
- Send Messages in Threads
- Embed Links
- Read Message History
- Use External Emoji

Refer also to [the Discord OAuth2 documentation](https://discordapp.com/developers/docs/topics/oauth2).

Afterwards, build and run the bot using your preferred method.

```sh
$ export DISCORD_TOKEN=<your_token_here>
$ cargo run
# or
$ cargo build --release
$ ./target/release/reaction_toplist_bot
```


## Configuration

A configuration file is read from `./config.toml`
relative to the current working directory.
See [example-config.toml](./example-config.toml) for an example configuration.


## Run-time Arguments

The calendar week (the first parameter)
can either be a relative number `+0`, `-1`
or an absolute week for a given year
in the format `yyyy-ww`, e.g. `2022-10`.
Defaults to the current week if not specified.

Examples:

```sh
$ reaction_toplist_bot
$ reaction_toplist_bot -1
$ reaction_toplist_bot 2022-10
```

## Known Issues

- The Bot cannot post emoji from other servers
  that it is not a member of itself;
  they will render as `:emoji_name:` instead.
  This is a Discord limitation.
