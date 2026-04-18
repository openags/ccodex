use time::OffsetDateTime;

use ccodex_protocol::{Item, ItemId, ItemPayload, ProtocolEvent, Turn};

use crate::{Kernel, KernelError};

impl Kernel {
    pub(crate) async fn append_item(
        &self,
        turn: &mut Turn,
        events: &mut Vec<ProtocolEvent>,
        payload: ItemPayload,
    ) -> Result<Item, KernelError> {
        self.append_item_with_id(turn, events, ItemId::new(), payload)
            .await
    }

    pub(crate) async fn append_item_with_id(
        &self,
        turn: &mut Turn,
        events: &mut Vec<ProtocolEvent>,
        item_id: ItemId,
        payload: ItemPayload,
    ) -> Result<Item, KernelError> {
        let item = Item {
            id: item_id,
            turn_id: turn.id.clone(),
            created_at: OffsetDateTime::now_utc(),
            payload,
        };
        self.store.append_item(&item).await?;
        turn.item_ids.push(item.id.clone());
        self.store.append_turn(turn).await?;
        self.emit(events, ProtocolEvent::ItemAppended(item.clone()))
            .await?;
        Ok(item)
    }

    pub(crate) async fn emit(
        &self,
        events: &mut Vec<ProtocolEvent>,
        event: ProtocolEvent,
    ) -> Result<(), KernelError> {
        self.notifications.notify_event(&event).await?;
        events.push(event);
        Ok(())
    }
}
