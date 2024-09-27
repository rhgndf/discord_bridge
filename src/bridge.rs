use log::info;
use rand::seq::SliceRandom;
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use serenity::{
    all::Http,
    async_trait,
    cache::Cache,
    model::id::{GuildId, UserId},
};
use songbird::{
    events::context_data::VoiceTick,
    model::payload::{ClientDisconnect, Speaking},
    Event, EventContext, EventHandler as VoiceEventHandler,
};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

use crate::{
    usrp::{
        packets::{AudioPacket, EndPacket, StartPacket, USRPPacket},
        USRPClient,
    },
    util::extract_callsign,
};

struct UserData {
    callsign: String,
    nick: String,
    name: String,
    id: UserId,
}
pub struct USRPEventHandlerData {
    client: Arc<USRPClient>,
    http: Arc<Http>,
    cache: Arc<Cache>,

    resampler: SincFixedIn<f64>,

    guild_id: GuildId,

    user_ssrc_map: HashMap<u64, u32>,
    ssrc_map: HashMap<u32, UserData>,
    cur_ssrc: Option<u32>,
    timeout_counter: u32,
}

impl USRPEventHandlerData {
    pub fn new(
        client: Arc<USRPClient>,
        guild_id: GuildId,
        http: Arc<Http>,
        cache: Arc<Cache>,
    ) -> Self {
        let params = SincInterpolationParameters {
            sinc_len: 256,
            f_cutoff: 0.95,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: 256,
            window: WindowFunction::BlackmanHarris2,
        };

        let resampler = SincFixedIn::new(8000 as f64 / 48000 as f64, 2.0, params, 960, 1).unwrap();

        Self {
            client,
            cache,
            http,
            resampler,

            guild_id,
            user_ssrc_map: HashMap::new(),
            ssrc_map: HashMap::new(),
            cur_ssrc: None,
            timeout_counter: 0,
        }
    }

    fn ssrc_to_user(&self, ssrc: u32) -> Option<&UserData> {
        self.ssrc_map.get(&ssrc)
    }
}

impl Drop for USRPEventHandlerData {
    fn drop(&mut self) {}
}

impl USRPEventHandler {
    pub fn new(
        client: Arc<USRPClient>,
        guild_id: GuildId,
        http: Arc<Http>,
        cache: Arc<Cache>,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(USRPEventHandlerData::new(
                client, guild_id, http, cache,
            ))),
        }
    }
}

#[derive(Clone)]
pub struct USRPEventHandler {
    inner: Arc<Mutex<USRPEventHandlerData>>,
}

#[async_trait]
impl VoiceEventHandler for USRPEventHandler {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        use EventContext as Ctx;
        match ctx {
            Ctx::SpeakingStateUpdate(Speaking {
                speaking: _,
                ssrc,
                user_id,
                ..
            }) => {
                // Discord voice calls use RTP, where every sender uses a randomly allocated
                // *Synchronisation Source* (SSRC) to allow receivers to tell which audio
                // stream a received packet belongs to. As this number is not derived from
                // the sender's user_id, only Discord Voice Gateway messages like this one
                // inform us about which random SSRC a user has been allocated. Future voice
                // packets will contain *only* the SSRC.
                //
                // You can implement logic here so that you can differentiate users'
                // SSRCs and map the SSRC to the User ID and maintain this state.
                // Using this map, you can map the `ssrc` in `voice_packet`
                // to the user ID and handle their audio packets separately.

                let mut data = self.inner.lock().await;
                let user_id = user_id.as_ref()?;
                let guild = data
                    .guild_id
                    .to_guild_cached(&data.cache)
                    .map(|x| x.clone())?;

                let member = &*(guild.member(&data.http, user_id.0).await.ok()?);

                let nick = member.nick.clone().unwrap_or("".to_string());
                let callsign = extract_callsign(&nick).unwrap_or("".to_string());
                let name = member.user.name.clone();
                let id = member.user.id;
                let user_data = UserData {
                    callsign,
                    nick,
                    name,
                    id,
                };

                info!(
                    "{} ({}) with id: {} has connected",
                    user_data.callsign, user_data.name, user_data.id
                );

                data.ssrc_map.insert(*ssrc, user_data);
                data.user_ssrc_map.insert(id.get(), *ssrc);
            }
            Ctx::VoiceTick(VoiceTick {
                speaking,
                silent: _,
                ..
            }) => {
                let mut data = self.inner.lock().await;

                let mut audio_vec = Vec::new();

                let is_previously_transmitting = data.cur_ssrc.is_some();

                // If we don't have a current SSRC, we'll just pick a random one.
                if data.cur_ssrc.is_none() {
                    // Filter ssrcs not known to be associated with a user
                    let active_ssrcs: Vec<_> = speaking
                        .keys()
                        .filter(|&x| data.ssrc_map.get(x).is_some())
                        .cloned()
                        .collect();
                    data.cur_ssrc = active_ssrcs.choose(&mut rand::thread_rng()).copied();
                    data.timeout_counter = 10;
                }

                if let Some(cur_ssrc) = data.cur_ssrc {
                    let audio_data = speaking
                        .get(&cur_ssrc)
                        .and_then(|packet| packet.decoded_voice.as_ref());

                    if let Some(audio_data) = audio_data {
                        data.timeout_counter = 10;
                        // audio_data is L, R, L, R, merge it into a single channel
                        audio_vec = audio_data
                            .chunks_exact(2)
                            .map(|x| (x[0] as f64 + x[1] as f64) / 65536.0)
                            .collect();
                    } else {
                        data.timeout_counter -= 1;
                        if data.timeout_counter == 0 {
                            let user_data = data.ssrc_to_user(data.cur_ssrc.unwrap())?;
                            info!(
                                "{} ({}) with id: {} stopped transmitting",
                                user_data.callsign, user_data.name, user_data.id
                            );
                            data.cur_ssrc = None;
                        }
                    }
                }

                let is_currently_transmitting = data.cur_ssrc.is_some();

                // Edge detector
                if !is_previously_transmitting && is_currently_transmitting {
                    let _ = data
                        .client
                        .send(USRPPacket::Start(StartPacket {
                            sequence_number: data.client.get_and_increment_sequence_number(),
                        }))
                        .await;
                    let user_data = data.ssrc_to_user(data.cur_ssrc.unwrap())?;
                    info!(
                        "{} ({}) with id: {} started transmitting",
                        user_data.callsign, user_data.name, user_data.id
                    );
                } else if is_previously_transmitting && !is_currently_transmitting {
                    let _ = data
                        .client
                        .send(USRPPacket::End(EndPacket {
                            sequence_number: data.client.get_and_increment_sequence_number(),
                        }))
                        .await;
                }

                if audio_vec.len() == 960 {
                    let audio_output: Vec<_> = data
                        .resampler
                        .process(&[&audio_vec], None)
                        .ok()?
                        .iter()
                        .next()?
                        .into_iter()
                        .map(|f| (f * 32768.0) as i16)
                        .collect();

                    let _ = data
                        .client
                        .send(USRPPacket::Audio(AudioPacket {
                            sequence_number: data.client.get_and_increment_sequence_number(),
                            transmit: true,
                            audio: audio_output,
                        }))
                        .await;
                }
            }
            Ctx::ClientDisconnect(ClientDisconnect { user_id, .. }) => {
                let mut data = self.inner.lock().await;
                let ssrc = data.user_ssrc_map.remove(&user_id.0)?;
                let user_data = data.ssrc_map.remove(&ssrc)?;

                info!(
                    "{} ({}) with id: {} has disconnected",
                    user_data.callsign, user_data.name, user_data.id
                );
            }
            _ => {
                // We won't be registering this struct for any more event classes.
                unimplemented!()
            }
        }

        None
    }
}
