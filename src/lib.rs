use std::borrow::Cow;

use audio::Signal;
use opus;
use symphonia::core::{
    audio::{AudioBuffer, AudioBufferRef, SignalSpec},
    codecs::{CodecParameters, Decoder, DecoderOptions, CODEC_TYPE_OPUS},
    errors::*,
    *,
};

/// Note that we only have to implement decoding, as .opus/.ogx files are actually
/// ogg-containers with opus packets inside and ogg is already demuxed nicely :)
pub struct SymphoniaDecoderLibOpus {
    libopus_decoder: opus::Decoder, // This prevents the struct from being Sync.
    libopus_output_buffer: [i16; 5760 * 2], // This struct is large. BUT, inst_func mallocs it.
    decoded_buffer: AudioBuffer<i16>,
    params: CodecParameters,
    channels: usize,
}

// It is safe for different threads to have &SymphoniaDecoderLibOpus non-mutable references concurrently.
// This is safe because Rust guarantees &mut self functions can't be called while &SymphoniaDecoderLibOpus exists.
// The &self functions don't call interior-mutability functions on libopus_decoder which would be unsafe.
// In fact, no libopus functions are called at all by functions that take &self.
// Thus this is safe. Q.E.D.
unsafe impl Sync for SymphoniaDecoderLibOpus {}

/// Information about the codec that this module provides.
pub const CODEC_DESCRIPTORS: [codecs::CodecDescriptor; 1] = [codecs::CodecDescriptor {
    codec: CODEC_TYPE_OPUS,
    short_name: "opus",
    long_name: "SymphoniaDecoderLibOpus opus decoding using dynamic linking to libopus.so",
    inst_func: inst_func,
}];

/// instantiates a malloced decoder and dynamically dispatches trait calls through vtable
fn inst_func(params: &CodecParameters, options: &DecoderOptions) -> Result<Box<dyn Decoder>> {
    match SymphoniaDecoderLibOpus::try_new(params, options) {
        Ok(decoder) => Ok(Box::new(decoder)),
        Err(e) => Err(e),
    }
}

impl Decoder for SymphoniaDecoderLibOpus {
    fn try_new(
        params: &codecs::CodecParameters,
        _options: &codecs::DecoderOptions, // TODO: verification of correct playback.
    ) -> errors::Result<Self>
    where
        Self: Sized,
    {
        // translate channels
        let channels = match params.channels.unwrap().count() {
            1 => opus::Channels::Mono,
            2 => opus::Channels::Stereo,
            // TODO: how to attach dynamic error data such as number of channels?
            _ => return Err(Error::Unsupported("unsupported channel count")),
        };

        // instantiate deocder and intermediate buffers
        Ok(SymphoniaDecoderLibOpus {
            libopus_decoder: opus::Decoder::new(params.sample_rate.unwrap_or_default(), channels)
                .unwrap(),
            // The buffer cannot be smaller than this, check libopus docs if in doubt!
            libopus_output_buffer: [0; 5760 * 2], // assume max channels for opus which is 2
            // The buffer cannot be smaller than this, check libopus docs if in doubt!
            decoded_buffer: AudioBuffer::new(
                5760, // frames
                SignalSpec::new(48000, params.channels.unwrap()),
            ),
            // Store this just to implement codec_params()
            params: params.clone(),
            channels: params.channels.unwrap().count(),
        })
    }

    fn supported_codecs() -> &'static [codecs::CodecDescriptor]
    where
        Self: Sized,
    {
        &CODEC_DESCRIPTORS
    }

    fn reset(&mut self) {
        // TODO: yea, this is 100% a guess!
        self.libopus_decoder.reset_state().unwrap();
    }

    fn codec_params(&self) -> &codecs::CodecParameters {
        // WARNING: calling a self.libopus_decoder function with interior mutability would be unsafe!
        // TODO: is this correct? we just returned stored requested params from try_new
        &self.params
    }

    fn decode(&mut self, packet: &formats::Packet) -> errors::Result<audio::AudioBufferRef> {
        // Decode some more data.
        // TODO: forward error correction if used in situations where data can be lost.
        let decoded = self
            .libopus_decoder
            .decode(&packet.data, &mut self.libopus_output_buffer[..], false)
            .unwrap();
        // TODO: detect end of file. How?

        // Clear out old data from symphonia intermediate buffer.
        let dbuf = &mut self.decoded_buffer;
        dbuf.clear();
        dbuf.render_reserved(Some(decoded));

        // Fill the symphonia audio buffer with decoded interleaved data from libopus.
        // TODO: could be a silly memcpy depending on the data layout of symphonia. Could potentially be optimized.
        {
            let mut planes = dbuf.planes_mut();
            let mut ch = 0;
            for plane in planes.planes() {
                let mut s = 0;
                for sample in plane.iter_mut() {
                    *sample = self.libopus_output_buffer[s * self.channels + ch];
                    s += 1;
                }
                ch += 1;
            }
        }

        // Return a reference to what we just decoded.
        Ok(self.last_decoded())
    }

    fn finalize(&mut self) -> codecs::FinalizeResult {
        // TODO: is this correct? I think we're saying that we can't verify if it went ok.
        codecs::FinalizeResult { verify_ok: None }
    }

    fn last_decoded(&self) -> audio::AudioBufferRef {
        // WARNING: calling a self.libopus_decoder function with interior mutability would be unsafe!
        // if called before we decode a frame, you get a buffer with length 0 (note capacity != length)
        AudioBufferRef::S16(Cow::Borrowed(&self.decoded_buffer))
    }
}
