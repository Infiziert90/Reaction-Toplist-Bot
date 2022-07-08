extern crate serde_json;

use std::fmt;
use std::env;
use rayon::prelude::*;
use chrono::{Datelike, NaiveDate, NaiveDateTime, Utc};
use serenity::{
    async_trait,
    model::{gateway::Ready},
    model::channel::{Message, ReactionType::Custom},
    prelude::*,
    model::id::{ChannelId, EmojiId}
};
use serenity::framework::standard::{
    StandardFramework,
    CommandResult,
    CommandError,
    Args,
    macros::{
        command,
        group
    }
};
use serenity::model::channel::{ChannelType, GuildChannel};
use serde::{Deserialize, Serialize};
use linkify::{LinkFinder};


const CHANNEL: u64 = 222037538238365696;
const TEST: u64 = 411952567795449867;

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
struct Counter {
    link: String,
    msg_link: String,
    based: u64,
    cringe: u64,
    author: String,
    custom_name: String
}

impl Counter {
    fn new(l: String, ml: String, b: u64, c: u64, a: String) -> Counter {
        Counter{
            link: l,
            msg_link: ml,
            based: b,
            cringe: c,
            author: a,
            custom_name: String::new()
        }
    }

    fn new_custom(ml: String, b: u64, a: String, name: String) -> Counter {
        Counter{
            link: String::new(),
            msg_link: ml,
            based: b,
            cringe: 0,
            author: a,
            custom_name: name,
        }
    }
}

impl fmt::Display for Counter {
    // This trait requires `fmt` with this exact signature.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}: {}    Author: {}", self.custom_name, self.based, self.author)
    }
}

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
    }
}

#[group]
#[commands(ranking_based, ranking_cringe, ranking_custom)]
struct General;

fn args_into_date(mut args: Args) -> std::result::Result<(NaiveDateTime, String), CommandError> {
    let year = args.single::<i32>()?;
    let month = args.single::<u32>()?;
    let day = args.single::<u32>()?;
    let name = match args.single::<String>() {
        Ok(t) => t,
        Err(_) => String::new()
    };

    Ok((NaiveDate::from_ymd(year, month, day).and_hms(0, 0, 0), name))
}

async fn custom_scan_channel(ctx: &Context, after: i64, custom_id: u64, name: String) -> Vec<Counter> {
    let channel_id: ChannelId = ChannelId(CHANNEL);
    let mut last_id = channel_id.to_channel(&ctx.http)
        .await
        .unwrap()
        .guild()
        .unwrap()
        .last_message_id
        .unwrap();

    let mut counted: Vec<Counter> = Vec::new();
    match get_custom_count(channel_id.message(&ctx.http, last_id).await.unwrap(), custom_id, name.clone()) {
        Some(t) => counted.push(t),
        None => {}
    };

    loop {
        let msg = channel_id
            .messages(&ctx.http, |retriever| retriever
                .before(last_id)
                .limit(100)
            )
            .await
            .unwrap();

        let last = msg.last().unwrap().clone();
        last_id = last.id;


        let mut counts: Vec<Counter> = msg.into_par_iter()
            .filter(|m| m.timestamp.timestamp() > after )
            .filter_map(|m| {
                return get_custom_count(m, custom_id, name.clone())
            })
            .collect();

        counted.append(&mut counts);
        if last.timestamp.timestamp() < after {
            break
        }
    }

    counted
}

fn get_custom_count(message: Message, custom_id: u64, name: String) -> Option<Counter> {
    let (based, cringe) = message.reactions
        .iter()
        .fold((0, 0), |(based, cringe), r| {
            match &r.reaction_type {
                Custom{id, ..} if id == &EmojiId(custom_id) => (r.count, r.count),
                _ => (based, cringe),
            }
        });

    if based == 0 || cringe == 0 {
        return None;
    }

    let ml = message.link();
    Some(Counter::new_custom(ml, based, message.author.name, name))
}

async fn scan_channel(ctx: &Context, after: i64) -> Vec<Counter> {
    let channel_id: ChannelId = ChannelId(CHANNEL);
    let mut last_id = channel_id.to_channel(&ctx.http)
        .await
        .unwrap()
        .guild()
        .unwrap()
        .last_message_id
        .unwrap();

    let mut counted: Vec<Counter> = Vec::new();
    match get_count(channel_id.message(&ctx.http, last_id).await.unwrap()) {
        Some(t) => counted.push(t),
        None => {}
    };

    loop {
        let msg = channel_id
            .messages(&ctx.http, |retriever| retriever
                .before(last_id)
                .limit(100)
            )
            .await
            .unwrap();

        let last = msg.last().unwrap().clone();
        last_id = last.id;


        let mut counts: Vec<Counter> = msg.into_par_iter()
            .filter(|m| m.timestamp.timestamp() > after )
            .filter_map(|m| {
                return get_count(m)
            })
            .collect();

        counted.append(&mut counts);
        if last.timestamp.timestamp() < after {
            break
        }
    }

    counted
}

fn get_count(message: Message) -> Option<Counter> {
    let (based, cringe) = message.reactions
        .iter()
        .fold((0, 0), |(based, cringe), r| {
            if !(&r.me) {return (based, cringe)};
            match &r.reaction_type {
                Custom{name, ..} if name == &Some("based".to_string()) => (r.count, cringe),
                Custom{name, ..} if name == &Some("cringe".to_string()) => (based, r.count),
                _ => (based, cringe),
            }
        });

    if based == 0 || cringe == 0 {
        return None;
    }

    let l;
    let ml = message.link();
    let finder = LinkFinder::new();

    match message.attachments.first() {
        Some(t) => {
            l = t.url.clone();
        },
        None => {
            let links: Vec<_> = finder.links(&message.content).collect();
            let link = match links.first() {
                Some(t) => t,
                None => {
                    println!("{}", message.content);
                    return None
                }
            };

            l = link.as_str().to_string();
        }
    };

    Some(Counter::new(l, ml, based, cringe, message.author.name))
}

fn generate_thread_name(name: String, t: &str) -> String {
    return if name.is_empty() {
        let isoweek = &Utc::now().naive_utc().iso_week();
        format!("{} WN {} {}", t, isoweek.week(), isoweek.year())
    } else {
        name
    }
}

async fn generate_thread_messages(ctx: &Context, counts: Vec<Counter>, name: String) {
    let thread = create_thread(ctx, name).await;

    println!("Start populating thread.");

    for (idx, item) in counts.iter().enumerate() {
        if idx > 14 {break;}
        thread.send_message(&ctx.http, |msg| {
            msg.content(format!("{}: {}", idx+1, &item.author))
        }).await.expect("Unable to send message.");

        thread.send_message(&ctx.http, |msg| {
            msg.content(format!("{}", &item.link))
        }).await.expect("Unable to send message.");

        thread.send_message(&ctx.http, |msg| {
            msg.add_embed(|embed| {
                embed.title("  ");
                embed.description(
                    format!("{} <:based:748564944449962017> {} <:cringe:748564944819060856>    [link]({})",
                            &item.based, &item.cringe, &item.msg_link)
                )
            })
        }).await.expect("Unable to send message.");
    }

    println!("Done populating thread.");
}

async fn create_thread(ctx: &Context, name: String) -> GuildChannel {
    let channel_id: ChannelId = ChannelId(CHANNEL);
    let channel = channel_id.to_channel(&ctx.http)
        .await
        .unwrap()
        .guild()
        .unwrap();
    channel.create_private_thread(&ctx.http, |thread| {
        thread.name(name);
        thread.auto_archive_duration(10080);
        thread.kind(ChannelType::PublicThread)
    }).await.expect("No permissions to create thread!")
}

#[command]
async fn ranking_based(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let (date, name) = args_into_date(args).expect("Wrong arguments");
    let after = date.timestamp();

    println!("Start counting based memes");
    let typing = msg.channel_id.start_typing(&ctx.http).unwrap();
    let mut counts = scan_channel(&ctx, after).await;
    counts.sort_by(|a, b| b.based.cmp(&a.based));
    typing.stop();

    //generate_html("Based", counts);
    generate_thread_messages(&ctx, counts, generate_thread_name(name, "Based")).await;
    Ok(())
}

#[command]
async fn ranking_cringe(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let (date, name) = args_into_date(args).expect("Wrong arguments");
    let after = date.timestamp();

    println!("Start counting cringe memes");
    let typing = msg.channel_id.start_typing(&ctx.http).unwrap();
    let mut counts = scan_channel(&ctx, after).await;
    counts.sort_by(|a, b| b.cringe.cmp(&a.cringe));
    typing.stop();

    generate_thread_messages(&ctx, counts, generate_thread_name(name, "Cringe")).await;
    Ok(())
}

#[command]
async fn ranking_custom(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let year = args.single::<i32>()?;
    let month = args.single::<u32>()?;
    let day = args.single::<u32>()?;
    let custom_id = args.single::<u64>()?;
    let name = args.single::<String>()?;


    let date = NaiveDate::from_ymd(year, month, day).and_hms(0, 0, 0);
    let after = date.timestamp();

    println!("Start counting custom memes");
    let t = msg.channel_id.start_typing(&ctx.http).unwrap();

    // let mut counts = custom_scan_channel(&ctx, after, custom_id, name.clone()).await;
    // counts.sort_by(|a, b| b.based.cmp(&a.based));
    //
    // let title = format!("The Ultimate {} Ranking!!!", name);
    // send_embed(&ctx, &msg, &title, date, counts).await;
    t.stop();

    Ok(())
}

#[tokio::main]
async fn main() {
    let framework = StandardFramework::new()
        .configure(|c| c.prefix(">>"))
        .group(&GENERAL_GROUP);

    // Login with a bot token from the environment
    let token = env::var("DISCORD_TOKEN").expect("token");
    let mut client = Client::builder(token)
        .event_handler(Handler)
        .framework(framework)
        .await
        .expect("Error creating client");

    // start listening for events by starting a single shard
    if let Err(why) = client.start().await {
        println!("An error occurred while running the client: {:?}", why);
    }
}