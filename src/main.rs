use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::str;

use clap::{Arg, App};
use rumqttc::{ self, AsyncClient, Event, EventLoop, MqttOptions, Packet, SubscribeFilter, Key, TlsConfiguration, Transport, QoS };
use rustls::ClientConfig;
use serde::{Deserialize, Serialize};
use tokio::{task};

const VERSION: &'static str = env!("CARGO_PKG_VERSION");

#[derive(Serialize, Deserialize, Debug)]
enum KeyType { RSA, ECC }

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
#[serde(untagged)]
enum Auth {
    #[serde(rename_all = "camelCase")]
    AuthPassword {
        login: String,
        password: String,
    },
    #[serde(rename_all = "camelCase")]
    AuthCertificate {
        ca: String,
        client_cert: String,
        client_key: String,
        #[serde(default = "Auth::default_key_type")]
        key_type: KeyType,
    },
}

impl Auth {
    fn default_key_type() -> KeyType { KeyType::RSA }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ConnectionConfig {
    host: String,
    auth: Auth,
    #[serde(rename = "clientID")]
    #[serde(default = "ConnectionConfig::default_client_id")]
    client_id: String, 
    #[serde(default = "ConnectionConfig::default_port")]
    port: u16,
    #[serde(default = "ConnectionConfig::default_keep_alive")]
    keep_alive: u16,
    #[serde(default = "ConnectionConfig::default_clean_session")]
    clean_session: bool,
    #[serde(default = "ConnectionConfig::default_conn_timeout")]
    conn_timeout: u64,
    #[serde(default = "ConnectionConfig::default_inflight")]
    inflight: u16,   
}

impl ConnectionConfig {
    fn default_client_id() -> String { String::from("rust-mqtt-repeater") }
    fn default_keep_alive() -> u16 { 30 }
    fn default_conn_timeout() -> u64 { 5 }
    fn default_inflight() -> u16 { 100 }
    fn default_port() -> u16 { 8883 }
    fn default_clean_session() -> bool { true }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
enum Behaviour {
    Copy,
    Omit,
    InvertBoolean
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
enum Payload {
    Bytes(Vec<u8>),
    String(String),
    Behaviour(Behaviour)
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Topic {
    from: String,
    to: String,
    #[serde(default = "Topic::default_payload")]
    payload: Payload
}

impl Topic {
    fn default_payload() -> Payload { Payload::Behaviour(Behaviour::Copy) }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Config {
    source: ConnectionConfig,
    destination: ConnectionConfig,
    topics: Vec<Topic>,
}


#[tokio::main]
async fn main() {
        let matches = App::new("Rust MQTT Repeater")
            .version(VERSION)
            .about("Subscribes to selected topics at one MQTT broker and publishes events to another broker. Connection details and topics are read from a config file. See sample-config.json")
            .arg(Arg::with_name("config")
                .short("c")
                .long("config")
                .value_name("FILE")
                .help("Path to the config file. If not provided will try config.json in current working dir")
                .takes_value(true))
            .arg(Arg::with_name("verbose")
                .short("v")
                .long("verbose")
                .takes_value(false)
                .help("Enable verbose mode"))
            .get_matches();


    let config_file_path = Path::new(matches.value_of("config").unwrap_or("config.json"));
    let is_verbose = matches.is_present("verbose");

    let config_string : String;
    match fs::read_to_string(config_file_path) {
        Ok(cs) => config_string = cs,
        Err(e) => {
            println!("Unable to read config file from {}: {}", config_file_path.display(), e);
            return;
        }
    
    }
    let config : Config = serde_json::from_str(&config_string).expect("Failed to parse config");

    let (src_client, mut src_eventloop) = make_client(&config.source);
    let (dest_client, mut dest_eventloop) = make_client(&config.destination);

    let topics_lookup = config.topics.iter().map(|t| (t.from.clone(), (t.to.clone(), t.payload.clone()))).collect::<HashMap<_, _>>();

    let t1 = task::spawn(async move {
        loop {
            match src_eventloop.poll().await {
                Ok(src_notification) => {
                    if is_verbose {
                        print_event("SRC", &src_notification);
                    }
                    
                    if let Event::Incoming(packet) = src_notification {
                        if let Packet::ConnAck(connack) = packet {
                            if connack.code == rumqttc::v4::ConnectReturnCode::Success {
                                src_client.subscribe_many(config.topics
                                    .iter()
                                    .map(|t| SubscribeFilter { path: t.from.clone(), qos: QoS::AtLeastOnce })
                                    .collect::<Vec<_>>()
                                ).await.expect("Failed to subscribe to source topics");
                            }
                        } else if let Packet::Publish(publish) = packet {
                            if let Some(t) = topics_lookup.get(&publish.topic) {
                                if is_verbose {
                                    println!("[SRC->DEST] {:?}", t);
                                }
                                let to = &t.0;
                                let payload_behaviour = &t.1;
                                let new_payload = match payload_behaviour {
                                    Payload::Behaviour(Behaviour::Copy) => publish.payload,
                                    Payload::Behaviour(Behaviour::Omit) => String::from("").into(),
                                    Payload::Behaviour(Behaviour::InvertBoolean) => {
                                        let payload_string = match String::from_utf8_lossy(&publish.payload).to_lowercase().as_str() {
                                            "false" | "0" => String::from("true"),
                                            "true" | "1" => String::from("false"),
                                            _ => String::from(""),
                                        };
                                        payload_string.into()
                                    },
                                    Payload::String(payload_string) => payload_string.clone().into(),
                                    Payload::Bytes(bytes) => bytes.to_owned().into(),
                                };

                                dest_client
                                    .publish_bytes(to, QoS::AtLeastOnce, publish.retain, new_payload)
                                    .await
                                    .expect("Failed to publish to destination");
                            }
                        }
                    }
                },
                Err(connection_error) => {
                    println!("[SRC CONNECTION_ERROR] {}", connection_error.to_string());
                }
            }
            task::yield_now().await;
        }
    });

    let t2 = task::spawn(async move {
        loop {
            match dest_eventloop.poll().await {
                 Ok(dest_notification) => {
                    if is_verbose {
                        print_event("DEST", &dest_notification);
                    }
                },
                Err(connection_error) => {
                    println!("[DEST CONNECTION_ERROR] {}", connection_error.to_string());
                }
            }
                            
            task::yield_now().await;
        }
    });

    let (_first, _second) = tokio::join!(t1, t2);
}

fn make_client(connection_cfg: &ConnectionConfig) -> (AsyncClient, EventLoop) {
    let mut mqttoptions = MqttOptions::new(&connection_cfg.client_id, &connection_cfg.host, connection_cfg.port);
    mqttoptions.set_keep_alive(connection_cfg.keep_alive);
    mqttoptions.set_inflight(connection_cfg.inflight);
    mqttoptions.set_clean_session(connection_cfg.clean_session);

    if let Auth::AuthPassword { login, password } = &connection_cfg.auth {
        let mut client_config = ClientConfig::new();
        client_config.root_store = rustls_native_certs::load_native_certs().expect("Failed to load platform certificates.");
        mqttoptions.set_credentials(login, password);
        mqttoptions.set_transport(Transport::tls_with_config(client_config.into()));
    } else if let Auth::AuthCertificate { ca, client_cert, client_key, key_type } = &connection_cfg.auth {
        let ca_bytes = fs::read(ca).expect("Failed to read CA certificate file");
        let client_cert_bytes = fs::read(client_cert).expect("Failed to read client certificate file");
        let client_key_bytes = fs::read(client_key).expect("Failed to read client key file");

        let key = match key_type {
            KeyType::RSA => Key::RSA(client_key_bytes),
            KeyType::ECC => Key::ECC(client_key_bytes),
        };

        mqttoptions.set_transport(
            Transport::Tls(TlsConfiguration::Simple {
                ca: ca_bytes,
                alpn: None,
                client_auth: Some((client_cert_bytes, key)),
            })
        );

    }

    return AsyncClient::new(mqttoptions, 10);
}

fn print_event(prefix: &str, event: &Event) {
    print!("[{}] Received = {:?};", prefix, event);
    
    if let Event::Incoming(packet) = event {
        if let Packet::Publish(publish) = packet {
            let payload = str::from_utf8(&publish.payload).unwrap();
            print!("{}", payload);
        }
    }
    println!("");
}
