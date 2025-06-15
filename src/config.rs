use std::{error::Error, path::Path};

use serde::Deserialize;
use serenity::model::id::{ChannelId, EmojiId};

fn default_max() -> usize {
    15
}

fn default_per_reaction_limit() -> u8 {
    50
}

#[derive(Deserialize, Debug)]
pub struct Config {
    pub channel_id: ChannelId,
    #[serde(default)]
    pub target_channel_id: Option<ChannelId>,
    #[serde(default = "default_per_reaction_limit")]
    pub per_reaction_limit: u8,
    pub toplist: Vec<Toplist>,
    pub other: Other,
}

impl Config {
    pub fn from_path(path: &Path) -> Result<Config, Box<dyn Error>> {
        let contents = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&contents)?)
    }

    pub fn target_channel_id(&self) -> ChannelId {
        self.target_channel_id.unwrap_or(self.channel_id)
    }
}

#[derive(Deserialize, Debug)]
pub struct Toplist {
    #[serde(default = "default_max")]
    pub max: usize,
    pub emoji: Emoji,
    pub min_reactions: Option<u64>,
}

#[derive(Deserialize, Debug)]
pub struct Other {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_max")]
    pub max: usize,
    #[serde(default)]
    pub ignore: Vec<Emoji>,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum Emoji {
    Custom { name: String, id: EmojiId },
    Unicode { string: String },
}
