#![feature(map_first_last, slice_concat_trait, let_else)]

use std::{collections::BTreeSet, sync::Arc};
use std::env;
use std::error::Error;
use std::path::Path;
use serenity::model::id::GuildId;
use serenity::model::prelude::CurrentUser;
use serenity::{
    async_trait,
    client::bridge::gateway::ShardManager,
    http::Typing,
    model::{
        channel::{ChannelType, GuildChannel, ReactionType},
        gateway::Ready,
        id::MessageId,
    },
    prelude::{Client, Context, SerenityError, Mutex, TypeMapKey, EventHandler},
};

mod config;
mod toplist;
mod time_utils;

use config::{Config, Emoji};
use toplist::{Toplist, MsgWrap};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::from_path(Path::new("./config.toml"))?;

    // Parse week from command line argument
    let week_param = std::env::args().nth(1);
    let week_param_str = week_param.as_ref().map(|s| s.as_str());
    let options = Options { calendar_week: time_utils::parse_iso_week(week_param_str)? };

    eprintln!("Config: {:?}", config);
    eprintln!("Options: {:?}", options);

    // Login with a bot token from the environment
    let token = env::var("DISCORD_TOKEN").expect("token missing");
    let mut client = Client::builder(token)
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
    type Value = Arc<Mutex<ShardManager>>;
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
        let mut data = ctx.data.write().await;
        data.insert::<CurrentUserContainer>(ready.user.clone());
    }

    async fn cache_ready(&self, ctx: Context, _guilds: Vec<GuildId>) {
        eprintln!("Cache ready");

        let user = {
            let data = ctx.data.read().await;
            data.get::<CurrentUserContainer>().unwrap().clone()
        };

        let toplist = self.scan_channel(&ctx, &user).await;

        // println!("{:#?}", &toplist.top);

        let typing = self.config.target_channel_id().start_typing(&ctx.http);

        let emoji_to_post: Vec<_> = self.config.toplist.iter()
            .map(|item| &item.emoji)
            .collect();

        for key in emoji_to_post {
            if let Some(list) = toplist.top.get(&key) {
                self.post_toplist_thread(&ctx, &Some(key.clone()), list).await.expect("unable to create message");
            }
        }
        if self.config.other.enabled {
            self.post_toplist_thread(&ctx, &None, &toplist.other).await.expect("unable to create message");
        }

        typing.map(Typing::stop).err().map(|e| eprintln!("wasn't able to (un-)set typing status; {:?}", e));

        self.shutdown(&ctx).await;
    }
}

impl ReactionCounter {

    async fn scan_channel<'c>(&'c self, ctx: &Context, user: &CurrentUser) -> Toplist<'c> {
        let channel_id = self.config.channel_id;
        eprintln!("Scanning channel {:?} over {:?}", channel_id, self.options.calendar_week);

        let start_time = time_utils::iso_week_to_datetime(self.options.calendar_week);
        let end_time = start_time + chrono::Duration::weeks(1);
        eprintln!("Time span: {:?} til {:?}", start_time, end_time);

        let mut toplist = Toplist::new(&self.config, user, ctx.http.clone());
        let mut first_id: MessageId = (time_utils::time_snowflake(start_time, false) - 1).into();

        'outer: for page in 1.. {
            eprintln!("Fetching page {} (after {})", page, time_utils::snowflake_time(first_id));
            let msgs = channel_id
                .messages(&ctx.http, |retriever| retriever.after(first_id).limit(100))
                .await
                .unwrap();
            eprintln!("Retrieved {} messages", msgs.len());

            first_id = match msgs.first() {
                Some(first) => first.id,
                None => break,
            };

            for msg in &msgs {
                if msg.timestamp > end_time {
                    break 'outer;
                }
                toplist.append(msg);
            }
        }

        toplist.finalize().await.expect("unable to fetch reaction users");

        eprintln!("Finished collecting messages");
        toplist
    }

    async fn post_toplist_thread(&self, ctx: &Context, emoji: &Option<Emoji>, list: &BTreeSet<MsgWrap>) -> Result<(), SerenityError> {
        let thread = self.create_thread(ctx, emoji).await;

        eprintln!("Starting to populate thread for {:?}", emoji);

        let items_with_rank: Vec<_> = list.iter()
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
            thread.send_message(&ctx.http, |msg| {
                msg.content(format!(
                    "```c\n{} // {} user{}\n```",
                    rank,
                    item.count,
                    if item.count == 1 { "" } else { "s" },
                ))
            }).await?;

            thread.send_message(&ctx.http, |msg| {
                msg.content(format!("{}", &item.content));
                msg.allowed_mentions(|am| am.empty_parse())
            }).await?;

            thread.send_message(&ctx.http, |msg| {
                msg.content(format!("by {} ({})", &item.message.author, &item.message.author.name));
                msg.add_embed(|embed| {
                    embed.title("  ");
                    let reaction_strs: Vec<_> = item.message.reactions
                        .iter()
                        .map(|r| format!("{} {}", reaction_for_message(&r.reaction_type), r.count - r.me as u64))
                        .collect();
                    embed.description(
                        format!("{} | [link]({})", reaction_strs.join(" | "), item.message.link())
                    )
                });
                msg.allowed_mentions(|am| am.empty_parse())
            }).await?;
        }

        eprintln!("Done populating thread for {:?}", emoji);
        Ok(())
    }

    async fn create_thread(&self, ctx: &Context, emoji: &Option<Emoji>) -> GuildChannel {
        let channel_id = self.config.target_channel_id();
        let channel = channel_id.to_channel(&ctx.http)
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
        channel.create_private_thread(&ctx.http, |thread| {
            thread.name(name);
            thread.auto_archive_duration(10080);
            thread.kind(ChannelType::PublicThread)
        }).await.expect("No permissions to create thread!")
    }

    async fn shutdown(&self, ctx: &Context) {
        let data = ctx.data.read().await;
        if let Some(manager) = data.get::<ShardManagerContainer>() {
            eprintln!("Shutting down...");
            manager.lock().await.shutdown_all().await;
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

fn reaction_for_message(reaction: &ReactionType) -> String {
    match &reaction {
        ReactionType::Custom { name, id, animated, .. } => format!(
            "<{}:{}:{}>",
            if *animated { "a" } else { "" },
            name.as_deref().unwrap_or("no_name"),
            id.0,
        ),
        ReactionType::Unicode(string) => string.clone(),
        _ => "?".to_string(),
    }
}
