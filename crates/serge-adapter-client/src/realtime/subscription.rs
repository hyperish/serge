use super::*;
use futures_channel::mpsc;
use futures_util::{FutureExt, StreamExt};
use hyve::{CancellationToken, Ulid, Value};
use std::{cell::RefCell, collections::HashSet, rc::Rc};

/// Changes a view can display while following a room.
pub enum RoomUpdate {
    /// Replay is complete and live commands can be delivered.
    Connected(Carrier),
    /// A new committed event or replacement snapshot.
    State(Event),
    /// The server rejected a command.
    Rejected(Event),
    /// The link was lost and will retry after this delay.
    Reconnecting(Duration),
    /// Ticket issuance or authorization failed permanently.
    Closed(HyveError),
}

struct Outbox {
    cursor: Rc<RefCell<RoomCursor>>,
    changed: RefCell<mpsc::Sender<()>>,
    cancel: CancellationToken,
}
impl Drop for Outbox {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// Submits commands during either a live connection or a reconnect.
/// Dropping the last handle closes the subscription.
#[derive(Clone)]
pub struct RoomHandle(Rc<Outbox>);
impl RoomHandle {
    /// Queues one command, returning its stable request ID.
    pub fn send(&self, kind: &str, payload: Value) -> HyveResult<String> {
        if self.0.cancel.is_cancelled() {
            return Err(HyveError::domain(
                "room_closed",
                "room subscription has closed",
            ));
        }
        let id = self
            .0
            .cursor
            .borrow_mut()
            .enqueue(kind, payload)?
            .id
            .clone();
        let _ = self.0.changed.borrow_mut().try_send(());
        Ok(id)
    }
    /// Cancels transport, pending requests and reconnection.
    pub fn close(&self) {
        self.0.cancel.cancel();
    }
}

/// Opens a view-scoped subscription. It reissues tickets, reapplies missed
/// state and resends the outbox across reconnects. Unmounting the view ends
/// the background task and closes browser or native transport handles.
pub fn subscribe<F, Fut>(
    endpoints: RoomEndpoints,
    mut issue_ticket: F,
    mut on_update: impl FnMut(RoomUpdate) + 'static,
) -> RoomHandle
where
    F: FnMut() -> Fut + 'static,
    Fut: Future<Output = HyveResult<String>> + 'static,
{
    let (changed, mut updates) = mpsc::channel(1);
    let cursor = Rc::new(RefCell::new(RoomCursor::default()));
    let cancel = CancellationToken::new();
    let handle = RoomHandle(Rc::new(Outbox {
        cursor: cursor.clone(),
        changed: RefCell::new(changed),
        cancel: cancel.clone(),
    }));
    if let Some(view) = serge_core::View::current() {
        let stop = cancel.clone();
        view.owner()
            .with(|| hyve::state::on_cleanup(move || stop.cancel()));
    }
    serge_core::spawn(async move {
        let work = async {
            let mut attempt = 0;
            loop {
                let result = match RoomConnection::connect(&endpoints, &mut issue_ticket).await {
                    Ok(mut conn) => {
                        follow(
                            &mut conn,
                            &cursor,
                            &mut updates,
                            &mut on_update,
                            &mut attempt,
                        )
                        .await
                    }
                    Err(e) => Err(e),
                };
                if let Err(e) = result
                    && !e.is_retryable()
                {
                    cancel.cancel();
                    on_update(RoomUpdate::Closed(e));
                    break;
                }
                let jitter = Ulid::new().to_u128() as u32;
                let delay = reconnect_delay(attempt, jitter);
                attempt = attempt.saturating_add(1);
                on_update(RoomUpdate::Reconnecting(delay));
                platform::sleep(delay).await;
            }
        };
        futures_util::pin_mut!(work);
        futures_util::future::select(work, Box::pin(cancel.cancelled())).await;
    });
    handle
}

async fn follow(
    conn: &mut RoomConnection,
    cursor: &RefCell<RoomCursor>,
    updates: &mut mpsc::Receiver<()>,
    on_update: &mut impl FnMut(RoomUpdate),
    attempt: &mut u32,
) -> HyveResult<()> {
    let resume = cursor.borrow().resume();
    conn.send(&resume).await?;
    let mut ready = false;
    let mut sent = HashSet::new();
    let heartbeat = || Box::pin(platform::sleep(Duration::from_secs(15)).fuse());
    let mut timer = heartbeat();
    let mut waiting_for_pong = false;
    loop {
        let incoming = futures_util::select! {
            msg=conn.receive().fuse()=>Some(msg?),
            changed=updates.next().fuse()=>{ if changed.is_none(){return Ok(());} None },
            _=timer.as_mut()=> {
                if waiting_for_pong {return Err(lost());}
                waiting_for_pong=true;
                conn.send(&Command::new("room.ping",Value::Null)).await?;
                timer=heartbeat();
                None
            }
        };
        if let Some(e) = incoming {
            if cursor.borrow_mut().accept(&e)? {
                on_update(RoomUpdate::State(e));
                continue;
            }
            match e.kind.as_str() {
                "room.ready" => {
                    ready = true;
                    *attempt = 0;
                    on_update(RoomUpdate::Connected(conn.carrier()));
                }
                "room.pong" => waiting_for_pong = false,
                "room.ack" => {
                    if let Some(id) = &e.request_id {
                        sent.remove(id);
                    }
                }
                "error" => {
                    let retry = e.payload.get("retryable") == Some(&Value::Bool(true));
                    if let Some(id) = &e.request_id {
                        sent.remove(id);
                    }
                    let terminal =
                        e.request_id.is_none() || e.request_id.as_ref() == Some(&resume.id);
                    on_update(RoomUpdate::Rejected(e));
                    if retry {
                        return Err(lost());
                    }
                    if terminal {
                        return Err(HyveError::domain(
                            "room_closed",
                            "the server closed the room or rejected the resume request",
                        ));
                    }
                }
                _ => {}
            }
        }
        if ready {
            let commands: Vec<_> = cursor
                .borrow()
                .pending()
                .filter(|c| !sent.contains(&c.id))
                .cloned()
                .collect();
            for cmd in commands {
                conn.send(&cmd).await?;
                sent.insert(cmd.id);
            }
        }
    }
}
