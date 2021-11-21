use anyhow::Result;
use lazy_static::lazy_static;
use regex::Regex;
use rs115_bot::commands::Command;
use rs115_bot::message_handlers::{download_file, json_handler, line_handler};
use rs115_bot::parsers::*;
use rs115_bot::{callbacks::*, global::*};
use scopeguard::defer;
use std::path::Path;
use strum::IntoEnumIterator;
use teloxide::adaptors::throttle::Limits;
use teloxide::error_handlers::OnError;
use teloxide::prelude::{
    Dispatcher, DispatcherHandlerRx, Requester, RequesterExt, StreamExt, UpdateWithCx,
};
use teloxide::requests::HasPayload;
use teloxide::types::{BotCommand as BC, Message};
use teloxide::types::{BotCommandScope, CallbackQuery};
use teloxide::utils::command::BotCommand;
use tokio::fs::create_dir_all;
use tokio_stream::wrappers::UnboundedReceiverStream;

#[tokio::main]
async fn main() -> Result<()> {
    let path = Path::new(ROOT_FOLDER);
    if !path.exists() {
        create_dir_all(path).await?;
    }
    if let Some(id) = std::env::var_os("DEBUG_CC_ID") {
        unsafe {
            DEBUG_CC_ID = id
                .to_string_lossy()
                .parse()
                .expect("DEBUG_CC_ID is invalid");
        }
    }
    let _ = run().await?;
    Ok(())
}

pub async fn set_up_commands(bot: &Bot) -> Result<()> {
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

async fn run() -> Result<()> {
    teloxide::enable_logging!();
    let bot = teloxide::Bot::from_env()
        .throttle(Limits::default())
        .auto_send();

    set_up_commands(&bot).await?;

    Dispatcher::new(bot)
        .messages_handler(|rx: DispatcherHandlerRx<Bot, Message>| {
            UnboundedReceiverStream::new(rx).for_each_concurrent(10, |cx| async move {
                message_handler(cx).await.log_on_error().await;
            })
        })
        .callback_queries_handler(|rx: DispatcherHandlerRx<Bot, CallbackQuery>| {
            UnboundedReceiverStream::new(rx).for_each_concurrent(10, |cx| async move {
                callback_handler(cx).await.log_on_error().await;
            })
        })
        .dispatch()
        .await;

    Ok(())
}

async fn callback_handler(cx: UpdateWithCx<Bot, CallbackQuery>) -> Result<()> {
    let UpdateWithCx {
        requester: bot,
        update: query,
    } = &cx;
    // let bot = bot.requester;

    if let (Some(version), Some(msg)) = (&query.data, &query.message) {
        let working = "请稍等...";
        let to_send = format!("{}\n{}", msg.text().unwrap_or(""), working);
        bot.edit_message_text(msg.chat.id, msg.id, &to_send).await?;

        let found_cache = match &version[..2] {
            "2j" => callback_to_json(bot, msg, &version[2..]).await?,
            "2l" => callback_to_line(bot, msg, &version[2..]).await?,
            "ls" => callback_line_strip_dir(bot, msg, &version[2..]).await?,
            "ld" => callback_to_dedup(bot, msg, &version[2..]).await?,
            _ => {
                bot.answer_callback_query(&query.id).await?;
                let text = msg.text().unwrap_or("").to_owned() + "\n发生了错误..";
                bot.edit_message_text(msg.chat.id, msg.id, text).await?;
                return Ok(());
            }
        };

        bot.answer_callback_query(&query.id).await?;

        if !found_cache {
            let mut req = bot.send_message(msg.chat_id(), "文件已过期，请重新发送");
            let payload = req.payload_mut();
            payload.reply_to_message_id = Some(msg.id);
            req.await?;
        }

        let text = msg.text().unwrap_or("");
        bot.edit_message_text(msg.chat.id, msg.id, text).await?;
    }

    Ok(())
}

async fn link_check(cx: &UpdateWithCx<Bot, Message>, text: &str) -> Result<()> {
    lazy_static! {
        static ref SHA1RE: Regex =
            Regex::new(r"115://(.*?)\|(\d*?)(?:\|[a-fA-F0-9]{40}){2}").unwrap();
    }

    let mut response: String = Default::default();
    let mut counter = 0;
    let mut sum: u128 = 0;
    for cap in SHA1RE.captures_iter(text) {
        counter += 1;
        let size: u128 = cap[2].parse()?;
        sum += size;
        response.push_str(&format!("{} => {}\n", to_iec(size), &cap[1]));
    }

    match counter {
        2.. => response.push_str(&format!("共 {} 个文件, 总计: {}", counter, to_iec(sum))),
        1 => response = format!("文件大小: {}", to_iec(sum)),
        _ => {}
    }

    if !response.is_empty() {
        cx.reply_to(response).await?;
    }

    Ok(())
}

async fn message_handler(cx: UpdateWithCx<Bot, Message>) -> Result<()> {
    let UpdateWithCx {
        requester: bot,
        update: msg,
    } = &cx;

    // handle command
    if let Some(text) = msg.text() {
        match BotCommand::parse(text, "") {
            Ok(Command::Help) => help(&cx).await?,
            Ok(Command::Version) => version(&cx).await?,
            Err(_) => {}
        }

        link_check(&cx, text).await?;
    }

    if let Some(caption) = msg.caption() {
        link_check(&cx, caption).await?;
    }

    if let Some(doc) = msg.document() {
        // log::info!("getting doc");
        if let Some(size) = &doc.file_size {
            if *size > 1024 * 1024 * 20 {
                //ignore
                return Ok(());
            }
        }

        if let Some(doc_type) = &doc.mime_type {
            if *doc_type == mime::TEXT_PLAIN
                || doc
                    .file_name
                    .as_ref()
                    .unwrap_or(&"".to_string())
                    .ends_with(".txt")
            {
                log::info!("getting a txt");
                line_handler(&cx, doc).await?;
            } else if *doc_type == mime::APPLICATION_JSON
                || doc
                    .file_name
                    .as_ref()
                    .unwrap_or(&"".to_string())
                    .ends_with(".json")
            {
                log::info!("getting a json");
                json_handler(&cx, doc).await?;
            } else if *doc_type == "application/x-bittorrent" {
                let path = download_file(bot, doc).await?;
                defer! { let _ = std::fs::remove_file(&path); }

                let mut request =
                    cx.reply_to(format!("`{}`", get_torrent_magnet_async(&path).await?));
                let payload = request.payload_mut();
                payload.parse_mode = Some(teloxide::types::ParseMode::MarkdownV2);
                request.await?;
            }
        }
    }
    Ok(())
}
async fn version(cx: &UpdateWithCx<Bot, Message>) -> Result<()> {
    cx.requester
        .send_message(cx.update.chat_id(), VERSION)
        .await?;
    Ok(())
}

async fn help(cx: &UpdateWithCx<Bot, Message>) -> Result<()> {
    cx.requester.send_message(cx.update.chat_id(), HELP).await?;
    Ok(())
}
