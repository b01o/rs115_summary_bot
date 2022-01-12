use crate::callback_handlers::callback_handler;
use crate::commands::Command;
use crate::global::{Bot, DEBUG_CC_ID, ROOT_FOLDER};
use crate::inline_handlers::inline_query_handler;
use crate::message_handlers::message_handler;
use crate::search::Librarian;
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use strum::IntoEnumIterator;
use teloxide::adaptors::throttle::Limits;
use teloxide::prelude::{
    CallbackQuery, Dispatcher, DispatcherHandlerRx, Message, OnError, Requester, RequesterExt,
    StreamExt,
};
use teloxide::requests::HasPayload;
use teloxide::types::{BotCommand as BC, BotCommandScope};
use tokio::fs::create_dir_all;
use tokio::sync::Mutex;
use tokio_stream::wrappers::UnboundedReceiverStream;

async fn set_up_commands(bot: &Bot) -> Result<()> {
    bot.delete_my_commands().await?;
    let list: Vec<BC> = Command::iter()
        .map(|command| BC::new(command.to_string(), command.description()))
        .collect();

    let mut smc = bot.set_my_commands(list);
    let mut payload = smc.payload_mut();
    payload.scope = Some(BotCommandScope::AllPrivateChats);
    smc.await?;
    Ok(())
}

async fn init_caches() -> Result<()> {
    let path = Path::new(ROOT_FOLDER);
    if !path.exists() {
        create_dir_all(path).await?;
    }
    Ok(())
}

fn parse_env() {
    if let Some(id) = std::env::var_os("DEBUG_CC_ID") {
        unsafe {
            DEBUG_CC_ID = id
                .to_string_lossy()
                .parse()
                .expect("DEBUG_CC_ID is invalid");
        }
    }
}

pub async fn run() -> Result<()> {
    init_caches().await?;
    parse_env();
    teloxide::enable_logging!();

    let bot = teloxide::Bot::from_env()
        .throttle(Limits::default())
        .auto_send();
    set_up_commands(&bot).await?;

    let librarian = Arc::new(Mutex::new(Librarian::new()?));

    Dispatcher::new(bot)
        .messages_handler(|rx: DispatcherHandlerRx<Bot, Message>| {
            UnboundedReceiverStream::new(rx).for_each_concurrent(5, |cx| async move {
                message_handler(cx).await.log_on_error().await;
            })
        })
        .callback_queries_handler(|rx: DispatcherHandlerRx<Bot, CallbackQuery>| {
            UnboundedReceiverStream::new(rx).for_each_concurrent(5, |cx| async move {
                callback_handler(cx).await.log_on_error().await;
            })
        })
        .inline_queries_handler(|rx| {
            UnboundedReceiverStream::new(rx).for_each_concurrent(6, move |cx| {
                let librarian = librarian.clone();
                async move {
                    inline_query_handler(cx, librarian)
                        .await
                        .log_on_error()
                        .await;
                }
            })
        })
        .dispatch()
        .await;

    Ok(())
}
