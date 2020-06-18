use actix::io::SinkWrite;
use actix::*;
use actix_codec::Framed;
use awc::{
    error::WsProtocolError,
    ws::{Codec, Frame, Message},
    BoxedSocket,
};
use bytes::Bytes;
use futures::stream::SplitSink;
use std::time::Duration;

#[derive(Message)]
#[rtype(result = "()")]
pub struct ClientCommand(pub String);

#[derive(Message)]
#[rtype(result = "()")]
pub struct Transaction(pub Bytes);

#[derive(Message)]
#[rtype(result = "()")]
pub struct Subscribe(pub Recipient<Transaction>);

pub struct BTCWebsocketActor {
    pub sink: SinkWrite<Message, SplitSink<Framed<BoxedSocket, Codec>, Message>>,
    subscribers: Vec<Recipient<Transaction>>,
}

impl Actor for BTCWebsocketActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        // start heartbeats
        self.hb(ctx)
    }

    fn stopped(&mut self, _: &mut Context<Self>) {
        println!("Bitcoin websocket disconnected");
        // Stop application on disconnect
        System::current().stop();
    }
}

impl BTCWebsocketActor {
    pub fn new(sink: SinkWrite<Message, SplitSink<Framed<BoxedSocket, Codec>, Message>>) -> Self {
        Self {
            sink,
            subscribers: vec![],
        }
    }

    fn hb(&self, ctx: &mut Context<Self>) {
        ctx.run_later(Duration::new(1, 0), |act, ctx| {
            act.sink
                .write(Message::Ping(Bytes::from_static(b"")))
                .unwrap();

            act.hb(ctx);

            // client should also check for a timeout here, similar to the
            // server code
        });
    }

    fn notify(&mut self, transaction: Bytes) {
        let mut closed = vec![];
        for (position, subscriber) in self.subscribers.iter().enumerate() {
            let result = subscriber.do_send(Transaction(transaction.clone()));

            // TODO: Handle "SendError" error and only disconnect these actors.
            if result.is_err() {
                println!("There was an error trying to send message to subscriber");
                closed.push(position);
            }
        }

        closed.iter().for_each(|index| {
            self.subscribers.remove(*index);
        });
    }
}

/// Subscribe to transaction event
impl Handler<Subscribe> for BTCWebsocketActor {
    type Result = ();

    fn handle(&mut self, msg: Subscribe, _: &mut Self::Context) {
        self.subscribers.push(msg.0);
    }
}

/// Handle stdin commands
impl Handler<ClientCommand> for BTCWebsocketActor {
    type Result = ();

    fn handle(&mut self, msg: ClientCommand, _ctx: &mut Context<Self>) {
        self.sink.write(Message::Text(msg.0)).unwrap();
    }
}

/// Handle server websocket messages
impl StreamHandler<Result<Frame, WsProtocolError>> for BTCWebsocketActor {
    fn handle(&mut self, msg: Result<Frame, WsProtocolError>, _: &mut Context<Self>) {
        if let Ok(Frame::Text(txt)) = msg {
            self.notify(txt);
        }
    }

    fn started(&mut self, ctx: &mut Context<Self>) {
        println!("Bitcoin websocket connected");

        let mut _cmd = String::from("{\"op\":\"ping\"}");
        let cmd = String::from("{\"op\":\"unconfirmed_sub\"}");

        ctx.address().do_send(ClientCommand(cmd));
    }

    fn finished(&mut self, ctx: &mut Context<Self>) {
        println!("Server disconnected");
        ctx.stop()
    }
}

impl actix::io::WriteHandler<WsProtocolError> for BTCWebsocketActor {}
