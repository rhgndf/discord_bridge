use chrono::prelude::Utc;
use log::{debug, info};
use poise::serenity_prelude as serenity;
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use serenity::{all::EditChannel, prelude::Mentionable};
use songbird::{
    input::{AsyncAdapterStream, RawAdapter},
    CoreEvent,
};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

use crate::{Context, Data, Error};

use crate::{
    bridge::USRPEventHandler,
    usrp::{packets::USRPPacket, USRPClient},
    util::RingBufferStream,
};

#[poise::command(slash_command)]
pub async fn data(
    ctx: Context<'_>,
    #[description = "user"] user: serenity::Member,
) -> Result<(), Error> {
    println!("{:?}", user);
    //user.nick.unwrap_or(user.user.name.clone())
    let nick = user.nick.unwrap_or(
        user.user
            .global_name
            .unwrap_or(user.user.name),
    );
    match crate::util::extract_callsign(&nick) {
        Some(callsign) => {
            ctx.say(format!("Callsign: {}", callsign)).await?;
        }
        None => {
            ctx.say("No callsign found").await?;
        }
    }

    Ok(())
}

#[poise::command(slash_command)]
pub async fn join(
    ctx: Context<'_>,
    #[description = "Selected channel"]
    #[channel_types("Voice")]
    channel: serenity::GuildChannel,
) -> Result<(), Error> {
    let guild = ctx.guild().ok_or("No guild?")?.clone();
    let guild_id = guild.id;

    let serenity_context = ctx.serenity_context();
    let manager = songbird::get(serenity_context)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Ok(handler_lock) = manager.join(guild_id, channel.id).await {
        let mut handler = handler_lock.lock().await;

        let mut usrpclient = USRPClient::new(
            "127.0.0.1:34001".parse().unwrap(),
            "127.0.0.1:32001".parse().unwrap(),
            None,
        );

        usrpclient
            .connect()
            .await
            .expect("Error connecting to USRP");

        let usrpclient = Arc::new(usrpclient);

        let usrp_channel = USRPEventHandler::new(
            usrpclient.clone(),
            guild_id,
            serenity_context.http.clone(),
            serenity_context.cache.clone(),
        );

        //let builder = EditChannel::new().status("");
        /*if let Err(why) = channel.edit(&serenity_context.http, builder).await {
            // properly handle the error
            println!("Error updating channel: {:?}", why);
        }*/

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
        let _ = handler.play_input(adapter.into());

        info!(
            "Connected to voice channel {} with id {}",
            channel.name(),
            channel.id.get()
        );

        /*ctx.data()
        .clients
        .lock()
        .await
        .insert(guild_id.get(), usrpclient.clone());*/

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

            while let Some(_handler) = handler.upgrade() {
                let packet = usrpclient.recv().await;
                if let Some(packet) = packet {
                    match packet {
                        USRPPacket::Audio(packet) => {
                            // Convert from i16 to f64
                            let audio_vec: Vec<_> = packet
                                .audio
                                .into_iter()
                                .map(|x| x as f64 / 32768.0)
                                .collect();
                            // Resample to 8kHz
                            let audio_data: Vec<_> = resampler
                                .process(&[&audio_vec], None)
                                .into_iter()
                                .flat_map(|x| x.into_iter())
                                .take(1)
                                .flat_map(|x| x.into_iter())
                                .map(|x| x as f32)
                                .map(|x| [x, x]) // Mono to stereo
                                .flatten()
                                .map(|x| x.to_le_bytes()) // Convert to byte stream
                                .flatten()
                                .collect();
                            let _ = audio_sender.write(&audio_data).await;
                        }
                        USRPPacket::Start(_) => {}
                        USRPPacket::End(_) => {}
                        _ => {
                            debug!("Unknown USRP packet");
                        }
                    }
                } else {
                    break;
                }
            }
        });
        ctx.say(&format!("Joined {}", channel.mention())).await?;
    } else {
        ctx.say("Error joining the channel").await?;
    }
    Ok(())
}

#[poise::command(slash_command)]
pub async fn leave(ctx: Context<'_>) -> Result<(), Error> {
    let guild = ctx.guild().ok_or("No guild?")?.clone();
    let guild_id = guild.id;
    let user_id = ctx.author_member().await.ok_or("No user?")?.user.id;

    let channel_id = guild
        .voice_states
        .get(&user_id)
        .and_then(|voice_state| voice_state.channel_id)
        .unwrap();

    let serenity_context = ctx.serenity_context();
    let manager = songbird::get(serenity_context)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();
    let has_handler = manager.get(guild_id).is_some();

    info!(
        "Disconnected from voice channel {} with id {}",
        channel_id
            .name(&serenity_context.http)
            .await
            .unwrap_or("{unknown}".to_string()),
        channel_id.get()
    );

    if has_handler {
        if let Err(e) = manager.remove(guild_id).await {
            ctx.say(format!("Failed: {:?}", e)).await?;
        }
        ctx.say(&format!("Left {}", channel_id.mention())).await?;
    } else {
        ctx.reply("⚠️ Not in a voice channel").await?;
    }

    Ok(())
}

#[poise::command(slash_command)]
pub async fn ping(ctx: Context<'_>, _command: Option<String>) -> Result<(), Error> {
    let now = Utc::now();
    let elapsed = now - *ctx.created_at();
    ctx.reply(format!("Pong! ({} ms)", elapsed.num_milliseconds()))
        .await?;
    Ok(())
}
