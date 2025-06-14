use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use linkify::LinkFinder;
use serenity::futures::future::try_join_all;
use serenity::http::Http;
use serenity::model::channel::{Message, MessageReaction, ReactionType};
use serenity::model::prelude::CurrentUser;
use serenity::prelude::SerenityError;

use crate::config::{Config, Emoji};

#[derive(Debug)]
pub struct Toplist<'c> {
    config: &'c Config,
    current_user: CurrentUser,
    http: Arc<Http>,
    pub top: HashMap<Emoji, BTreeSet<MsgWrap>>,
    pub other_prep: BTreeSet<MsgWrap>,
    pub other: BTreeSet<MsgWrap>,
    link_finder: LinkFinder,
}

impl<'c> Toplist<'c> {
    pub fn new(config: &'c Config, current_user: &CurrentUser, http: Arc<Http>) -> Self {
        Toplist {
            config,
            current_user: current_user.clone(),
            http,
            top: Default::default(),
            other_prep: Default::default(),
            other: Default::default(),
            link_finder: Default::default(),
        }
    }

    pub fn append(&mut self, message: &Message) {
        let content = self.find_content(message);

        self.append_known(message, &content);
        if self.config.other.enabled {
            self.append_other(message, &content);
        }
    }

    fn find_content(&self, message: &Message) -> String {
        match message.attachments.first() {
            Some(t) => t.url.clone(),
            None => {
                let links: Vec<_> = self.link_finder.links(&message.content).collect();
                links
                    .first()
                    .map(|s| s.as_str().to_string())
                    .unwrap_or_else(|| message.content.clone())
            }
        }
    }

    fn append_known(&mut self, message: &Message, content: &str) {
        for entry in self.config.toplist.iter() {
            let Some(count) = message
                .reactions
                .iter()
                .find_map(|r| is_same_emoji(r, &entry.emoji).then_some(r.count - r.me as u64))
            else {
                continue;
            };

            let list = self.top.entry(entry.emoji.clone()).or_default();
            if Self::prepare_list_for_insert(list, entry.max, count).is_some() {
                let msg_wrap = MsgWrap {
                    count,
                    message: message.clone(),
                    content: content.to_string(),
                };
                list.insert(msg_wrap);
            }
        }
    }

    fn append_other(&mut self, message: &Message, content: &str) {
        let stripped_reactions: Vec<_> = message
            .reactions
            .iter()
            .filter(|r| {
                !self
                    .config
                    .other
                    .ignore
                    .iter()
                    .any(|ignore| is_same_emoji(r, ignore))
            })
            .cloned()
            .collect();

        // This is an approximation of the actual count
        // since it has the number of total reactions
        // where we want the number of distinct users that reacted.
        // It serves to maintain rough ordering of the BTreeSet.
        let count: u64 = stripped_reactions
            .iter()
            .map(|r| r.count - r.me as u64)
            .sum();
        if count == 0 {
            return;
        }

        let mut message = message.clone();
        message.reactions = stripped_reactions;
        let msg_wrap = MsgWrap {
            count,
            message,
            content: content.to_string(),
        };
        self.other_prep.insert(msg_wrap);
    }

    fn prepare_list_for_insert(
        list: &mut BTreeSet<MsgWrap>,
        max: usize,
        count: u64,
    ) -> Option<u64> {
        if count == 0 {
            return None;
        }
        let min_count = Self::min_count(list, max);
        let should_insert = count > min_count;
        if should_insert && min_count > 0 {
            list.pop_first();
        }
        should_insert.then_some(min_count.min(count))
    }

    pub async fn finalize(&mut self) -> Result<(), SerenityError> {
        if !self.config.other.enabled {
            return Ok(());
        }

        let ids_to_ignore: HashSet<_> = self
            .top
            .values()
            .flat_map(|list| list.iter().map(|wrap| wrap.message.id))
            .collect();

        eprintln!(
            "Starting to fill 'Other' toplist (from {} messages, ignoring {})",
            self.other_prep.len(),
            ids_to_ignore.len(),
        );

        let mut min = 0;
        for (i, wrap) in self.other_prep.iter().rev().enumerate() {
            if i != 0 && i % 100 == 0 {
                eprintln!("Checking reactions for post {i}");
            }
            if wrap.count <= min {
                // Impossible to have more unique users than sum of reactions
                eprintln!("Early-exiting 'Other' collection after {i} posts");
                break;
            }
            if ids_to_ignore.contains(&wrap.message.id) {
                continue;
            }

            let count = self.count_distinct_users(&wrap.message).await?;
            let Some(new_min) =
                Self::prepare_list_for_insert(&mut self.other, self.config.other.max, count)
            else {
                continue;
            };
            eprintln!("Adding post {i} to 'Other' collection (with {count}), new min: {new_min}");
            let new_wrap = MsgWrap {
                count,
                ..wrap.clone()
            };
            self.other.insert(new_wrap);
            min = new_min;
        }
        eprintln!(
            "Collected {} messages for the 'Other' toplist",
            self.other.len()
        );
        Ok(())
    }

    fn min_count(list: &BTreeSet<MsgWrap>, max_entries: usize) -> u64 {
        if list.len() < max_entries {
            0
        } else {
            list.first().map(|mw| mw.count).unwrap_or(0)
        }
    }

    async fn count_distinct_users(&self, message: &Message) -> Result<u64, SerenityError> {
        let futures: Vec<_> = message
            .reactions
            .iter()
            .map(|r| {
                message.reaction_users(
                    &self.http,
                    r.reaction_type.clone(),
                    Some(self.config.per_reaction_limit),
                    None,
                )
            })
            .collect();

        let mut users: HashSet<_> = try_join_all(futures)
            .await?
            .iter()
            .flat_map(|users| users.into_iter().map(|user| user.id))
            .collect();

        users.remove(&self.current_user.id);
        Ok(users.len() as u64)
    }
}

fn is_same_emoji(r: &MessageReaction, emoji: &Emoji) -> bool {
    match (&r.reaction_type, emoji) {
        (ReactionType::Custom { id, .. }, Emoji::Custom { id: id2, .. }) if id == id2 => true,
        (ReactionType::Unicode(s), Emoji::Unicode { string, .. }) if s == string => true,
        _ => false,
    }
}

#[derive(Debug, Clone)]
pub struct MsgWrap {
    pub count: u64,
    pub content: String,
    pub message: Message,
}

impl PartialEq for MsgWrap {
    fn eq(&self, other: &Self) -> bool {
        self.message.id == other.message.id
    }
}

impl Eq for MsgWrap {}

impl PartialOrd for MsgWrap {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MsgWrap {
    fn cmp(&self, other: &Self) -> Ordering {
        self.count
            .cmp(&other.count)
            .then_with(|| self.message.id.cmp(&other.message.id))
    }
}
