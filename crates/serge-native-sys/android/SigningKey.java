package org.hyperish.serge;

import android.content.Context;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyInfo;
import android.security.keystore.KeyProperties;

import java.io.File;
import java.io.IOException;
import java.io.RandomAccessFile;
import java.nio.channels.FileLock;
import java.security.KeyFactory;
import java.security.KeyPairGenerator;
import java.security.KeyStore;
import java.security.PrivateKey;
import java.security.PublicKey;
import java.security.Signature;
import java.security.spec.ECGenParameterSpec;
import java.util.Arrays;

/** Named, non-exportable P-256 keys. Creation and explicit deletion serialize across app processes. */
public final class SigningKey {
    private final String alias;
    private final PrivateKey key;
    private final PublicKey publicKey;
    private final boolean hardware;
    private final File lockFile;

    private SigningKey(String alias, PrivateKey key, PublicKey publicKey, boolean hardware, File lockFile) {
        this.alias = alias;
        this.key = key;
        this.publicKey = publicKey;
        this.hardware = hardware;
        this.lockFile = lockFile;
    }
    public static synchronized SigningKey open(Context context, String name, boolean requireHardware) throws Exception {
        String alias = "org.hyperish.serge.signing." + name;
        File lockFile = new File(context.getNoBackupFilesDir(), "serge-signing.lock");
        try (RandomAccessFile file = new RandomAccessFile(lockFile, "rw"); FileLock lock = file.getChannel().lock()) {
            KeyStore store = KeyStore.getInstance("AndroidKeyStore");
            store.load(null);
            boolean created = !store.containsAlias(alias);
            if (created) {
                KeyPairGenerator generator = KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC, "AndroidKeyStore");
                generator.initialize(new KeyGenParameterSpec.Builder(alias,
                    KeyProperties.PURPOSE_SIGN | KeyProperties.PURPOSE_VERIFY)
                    .setAlgorithmParameterSpec(new ECGenParameterSpec("secp256r1"))
                    .setDigests(KeyProperties.DIGEST_SHA256).build());
                generator.generateKeyPair();
            }
            try {
                PrivateKey key = (PrivateKey) store.getKey(alias, null);
                if (key == null || key.getEncoded() != null) throw new IOException("non-exportable key required");
                KeyInfo info = KeyFactory.getInstance(key.getAlgorithm(), "AndroidKeyStore").getKeySpec(key, KeyInfo.class);
                boolean hardware = info.isInsideSecureHardware();
                if (requireHardware && !hardware) throw new IOException("device has no hardware-backed key for this identity");
                return new SigningKey(alias, key, store.getCertificate(alias).getPublicKey(), hardware, lockFile);
            } catch (Exception error) {
                if (created) store.deleteEntry(alias);
                throw error;
            }
        }
    }
    public boolean hardware() { return hardware; }
    public byte[] publicKey() { return publicKey.getEncoded(); }
    public byte[] sign(byte[] message) throws Exception {
        Signature algorithm = Signature.getInstance("SHA256withECDSA");
        algorithm.initSign(key);
        algorithm.update(message);
        return algorithm.sign();
    }
    public boolean verify(byte[] message, byte[] signature) throws Exception {
        Signature algorithm = Signature.getInstance("SHA256withECDSA");
        algorithm.initVerify(publicKey);
        algorithm.update(message);
        return algorithm.verify(signature);
    }
    public void delete() throws Exception {
        synchronized (SigningKey.class) {
            try (RandomAccessFile file = new RandomAccessFile(lockFile, "rw"); FileLock lock = file.getChannel().lock()) {
                KeyStore store = KeyStore.getInstance("AndroidKeyStore");
                store.load(null);
                java.security.cert.Certificate current = store.getCertificate(alias);
                if (current == null) return;
                if (!Arrays.equals(current.getPublicKey().getEncoded(), publicKey())) throw new IOException("key was replaced");
                store.deleteEntry(alias);
            }
        }
    }
}
