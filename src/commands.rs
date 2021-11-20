use anyhow::Result;
use strum::EnumIter;
use teloxide::{
    utils::command::BotCommand,
};



#[derive(BotCommand, Debug, EnumIter)]
#[command(rename = "lowercase", description = "These commands are supported:")]
pub enum Command {
    #[command(description = "Display this text")]
    Help,
    Version,
}

impl Command {
    pub fn description(&self) -> String {
        match self {
            Command::Help => "打印帮助",
            Command::Version => "版本信息",
        }
        .to_string()
    }

    // pub async fn call(&self, cx: &UpdateWithCx<AutoSend<Bot>, Message>) -> Result<()> {
    //     match self {
    //         Command::Help => help(cx).await?,
    //         Command::Version => version(cx).await?,
    //     }
    //     Ok(())
    // }
}

impl std::fmt::Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = format!("{:?}", self);
        write!(f, "{}", name.to_lowercase())
    }
}

// async fn version(cx: &UpdateWithCx<AutoSend<Bot>, Message>) -> Result<()> {
//     cx.requester
//         .send_message(cx.update.chat_id(), VERSION)
//         .await?;
//     Ok(())
// }

// async fn help(cx: &UpdateWithCx<AutoSend<Bot>, Message>) -> Result<()> {
//     cx.requester.send_message(cx.update.chat_id(), HELP).await?;
//     Ok(())
// }
