use crate::{HostAudioConfig, Terminate, audio_utils};
use synapto_interface::cognitive_output_audio::types::CognitiveOutputAudio;
use synapto_interface::sync::mpsc;
use libspa_sys as spa_sys;
use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use pw::spa::pod::Pod;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub fn run_playback_task(
    config: HostAudioConfig,
    mut rx: mpsc::Receiver<CognitiveOutputAudio>,
    quit_tx_handle: Arc<Mutex<Option<pw::channel::Sender<Terminate>>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (audio_tx, audio_rx) = pw::channel::channel::<Vec<f32>>();

    std::thread::spawn(move || {
        while let Some(audio_data) = rx.blocking_recv() {
            match audio_utils::decode_opus_from_memory(&audio_data.0) {
                Ok(decoded) => {
                    if audio_tx.send(decoded.samples).is_err() {
                        break;
                    }
                }
                Err(e) => tracing::error!("HostAudio playback decode error: {}", e),
            }
        }
    });

    pw::init();
    let mainloop = Rc::new(pw::main_loop::MainLoopBox::new(None)?);
    let mainloop_clone = mainloop.clone();

    let (quit_tx, quit_rx) = pw::channel::channel();
    *quit_tx_handle.lock().unwrap_or_else(|e| panic!("Failed to lock: {:?}", e)) = Some(quit_tx);

    let _quit_receiver = quit_rx.attach(mainloop.loop_(), move |_| {
        mainloop_clone.quit();
    });

    let audio_buffer = Arc::new(Mutex::new(VecDeque::<f32>::new()));
    let audio_buffer_clone = audio_buffer.clone();

    let _audio_receiver = audio_rx.attach(mainloop.loop_(), move |samples| {
        audio_buffer_clone.lock().unwrap_or_else(|e| panic!("Failed to lock: {:?}", e)).extend(samples);
    });

    let context = pw::context::ContextBox::new(mainloop.loop_(), None)?;
    let core = context.connect(None)?;

    let stream = pw::stream::StreamBox::new(
        &core,
        "audio-src",
        if let Some(ref audio_output_target) = config.audio_output_target
            && !audio_output_target.is_empty()
        {
            properties! {
                *pw::keys::MEDIA_TYPE => "Audio",
                *pw::keys::MEDIA_ROLE => "Music",
                *pw::keys::MEDIA_CATEGORY => "Playback",
                *pw::keys::AUDIO_CHANNELS => "1",
                *pw::keys::TARGET_OBJECT => audio_output_target.as_str(),
            }
        } else {
            properties! {
                *pw::keys::MEDIA_TYPE => "Audio",
                *pw::keys::MEDIA_ROLE => "Music",
                *pw::keys::MEDIA_CATEGORY => "Playback",
                *pw::keys::AUDIO_CHANNELS => "1",
            }
        },
    )?;

    let _listener = stream
        .add_local_listener_with_user_data(audio_buffer)
        .process(|stream, audio_buffer| match stream.dequeue_buffer() {
            None => tracing::debug!("No buffer received"),
            Some(mut buffer) => {
                let datas = buffer.datas_mut();
                let data = &mut datas[0];
                let size = if let Some(slice) = data.data() {
                    let n_frames = slice.len() / size_of::<f32>();
                    let mut buffer_locked = audio_buffer.lock().unwrap_or_else(|e| panic!("Failed to lock: {:?}", e));
                    for i in 0..n_frames {
                        let sound = buffer_locked.pop_front().unwrap_or(0.0);
                        let start = i * size_of::<f32>();
                        let end = start + size_of::<f32>();
                        let chan = &mut slice[start..end];
                        chan.copy_from_slice(&f32::to_le_bytes(sound));
                    }
                    slice.len()
                } else {
                    0
                };
                let chunk = data.chunk_mut();
                *chunk.offset_mut() = 0;
                *chunk.stride_mut() = 0;
                *chunk.size_mut() = size as _;
            }
        })
        .register()?;

    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
    audio_info.set_rate(48000); // Decoded audio is 48kHz
    audio_info.set_channels(1);
    let mut position = [0; spa::param::audio::MAX_CHANNELS];
    position[0] = spa_sys::SPA_AUDIO_CHANNEL_MONO;
    audio_info.set_position(position);

    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(pw::spa::pod::Object {
            type_: spa_sys::SPA_TYPE_OBJECT_Format,
            id: spa_sys::SPA_PARAM_EnumFormat,
            properties: audio_info.into(),
        }),
    )
    .unwrap_or_else(|e| panic!("Error: {:?}", e))
    .0
    .into_inner();

    #[allow(clippy::expect_used)]
    let mut params = [Pod::from_bytes(&values).expect("Missing value")];

    stream.connect(
        spa::utils::Direction::Output,
        None,
        pw::stream::StreamFlags::AUTOCONNECT
            | pw::stream::StreamFlags::MAP_BUFFERS
            | pw::stream::StreamFlags::RT_PROCESS,
        &mut params,
    )?;

    mainloop.run();

    Ok(())
}

pub fn play(
    sample_rate: u32,
    channels: u32,
    samples: impl IntoIterator<Item = f32> + Send + 'static,
    audio_output_target: Option<String>,
) -> Result<(), pw::Error> {
    pw::init();
    let mainloop = Rc::new(pw::main_loop::MainLoopBox::new(None)?);
    let context = pw::context::ContextBox::new(mainloop.loop_(), None)?;
    let core = context.connect(None)?;

    let data = (mainloop.clone(), samples.into_iter());

    let stream = pw::stream::StreamBox::new(
        &core,
        "audio-src",
        if let Some(audio_output_target) = audio_output_target
            && !audio_output_target.is_empty()
        {
            properties! {
                *pw::keys::MEDIA_TYPE => "Audio",
                *pw::keys::MEDIA_ROLE => "Music",
                *pw::keys::MEDIA_CATEGORY => "Playback",
                *pw::keys::AUDIO_CHANNELS => channels.to_string(),
                *pw::keys::TARGET_OBJECT => audio_output_target,
            }
        } else {
            properties! {
                *pw::keys::MEDIA_TYPE => "Audio",
                *pw::keys::MEDIA_ROLE => "Music",
                *pw::keys::MEDIA_CATEGORY => "Playback",
                *pw::keys::AUDIO_CHANNELS => channels.to_string(),
            }
        },
    )?;

    let _listener = stream
        .add_local_listener_with_user_data(data)
        .process(|stream, acc| match stream.dequeue_buffer() {
            None => tracing::debug!("No buffer received"),
            Some(mut buffer) => {
                let datas = buffer.datas_mut();
                let data = &mut datas[0];
                let size = if let Some(slice) = data.data() {
                    let n_frames = slice.len() / size_of::<f32>();
                    for i in 0..n_frames {
                        if let Some(sound) = acc.1.next() {
                            let start = i * size_of::<f32>();
                            let end = start + size_of::<f32>();
                            let chan = &mut slice[start..end];
                            chan.copy_from_slice(&f32::to_le_bytes(sound));
                        } else {
                            acc.0.quit();
                        }
                    }
                    slice.len()
                } else {
                    0
                };
                let chunk = data.chunk_mut();
                *chunk.offset_mut() = 0;
                *chunk.stride_mut() = 0;
                *chunk.size_mut() = size as _;
            }
        })
        .register()?;

    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
    audio_info.set_rate(sample_rate);
    audio_info.set_channels(channels);
    let mut position = [0; spa::param::audio::MAX_CHANNELS];
    position[0] = spa_sys::SPA_AUDIO_CHANNEL_FL;
    position[1] = spa_sys::SPA_AUDIO_CHANNEL_FR;
    audio_info.set_position(position);

    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(pw::spa::pod::Object {
            type_: spa_sys::SPA_TYPE_OBJECT_Format,
            id: spa_sys::SPA_PARAM_EnumFormat,
            properties: audio_info.into(),
        }),
    )
    .unwrap_or_else(|e| panic!("Error: {:?}", e))
    .0
    .into_inner();
 #[allow(clippy::expect_used)]

    let mut params = [Pod::from_bytes(&values).expect("Missing value")];

    stream.connect(
        spa::utils::Direction::Output,
        None,
        pw::stream::StreamFlags::AUTOCONNECT
            | pw::stream::StreamFlags::MAP_BUFFERS
            | pw::stream::StreamFlags::RT_PROCESS,
        &mut params,
    )?;

    mainloop.run();

    Ok(())
}
