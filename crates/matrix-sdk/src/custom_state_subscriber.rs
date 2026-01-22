use crate::client::Client;
use dashmap::DashMap;

use crate::Room;
use matrix_sdk_base::StateChanges;
use ruma::events::AnySyncStateEvent;
use ruma::serde::Raw;
use std::collections::HashSet;
use std::sync::{Arc, OnceLock};
use serde_json::json;
use tracing::{error, info};

pub(crate) static GLOBAL_SUBSCRIBER: OnceLock<Arc<GlobalCustomStateSubscriber>> = OnceLock::new();
///回调接口
pub trait SubscriberListener: Send + Sync {
    /// 回调
    fn call(&self, event_json: String);
}
pub(crate) struct RoomSubscription {
    pub(crate) listener: Arc<dyn SubscriberListener>,
    pub(crate) event_types: HashSet<String>,
}
/// 全局自定义状态订阅器，只需在matrix-sdk::Client::new中初始化一次
pub struct GlobalCustomStateSubscriber {
    pub(crate) rooms: DashMap<String, RoomSubscription>,
    client: Arc<Client>,
}

impl GlobalCustomStateSubscriber {
    ///获取单例
    pub fn instance() -> Arc<Self> {
        GLOBAL_SUBSCRIBER.get().unwrap().clone()
    }

    /// init and register;
    /// clled at matrix-sdd-ffi::Client::new
    pub fn init_and_register(client: Arc<Client>) {
        info!("GlobalCustomStateSubscriber init_and_register");
        let subscriber = Arc::new(Self { rooms: DashMap::new(), client });
        // 如果已经初始化过，直接返回
        if GLOBAL_SUBSCRIBER.set(Arc::clone(&subscriber)).is_err() {
            info!("GlobalCustomStateSubscriber already initialized, skip");
            return;
        }
        // 只有第一次才会走到这里
        let subscriber_clone = Arc::clone(&subscriber);
        subscriber.client.add_event_handler(
            move |raw: Raw<AnySyncStateEvent>, room: Room| {
                let subscriber_clone = Arc::clone(&subscriber_clone);
                async move {
                    Self::handle_sync_state_event(&subscriber_clone, raw, room).await;
                }
            }
        );
    }
    /// 公用函数，处理 AnySyncStateEvent
    pub async fn handle_sync_state_event(
        &self,
        raw: Raw<AnySyncStateEvent>,
        room: Room,
    ) {
        // 反序列化事件
        let ev = match raw.deserialize() {
            Ok(e) => e,
            Err(_) => return,
        };

        let room_id_str = room.room_id().to_string();
        let event_type = ev.event_type().to_string();

        // 获取 listener
        let listener = match self.rooms.get(&room_id_str) {
            Some(sub) if sub.event_types.contains(&event_type) => sub.listener.clone(),
            _ => return,
        };

        // 调用 listener
        let event_data = json!([raw]);
        listener.call(event_data.to_string());

        // 更新 state_store
        let mut state_changes = StateChanges::default();
        state_changes.add_state_event(room.room_id(), ev, raw);

        if let Err(e) = self.client.state_store().save_changes(&state_changes).await {
            error!("save_changes failed: {e}");
        }
    }

    /// 订阅指定房间的事件类型
    pub fn subscribe_room(
        &self,
        room_id: String,
        event_types: Vec<String>,
        listener: Arc<dyn SubscriberListener>,
    ) {
        let subscription =
            RoomSubscription { listener, event_types: event_types.iter().cloned().collect() };
        self.rooms.insert(room_id, subscription);
        info!("GlobalCustomStateSubscriber subscribe_room size={}", self.rooms.len());
    }
}
