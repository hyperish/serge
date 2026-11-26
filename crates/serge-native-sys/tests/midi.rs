//! Only the randomly named test endpoint receives these messages.
#[cfg(all(target_os = "macos", feature = "midi"))]
#[test]
fn coremidi_virtual_destination_receives_the_original_messages() {
    use serge_native_sys::hw::midi::{MidiInput, MidiOutput, os::unix::VirtualInput};
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };
    let name = format!("Serge MIDI probe {}", std::process::id());
    let (tx, rx) = mpsc::channel();
    let _virtual = MidiInput::new("Serge receiver")
        .unwrap()
        .create_virtual(
            &name,
            move |_, bytes, _| {
                let _ = tx.send(bytes.to_vec());
            },
            (),
        )
        .unwrap();
    let output = MidiOutput::new("Serge sender").unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    let port = loop {
        if let Some(port) = output
            .ports()
            .into_iter()
            .find(|p| output.port_name(p).is_ok_and(|s| s.contains(&name)))
        {
            break port;
        }
        assert!(
            Instant::now() < until,
            "virtual CoreMIDI endpoint did not appear"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut wire = output.connect(&port, "Serge acceptance").unwrap();
    for msg in [[0x90, 60, 95], [0x80, 60, 0]] {
        wire.send(&msg).unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), msg);
    }
}
