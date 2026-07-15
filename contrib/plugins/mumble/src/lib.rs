#![feature(iter_next_chunk)]

use anyhow::Context;
use futures::{SinkExt, StreamExt};
use mumble_protocol::control::ControlPacket;
use mumble_protocol::voice::VoicePacket;
use mumble_protocol::voice::VoicePacketPayload;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use synapto_interface::cognitive_output_audio::types::CognitiveOutputAudio;
use synapto_interface::cognitive_output_text::types::CognitiveOutputText;
use synapto_interface::peer_input_audio::types::{PEER_INPUT_AUDIO_CHUNK_SIZE, PeerInputAudio};
use synapto_interface::peer_input_text::types::PeerInputText;
use synapto_interface::sync::{broadcast, mpsc};
use synapto_interface::types::{MessageChannel, MessageText, SenderId};
use synapto_interface::{AudioInputPlugin, AudioOutputPlugin, ChatPlugin, Plugin};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_native_tls::TlsConnector;
use tokio_util::codec::Decoder;

mod audio_utils;

fn default_host() -> String {
    "127.0.0.1".to_string()
}
fn default_port() -> u16 {
    64738
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct MumbleConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
}

#[derive(Default)]
struct MumbleChannels {
    cognitive_output_text_rx: Option<mpsc::Receiver<CognitiveOutputText>>,
    peer_input_text_tx: Option<mpsc::Sender<PeerInputText>>,
    cognitive_output_audio_rx: Option<mpsc::Receiver<CognitiveOutputAudio>>,
    peer_input_audio_tx: Option<mpsc::Sender<PeerInputAudio>>,
}

pub struct MumblePlugin {
    config: MumbleConfig,
    channels: Arc<Mutex<MumbleChannels>>,
}

#[async_trait::async_trait]
impl Plugin for MumblePlugin {
    fn register<R: synapto_interface::PluginRegistry + ?Sized>(
        self: std::sync::Arc<Self>,
        registry: &mut R,
    ) where
        Self: Sized,
    {
        registry.register_chat(self.clone());
        registry.register_audio_input(self.clone());
        registry.register_audio_output(self.clone());
    }

    async fn create(context: &synapto_interface::plugin::PluginInitContext<'_>) -> Result<Self, String> {
        let config: MumbleConfig = context.config()?;
        Ok(Self {
            config,
            channels: Arc::new(Mutex::new(MumbleChannels::default())),
        })
    }
}

#[async_trait::async_trait]
impl ChatPlugin for MumblePlugin {
    fn channel_context_schema() -> schemars::Schema {
        schemars::schema_for!(MumbleContext)
    }

    async fn start(
        &self,
        peer_input_text_tx: mpsc::Sender<PeerInputText>,
        cognitive_output_text_rx: mpsc::Receiver<CognitiveOutputText>,
        _cognitive_state_rx: broadcast::Receiver<synapto_interface::types::CognitiveStateUpdate>,
    ) -> Result<(), String> {
        let mut channels = self.channels.lock().await;
        channels.peer_input_text_tx = Some(peer_input_text_tx);
        channels.cognitive_output_text_rx = Some(cognitive_output_text_rx);
        self.try_start_client(&mut channels);
        Ok(())
    }
}

#[async_trait::async_trait]
impl AudioInputPlugin for MumblePlugin {
    async fn start(&self, tx: mpsc::Sender<PeerInputAudio>) -> Result<(), String> {
        let mut channels = self.channels.lock().await;
        channels.peer_input_audio_tx = Some(tx);
        self.try_start_client(&mut channels);
        Ok(())
    }
}

#[async_trait::async_trait]
impl AudioOutputPlugin for MumblePlugin {
    async fn start(&self, rx: mpsc::Receiver<CognitiveOutputAudio>) -> Result<(), String> {
        let mut channels = self.channels.lock().await;
        channels.cognitive_output_audio_rx = Some(rx);
        self.try_start_client(&mut channels);
        Ok(())
    }
}

impl MumblePlugin {
    fn try_start_client(&self, channels: &mut MumbleChannels) {
        if channels.cognitive_output_text_rx.is_some()
            && channels.peer_input_text_tx.is_some()
            && channels.cognitive_output_audio_rx.is_some()
            && channels.peer_input_audio_tx.is_some()
        {
            let mut cognitive_output_text_rx = channels
                .cognitive_output_text_rx
                .take()
                .expect("Missing value");
            let peer_input_text_tx = channels.peer_input_text_tx.take().expect("Missing value");
            let mut cognitive_output_audio_rx = channels
                .cognitive_output_audio_rx
                .take()
                .expect("Missing value");
            let peer_input_audio_tx = channels.peer_input_audio_tx.take().expect("Missing value");

            let mumble_config = self.config.clone();

            tokio::spawn(async move {
                loop {
                    tracing::info!(
                        "Connecting to Mumble server {}:{}",
                        mumble_config.host,
                        mumble_config.port
                    );

                    if let Err(e) = run_mumble_client(
                        &mumble_config,
                        &mut cognitive_output_text_rx,
                        &peer_input_text_tx,
                        &peer_input_audio_tx,
                        &mut cognitive_output_audio_rx,
                    )
                    .await
                    {
                        tracing::error!("Mumble client error: {:?}", e);
                    }

                    tracing::info!("Mumble client disconnected, retrying in 5 seconds...");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            });
        }
    }
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct MumbleContext {
    pub channel_id: u32,
}

async fn run_mumble_client(
    config: &MumbleConfig,
    cognitive_output_text_rx: &mut mpsc::Receiver<CognitiveOutputText>,
    peer_input_text_tx: &mpsc::Sender<PeerInputText>,
    peer_input_audio_tx: &mpsc::Sender<PeerInputAudio>,
    cognitive_output_audio_rx: &mut mpsc::Receiver<CognitiveOutputAudio>,
) -> anyhow::Result<()> {
    let addr = format!("{}:{}", config.host, config.port);
    let stream = TcpStream::connect(&addr)
        .await
        .context("Failed to connect to Mumble server")?;

    let cx = tokio_native_tls::native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .build()?;
    let connector = TlsConnector::from(cx);
    let tls_stream = connector
        .connect(&config.host, stream)
        .await
        .context("Failed to establish TLS connection")?;

    let codec = mumble_protocol::control::ControlCodec::<
        mumble_protocol::Serverbound,
        mumble_protocol::Clientbound,
    >::new();
    let (mut sink, mut stream) = codec.framed(tls_stream).split();

    // 1. Send Version
    let mut version = mumble_protocol::control::msgs::Version::new();
    version.set_version(0x00010400); // 1.4.0
    version.set_release("ai-robot".to_string());
    version.set_os("Linux".to_string());
    sink.send(ControlPacket::Version(Box::new(version))).await?;

    // 2. Send Authenticate
    let mut auth = mumble_protocol::control::msgs::Authenticate::new();
    auth.set_username(config.username.clone());
    if let Some(ref pw) = config.password {
        auth.set_password(pw.clone());
    }
    auth.set_opus(true);
    sink.send(ControlPacket::Authenticate(Box::new(auth)))
        .await?;

    tracing::info!("Mumble connected and authenticated as {}", config.username);

    let mut ping_interval = tokio::time::interval(Duration::from_secs(10));

    let mut decoder =
        opus::Decoder::new(16000, opus::Channels::Mono).context("Failed to create Opus decoder")?;
    let mut encoder = opus::Encoder::new(48000, opus::Channels::Mono, opus::Application::Voip)
        .context("Failed to create Opus encoder")?;

    let mut input_buffer = Vec::new();
    let mut output_seq_num = 0u64;

    // Main loop
    loop {
        better_tokio_select::tokio_select!(match .. {
            .. if let _ = ping_interval.tick() => {
                let ping = mumble_protocol::control::msgs::Ping::new();
                sink.send(ControlPacket::Ping(Box::new(ping))).await?;

                let udp_ping = mumble_protocol::voice::VoicePacket::Ping {
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_else(|e| panic!("Error: {:?}", e))
                        .as_millis() as u64,
                };
                sink.send(ControlPacket::UDPTunnel(Box::new(udp_ping)))
                    .await?;
            }
            .. if let packet = stream.next() => {
                let packet = match packet {
                    Some(Ok(p)) => p,
                    Some(Err(e)) => return Err(e.into()),
                    None => return Ok(()),
                };
                match packet {
                    ControlPacket::TextMessage(mut msg) => {
                        // FIXME
                        let text = msg.take_message().replace("&nbsp;", " ");
                        let sender_id = msg.get_actor();
                        let sender_name = format!("MumbleUser:{}", sender_id);
                        tracing::info!("Mumble message from {}: {}", sender_name, text);
                        println!("Mumble message from {}: {}", sender_name, text);

                        let text_message = PeerInputText {
                            channel: MessageChannel {
                                context: serde_json::json!({ "channel_id": 0 }),
                            },
                            sender_id: SenderId(sender_name.clone()),
                            text: MessageText(text),
                            attached_documents: vec![],
                            explicitly_addressed: true,
                        };
                        peer_input_text_tx
                            .send(text_message)
                            .await
                            .inspect_err(|e| tracing::error!("{}", e))
                            .ok();
                    }
                    ControlPacket::Ping(_ping) => {}
                    ControlPacket::CryptSetup(_crypt_setup) => {
                        let mut reply = mumble_protocol::control::msgs::CryptSetup::new();
                        reply.set_client_nonce(vec![0; 16]); // 16 zeroes
                        sink.send(ControlPacket::CryptSetup(Box::new(reply)))
                            .await?;
                        tracing::debug!("Acknowledged CryptSetup to stop resync requests");
                    }
                    ControlPacket::UDPTunnel(voice) => {
                        match *voice {
                            VoicePacket::Audio {
                                payload: VoicePacketPayload::Opus(frame, _),
                                ..
                            } => {
                                tracing::trace!(
                                    "Received Opus audio frame of {} bytes",
                                    frame.len()
                                );
                                let mut pcm = [0i16; 1920]; // 120ms at 16kHz
                                match decoder.decode(&frame, &mut pcm, false) {
                                    Ok(len) => {
                                        input_buffer.extend_from_slice(&pcm[..len]);
                                        if input_buffer.len() >= PEER_INPUT_AUDIO_CHUNK_SIZE {
                                            let chunk: [i16; PEER_INPUT_AUDIO_CHUNK_SIZE] =
                                                input_buffer
                                                    .drain(..PEER_INPUT_AUDIO_CHUNK_SIZE)
                                                    .next_chunk()
                                                    .unwrap_or_else(|e| {
                                                        panic!("Buffer too small: {:?}", e)
                                                    });
                                            let msg = PeerInputAudio::new(chunk);
                                            peer_input_audio_tx
                                                .send(msg)
                                                .await
                                                .inspect_err(|e| tracing::error!("{}", e))
                                                .ok();
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("Opus decode error: {}", e);
                                    }
                                }
                            }
                            VoicePacket::Ping { timestamp } => {
                                sink.send(ControlPacket::UDPTunnel(Box::new(VoicePacket::Ping {
                                    timestamp,
                                })))
                                .await?;
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            .. if let Some(audio_data) = cognitive_output_audio_rx.recv() => {
                tracing::info!(
                    "Received audio_data from audio_output_rx of length {}",
                    audio_data.len()
                );
                if let Ok(decoded) = audio_utils::decode_opus_from_memory(&audio_data[..]) {
                    tracing::info!("Decoded audio to {} samples", decoded.samples.len());
                    // Mumble expects 48kHz. Re-encode to 20ms chunks (960 samples per channel).
                    let frame_size = 960;
                    let chunk_size = frame_size * decoded.channels as usize;

                    let mut packets_sent = 0;
                    for chunk in decoded.samples.chunks(chunk_size) {
                        if chunk.len() < chunk_size {
                            break;
                        } // Skip incomplete chunk
                        let mut out = vec![0u8; 1024];
                        match encoder.encode_float(chunk, &mut out) {
                            Ok(len) => {
                                out.truncate(len);
                                let packet = mumble_protocol::voice::VoicePacket::Audio {
                                    _dst: std::marker::PhantomData,
                                    target: 0,
                                    session_id: (),
                                    seq_num: output_seq_num,
                                    payload: VoicePacketPayload::Opus(
                                        bytes::Bytes::from(out),
                                        false,
                                    ),
                                    position_info: None,
                                };
                                sink.send(ControlPacket::UDPTunnel(Box::new(packet)))
                                    .await?;
                                output_seq_num += 2; // +2 for 20ms
                                packets_sent += 1;
                            }
                            Err(e) => {
                                tracing::error!("Opus encode error: {}", e);
                            }
                        }
                    }
                    tracing::info!("Sent {} Opus packets to Mumble", packets_sent);
                } else {
                    tracing::error!("Failed to decode audio from audio_output_rx");
                }
            }
            .. if let Some(cmd) = cognitive_output_text_rx.recv() => {
                let mut msg = mumble_protocol::control::msgs::TextMessage::new();
                if let Some(channel_id) = cmd
                    .target_channel
                    .context
                    .get("channel_id")
                    .and_then(|v| v.as_u64())
                {
                    msg.mut_channel_id().push(channel_id as u32);
                } else {
                    msg.mut_channel_id().push(0);
                }
                msg.set_message(cmd.text);
                sink.send(ControlPacket::TextMessage(Box::new(msg))).await?;
            }
        })
    }
}
