mod bridge;
mod commands;
mod handler;
mod usrp;
mod util;

use dotenv::dotenv;
use handler::Handler;
use log::info;
use poise::serenity_prelude as serenity;
use serenity::{all::GatewayIntents, client::Client};
use songbird::{driver::DecodeMode, Config, SerenityInit};
use usrp::USRPClient;
use std::{
    collections::HashMap,
    env,
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;
pub struct Data {
    clients: Mutex<HashMap<u64, Arc<USRPClient>>>,
}

async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
    // This is our custom error handler
    // They are many errors that can occur, so we only handle the ones we want to customize
    // and forward the rest to the default handler
    match error {
        poise::FrameworkError::Setup { error, .. } => panic!("Failed to start bot: {:?}", error),
        poise::FrameworkError::Command { error, ctx, .. } => {
            println!("Error in command `{}`: {:?}", ctx.command().name, error,);
        }
        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                println!("Error while handling error: {}", e)
            }
        }
    }
}

#[tokio::main]
async fn main() {
    dotenv().ok();

    env::var("TARGET_RX_ADDR").expect("Expected a target rx address in the environment");
    env::var("LOCAL_RX_ADDR").expect("Expected a local rx address in the environment");

    fern::Dispatch::new()
        // Perform allocation-free log formatting
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {} {}] {}",
                humantime::format_rfc3339(std::time::SystemTime::now()),
                record.level(),
                record.target(),
                message
            ))
        })
        // Add blanket level filter -
        .level(log::LevelFilter::Warn)
        // - and per-module overrides
        .level_for("hyper", log::LevelFilter::Info)
        .level_for("discord_bridge", log::LevelFilter::Info)
        // Output to stdout, files, and other Dispatch configurations
        .chain(std::io::stdout())
        .chain(fern::log_file("output.log").expect("Log file open failed"))
        // Apply globally
        .apply()
        .expect("Failed to initialise logging");

    let token = env::var("BOT_TOKEN").expect("Expected a token in the environment");

    let options = poise::FrameworkOptions {
        commands: vec![commands::data(), commands::join(), commands::leave(), commands::ping()],
        prefix_options: poise::PrefixFrameworkOptions {
            prefix: Some("!".into()),
            edit_tracker: Some(Arc::new(poise::EditTracker::for_timespan(
                Duration::from_secs(3600),
            ))),
            additional_prefixes: vec![poise::Prefix::Literal("hey bot")],
            ..Default::default()
        },
        // The global error handler for all error cases that may occur
        on_error: |error| Box::pin(on_error(error)),
        // This code is run before every command
        pre_command: |_ctx| Box::pin(async move {}),
        // This code is run after a command if it was successful (returned Ok)
        post_command: |_ctx| Box::pin(async move {}),
        // Every command invocation must pass this check to continue execution
        command_check: Some(|_ctx| Box::pin(async move { Ok(true) })),
        // Enforce command checks even for owners (enforced by default)
        // Set to true to bypass checks, which is useful for testing
        skip_checks_for_owners: false,
        event_handler: |_ctx, _event, _framework, _data| Box::pin(async move { Ok(()) }),
        ..Default::default()
    };

    let framework = poise::Framework::builder()
        .setup(move |ctx, ready, framework| {
            Box::pin(async move {
                info!(
                    "Logged in as {} with id {}",
                    ready.user.name,
                    ready.user.id.get()
                );
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(Data {
                    clients: Mutex::new(HashMap::new()),
                })
            })
        })
        .options(options)
        .build();

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

    let _ = client
        .start()
        .await
        .map_err(|why| println!("Client ended: {:?}", why));
}
