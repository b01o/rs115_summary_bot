use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use scopeguard::defer;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use teloxide::requests::HasPayload;
use teloxide::types::Document;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, InputFile};
use tokio::fs::{read_dir, File};
use tokio_stream::wrappers::UnboundedReceiverStream;

use rs115_bot::parsers::*;
use teloxide::net::Download;
use teloxide::prelude::*;

const ROOT_FOLDER: &str = "/tmp/tgtmp/";
static mut DEBUG_CC_ID: i64 = -1;

#[tokio::main]
async fn main() -> Result<()> {
    let path = Path::new(ROOT_FOLDER);
    if !path.exists() {
        fs::create_dir_all(path)?;
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

async fn run() -> Result<()> {
    teloxide::enable_logging!();
    let bot = Bot::from_env().auto_send();

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

fn btn(name: impl Into<String>, data: impl Into<String>) -> InlineKeyboardMarkup {
    let btn = InlineKeyboardButton::callback(name.into(), data.into());
    InlineKeyboardMarkup::default().append_row(vec![btn])
}

async fn callback_handler(cx: UpdateWithCx<AutoSend<Bot>, CallbackQuery>) -> Result<()> {
    let UpdateWithCx {
        requester: bot,
        update: query,
    } = cx;

    if let Some(version) = query.data {
        let working = "请稍等...";
        let mut found_cache = false;
        if let Some(msg) = query.message {
            let to_send = format!("{}\n{}", msg.text().unwrap_or(""), working);
            bot.edit_message_text(msg.chat.id, msg.id, &to_send).await?;

            if version.starts_with("2j") || version.starts_with("2l") {
                let root = Path::new(ROOT_FOLDER);
                let mut paths = read_dir(root).await?;

                while let Some(dir) = paths.next_entry().await? {
                    let filename = dir.file_name();
                    let filename = filename.to_string_lossy();
                    let arr: Vec<&str> = filename.splitn(2, '.').collect();
                    if arr.len() != 2 {
                        continue;
                    }

                    let file_id = arr[0];
                    if file_id.ends_with(&version[2..]) {
                        // found cache!
                        found_cache = true;

                        let mut file = dir.path();
                        let cache = file.as_path().to_owned();
                        file.pop();

                        let mut new_filename = arr[1].rsplit_once('.').unwrap().0.to_string();
                        if version.starts_with("2j") {
                            new_filename += ".json";
                        } else if version.starts_with("2l") {
                            new_filename += ".txt";
                        }
                        file.push(new_filename);

                        let new_file = file.as_path();

                        defer! {
                            if cache.exists(){
                                let _ = fs::remove_file(&cache);
                            }
                            if new_file.exists(){
                                let _ = fs::remove_file(&new_file);
                            }
                        }

                        if !cache.exists() || new_file.exists() {
                            bail!("file problem");
                        }

                        if version.starts_with("2j") {
                            line2json(&cache, new_file)?;
                        } else if version.starts_with("2l") {
                            json2line(&cache, new_file)?;
                        }

                        let input_file = InputFile::File(new_file.to_path_buf());

                        let mut req = bot.send_document(msg.chat_id(), input_file);
                        let payload = req.payload_mut();
                        payload.reply_to_message_id = Some(msg.id);
                        req.await?;
                    }
                }

                if !found_cache {
                    let mut req = bot.send_message(msg.chat_id(), "文件已过期，请重新发送");
                    let payload = req.payload_mut();
                    payload.reply_to_message_id = Some(msg.id);
                    req.await?;
                }

                bot.edit_message_text(msg.chat.id, msg.id, msg.text().unwrap_or(""))
                    .await?;
            }
        }
        log::info!("You chose: {}", version);
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

    let text_to_send = msg.text().unwrap_or("").to_string() + msg.caption().unwrap_or("");
    let pl = req.payload_mut();
    pl.caption = Some(format!(
        "{}\n{}:{} {},@{}",
        text_to_send,
        &msg.chat.id,
        &msg.chat.first_name().unwrap_or(""),
        &msg.chat.last_name().unwrap_or(""),
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

async fn message_handler(cx: UpdateWithCx<AutoSend<Bot>, Message>) -> Result<()> {
    let UpdateWithCx {
        requester: bot,
        update: msg,
    } = &cx;
    if let Some(doc) = msg.document() {
        if let Some(size) = &doc.file_size {
            if *size > 1024 * 1024 * 20 {
                //ignore
                return Ok(());
            }
        }

        if let Some(doc_type) = &doc.mime_type {
            if *doc_type == mime::TEXT_PLAIN {
                let path = download_file(bot, doc).await?;

                let summary = line_summary(&path).map_err(|e| {
                    let _ = fs::remove_file(&path);
                    e
                })?;

                let _ = copied(bot, msg).await;

                let mut request = cx.reply_to(summary.to_string());

                if msg.chat.is_private() && summary.has_folder {
                    let len = doc.file_id.len();
                    let last_part: String = doc.file_id.chars().skip(len - 62).collect();
                    request =
                        request.reply_markup(btn("转成JSON", format!("{}{}", "2j", last_part)));
                } else {
                    let _ = fs::remove_file(&path);
                }
                request.await?;
            } else if *doc_type == mime::APPLICATION_JSON {
                let path = download_file(bot, doc).await?;

                let sha1 = path
                    .to_str()
                    .ok_or_else(|| {
                        let _ = fs::remove_file(&path);
                        anyhow!("invalid path str")
                    })?
                    .parse()
                    .map_err(|e| {
                        let _ = fs::remove_file(&path);
                        e
                    })?;

                let _ = copied(bot, msg).await;
                let summary = json_summary(&sha1).map_err(|e| {
                    let _ = fs::remove_file(&path);
                    e
                })?;

                let mut request = cx.reply_to(summary.to_string());
                if msg.chat.is_private() {
                    let len = doc.file_id.len();
                    let last_part: String = doc.file_id.chars().skip(len - 62).collect();
                    request =
                        request.reply_markup(btn("转成TXT", format!("{}{}", "2l", last_part)));
                } else {
                    let _ = fs::remove_file(&path);
                }
                request.await?;
            } else if *doc_type == "application/x-bittorrent" {
                let path = download_file(bot, doc).await?;
                defer! { let _ = fs::remove_file(&path); }

                let mut request = cx.reply_to(format!("`{}`", get_torrent_magnet(&path)?));
                let payload = request.payload_mut();
                payload.parse_mode = Some(teloxide::types::ParseMode::MarkdownV2);
                request.await?;
            }
        }
    }
    Ok(())
}
