//! Bridge from the session server connection (crossbeam) to an iced subscription stream.

use crate::app::Message;
use at_proto::ServerMsg;
use crossbeam_channel::Receiver;
use iced::futures::{SinkExt, Stream, StreamExt};
use parking_lot::Mutex;
use std::sync::Arc;

static RX: Mutex<Option<Receiver<ServerMsg>>> = Mutex::new(None);

pub fn install(rx: Receiver<ServerMsg>) {
    *RX.lock() = Some(rx);
}

pub fn stream() -> impl Stream<Item = Message> {
    iced::stream::channel(
        4096,
        async |mut out: iced::futures::channel::mpsc::Sender<Message>| {
            let Some(rx) = RX.lock().take() else {
                iced::futures::future::pending::<()>().await;
                return;
            };
            let (atx, mut arx) = iced::futures::channel::mpsc::unbounded::<Option<ServerMsg>>();
            std::thread::Builder::new()
                .name("server-rx".into())
                .spawn(move || {
                    while let Ok(m) = rx.recv() {
                        if atx.unbounded_send(Some(m)).is_err() {
                            return;
                        }
                    }
                    let _ = atx.unbounded_send(None);
                })
                .expect("spawn receiver thread");
            while let Some(m) = arx.next().await {
                match m {
                    Some(m) => {
                        if out.send(Message::Server(Arc::new(m))).await.is_err() {
                            return;
                        }
                    }
                    None => {
                        let _ = out
                            .send(Message::Disconnected("connection closed".into()))
                            .await;
                        break;
                    }
                }
            }
            iced::futures::future::pending::<()>().await;
        },
    )
}
