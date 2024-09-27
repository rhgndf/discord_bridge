use serenity::{
    client::Client,
    framework::{standard::Configuration, StandardFramework},
    prelude::Mutex,
};

use songbird::{driver::DecodeMode, Config, SerenityInit};

use dotenv::dotenv;
use serenity::prelude::*;
use std::{env, sync::Arc};

mod bridge;
mod commands;
mod handler;
mod usrp;

use handler::Handler;

#[tokio::main]
async fn main() {
    dotenv().ok();

    env::var("TARGET_RX_ADDR").expect("Expected a target rx address in the environment");
    env::var("LOCAL_RX_ADDR").expect("Expected a local rx address in the environment");

    let token = env::var("BOT_TOKEN").expect("Expected a token in the environment");

    let framework = StandardFramework::new().group(&commands::GENERAL_GROUP);
    framework.configure(
        Configuration::new().prefix(
            env::var("BOT_PREFIX")
                .unwrap_or_else(|_| String::from("!"))
                .as_str(),
        ),
    );

    let intents = GatewayIntents::non_privileged()
        | GatewayIntents::MESSAGE_CONTENT
        | GatewayIntents::GUILD_VOICE_STATES;

    let songbird_config = Config::default().decode_mode(DecodeMode::Decode);

    let mut client = Client::builder(&token, intents)
        .event_handler(Handler)
        .framework(framework)
        .register_songbird_from_config(songbird_config)
        .await
        .expect("Error creating client");

    {
        //let mut data = client.data.write().await;
        //data.insert::<commands::DiscordBotContext>(Arc::new(Mutex::new(Receiver::new())));
    }

    let _ = client
        .start()
        .await
        .map_err(|why| println!("Client ended: {:?}", why));
}
