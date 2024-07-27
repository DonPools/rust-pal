use std::fs;

use pal::utils::open_mkf;


#[test]
fn test_decompress_midi() {
    let mut midi_mkf = open_mkf("MIDI.MKF").unwrap();
    for i in 0..midi_mkf.chunk_count() {
        let chunk = midi_mkf.read_chunk(i).unwrap();
        fs::write(format!("/tmp/midi/chunk_{}.mid", i), &chunk).unwrap();
    }
}