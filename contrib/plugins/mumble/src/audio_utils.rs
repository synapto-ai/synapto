use ogg::PacketReader;
use opus::{Channels, Decoder};
use std::error::Error;
use std::io::Cursor;
use tracing::instrument;

#[derive(Debug)]
#[allow(dead_code)]
pub struct DecodedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u32,
}

#[instrument(name = "Audio Decoding", level = "debug", skip_all)]
pub fn decode_opus_from_memory(
    input_data: &[u8],
) -> Result<DecodedAudio, Box<dyn Error + Send + Sync>> {
    let cursor = Cursor::new(input_data);
    let mut packet_reader = PacketReader::new(cursor);

    let sample_rate = 48000;

    let mut decoder: Option<Decoder> = None;
    let mut all_samples = Vec::new();
    let mut channel_count: usize = 0;

    while let Some(packet) = packet_reader.read_packet()? {
        let data = packet.data;

        if decoder.is_none() {
            if data.len() > 8 && &data[0..8] == b"OpusHead" {
                let c = data[9];
                channel_count = c as usize;

                let channels = match c {
                    1 => Channels::Mono,
                    2 => Channels::Stereo,
                    _ => {
                        return Err("Unsupported channel count (only Mono/Stereo supported)".into());
                    }
                };

                decoder = Some(Decoder::new(sample_rate, channels)?);
            }
            continue;
        }

        if data.len() > 8 && &data[0..8] == b"OpusTags" {
            continue;
        }

        if let Some(dec) = decoder.as_mut() {
            let mut output_buffer = [0.0f32; 5760 * 2];

            match dec.decode_float(&data, &mut output_buffer, false) {
                Ok(samples_per_channel) => {
                    let total_samples = samples_per_channel * channel_count;
                    all_samples.extend_from_slice(&output_buffer[..total_samples]);
                }
                Err(e) => {
                    tracing::debug!("Skipping corrupt audio packet: {}", e);
                }
            }
        }
    }

    if all_samples.is_empty() {
        return Err("No audio data could be decoded".into());
    }

    Ok(DecodedAudio {
        samples: all_samples,
        sample_rate,
        channels: channel_count as u32,
    })
}
