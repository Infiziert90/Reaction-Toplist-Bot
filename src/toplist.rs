use std::cmp::Ordering;
use std::collections::{HashMap, BTreeSet, HashSet};
use std::sync::Arc;

use linkify::LinkFinder;
use serenity::futures::future::try_join_all;
use serenity::http::Http;
use serenity::model::channel::{Message, ReactionType, MessageReaction};
use serenity::model::prelude::CurrentUser;
use serenity::prelude::SerenityError;

use crate::config::{Config, Emoji};

#[derive(Debug)]
pub struct Toplist<'c> {
    config: &'c Config,
    current_user: CurrentUser,
    http: Arc<Http>,
    pub top: HashMap<Option<Emoji>, BTreeSet<MsgWrap>>,
    link_finder: LinkFinder,
}

impl<'c> Toplist<'c> {
    pub fn new(config: &'c Config, current_user: &CurrentUser, http: Arc<Http>) -> Self {
        Toplist {
            config,
            current_user: current_user.clone(),
            http,
            top: HashMap::new(),
            link_finder: LinkFinder::new(),
        }
    }

    pub async fn append(&mut self, message: &Message) -> Result<(), SerenityError> {
        let content = self.find_content(message);

        let handled = self.append_known(message, &content);
        if !handled && self.config.other.enabled {
            self.append_other(message, &content).await?;
        }
        Ok(())
    }

    fn find_content(&self, message: &Message) -> String {
        match message.attachments.first() {
            Some(t) => t.url.clone(),
            None => {
                let links: Vec<_> = self.link_finder.links(&message.content).collect();
                links.first()
                    .map(|s| s.as_str().to_string())
                    // TODO or discard if no link?
                    .unwrap_or_else(|| message.content.clone())
            }
        }
    }

    fn append_known(&mut self, message: &Message, content: &str) -> bool {
        let mut appended = false;
        for entry in self.config.toplist.iter() {
            let count_opt = message.reactions.iter().find_map(|r| is_same_emoji(r, &entry.emoji).then_some(r.count));
            if let Some(count) = count_opt {
                if let Some(list) = self.prepate_for_insert(Some(entry.emoji.clone()), entry.max, count) {
                    let msg_wrap = MsgWrap { count, message: message.clone(), content: content.to_string() };
                    list.insert(msg_wrap);
                    appended = true;
                }
            }
        }
        appended
    }

    async fn append_other(&mut self, message: &Message, content: &str) -> Result<(), SerenityError> {
        let stripped_reactions: Vec<_> = message.reactions.iter()
            .filter(|r| !self.config.other.ignore.iter().any(|ignore| is_same_emoji(r, ignore)))
            .cloned()
            .collect();

        let count = self.count_distinct_users(&stripped_reactions, message).await?;

        if let Some(list) = self.prepate_for_insert(None, self.config.other.max, count) {
            let mut message = message.clone();
            message.reactions = stripped_reactions;
            let msg_wrap = MsgWrap { count, message, content: content.to_string() };
            list.insert(msg_wrap);
        };
        Ok(())
    }

    fn prepate_for_insert(&mut self, emoji: Option<Emoji>, max: usize, count: u64) -> Option<&mut BTreeSet<MsgWrap>> {
        if count == 0 {
            return None;
        }
        let list = self.top.entry(emoji).or_insert_with(BTreeSet::default);
        let min = list.first().map(|mw| mw.count).unwrap_or(0);
        if min > count {
            return None;
        }
        if list.len() == max {
            list.pop_first();
        }
        Some(list)
    }

    async fn count_distinct_users(&mut self, reactions: &[MessageReaction], message: &Message) -> Result<u64, SerenityError> {
        let futures: Vec<_> = reactions.iter()
            .map(|r| message.reaction_users(
                &self.http,
                r.reaction_type.clone(),
                Some(self.config.per_reaction_limit),
                None
            ))
            .collect();

        let mut users: HashSet<_> = try_join_all(futures).await?
            .iter()
            .flat_map(|users| users.into_iter().map(|user| user.id))
            .collect();

        users.remove(&self.current_user.id);
        Ok(users.len() as u64)
    }
}

fn is_same_emoji(r: &MessageReaction, emoji: &Emoji) -> bool {
    match (&r.reaction_type, emoji) {
        (ReactionType::Custom { id, ..}, Emoji::Custom { id: id2, .. })
            if id == id2 => true,
        (ReactionType::Unicode(s), Emoji::Unicode { string, .. })
            if s == string => true,
        _ => false,
    }
}

#[derive(Debug)]
pub struct MsgWrap {
    pub count: u64, // must be first for auto-deriving ordering
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
        self.count.cmp(&other.count)
            .then_with(|| self.message.id.cmp(&other.message.id))
    }
}
