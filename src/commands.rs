use anyhow::Result;
use strum::EnumIter;
use teloxide::{
    utils::command::BotCommand,
};


#[derive(BotCommand, Debug, EnumIter)]
#[command(rename = "lowercase", description = "These commands are supported:")]
pub(crate) enum Command {
    #[command(description = "Display this text")]
    Help,
    Version,
}

impl Command {
    pub(crate) fn description(&self) -> String {
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
