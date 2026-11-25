//! Steam Web API: the list of games you own (needs a free API key, see
//! <https://steamcommunity.com/dev/apikey>, and your 64-bit Steam id).

use anyhow::{bail, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Owned {
    pub appid: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub playtime_forever: u64,
}

pub fn owned_games(key: &str, steam_id: &str) -> Result<Vec<Owned>> {
    if key.is_empty() || steam_id.is_empty() {
        bail!("set your Steam API key and Steam id in the settings");
    }
    #[derive(Deserialize)]
    struct Resp {
        response: Inner,
    }
    #[derive(Deserialize)]
    struct Inner {
        #[serde(default)]
        games: Vec<Owned>,
    }
    let r: Resp = ureq::get("https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/")
        .query("key", key)
        .query("steamid", steam_id)
        .query("include_appinfo", "1")
        .query("include_played_free_games", "1")
        .timeout(std::time::Duration::from_secs(20))
        .call()?
        .into_json()?;
    let mut g = r.response.games;
    g.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(g)
}
