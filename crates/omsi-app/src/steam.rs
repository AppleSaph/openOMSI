//! Steamworks API layer for displaying in-game status using steam's presence

pub struct Steam {
    pub client: steamworks::Client,
    pub presence: Presence
}
pub struct Presence {
    pub map_name: Option<String>,
}

impl Steam {
    pub fn start() -> Option<Steam> {
        if let Ok(client) = steamworks::Client::init_app(252530) {
            Some(Steam {
                client,
                presence: Presence {
                    map_name: None
                }
            })
        } else {
            None
        }
    }

    pub fn set_presence(&mut self, new_presence: Presence) {
        self.presence = new_presence;

        let success = self.client.friends().set_rich_presence("map_name", self.presence.map_name.as_deref());

        match self.presence.map_name.clone() {
            Some(map_name) => log::info!("steam: map_name set to {} with return {}", map_name, success),
            None => log::info!("steam: map_name set to None with return {}", success)
        }
    }
}
