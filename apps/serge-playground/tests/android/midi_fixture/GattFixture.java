package org.hyperish.midifixture;

import android.app.Activity;
import android.bluetooth.*;
import android.bluetooth.le.*;
import android.os.Bundle;
import android.os.ParcelUuid;
import android.util.Log;
import java.util.Arrays;
import java.util.UUID;

/** Peer used on a second emulator. Each run advertises its own private service UUID. */
public final class GattFixture extends Activity {
    private static final UUID VALUE = UUID.fromString("d1fcf31e-253b-43da-91fc-4bc7d8848211");
    private static final UUID CCC = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb");
    private BluetoothGattServer server;
    private BluetoothLeAdvertiser advertiser;
    private BluetoothGattCharacteristic value;
    private volatile byte[] payload = {1, 2, 3};
    private volatile BluetoothDevice subscriber;
    private final AdvertiseCallback advertising = new AdvertiseCallback() {
        public void onStartSuccess(AdvertiseSettings settings) { Log.i("SergeGatt", "advertising"); }
        public void onStartFailure(int code) { Log.e("SergeGatt", "advertising failed " + code); }
    };
    private final BluetoothGattServerCallback callback = new BluetoothGattServerCallback() {
        public void onServiceAdded(int status, BluetoothGattService service) {
            if (status != BluetoothGatt.GATT_SUCCESS) throw new IllegalStateException("service " + status);
            AdvertiseSettings settings = new AdvertiseSettings.Builder().setConnectable(true)
                .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY).build();
            advertiser.startAdvertising(settings, new AdvertiseData.Builder()
                .addServiceUuid(new ParcelUuid(service.getUuid())).build(), advertising);
        }
        public void onCharacteristicReadRequest(BluetoothDevice d, int req, int offset, BluetoothGattCharacteristic c) {
            byte[] data = payload;
            boolean valid = offset >= 0 && offset <= data.length;
            server.sendResponse(d, req, valid ? BluetoothGatt.GATT_SUCCESS : BluetoothGatt.GATT_INVALID_OFFSET,
                offset, valid ? Arrays.copyOfRange(data, offset, data.length) : null);
        }
        public void onDescriptorWriteRequest(BluetoothDevice d, int req, BluetoothGattDescriptor descriptor,
                boolean prepared, boolean response, int offset, byte[] bytes) {
            boolean valid = !prepared && offset == 0 && CCC.equals(descriptor.getUuid());
            if (valid) subscriber = Arrays.equals(bytes, BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE) ? d : null;
            if (response) server.sendResponse(d, req, valid ? BluetoothGatt.GATT_SUCCESS : BluetoothGatt.GATT_FAILURE, offset, null);
        }
        public void onCharacteristicWriteRequest(BluetoothDevice d, int req, BluetoothGattCharacteristic c,
                boolean prepared, boolean response, int offset, byte[] bytes) {
            boolean valid = !prepared && offset == 0 && VALUE.equals(c.getUuid());
            if (valid) payload = bytes.clone();
            if (response) server.sendResponse(d, req, valid ? BluetoothGatt.GATT_SUCCESS : BluetoothGatt.GATT_FAILURE, offset, null);
            if (valid && d.equals(subscriber)) {
                value.setValue(payload);
                server.notifyCharacteristicChanged(d, value, false);
            }
        }
    };

    protected void onCreate(Bundle state) {
        super.onCreate(state);
        UUID id = UUID.fromString(getIntent().getStringExtra("service"));
        BluetoothManager manager = getSystemService(BluetoothManager.class);
        advertiser = manager.getAdapter().getBluetoothLeAdvertiser();
        server = manager.openGattServer(this, callback);
        BluetoothGattService service = new BluetoothGattService(id, BluetoothGattService.SERVICE_TYPE_PRIMARY);
        value = new BluetoothGattCharacteristic(VALUE, BluetoothGattCharacteristic.PROPERTY_READ |
                BluetoothGattCharacteristic.PROPERTY_WRITE | BluetoothGattCharacteristic.PROPERTY_NOTIFY,
                BluetoothGattCharacteristic.PERMISSION_READ | BluetoothGattCharacteristic.PERMISSION_WRITE);
        value.addDescriptor(new BluetoothGattDescriptor(CCC, BluetoothGattDescriptor.PERMISSION_WRITE));
        service.addCharacteristic(value);
        if (!server.addService(service)) throw new IllegalStateException("GATT service registration failed");
    }
    protected void onDestroy() {
        if (advertiser != null) advertiser.stopAdvertising(advertising);
        if (server != null) server.close();
        super.onDestroy();
    }
}
