#![feature(slice_concat_trait)]

use chrono::{DateTime, Utc};
use serenity::all::{
    AutoArchiveDuration, CreateAllowedMentions, CreateEmbed, CreateMessage, CreateThread,
    GetMessages,
};
use serenity::model::gateway::GatewayIntents;
use serenity::model::id::GuildId;
use serenity::model::prelude::CurrentUser;
use serenity::{
    async_trait,
    gateway::ShardManager,
    model::{
        channel::{ChannelType, GuildChannel},
        gateway::Ready,
        id::MessageId,
    },
    prelude::{Client, Context, EventHandler, SerenityError, TypeMapKey},
};
use std::env;
use std::error::Error;
use std::path::Path;
use std::{collections::BTreeSet, sync::Arc};

mod config;
mod time_utils;
mod toplist;

use config::{Config, Emoji};
use toplist::{MsgWrap, Toplist};

// https://discord.com/developers/docs/events/gateway#gateway-intents
const GATEWAY_INTENTS: GatewayIntents = GatewayIntents::GUILDS
    .union(GatewayIntents::GUILD_MESSAGES)
    .union(GatewayIntents::GUILD_MESSAGE_TYPING);

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::from_path(Path::new("./config.toml"))?;

    // Parse week from command line argument
    let week_param = std::env::args().nth(1);
    let week_param_str = week_param.as_ref().map(|s| s.as_str());
    let options = Options {
        calendar_week: time_utils::parse_iso_week(week_param_str)?,
    };

    eprintln!("Config: {:?}", config);
    eprintln!("Options: {:?}", options);

    // Login with a bot token from the environment
    let token = env::var("DISCORD_TOKEN").expect("token missing");
    let mut client = Client::builder(token, GATEWAY_INTENTS)
        .event_handler(ReactionCounter { config, options })
        .await
        .expect("Error creating client");

    {
        // Insert shard manager so we can shut it down from within an event handler
        let mut data = client.data.write().await;
        data.insert::<ShardManagerContainer>(client.shard_manager.clone());
    }

    if let Err(why) = client.start().await {
        eprintln!("An error occurred while running the client: {:?}", why);
    }
    Ok(())
}

#[derive(Debug)]
struct Options {
    calendar_week: chrono::IsoWeek,
}

/// Wrapping to be able to shutdown the client from within an event handler.
/// Taken from:
/// https://github.com/serenity-rs/serenity/blob/5363f2a8a362dc9bc210c9a87da985d43ab7faca/examples/e06_sample_bot_structure/src/main.rs
pub struct ShardManagerContainer;

impl TypeMapKey for ShardManagerContainer {
    type Value = Arc<ShardManager>;
}

struct CurrentUserContainer;

impl TypeMapKey for CurrentUserContainer {
    type Value = CurrentUser;
}

struct ReactionCounter {
    /// file-based configuration
    config: Config,
    /// command-line arguments
    options: Options,
}

#[async_trait]
impl EventHandler for ReactionCounter {
    async fn ready(&self, ctx: Context, ready: Ready) {
        eprintln!("Connected as {}! Waiting for cache...", ready.user.name);
        {
            let mut data = ctx.data.write().await;
            data.insert::<CurrentUserContainer>(ready.user.clone());
        }
    }

    async fn cache_ready(&self, ctx: Context, _guilds: Vec<GuildId>) {
        eprintln!("Cache ready");

        let user = {
            let data = ctx.data.read().await;
            data.get::<CurrentUserContainer>().unwrap().clone()
        };

        let toplist = self.scan_channel(&ctx, &user).await;

        let typing = self.config.target_channel_id().start_typing(&ctx.http);

        let emoji_to_post: Vec<_> = self.config.toplist.iter().map(|item| &item.emoji).collect();

        for key in emoji_to_post {
            if let Some(list) = toplist.top.get(&key) {
                self.post_toplist_thread(&ctx, &Some(key.clone()), list)
                    .await
                    .expect("unable to create message");
            }
        }
        if self.config.other.enabled {
            self.post_toplist_thread(&ctx, &None, &toplist.other)
                .await
                .expect("unable to create message");
        }

        typing.stop();

        self.shutdown(&ctx).await;
    }
}

impl ReactionCounter {
    async fn scan_channel<'c>(&'c self, ctx: &Context, user: &CurrentUser) -> Toplist<'c> {
        let channel_id = self.config.channel_id;
        eprintln!(
            "Scanning channel {:?} over {:?}",
            channel_id, self.options.calendar_week
        );

        let start_time = time_utils::iso_week_to_datetime(self.options.calendar_week);
        let end_time = start_time + chrono::Duration::weeks(1);
        eprintln!("Time span: {:?} til {:?}", start_time, end_time);

        let mut toplist = Toplist::new(&self.config, user, ctx.http.clone());
        let mut first_id: MessageId = (time_utils::time_snowflake(start_time, false) - 1).into();

        'outer: for page in 1.. {
            eprintln!(
                "Fetching page {} (after {})",
                page,
                time_utils::snowflake_time(first_id)
            );
            let msgs = channel_id
                .messages(&ctx.http, GetMessages::new().after(first_id).limit(100))
                .await
                .unwrap();
            eprintln!("Retrieved {} messages", msgs.len());

            // Messages are returned newest to oldest
            first_id = match msgs.first() {
                Some(first) => first.id.into(),
                None => break,
            };

            for msg in &msgs {
                if (&msg.timestamp as &DateTime<Utc>) > &end_time {
                    break 'outer;
                }
                if !msg.reactions.is_empty() {
                    toplist.append(msg).await;
                }
            }
        }

        toplist
            .finalize()
            .await
            .expect("unable to fetch reaction users");

        eprintln!("Finished collecting messages");
        toplist
    }

    async fn post_toplist_thread(
        &self,
        ctx: &Context,
        emoji: &Option<Emoji>,
        list: &BTreeSet<MsgWrap>,
    ) -> Result<(), SerenityError> {
        let thread = self.create_thread(ctx, emoji).await;

        eprintln!("Starting to populate thread for {:?}", emoji);

        let items_with_rank: Vec<_> = list
            .iter()
            .rev()
            .enumerate()
            .scan((0, 0), |(rank, count), (i, item)| {
                if *count != item.count {
                    *rank = i + 1;
                }
                *count = item.count;
                Some((item, *rank))
            })
            .collect();

        for (item, rank) in items_with_rank.into_iter().rev() {
            thread
                .send_message(
                    &ctx.http,
                    CreateMessage::new().content(format!(
                        "```c\n{} // {} user{}\n```",
                        rank,
                        item.count,
                        if item.count == 1 { "" } else { "s" },
                    )),
                )
                .await?;

            thread
                .send_message(
                    &ctx.http,
                    CreateMessage::new()
                        .content(format!("{}", &item.content))
                        .allowed_mentions(CreateAllowedMentions::new()),
                )
                .await?;

            let reaction_strs: Vec<_> = item
                .message
                .reactions
                .iter()
                .map(|r| format!("{} {}", &r.reaction_type, r.count - r.me as u64))
                .collect();
            thread
                .send_message(
                    &ctx.http,
                    CreateMessage::new()
                        .content(format!(
                            "by {} ({})",
                            &item.message.author, &item.message.author.name
                        ))
                        .embed(CreateEmbed::new().title("  ").description(format!(
                            "{} | [link]({})",
                            reaction_strs.join(" | "),
                            item.message.link()
                        )))
                        .allowed_mentions(CreateAllowedMentions::new()),
                )
                .await?;
        }

        eprintln!("Done populating thread for {:?}", emoji);
        Ok(())
    }

    async fn create_thread(&self, ctx: &Context, emoji: &Option<Emoji>) -> GuildChannel {
        let channel_id = self.config.target_channel_id();
        let channel = channel_id
            .to_channel(&ctx.http)
            .await
            .unwrap()
            .guild()
            .unwrap();
        let name = format!(
            "{:?} - {}",
            self.options.calendar_week,
            emoji.as_ref().map(emoji_as_string).unwrap_or("Other"),
        );

        eprintln!("Creating thread for {:?} in {:?}", emoji, channel_id);
        channel
            .create_thread(
                &ctx.http,
                CreateThread::new(name)
                    .auto_archive_duration(AutoArchiveDuration::OneWeek)
                    .kind(ChannelType::PublicThread),
            )
            .await
            .expect("No permissions to create thread!")
    }

    async fn shutdown(&self, ctx: &Context) {
        let data = ctx.data.read().await;
        if let Some(manager) = data.get::<ShardManagerContainer>() {
            eprintln!("Shutting down...");
            manager.shutdown_all().await;
        } else {
            eprintln!("There was a problem getting the shard manager");
        }
    }
}

fn emoji_as_string(emoji: &Emoji) -> &str {
    match emoji {
        Emoji::Custom { name, .. } => name,
        Emoji::Unicode { string } => string,
    }
}
