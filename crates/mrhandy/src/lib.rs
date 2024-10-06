pub use serenity::{
    self,
    model::guild::{Guild, Role},
};
use serenity::{
    all::{standard::Configuration, ActivityData, CreateMessage, GuildId}, cache::Cache, client::Client, framework::standard::{
        macros::{command, group, hook},
        CommandError, CommandResult, DispatchError, StandardFramework,
    }, gateway::ShardManager, http::Http, model::prelude::{Message, UserId}, prelude::{Context, EventHandler, Mutex, TypeMapKey}
};
use serenity::{
    model::{guild::Member, prelude::Channel},
    prelude::GatewayIntents,
};
use std::{collections::HashMap, sync::Arc};
use let_clone::let_clone;

#[group]
#[commands(private)]
struct General;

struct Handler;

impl EventHandler for Handler {}

#[derive(Clone)]
pub struct MrHandy {
    pub cache: Arc<Cache>,
    pub http: Arc<Http>,
    pub shard_manager: Arc<ShardManager>,
    pub main_guild_id: GuildId,
}

impl MrHandy {
    pub async fn with_guild_member<O, F: Fn(&Guild, &Member) -> O>(
        &self,
        user_id: u64,
        fun: F,
    ) -> Result<O, &'static str> {
        self.with_guild(|guild| match guild {
            Some(guild) => {
                let member = guild.members.get(&user_id.into()).ok_or("Member is None")?;
                Ok(fun(guild, member))
            }
            None => Err("MainGuild isn't in cache."),
        })
        .await
    }

    pub async fn with_guild<O, F: FnOnce(Option<&Guild>) -> O>(&self, fun: F) -> O {
        let res = self.cache.guild(self.main_guild_id);
        fun(res.as_deref())
    }

    pub async fn clone_members(&self) -> Option<Members> {
        self.with_guild(move |guild| {
            guild.map(|guild| Members {
                members: guild.members.clone(),
            })
        })
        .await
    }

    pub async fn send_message(&self, channel: String, text: String) -> Result<(), Error> {
        let channel_id = self
            .with_guild(move |guild| {
                let guild = guild.ok_or(Error::NoMainGuild)?;
                let channel = guild
                    .channels
                    .values()
                    .find(|ch| &ch.name == &channel)
                    .ok_or_else(|| Error::ChannelNotFound(channel))?;
                Ok(channel.id)
            })
            .await?;
        let _ = channel_id
            .say(&self.http, text)
            .await
            .map_err(Error::Serenity)?;
        Ok(())
    }

    pub fn get_roles<O, F: Fn(&Role) -> O>(guild: &Guild, member: &Member, fun: F) -> Vec<O> {
        member
            .roles
            .iter()
            .filter_map(|role_id| guild.roles.get(role_id))
            .map(fun)
            .collect()
    }

    pub fn get_name_nick(member: &Member) -> (String, Option<String>) {
        let user = &member.user;
        (user.name.clone(), member.nick.clone())
    }
    pub async fn edit_nickname(&self, new_nickname: Option<String>) -> Result<(), serenity::Error> {
        //let shards = self.shard_manager.lock().await;
        self.http
            .edit_nickname(self.main_guild_id, new_nickname.as_deref(), None)
            .await
        //TODO: return local Error
        //.map_err(Error::Serenity)
    }
    pub async fn set_activity(&self, condition: Condition) -> bool {
        use serenity::model::user::OnlineStatus;

        let activity = ActivityData::custom(condition.name);

        //TODO: Discord API doesn't support setting of custom status emoji, fix when it's supported
        //activity.emoji = Some(ActivityEmoji {
        //    name: condition.emoji,
        //    id: None,
        //    animated: None,
        //});
        //println!("set_activity: {:?}", activity);
        let status = match condition.color {
            ConditionColor::Green => OnlineStatus::Online,
            ConditionColor::Yellow => OnlineStatus::Idle,
            ConditionColor::Red => OnlineStatus::DoNotDisturb,
        };

        let runners = self.shard_manager.runners.lock().await;
        runners
            .values()
            .inspect(|runner| {
                runner
                    .as_ref()
                    .set_presence(Some(activity.clone()), status.clone());
            })
            .count()
            > 0
    }
}

#[derive(Debug)]
pub struct Condition {
    pub name: String,
    pub color: ConditionColor,
    //pub emoji: String,
}
#[derive(Debug)]
pub enum ConditionColor {
    Green,
    Yellow,
    Red,
}

pub enum Error {
    NoMainGuild,
    ChannelNotFound(String),
    Serenity(serenity::Error),
}

pub struct Members {
    members: HashMap<UserId, Member>,
}
impl Members {
    pub fn get(&self, user_id: u64) -> Option<&Member> {
        self.members.get(&user_id.into())
    }
}

#[hook]
async fn dispatch_error_hook(
    _context: &Context,
    _msg: &Message,
    error: DispatchError,
    command_name: &str,
) {
    eprintln!("DispatchError: {error:?}, command: {command_name}")
}

#[hook]
async fn after_hook(_: &Context, _: &Message, cmd_name: &str, error: Result<(), CommandError>) {
    if let Err(why) = error {
        println!("Error in {}: {:?}", cmd_name, why);
    }
}

pub async fn init(token: &str, main_guild_id: u64) -> (MrHandy, Client) {
    let framework = StandardFramework::new()
        //.configure(|c| c.prefix("~")) // set the bot's prefix to "~"
        .on_dispatch_error(dispatch_error_hook)
        .group(&GENERAL_GROUP)
        .after(after_hook);
    framework
        .configure(Configuration::new().prefix("~"));
    let client = Client::builder(token, GatewayIntents::all())
        .event_handler(Handler)
        .framework(framework)
        .await
        .expect("Error creating client");
    let_clone!(client.cache, client.http, client.shard_manager);
    (
        MrHandy {
            cache,
            http,
            shard_manager,
            main_guild_id: GuildId::new(main_guild_id),
        },
        client,
    )
}

#[command]
async fn private(ctx: &Context, msg: &Message) -> CommandResult {
    let builder = CreateMessage::new().content(":eyes:");
    msg.author.dm(ctx, builder).await?;
    Ok(())
}
