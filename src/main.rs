use actix::io::SinkWrite;
use actix::prelude::*;
use actix_web::{middleware, web, App, Error, HttpRequest, HttpResponse, HttpServer};
use actix_web_actors::ws;
use futures::stream::StreamExt;
use std::env;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str;
use std::time::{Duration, Instant};

mod actors;
mod connector;
use actors::btc::BitcoinActor;
use actors::btc::{Subscribe, Unsubscribe};

/// How often heartbeat pings are sent
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
/// How long before lack of client response causes a timeout
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

/// Do websocket handshake and start `WebsocketSession` actor
async fn ws_index(
    req: HttpRequest,
    stream: web::Payload,
    actor: web::Data<Addr<BitcoinActor>>,
) -> Result<HttpResponse, Error> {
    let srv = actor.get_ref().clone();
    ws::start(WebsocketSession::new(srv), &req, stream)
}

pub struct WebsocketSession {
    /// Client must send ping at least once per 10 seconds (CLIENT_TIMEOUT),
    /// otherwise we drop connection.
    id: String,
    hb: Instant,
    addr: Addr<BitcoinActor>,
}

impl Actor for WebsocketSession {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        println!("Websocket connection started");
        self.hb(ctx);

        // Register self in Bitcoin Actor. `AsyncContext::wait` register
        // future within context, but context waits until this future resolves
        // before processing any other events.
        // HttpContext::state() is instance of WebsocketSession, state is shared
        // across all routes within application.
        let subscriber = Subscribe(ctx.address().recipient());
        self.addr
            .send(subscriber)
            .into_actor(self)
            .then(|res, act, ctx| {
                match res {
                    Ok(res) => act.id = res,
                    // something is wrong with chat server
                    _ => ctx.stop(),
                }
                fut::ready(())
            })
            .wait(ctx);
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        self.addr.do_send(Unsubscribe(self.id.clone()));
        Running::Stop
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WebsocketSession {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        // process websocket messages
        match msg {
            Ok(ws::Message::Ping(msg)) => {
                self.hb = Instant::now();
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {
                self.hb = Instant::now();
            }
            Ok(ws::Message::Text(text)) => {
                // There is no Javascript API to send ping frames or receive pong frames.
                // There is also no API to enable, configure or detect whether the browser
                // supports and is using ping/pong frames.
                if &text == "ping" {
                    self.hb = Instant::now();
                    ctx.text("pong");
                    return;
                }
                ctx.text(text);
            }
            Ok(ws::Message::Binary(bin)) => ctx.binary(bin),
            Ok(ws::Message::Close(_)) => ctx.stop(),
            _ => ctx.stop(),
        }
    }
}

impl Handler<actors::btc::Transaction> for WebsocketSession {
    type Result = ();
    fn handle(&mut self, msg: actors::btc::Transaction, ctx: &mut Self::Context) -> Self::Result {
        let message = str::from_utf8(&msg.0).unwrap();
        ctx.text(message);
    }
}

impl WebsocketSession {
    fn new(addr: Addr<BitcoinActor>) -> Self {
        Self {
            id: "".to_owned(),
            hb: Instant::now(),
            addr,
        }
    }

    /// helper method that sends ping to client every x second. This method also
    /// checks heartbeats from client
    fn hb(&self, ctx: &mut <Self as Actor>::Context) {
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            // check client heartbeats
            if Instant::now().duration_since(act.hb) > CLIENT_TIMEOUT {
                // heartbeat timed out
                println!("Websocket client heartbeat failed, disconnecting!");
                ctx.stop();

                return;
            }

            ctx.ping(b"");
        });
    }
}

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    std::env::set_var("RUST_LOG", "actix_server=info,actix_web=info");
    env_logger::init();

    let client = connector::create_client();
    let (_response, framed) = client
        .ws("wss://ws.blockchain.info/inv")
        .connect()
        .await
        .map_err(|err| panic!("Error trying to connect: {}", err))
        .unwrap();

    let (sink, stream) = framed.split();
    let address = BitcoinActor::create(|ctx| {
        BitcoinActor::add_stream(stream, ctx);
        BitcoinActor::new(SinkWrite::new(sink, ctx))
    });

    let port: u16 = env::var("PORT")
        .unwrap_or("8080".to_owned())
        .parse()
        .expect("Failed to parse port environment variable");

    let localhost = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    let socket = SocketAddr::new(localhost, port);

    HttpServer::new(move || {
        App::new()
            .data(address.clone())
            .wrap(middleware::Logger::default())
            .service(web::resource("/ws/").route(web::get().to(ws_index)))
    })
    .bind(socket)?
    .run()
    .await
}
