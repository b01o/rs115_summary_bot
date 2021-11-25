use teloxide::adaptors::{AutoSend, Throttle};
pub type Bot = AutoSend<Throttle<teloxide::Bot>>;

pub const ROOT_FOLDER: &str = ".cache/tgtmp/";
pub static mut DEBUG_CC_ID: i64 = -1;
pub const HELP: &str = r"使用方法: 向机器人发送 sha1 文件, 出现对应选项。
FAQ: 
1. json 文件需要以 .json 文件名后缀结尾，否则忽略。
2. 仅含有目录信息的 txt 才支持转换成 json 文件。
3. 目前去重和除错功能仅对 txt 格式的 115sha1 文件有效, 请需要进行相应操作用 line 格式的 txt 文件
4. 目前仅支持20M内的文件。
5. 有问题群里@我";

pub const VERSION: &str = "2.2.2 Nov 26 2021 CST 测试磁力汇报中";
