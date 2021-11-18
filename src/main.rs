use anyhow::anyhow;
use anyhow::Result;
use lazy_static::lazy_static;
use regex::Regex;
use rs115_bot::{callbacks::*, global::*};
use scopeguard::defer;
use std::path::Path;
use std::path::PathBuf;
use strum::EnumIter;
use strum::IntoEnumIterator;
use teloxide::requests::HasPayload;
use teloxide::types::BotCommandScope;
use teloxide::types::Document;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};
use teloxide::utils::command::BotCommand;

use teloxide::types::BotCommand as BC;
use tokio::fs::create_dir_all;
use tokio::fs::File;
use tokio_stream::wrappers::UnboundedReceiverStream;

use rs115_bot::parsers::*;
use teloxide::net::Download;
use teloxide::prelude::*;

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

#[derive(BotCommand, Debug, EnumIter)]
#[command(rename = "lowercase", description = "These commands are supported:")]
enum Command {
    #[command(description = "Display this text")]
    Help,
    Version,
}
impl Command {
    fn description(&self) -> String {
        match self {
            Command::Help => "打印帮助",
            Command::Version => "版本信息",
        }
        .to_string()
    }
}

impl std::fmt::Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = format!("{:?}", self);
        write!(f, "{}", name.to_lowercase())
    }
}

async fn set_up_commands(bot: &AutoSend<Bot>) -> Result<()> {
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
    let bot = Bot::from_env().auto_send();

    set_up_commands(&bot).await?;

    Dispatcher::new(bot)
        .messages_handler(|rx: DispatcherHandlerRx<AutoSend<Bot>, Message>| {
            UnboundedReceiverStream::new(rx).for_each_concurrent(None, |cx| async move {
                message_handler(cx).await.log_on_error().await;
            })
        })
        .callback_queries_handler(|rx: DispatcherHandlerRx<AutoSend<Bot>, CallbackQuery>| {
            UnboundedReceiverStream::new(rx).for_each_concurrent(None, |cx| async move {
                callback_handler(cx).await.log_on_error().await;
            })
        })
        .dispatch()
        .await;

    Ok(())
}

fn btn(
    name: impl Into<String>,
    code: impl Into<String>,
    data: impl Into<String>,
) -> InlineKeyboardButton {
    InlineKeyboardButton::callback(name.into(), format!("{}{}", code.into(), data.into()))
}

async fn callback_handler(cx: UpdateWithCx<AutoSend<Bot>, CallbackQuery>) -> Result<()> {
    let UpdateWithCx {
        requester: bot,
        update: query,
    } = &cx;
    // let mut text_to_append = String::new();

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

// save file for debugging
async fn copied(bot: &AutoSend<Bot>, msg: &Message) -> Result<Message> {
    unsafe {
        if DEBUG_CC_ID == -1 || DEBUG_CC_ID == msg.chat_id() {
            return Err(anyhow!("ignore"));
        }
    }

    let mut req = unsafe { bot.copy_message(DEBUG_CC_ID, msg.chat_id(), msg.id) };

    let mut text_to_send = format!(
        "{}{}",
        msg.text().unwrap_or(""),
        msg.caption().unwrap_or("")
    );

    if let Some(user) = msg.from() {
        text_to_send = format!(
            "{}\n{}:{} @{}",
            text_to_send,
            user.id,
            user.full_name(),
            user.username.as_ref().unwrap_or(&"None".to_string())
        );
    }

    let pl = req.payload_mut();
    pl.caption = Some(format!(
        "{}\n{},@{}",
        text_to_send,
        &msg.chat.id,
        &msg.chat.username().unwrap_or(""),
    ));
    Ok(req.await?)
}

async fn download_file(bot: &AutoSend<Bot>, doc: &Document) -> Result<PathBuf> {
    let Document {
        file_name, file_id, ..
    } = doc;

    let default_name = "default_name".to_string();
    let file_name = file_name.as_ref().unwrap_or(&default_name);

    let teloxide::types::File { file_path, .. } = bot.get_file(file_id).send().await?;
    let path_str = ROOT_FOLDER.to_owned() + file_id + "." + file_name;
    let path = Path::new(&path_str);
    if path.exists() {
        return Ok(path.to_path_buf());
    }
    let mut new_file = File::create(path).await?;
    bot.download_file(&file_path, &mut new_file).await?;

    Ok(path.to_path_buf())
}

async fn line_handler(cx: &UpdateWithCx<AutoSend<Bot>, Message>, doc: &Document) -> Result<()> {
    let UpdateWithCx {
        requester: bot,
        update: msg,
    } = &cx;

    let path = download_file(bot, doc).await?;

    let mut cached = false;

    is_valid_line(&path).await?;

    let summary = line_summary(&path).await.map_err(|e| {
        let _ = std::fs::remove_file(&path);
        e
    })?;

    let _ = copied(bot, msg).await;

    let mut send_str = summary.to_string();
    let mut request = cx.reply_to(&send_str);

    if msg.chat.is_private() {
        let (dup_num, invalid_num) = check_dup_n_err(&path).await?;
        if dup_num == 0 {
            send_str = format!("{}\n恭喜，这个文件没有重复文件链接。", &send_str);
        } else {
            send_str = format!("{}\n! 检测到重复文件 {} 个。", &send_str, dup_num);
        }

        if invalid_num != 0 {
            send_str = format!(
                "{}\n! 包含 {} 个格式不正确的错误链接",
                &send_str, invalid_num
            );
        }

        request = cx.reply_to(&send_str);

        let mut btns = InlineKeyboardMarkup::default();
        let len = doc.file_id.len();
        let last_part: String = doc.file_id.chars().skip(len - 62).collect();

        if summary.has_folder {
            cached = true;
            let btn1 = btn("转成JSON", "2j", &last_part);
            let btn2 = btn("去掉目录信息", "ls", &last_part);
            btns = btns.append_row(vec![btn1, btn2]);
        }

        let btn3 = btn("去除重复/无效文件", "ld", &last_part);
        if dup_num != 0 {
            btns = btns.append_row(vec![btn3]);
            cached = true;
        }
        request = request.reply_markup(btns);
    }

    if !cached {
        let _ = std::fs::remove_file(&path);
    }

    request.await?;
    Ok(())
}

async fn json_handler(cx: &UpdateWithCx<AutoSend<Bot>, Message>, doc: &Document) -> Result<()> {
    let UpdateWithCx {
        requester: bot,
        update: msg,
    } = &cx;

    let path = download_file(bot, doc).await?;
    let sha1: Sha1Entity = path_to_sha1_entity(&path).await?;
    let _ = copied(bot, msg).await;
    let summary = json_summary(&sha1).map_err(|e| {
        let _ = std::fs::remove_file(&path);
        e
    })?;

    let mut request = cx.reply_to(summary.to_string());
    if msg.chat.is_private() {
        let len = doc.file_id.len();
        let last_part: String = doc.file_id.chars().skip(len - 62).collect();
        let btns =
            InlineKeyboardMarkup::default().append_row(vec![btn("转成TXT", "2l", last_part)]);
        request = request.reply_markup(btns);
    } else {
        let _ = std::fs::remove_file(&path);
    }
    request.await?;
    Ok(())
}

async fn link_check(cx: &UpdateWithCx<AutoSend<Bot>, Message>, text: &str) -> Result<()> {
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

async fn message_handler(cx: UpdateWithCx<AutoSend<Bot>, Message>) -> Result<()> {
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

    if let Some(doc) = msg.document() {
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

async fn version(cx: &UpdateWithCx<AutoSend<Bot>, Message>) -> Result<()> {
    cx.requester
        .send_message(cx.update.chat_id(), VERSION)
        .await?;
    Ok(())
}

async fn help(cx: &UpdateWithCx<AutoSend<Bot>, Message>) -> Result<()> {
    cx.requester.send_message(cx.update.chat_id(), HELP).await?;
    Ok(())
}
