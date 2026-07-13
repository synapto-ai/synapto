use crate::{HostAudioConfig, Terminate};
use synapto_interface::peer_input_audio::types::{
    PEER_INPUT_AUDIO_CHUNK_SIZE, PEER_INPUT_AUDIO_SAMPLE_RATE, PeerInputAudio,
};
use synapto_interface::sync::mpsc;
use libspa::param::audio::AudioFormat;
use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use spa::param::format::{MediaSubtype, MediaType};
use spa::param::format_utils;
use spa::pod::Pod;
use std::convert::TryInto;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

struct CaptureUserData {
    format: spa::param::audio::AudioInfoRaw,
    sample_sender: std::sync::mpsc::Sender<Vec<i16>>,
    main_loop: Rc<pw::main_loop::MainLoopBox>,
}

fn capture_merger(
    sample_receiver: std::sync::mpsc::Receiver<Vec<i16>>,
    peer_input_audio_tx: mpsc::Sender<PeerInputAudio>,
) {
    let mut chunk = [0i16; PEER_INPUT_AUDIO_CHUNK_SIZE];
    let mut chunk_len = 0;

    for sample in sample_receiver.iter() {
        let mut sample_idx = 0;
        while sample_idx < sample.len() {
            let space_left = PEER_INPUT_AUDIO_CHUNK_SIZE - chunk_len;
            let to_copy = std::cmp::min(space_left, sample.len() - sample_idx);

            chunk[chunk_len..chunk_len + to_copy]
                .copy_from_slice(&sample[sample_idx..sample_idx + to_copy]);

            chunk_len += to_copy;
            sample_idx += to_copy;

            if chunk_len == PEER_INPUT_AUDIO_CHUNK_SIZE {
                if let Err(e) = peer_input_audio_tx.blocking_send(PeerInputAudio::new(chunk)) {
                    tracing::debug!("HostAudio capture merger: channel closed ({})", e);
                    return;
                }
                chunk_len = 0;
            }
        }
    }
}

pub fn run_capture_task(
    config: HostAudioConfig,
    tx: mpsc::Sender<PeerInputAudio>,
    quit_tx_handle: Arc<Mutex<Option<pw::channel::Sender<Terminate>>>>,
) -> Result<(), pw::Error> {
    let (sample_sender, sample_receiver) = std::sync::mpsc::channel::<Vec<i16>>();
    std::thread::spawn(move || {
        capture_merger(sample_receiver, tx);
    });

    pw::init();
    let mainloop = Rc::new(pw::main_loop::MainLoopBox::new(None)?);
    let mainloop_clone = mainloop.clone();

    let (quit_tx, quit_rx) = pw::channel::channel();
    *quit_tx_handle.lock().unwrap_or_else(|e| panic!("Failed to lock: {:?}", e)) = Some(quit_tx);

    let _quit_receiver = quit_rx.attach(mainloop.loop_(), move |_| {
        mainloop_clone.quit();
    });

    let context = pw::context::ContextBox::new(mainloop.loop_(), None)?;
    let core = context.connect(None)?;
    let data = CaptureUserData {
        format: spa::param::audio::AudioInfoRaw::default(),
        sample_sender,
        main_loop: mainloop.clone(),
    };

    let props = if let Some(ref audio_input_target) = config.audio_input_target
        && !audio_input_target.is_empty()
    {
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Music",
            *pw::keys::TARGET_OBJECT => audio_input_target.as_str(),
            *pw::keys::STREAM_CAPTURE_SINK => "true",
        }
    } else {
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Music",
        }
    };

    let stream = pw::stream::StreamBox::new(&core, "audio-capture", props)?;
    let _listener = stream
        .add_local_listener_with_user_data(data)
        .param_changed(|_, user_data, id, param| {
            let Some(param) = param else {
                return;
            };
            if id != pw::spa::param::ParamType::Format.as_raw() {
                return;
            }
            let (media_type, media_subtype) = match format_utils::parse_format(param) {
                Ok(v) => v,
                Err(_) => return,
            };
            if media_type != MediaType::Audio || media_subtype != MediaSubtype::Raw {
                return;
            }
            user_data.format.parse(param).unwrap_or_else(|e| {
                panic!("Failed to parse param changed to AudioInfoRaw: {:?}", e)
            });
        })
        .process(|stream, user_data| match stream.dequeue_buffer() {
            None => tracing::debug!("out of buffers"),
            Some(mut buffer) => {
                let datas = buffer.datas_mut();
                if datas.is_empty() {
                    return;
                }
                let data = &mut datas[0];
                let chunk = data.chunk();
                let chunk_offset = chunk.offset() as usize;
                let chunk_size = chunk.size() as usize;

                if let Some(samples) = data.data() {
                    let sub: &mut [u8] = &mut samples[chunk_offset..chunk_offset + chunk_size];
                    let i16_samples: Vec<i16> = sub
                        .chunks(size_of::<i16>())
                        .map(|chunk| -> i16 { i16::from_le_bytes(chunk.try_into().unwrap_or_else(|e| panic!("Error: {:?}", e))) })
                        .collect();
                    if let Err(e) = user_data.sample_sender.send(i16_samples) {
                        tracing::debug!("HostAudio capture process: channel closed ({})", e);
                        user_data.main_loop.quit();
                    }
                }
            }
        })
        .register()?;

    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_channels(1);
    audio_info.set_format(AudioFormat::S16LE);
    audio_info.set_rate(PEER_INPUT_AUDIO_SAMPLE_RATE as u32);
    let obj = pw::spa::pod::Object {
        type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: pw::spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };
    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .unwrap_or_else(|e| panic!("Error: {:?}", e))
    .0
    .into_inner();
    #[allow(clippy::expect_used)]
    let mut params = [Pod::from_bytes(&values).expect("Missing value")];
    stream.connect(
        spa::utils::Direction::Input,
        None,
        pw::stream::StreamFlags::AUTOCONNECT
            | pw::stream::StreamFlags::MAP_BUFFERS
            | pw::stream::StreamFlags::RT_PROCESS,
        &mut params,
    )?;

    mainloop.run();
    Ok(())
}
