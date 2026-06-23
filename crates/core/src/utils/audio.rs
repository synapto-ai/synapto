use ogg::PacketReader;
use opus::packet;
use std::error::Error;
use std::io::Cursor;
use std::time::Duration;

pub fn get_ogg_opus_duration(input_data: &[u8]) -> Result<Duration, Box<dyn Error + Send + Sync>> {
    let cursor = Cursor::new(input_data);
    let mut packet_reader = PacketReader::new(cursor);
    let mut total_samples = 0;

    while let Some(packet) = packet_reader.read_packet()? {
        let data = packet.data;

        if data.len() > 8 && &data[0..8] == b"OpusHead" {
            continue;
        }
        if data.len() > 8 && &data[0..8] == b"OpusTags" {
            continue;
        }

        match packet::get_nb_samples(&data, 48000) {
            Ok(samples) => total_samples += samples,
            Err(e) => {
                tracing::debug!("Failed to get samples from Opus packet: {}", e);
            }
        }
    }

    Ok(Duration::from_secs_f64(total_samples as f64 / 48000.0))
}
