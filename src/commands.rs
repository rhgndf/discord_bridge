use chrono::prelude::Utc;
use std::{
    io::{ErrorKind as IoErrorKind, Result as IoResult, SeekFrom},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncSeek, AsyncWriteExt, ReadBuf};

use pin_project::pin_project;
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use serenity::{
    async_trait,
    client::Context as ClientContext,
    framework::standard::{
        macros::{command, group},
        CommandResult,
    },
    model::channel::Message,
    prelude::{Mentionable, Mutex, TypeMapKey},
    Result as SerenityResult,
};
use songbird::{
    input::{AsyncAdapterStream, AsyncMediaSource, AudioStreamError, RawAdapter},
    CoreEvent,
};

use crate::{
    bridge::USRPEventHandler,
    usrp::{packets::USRPPacket, USRPClient},
};

pub struct DiscordBotContext;

impl TypeMapKey for DiscordBotContext {
    type Value = Arc<Mutex<u64>>;
}

#[pin_project]
struct RingBufferStream {
    #[pin]
    stream: Box<dyn AsyncRead + Send + Sync + Unpin>,
}

impl AsyncRead for RingBufferStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<IoResult<()>> {
        AsyncRead::poll_read(self.project().stream, cx, buf)
    }
}

impl AsyncSeek for RingBufferStream {
    fn start_seek(self: Pin<&mut Self>, _position: SeekFrom) -> IoResult<()> {
        Err(IoErrorKind::Unsupported.into())
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<IoResult<u64>> {
        unreachable!()
    }
}

#[async_trait]
impl AsyncMediaSource for RingBufferStream {
    fn is_seekable(&self) -> bool {
        false
    }

    async fn byte_len(&self) -> Option<u64> {
        None
    }

    async fn try_resume(
        &mut self,
        _offset: u64,
    ) -> Result<Box<dyn AsyncMediaSource>, AudioStreamError> {
        Err(AudioStreamError::Unsupported)
    }
}

#[group]
#[commands(join, leave, ping)]
pub struct General;

#[command]
#[only_in(guilds)]
async fn join(ctx: &ClientContext, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap().clone();
    let guild_id = guild.id;

    let channel_id = guild
        .voice_states
        .get(&msg.author.id)
        .and_then(|voice_state| voice_state.channel_id);

    let channel = match channel_id {
        Some(channel) => channel,
        None => {
            check_msg(msg.reply(ctx, "⚠️ Not in a voice channel").await);

            return Ok(());
        }
    };

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Ok(handler_lock) = manager.join(guild_id, channel).await {
        let mut handler = handler_lock.lock().await;

        let mut usrpclient = USRPClient::new(
            "127.0.0.1:34001".parse().unwrap(),
            "127.0.0.1:32001".parse().unwrap(),
        );

        usrpclient
            .connect()
            .await
            .expect("Error connecting to USRP");

        let usrpclient = Arc::new(usrpclient);

        let usrp_channel = USRPEventHandler::new(
            usrpclient.clone(),
            guild_id,
            ctx.http.clone(),
            ctx.cache.clone(),
        );

        handler.add_global_event(CoreEvent::SpeakingStateUpdate.into(), usrp_channel.clone());
        handler.add_global_event(CoreEvent::VoiceTick.into(), usrp_channel.clone());
        handler.add_global_event(CoreEvent::ClientDisconnect.into(), usrp_channel.clone());

        // 100ms of buffering
        let (audio_receiver, mut audio_sender) = tokio::io::simplex(7680 * 5);

        let audio_stream = AsyncAdapterStream::new(
            Box::new(RingBufferStream {
                stream: Box::new(audio_receiver),
            }),
            7680 * 5,
        );
        let adapter = RawAdapter::new(audio_stream, 48000, 2);
        let audio_stream = handler.play_input(adapter.into());

        let handler = Arc::downgrade(&handler_lock);
        tokio::spawn(async move {
            let params = SincInterpolationParameters {
                sinc_len: 256,
                f_cutoff: 0.95,
                interpolation: SincInterpolationType::Linear,
                oversampling_factor: 256,
                window: WindowFunction::BlackmanHarris2,
            };
            let mut resampler =
                SincFixedIn::new(48000 as f64 / 8000 as f64, 2.0, params, 160, 1).unwrap();

            while let Some(handler) = handler.upgrade() {
                let packet = usrpclient.recv().await;
                if let Some(packet) = packet {
                    match packet {
                        USRPPacket::Audio(packet) => {
                            let audio_vec: Vec<_> = packet
                                .audio
                                .into_iter()
                                .map(|x| x as f64 / 32768.0)
                                .collect();
                            // Convert from mono to stereo
                            let audio_data: Vec<_> =
                                resampler.process(&[&audio_vec], None).unwrap()[0]
                                    .iter()
                                    .map(|x| *x as f32)
                                    .map(|x| [x, x])
                                    .flatten()
                                    .map(|x| x.to_le_bytes())
                                    .flatten()
                                    .collect();
                            audio_sender.write(&audio_data).await.unwrap();
                        }
                        USRPPacket::Start(packet) => {}
                        USRPPacket::End(packet) => {}
                        _ => {
                            println!("Unknown packet");
                        }
                    }
                }
            }
        });

        check_msg(
            msg.reply(ctx, &format!("Joined {}", channel.mention()))
                .await,
        );
    } else {
        check_msg(msg.reply(ctx, "Error joining the channel").await);
    }

    Ok(())
}

#[command]
#[only_in(guilds)]
async fn leave(ctx: &ClientContext, msg: &Message) -> CommandResult {
    let guild = msg.guild(&ctx.cache).unwrap().clone();
    let guild_id = guild.id;

    let channel_id = guild
        .voice_states
        .get(&msg.author.id)
        .and_then(|voice_state| voice_state.channel_id)
        .unwrap();

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();
    let has_handler = manager.get(guild_id).is_some();

    if has_handler {
        if let Err(e) = manager.remove(guild_id).await {
            check_msg(msg.reply(ctx, format!("Failed: {:?}", e)).await);
        }
        check_msg(
            msg.reply(ctx, &format!("Left {}", channel_id.mention()))
                .await,
        );
    } else {
        check_msg(msg.reply(ctx, "⚠️ Not in a voice channel").await);
    }

    Ok(())
}

#[command]
async fn ping(ctx: &ClientContext, msg: &Message) -> CommandResult {
    let now = Utc::now();
    let elapsed = now - *msg.timestamp;
    check_msg(
        msg.reply(ctx, format!("Pong! ({} ms)", elapsed.num_milliseconds()))
            .await,
    );

    Ok(())
}

fn check_msg(result: SerenityResult<Message>) {
    if let Err(why) = result {
        println!("Error sending message: {:?}", why);
    }
}
