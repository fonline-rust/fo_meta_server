use std::ffi::CString;

use serde::{Deserialize, Serialize};

pub const HANDSHAKE: u16 = 0xBABA;
pub const VERSION: u16 = 6;
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum GameServerToMetaServer {
    PlayerConnected(u32),
    PlayerAuth(u32),
    Status(ServerStatus),
    DiscordSendMessage { channel: String, text: String },
}
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub enum DayTime {
    Morning,
    Day,
    Evening,
    Night,
}
impl DayTime {
    pub fn from_hour(hour: u16) -> Self {
        match hour {
            0..=5 => DayTime::Night,
            6..=11 => DayTime::Morning,
            12..=16 => DayTime::Day,
            17..=20 => DayTime::Evening,
            _ => DayTime::Night,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[repr(C)]
pub struct ServerStatistics {
    pub server_start_tick: u32,
    pub uptime: u32,
    pub bytes_send: i64,
    pub bytes_recv: i64,
    pub data_real: i64,
    pub data_compressed: i64,
    pub compress_ratio: f32,
    pub max_online: u32,
    pub cur_online: u32,

    pub cycle_time: u32,
    pub fps: u32,
    pub loop_time: u32,
    pub loop_cycles: u32,
    pub loop_min: u32,
    pub loop_max: u32,
    pub lags_count: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ServerStatus {
    pub connections: u32,
    pub day_time: DayTime,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum MetaServerToGameServer {
    UpdateCharLeaf { id: u32, ver: u32, secret: u32 },
    SendKeyToPlayer(u32, [u32; 3]),
    SendConfig { player_id: u32, url: CString },
    StartGame { player_id: u32 },
    Nop,
}
