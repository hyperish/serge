package org.hyperish.midifixture;
import android.media.midi.MidiDeviceService;
import android.media.midi.MidiReceiver;
import java.io.IOException;

/** Virtual, clocked loopback wholly owned by the SDK acceptance test. */
public final class Echo extends MidiDeviceService {
    private final MidiReceiver input = new MidiReceiver() {
        public void onSend(byte[] buffer, int offset, int count, long timestamp) throws IOException {
            getOutputPortReceivers()[0].send(buffer, offset, count, timestamp);
        }
    };
    @Override public MidiReceiver[] onGetInputPortReceivers() { return new MidiReceiver[]{input}; }
}
