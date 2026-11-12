//! Opening and decoding audio files.

use std::fs::File;
use std::io::ErrorKind;
use std::path::Path;

use symphonia::core::codecs::audio::well_known::CODEC_ID_OPUS;
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Time, TimeBase};

use crate::AudioError;

/// Technical facts about an opened stream.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StreamInfo {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: Option<u32>,
}

enum Decoder {
    Symphonia(Box<dyn AudioDecoder>),
    Opus { dec: Box<opus_decoder::OpusDecoder>, channels: usize, pcm: Vec<f32> },
}

/// A demuxer + decoder pair producing interleaved `f32` samples at the file's native rate.
pub struct Source {
    reader: Box<dyn FormatReader>,
    decoder: Decoder,
    params: AudioCodecParameters,
    track_id: u32,
    time_base: Option<TimeBase>,
    pub sample_rate: u32,
    pub channels: usize,
    pub duration_ms: Option<u64>,
    pub info: StreamInfo,
    /// Frames to drop after an accurate seek so playback starts exactly where requested.
    skip_frames: usize,
}

impl Source {
    pub fn open(path: &Path) -> Result<Self, AudioError> {
        let file = File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let reader = symphonia::default::get_probe()
            .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
            .map_err(|e| AudioError::Unsupported(format!("unrecognised file: {e}")))?;

        let track = reader
            .default_track(TrackType::Audio)
            .ok_or_else(|| AudioError::Unsupported("no audio track".into()))?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .cloned()
            .ok_or_else(|| AudioError::Unsupported("unknown audio codec".into()))?;
        let track_id = track.id;
        let time_base = track.time_base;
        let sample_rate = params.sample_rate.ok_or_else(|| AudioError::Unsupported("unknown sample rate".into()))?;
        let channels = params.channels.as_ref().map(|c| c.count()).unwrap_or(2).max(1);

        let duration_ms = track
            .duration
            .zip(time_base)
            .and_then(|(d, tb)| tb.calc_duration(d))
            .map(|t| t.as_millis().max(0) as u64)
            .or_else(|| track.num_frames.map(|n| n * 1000 / u64::from(sample_rate)));

        let (decoder, codec) = if params.codec == CODEC_ID_OPUS {
            if channels > 2 {
                return Err(AudioError::Unsupported("surround Opus is not supported yet".into()));
            }
            let dec = opus_decoder::OpusDecoder::new(48_000, channels)
                .map_err(|e| AudioError::Decode(format!("opus: {e:?}")))?;
            (Decoder::Opus { dec: Box::new(dec), channels, pcm: Vec::new() }, "Opus".to_string())
        } else {
            let codecs = symphonia::default::get_codecs();
            let dec = codecs
                .make_audio_decoder(&params, &AudioDecoderOptions::default())
                .map_err(|e| AudioError::Unsupported(format!("unsupported codec: {e}")))?;
            let name = codecs
                .get_audio_decoder(params.codec)
                .map(|d| d.codec.info.short_name.to_uppercase())
                .unwrap_or_else(|| "UNKNOWN".into());
            (Decoder::Symphonia(dec), name)
        };

        let info = StreamInfo {
            codec,
            sample_rate: if matches!(decoder, Decoder::Opus { .. }) { 48_000 } else { sample_rate },
            channels: channels as u16,
            bits_per_sample: params.bits_per_sample,
        };

        Ok(Source {
            reader,
            decoder,
            params,
            track_id,
            time_base,
            sample_rate: info.sample_rate,
            channels,
            duration_ms,
            info,
            skip_frames: 0,
        })
    }

    /// Decode the next packet, appending interleaved samples to `out`.
    /// Returns `Ok(false)` at the end of the stream.
    pub fn decode_next(&mut self, out: &mut Vec<f32>) -> Result<bool, AudioError> {
        loop {
            let packet = match self.reader.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(false),
                Err(SymError::IoError(e)) if e.kind() == ErrorKind::UnexpectedEof => return Ok(false),
                Err(SymError::ResetRequired) => {
                    self.rebuild_decoder()?;
                    continue;
                }
                Err(e) => return Err(AudioError::Decode(e.to_string())),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let start = out.len();
            let channels = self.channels;
            match &mut self.decoder {
                Decoder::Symphonia(dec) => match dec.decode(&packet) {
                    Ok(buf) => {
                        let buf_channels = buf.spec().channels().count().max(1);
                        out.resize(start + buf.samples_interleaved(), 0.0);
                        buf.copy_to_slice_interleaved(&mut out[start..]);
                        if buf_channels != channels {
                            // Rare mid-stream channel change: keep the first channels / duplicate mono.
                            let decoded: Vec<f32> = out.drain(start..).collect();
                            crate::remix::remix(&decoded, buf_channels, channels, out);
                        }
                    }
                    Err(SymError::DecodeError(msg)) => {
                        tracing::warn!("skipping undecodable packet: {msg}");
                        continue;
                    }
                    Err(e) => return Err(AudioError::Decode(e.to_string())),
                },
                Decoder::Opus { dec, channels, pcm } => {
                    pcm.resize(dec.max_frame_size_per_channel() * *channels, 0.0);
                    match dec.decode_float(&packet.data, pcm, false) {
                        Ok(frames) => {
                            let skip = packet.trim_start.get() as usize;
                            let end = frames.saturating_sub(packet.trim_end.get() as usize);
                            if skip < end {
                                out.extend_from_slice(&pcm[skip * *channels..end * *channels]);
                            }
                        }
                        Err(e) => {
                            tracing::warn!("skipping undecodable opus packet: {e:?}");
                            continue;
                        }
                    }
                }
            }
            if self.skip_frames > 0 {
                let available = (out.len() - start) / channels;
                let drop = self.skip_frames.min(available);
                out.drain(start..start + drop * channels);
                self.skip_frames -= drop;
            }
            return Ok(true);
        }
    }

    fn rebuild_decoder(&mut self) -> Result<(), AudioError> {
        if let Decoder::Symphonia(dec) = &mut self.decoder {
            *dec = symphonia::default::get_codecs()
                .make_audio_decoder(&self.params, &AudioDecoderOptions::default())
                .map_err(|e| AudioError::Decode(e.to_string()))?;
        } else if let Decoder::Opus { dec, .. } = &mut self.decoder {
            dec.reset();
        }
        Ok(())
    }

    /// Seek to `ms`. Returns the position actually reached.
    pub fn seek(&mut self, ms: u64) -> Result<u64, AudioError> {
        let to = SeekTo::Time { time: Time::from_millis(ms as i64), track_id: Some(self.track_id) };
        let seeked = self.reader.seek(SeekMode::Accurate, to).map_err(|e| AudioError::Decode(format!("seek failed: {e}")))?;
        match &mut self.decoder {
            Decoder::Symphonia(dec) => dec.reset(),
            Decoder::Opus { dec, .. } => dec.reset(),
        }
        // Accurate seeks land at or before the target; drop the difference ourselves.
        self.skip_frames = 0;
        if let Some(tb) = self.time_base {
            let required = tb.calc_time(seeked.required_ts).map(|t| t.as_secs_f64());
            let actual = tb.calc_time(seeked.actual_ts).map(|t| t.as_secs_f64());
            if let (Some(req), Some(act)) = (required, actual) {
                if req > act {
                    self.skip_frames = ((req - act) * f64::from(self.sample_rate)) as usize;
                }
                return Ok((req.max(0.0) * 1000.0) as u64);
            }
        }
        Ok(ms)
    }
}
