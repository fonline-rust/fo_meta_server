pub use serenity::{
    self,
    model::guild::{Guild, Role},
};
use serenity::{
    client::{bridge::gateway::ShardManager, Client},
    framework::standard::{
        macros::{command, group, hook},
        CommandError, CommandResult, DispatchError, StandardFramework,
    },
    model::prelude::{Message, UserId},
    prelude::{Context, EventHandler, Mutex, TypeMapKey},
    CacheAndHttp,
};
use serenity::{
    model::{guild::Member, prelude::Channel},
    prelude::GatewayIntents,
};
use std::{collections::HashMap, sync::Arc};

#[group]
#[commands(private)]
struct General;

struct Handler;

impl EventHandler for Handler {}

struct MainGuild;
impl TypeMapKey for MainGuild {
    type Value = u64;
}

#[derive(Clone)]
pub struct MrHandy {
    pub cache_and_http: Arc<CacheAndHttp>,
    pub shard_manager: Arc<Mutex<ShardManager>>,
    pub main_guild_id: u64,
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
        let cache = &self.cache_and_http.cache;
        let mut fun = Some(fun);
        cache
            .guild_field(self.main_guild_id, |guild| {
                (fun.take().unwrap())(Some(guild))
            })
            .unwrap_or_else(|| (fun.take().unwrap())(None))
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
                    .filter_map(|ch| match ch {
                        Channel::Guild(ch) => Some(ch),
                        _ => None,
                    })
                    .find(|ch| &ch.name == &channel)
                    .ok_or_else(|| Error::ChannelNotFound(channel))?;
                Ok(channel.id)
            })
            .await?;
        let _ = channel_id
            .say(&self.cache_and_http.http, text)
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
        self.cache_and_http
            .http
            .edit_nickname(self.main_guild_id, new_nickname.as_deref())
            .await
        //TODO: return local Error
        //.map_err(Error::Serenity)
    }
    pub async fn set_activity(&self, condition: Condition) -> bool {
        use serenity::model::{gateway::Activity, user::OnlineStatus};

        let activity = Activity::playing(condition.name.clone());

        //TODO: Discord API doesn't support setting of custom status, fix when it's supported
        //activity.kind = ActivityType::Custom;
        //activity.state = Some(condition.name);
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

        let shard_manager = self.shard_manager.lock().await;
        let runners = shard_manager.runners.lock().await;
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
        .configure(|c| c.prefix("~")) // set the bot's prefix to "~"
        .on_dispatch_error(dispatch_error_hook)
        .group(&GENERAL_GROUP)
        .after(after_hook);
    let client = Client::builder(token, GatewayIntents::all())
        .event_handler(Handler)
        .framework(framework)
        .await
        .expect("Error creating client");
    let cache_and_http = Arc::clone(&client.cache_and_http);
    let shard_manager = Arc::clone(&client.shard_manager);

    (
        MrHandy {
            cache_and_http,
            shard_manager,
            main_guild_id,
        },
        client,
    )
}

#[command]
async fn private(ctx: &Context, msg: &Message) -> CommandResult {
    msg.author.dm(ctx, |msg| msg.content(":eyes:")).await?;
    Ok(())
}
