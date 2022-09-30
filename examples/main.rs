use symphonia::{core::codecs::CodecRegistry, default::register_enabled_codecs};
use symphonia_decoder_libopus::SymphoniaDecoderLibOpus;

fn main() {
    let mut codecs = CodecRegistry::new();
    register_enabled_codecs(&mut codecs);
    codecs.register_all::<SymphoniaDecoderLibOpus>();
    // Now proceed as normal to instantiate a suitable decoder for a file.
}
